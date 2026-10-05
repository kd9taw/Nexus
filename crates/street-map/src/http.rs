//! Byte ranges from the street-map host, and the retry policy around them.
//!
//! The hosted planet copy is about 138 GB, so the one rule that matters most here is that a
//! response is used only when it is exactly the part asked for:
//!
//! - **206 with a matching `Content-Range`** is the only success. The range may come back
//!   short only where the file ends.
//! - **200** means something between Nexus and the host ignored `Range` and is sending the
//!   whole file. The response is dropped unread and the job stops (`RangeIgnored`), as the
//!   pmtiles reader does. Retrying would only ask again.
//! - **404 or 410** means the build is no longer on the host (`BuildGone`). Builds live at
//!   names that are never reused, so this can only mean it was retired.
//! - A **total size** that differs from the first response's means the object changed under
//!   its name (`Changed`), which an immutable build never does.
//! - Connection failures, timeouts, 408, 429 and 5xx are **transient** and retried.
//!
//! Responses are never decompressed in transit: the client turns every decoder off and asks for
//! `identity`, because a byte range of a re-encoded body is not the bytes of the file.

use std::io::Read;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

use reqwest::blocking::Client;
use reqwest::header::{ACCEPT_ENCODING, CONTENT_ENCODING, CONTENT_LENGTH, CONTENT_RANGE, RANGE};

use crate::error::{ErrorKind, StreetError};

/// Why a read failed. The job and the sizing step map these to [`ErrorKind`]s differently:
/// an exhausted retry is `Paused` mid-download but `Network` while sizing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchError {
    Transient(String),
    /// The retries ran out; the last transient failure.
    Exhausted(String),
    BuildGone,
    RangeIgnored(String),
    Changed(String),
    Cancelled,
    Fatal(String),
}

impl FetchError {
    /// The error for the operator's view of a failed step. `exhausted` is the kind an
    /// exhausted retry becomes in this step.
    pub fn into_street(self, exhausted: ErrorKind) -> StreetError {
        match self {
            FetchError::Transient(m) | FetchError::Exhausted(m) => StreetError::new(exhausted, m),
            FetchError::BuildGone => StreetError::new(
                ErrorKind::BuildGone,
                "the map build is no longer on the street-map host",
            ),
            FetchError::RangeIgnored(m) => StreetError::new(ErrorKind::RangeIgnored, m),
            FetchError::Changed(m) => StreetError::new(ErrorKind::BuildChanged, m),
            FetchError::Cancelled => StreetError::new(ErrorKind::Cancelled, "cancelled"),
            FetchError::Fatal(m) => StreetError::new(ErrorKind::Server, m),
        }
    }
}

/// Somewhere a map file's bytes can be read from by offset.
pub trait RangeSource: Sync {
    /// Exactly `len` bytes at `offset`, or fewer only where the file ends.
    fn read(&self, offset: u64, len: u64, cancel: &AtomicBool) -> Result<Vec<u8>, FetchError>;

    /// The file's whole size, once a read has learned it.
    fn total_len(&self) -> Option<u64> {
        None
    }
}

/// The one client every street-map request uses.
pub fn client() -> Result<Client, StreetError> {
    Client::builder()
        .user_agent(concat!("Nexus street map/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(20))
        // reqwest's blocking client applies this to each read of a body, not to the whole
        // request, so it is a stall detector: a range of any size may take as long as it
        // needs while bytes keep arriving.
        .timeout(Duration::from_secs(30))
        .no_gzip()
        .no_brotli()
        .no_deflate()
        .no_zstd()
        .build()
        .map_err(|e| StreetError::new(ErrorKind::Network, format!("HTTP client: {e}")))
}

fn describe(e: reqwest::Error) -> String {
    let what = if e.is_timeout() {
        "timed out"
    } else if e.is_connect() {
        "could not connect"
    } else {
        "connection failed"
    };
    // The message is about the network; the URL is already known to whoever reads it.
    format!("{what}: {}", e.without_url())
}

fn status_error(status: u16) -> FetchError {
    match status {
        404 | 410 => FetchError::BuildGone,
        408 | 429 | 500..=599 => FetchError::Transient(format!("HTTP {status}")),
        _ => FetchError::Fatal(format!("the host answered HTTP {status}")),
    }
}

/// `bytes <first>-<last>/<total>`.
fn parse_content_range(v: &str) -> Option<(u64, u64, u64)> {
    let rest = v.trim().strip_prefix("bytes ")?;
    let (span, total) = rest.split_once('/')?;
    let (first, last) = span.split_once('-')?;
    let (first, last, total) = (first.parse().ok()?, last.parse().ok()?, total.parse().ok()?);
    (first <= last && last < total).then_some((first, last, total))
}

/// Read a body of exactly `expect` bytes, in pieces so a cancel is seen within one piece.
fn read_body(
    resp: &mut reqwest::blocking::Response,
    expect: u64,
    cancel: &AtomicBool,
) -> Result<Vec<u8>, FetchError> {
    let mut out = Vec::with_capacity(expect.min(64 << 20) as usize);
    let mut buf = vec![0u8; 64 << 10];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(FetchError::Cancelled);
        }
        let n = match resp.read(&mut buf) {
            Ok(n) => n,
            Err(e) => {
                return Err(FetchError::Transient(format!(
                    "the connection failed after {} of {expect} bytes: {e}",
                    out.len()
                )))
            }
        };
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() as u64 > expect {
            return Err(FetchError::Fatal(format!(
                "the host sent more than the {expect} bytes it announced"
            )));
        }
    }
    if (out.len() as u64) < expect {
        return Err(FetchError::Transient(format!(
            "the connection closed after {} of {expect} bytes",
            out.len()
        )));
    }
    Ok(out)
}

/// A map file on the host, read by range.
pub struct HttpSource {
    client: Client,
    url: String,
    /// The file's size as the first response stated it; every later response must agree.
    total: OnceLock<u64>,
}

impl HttpSource {
    pub fn new(client: &Client, url: &str) -> Self {
        Self {
            client: client.clone(),
            url: url.to_string(),
            total: OnceLock::new(),
        }
    }

    /// A source that already knows the file's size (from sizing): a response stating any other
    /// size is `Changed` from the first request on.
    pub fn expecting(client: &Client, url: &str, total: Option<u64>) -> Self {
        let s = Self::new(client, url);
        if let Some(n) = total {
            let _ = s.total.set(n);
        }
        s
    }
}

impl RangeSource for HttpSource {
    fn total_len(&self) -> Option<u64> {
        self.total.get().copied()
    }

    fn read(&self, offset: u64, len: u64, cancel: &AtomicBool) -> Result<Vec<u8>, FetchError> {
        if len == 0 {
            return Ok(Vec::new());
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(FetchError::Cancelled);
        }
        let last = offset + len - 1;
        let mut resp = self
            .client
            .get(&self.url)
            .header(RANGE, format!("bytes={offset}-{last}"))
            .header(ACCEPT_ENCODING, "identity")
            .send()
            .map_err(|e| FetchError::Transient(describe(e)))?;
        let status = resp.status().as_u16();
        if status == 200 {
            // Dropping the response closes the connection without reading the whole file.
            return Err(FetchError::RangeIgnored(
                "the host (or a proxy) ignored the byte range and began sending the whole \
                 map file"
                    .into(),
            ));
        }
        if status != 206 {
            return Err(status_error(status));
        }
        if let Some(enc) = resp.headers().get(CONTENT_ENCODING) {
            if enc.as_bytes() != b"identity" {
                return Err(FetchError::RangeIgnored(format!(
                    "the byte range came back re-encoded ({})",
                    String::from_utf8_lossy(enc.as_bytes())
                )));
            }
        }
        let (first, end, total) = resp
            .headers()
            .get(CONTENT_RANGE)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_content_range)
            .ok_or_else(|| {
                FetchError::RangeIgnored("a partial response without a usable Content-Range".into())
            })?;
        // Exactly what was asked for, or a shorter tail where the file ends.
        if first != offset || end > last || (end < last && end + 1 != total) {
            return Err(FetchError::RangeIgnored(format!(
                "asked for bytes {offset}-{last}, got {first}-{end}/{total}"
            )));
        }
        let seen = *self.total.get_or_init(|| total);
        if seen != total {
            return Err(FetchError::Changed(format!(
                "the map file was {seen} bytes and is now {total}"
            )));
        }
        read_body(&mut resp, end - first + 1, cancel)
    }
}

/// A whole small file (the index, the font/icon archive), refusing more than `limit` bytes.
/// `on_bytes` hears the running total as it arrives.
pub fn get_all(
    client: &Client,
    url: &str,
    limit: u64,
    cancel: &AtomicBool,
    on_bytes: &dyn Fn(u64),
) -> Result<Vec<u8>, FetchError> {
    if cancel.load(Ordering::Relaxed) {
        return Err(FetchError::Cancelled);
    }
    let mut resp = client
        .get(url)
        .header(ACCEPT_ENCODING, "identity")
        .send()
        .map_err(|e| FetchError::Transient(describe(e)))?;
    let status = resp.status().as_u16();
    if status != 200 {
        return Err(match status_error(status) {
            // A missing index or archive is the host's problem, not a retired build.
            FetchError::BuildGone => FetchError::Fatal(format!("the host answered HTTP {status}")),
            other => other,
        });
    }
    let declared = resp
        .headers()
        .get(CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok());
    if declared.is_some_and(|n| n > limit) {
        return Err(FetchError::Fatal(format!("the file is over {limit} bytes")));
    }
    let mut out = Vec::new();
    let mut buf = vec![0u8; 64 << 10];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err(FetchError::Cancelled);
        }
        let n = resp
            .read(&mut buf)
            .map_err(|e| FetchError::Transient(format!("the connection failed: {e}")))?;
        if n == 0 {
            break;
        }
        out.extend_from_slice(&buf[..n]);
        if out.len() as u64 > limit {
            return Err(FetchError::Fatal(format!("the file is over {limit} bytes")));
        }
        on_bytes(out.len() as u64);
    }
    if declared.is_some_and(|n| n != out.len() as u64) {
        return Err(FetchError::Transient(format!(
            "the connection closed after {} bytes",
            out.len()
        )));
    }
    Ok(out)
}

/// Waits between attempts after a transient failure. The default backs off from 1 s to 30 s
/// over six retries (about a minute in all) before a download pauses.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    pub delays: Vec<Duration>,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            delays: [1, 2, 4, 8, 16, 30]
                .into_iter()
                .map(Duration::from_secs)
                .collect(),
        }
    }
}

/// Sleep for `d`, waking early if `cancel` is set. Returns false when cancelled.
pub(crate) fn sleep_unless_cancelled(d: Duration, cancel: &AtomicBool) -> bool {
    let step = Duration::from_millis(50);
    let mut left = d;
    while !left.is_zero() {
        if cancel.load(Ordering::Relaxed) {
            return false;
        }
        let nap = left.min(step);
        std::thread::sleep(nap);
        left -= nap;
    }
    !cancel.load(Ordering::Relaxed)
}

/// Run `attempt`, retrying transient failures by `policy`. `on_retry(attempt, wait, reason)` is
/// told before each wait. Anything that is not transient is returned at once.
pub fn with_retry<T>(
    policy: &RetryPolicy,
    cancel: &AtomicBool,
    on_retry: &dyn Fn(u32, Duration, &str),
    mut attempt: impl FnMut() -> Result<T, FetchError>,
) -> Result<T, FetchError> {
    let mut n = 0usize;
    loop {
        match attempt() {
            Err(FetchError::Transient(reason)) => {
                let Some(&wait) = policy.delays.get(n) else {
                    return Err(FetchError::Exhausted(reason));
                };
                n += 1;
                on_retry(n as u32, wait, &reason);
                if !sleep_unless_cancelled(wait, cancel) {
                    return Err(FetchError::Cancelled);
                }
            }
            other => return other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_range_is_parsed_strictly() {
        assert_eq!(parse_content_range("bytes 0-99/1000"), Some((0, 99, 1000)));
        assert_eq!(
            parse_content_range("bytes 990-999/1000"),
            Some((990, 999, 1000))
        );
        for bad in [
            "bytes 0-99/*",
            "bytes */1000",
            "bytes 99-0/1000",
            "bytes 0-1000/1000",
            "0-99/1000",
            "",
        ] {
            assert_eq!(parse_content_range(bad), None, "{bad}");
        }
    }

    #[test]
    fn transient_failures_are_retried_then_exhausted_and_others_return_at_once() {
        let cancel = AtomicBool::new(false);
        let policy = RetryPolicy {
            delays: vec![Duration::ZERO; 3],
        };
        let retries = std::sync::Mutex::new(Vec::new());
        let note =
            |n: u32, _: Duration, why: &str| retries.lock().unwrap().push((n, why.to_string()));

        let mut calls = 0;
        let r: Result<(), _> = with_retry(&policy, &cancel, &note, || {
            calls += 1;
            Err(FetchError::Transient(format!("drop {calls}")))
        });
        assert_eq!(r, Err(FetchError::Exhausted("drop 4".into())));
        assert_eq!(calls, 4, "one attempt plus three retries");
        assert_eq!(retries.lock().unwrap().len(), 3);

        // Success after two failures.
        let mut calls = 0;
        let r = with_retry(&policy, &cancel, &note, || {
            calls += 1;
            if calls < 3 {
                Err(FetchError::Transient("x".into()))
            } else {
                Ok(calls)
            }
        });
        assert_eq!(r, Ok(3));

        // A 404 or a 200 is not retried.
        for e in [
            FetchError::BuildGone,
            FetchError::RangeIgnored("200".into()),
        ] {
            let mut calls = 0;
            let r: Result<(), _> = with_retry(&policy, &cancel, &note, || {
                calls += 1;
                Err(e.clone())
            });
            assert_eq!((r, calls), (Err(e), 1));
        }
    }
}
