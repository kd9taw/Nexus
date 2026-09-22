//! LoTW report transport (the `live` feature) — a thin authenticated HTTPS GET.
//!
//! This is the *only* non-pure piece of the LoTW connector: it takes a fully
//! built report URL (from [`tempo-core::lotw::build_report_url`], constructed by
//! the shell with the operator's credentials) and returns the raw ADIF body. All
//! LoTW knowledge — URL shape, validation, high-water extraction — lives in the
//! pure core; this module just moves bytes.
//!
//! ⚠️ The URL contains the LoTW password in its query string, so this module is
//! deliberately stricter than the other `live` adapters:
//! - **HTTPS is enforced** end-to-end (`https_only` + no redirect-following), so a
//!   redirect can never downgrade the password onto `http://`.
//! - **Errors are redacted.** A `reqwest::Error`'s `Display`/`source` can echo the
//!   request URL (and thus the password), so we NEVER stringify the raw error —
//!   only a fixed, category-based message derived from boolean predicates.

use super::neterr;
use std::io::Read;
use std::time::{Duration, Instant};

const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

/// How long the fetch may be SILENT before it is called dead.
///
/// ⚠️ #266. This is the client's `timeout`, and it is deliberately **not** a whole-body
/// deadline any more. reqwest's blocking client re-applies this value to *every* `read` on
/// the response body (`reqwest-0.12.28/src/blocking/response.rs`, `impl Read for Response`),
/// so reading the body ourselves turns it into exactly an inactivity bound: a report that
/// keeps arriving keeps its budget, and one that stops arriving ends. It still bounds
/// `send()` — connect, TLS and the response headers as one — which is the phase where "LoTW
/// is unreachable" actually shows up.
const IDLE_TIMEOUT_SECS: u64 = 120;

/// The hard ceiling on one report fetch, enforced by [`body_text`]'s own loop.
///
/// The idle bound above cannot supply this: a body that keeps trickling resets it forever,
/// and this request carries the operator's LoTW password in its query string, so it must
/// always end. Ten minutes is well above what a first full pull of a large log costs even on
/// a slow LoTW, and well below a wait anyone would sit through twice.
const TOTAL_TIMEOUT_SECS: u64 = 600;

/// The two bounds mean different things and must never be swapped: silence has to be called
/// dead well before the whole transfer is, or the ceiling would fire first and every stall
/// would be reported as a report too large to finish. A property of the constants, so it is
/// checked where they are written rather than in a test that could be deleted with them.
const _: () = assert!(IDLE_TIMEOUT_SECS * 2 <= TOTAL_TIMEOUT_SECS);

/// Fetch a LoTW report given a fully-built report URL (which carries the
/// credentials). Returns the raw response body (ADIF on success); the caller
/// validates it with `tempo_core::lotw::is_lotw_adif` before parsing.
///
/// On any failure returns a **redacted** message that never contains the URL,
/// the password, or the raw transport error.
pub fn fetch_report(url: &str) -> Result<String, String> {
    fetch_report_with(
        url,
        Duration::from_secs(IDLE_TIMEOUT_SECS),
        Duration::from_secs(TOTAL_TIMEOUT_SECS),
    )
}

/// [`fetch_report`] with its two bounds supplied. Production only ever calls it through
/// `fetch_report`; the split exists so the constants above live in exactly one place.
fn fetch_report_with(url: &str, idle: Duration, total: Duration) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(idle)
        .user_agent(UA)
        .https_only(true) // reject any non-https URL outright
        .redirect(reqwest::redirect::Policy::none()) // never follow a redirect off https
        .build()
        .map_err(|_| "LoTW: HTTP client initialization failed".to_string())?;

    let resp = client.get(url).send().map_err(redact)?;
    let status = resp.status();
    if !status.is_success() {
        // 3xx lands here too (Policy::none doesn't follow), so a redirect attempt
        // is reported, not chased. Only the numeric status is surfaced — no URL.
        return Err(format!("LoTW: server returned HTTP {}", status.as_u16()));
    }
    body_text(resp, idle, total)
}

/// Read the report body off a response LoTW has already answered 2xx to, a chunk at a time,
/// under two bounds that mean different things.
///
/// ⚠️ **#266 — THE DEFECT THIS REPLACES.** This used to be `Response::text()`, which puts the
/// WHOLE body read under one deadline. That is the wrong question to ask LoTW. It answers 2xx
/// as soon as it accepts the request and then streams the ADIF (chunked, and uncompressed — it
/// sends no `Content-Encoding` even when gzip is offered), so the deadline was a budget on the
/// server's own work rather than on the connection being alive. A first sync has no cursor
/// (`lotw_last_qsl` is empty), which makes it a full pull of the operator's entire confirmation
/// history; when that could not finish inside the budget the download failed, and because the
/// cursor only advances on a complete body, every retry was the identical request. That is the
/// reported symptom exactly: confirmations that never download, permanently.
///
/// The reporter's "short date ranges failed too" was not a counter-example to the deadline —
/// Nexus has no LoTW date-range control at all (the Logbook button and the Settings button both
/// call `download_lotw_report` with no arguments), so every attempt was the same full pull.
///
/// So the bounds now ask the right questions. `idle` — the client timeout, which reqwest
/// re-applies to each `read` — bounds SILENCE; `total` bounds the whole transfer, because the
/// idle bound alone would let a trickling body hold a password-bearing request open forever. A
/// large report that keeps arriving now completes. The ceiling is checked before each read, so
/// the true stop is at most one `idle` past it — a bound, not a promise of precision.
///
/// Same redaction discipline as [`redact`] and for the same reason: a `reqwest::Error`'s
/// `Display`/`source` can echo the password-bearing URL, so every message on this path is built
/// from boolean predicates and a byte count, never from the error's own text.
fn body_text(
    mut resp: reqwest::blocking::Response,
    idle: Duration,
    total: Duration,
) -> Result<String, String> {
    let deadline = Instant::now() + total;
    let mut body: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 64 * 1024];
    loop {
        if Instant::now() >= deadline {
            return Err(overall_timeout_message(body.len(), total));
        }
        match resp.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => body.extend_from_slice(&chunk[..n]),
            Err(e) => return Err(read_error_message(&e, body.len(), idle)),
        }
    }
    // Byte-for-byte what `Response::text()` would have returned: this crate's reqwest does not
    // enable the `charset` feature, and without it `text()` is `String::from_utf8_lossy` over
    // the same bytes regardless of Content-Type (`async_impl/response.rs`).
    Ok(String::from_utf8_lossy(&body).into_owned())
}

/// Classify a failed body read **without stringifying it**.
///
/// Both arms of `impl Read for blocking::Response` hand back an `io::Error` carrying a
/// `reqwest::Error` — the timeout arm through `Error::into_io`, the stream arm through the same
/// conversion inside `body_mut` — so the category is reachable by predicate, which is the only
/// way it may be read here.
fn read_error_message(e: &std::io::Error, got: usize, idle: Duration) -> String {
    match e.get_ref().and_then(|r| r.downcast_ref::<reqwest::Error>()) {
        Some(re) if re.is_timeout() => stalled_message(got, idle),
        Some(re) => format!(
            "{} while downloading the report",
            neterr::redact("LoTW", re)
        ),
        // No reqwest error inside — the socket itself failed. Reported as a category like
        // everything else on this path; nothing here is ever printed.
        None => format!(
            "LoTW: the download stopped after {} — the connection dropped. Try again.",
            human_bytes(got)
        ),
    }
}

/// What the operator is told when the transfer went quiet.
///
/// ⚠️ It states **which** of the two cases happened instead of offering both, and that is the
/// point of reading the body here. The previous wording had to name "still assembling" and "too
/// large" and assert neither, because one whole-body deadline cannot tell them apart. A byte
/// count can: nothing arrived means LoTW has not started sending; something arrived and then
/// stopped is a stalled transfer, and telling that operator to wait for a queue would send them
/// after the wrong thing.
fn stalled_message(got: usize, idle: Duration) -> String {
    let secs = idle.as_secs();
    if got == 0 {
        format!(
            "LoTW: it accepted the request and then sent nothing for {secs}s — it is still \
             assembling the report at its end. Try again in a few minutes."
        )
    } else {
        format!(
            "LoTW: the report stopped arriving after {} and the connection stayed quiet for \
             {secs}s. Nothing was merged, so nothing is lost — try the download again.",
            human_bytes(got)
        )
    }
}

/// What the operator is told when the report was still arriving when the ceiling ran out.
fn overall_timeout_message(got: usize, total: Duration) -> String {
    format!(
        "LoTW: the report was still arriving after {} ({} so far) and Nexus stopped waiting. \
         That is an unusually large pull — try again when LoTW is less busy.",
        human_duration(total),
        human_bytes(got)
    )
}

/// A byte count as an operator reads it. Message text only.
fn human_bytes(n: usize) -> String {
    match n {
        n if n >= 1024 * 1024 => format!("{:.1} MB", n as f64 / (1024.0 * 1024.0)),
        n if n >= 1024 => format!("{} kB", n / 1024),
        n => format!("{n} bytes"),
    }
}

/// A duration as an operator reads it — "10 minutes", not "600s". Message text only.
fn human_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 120 {
        format!("{} minutes", s / 60)
    } else {
        format!("{s}s")
    }
}

/// Map a transport error to a safe, category-only message. Uses ONLY boolean
/// predicates — never `Display`/`to_string`/`source`, any of which can leak the
/// password-bearing URL.
fn redact(e: reqwest::Error) -> String {
    if e.is_timeout() {
        "LoTW: request timed out — LoTW can be slow, try again shortly".to_string()
    } else {
        neterr::redact("LoTW", &e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    #[test]
    fn non_https_url_is_rejected_without_leaking_it() {
        // `.https_only(true)` rejects the http scheme before any network I/O, and
        // `redact` must scrub the URL/secret from the returned message. (.example
        // is a reserved TLD that won't resolve, so even if rejection didn't short-
        // circuit, the connect fails fast — and still must not leak.)
        let secret_url =
            "http://lotw.example/lotwuser/lotwreport.adi?login=ke3z&password=Sup3rSecret";
        let err = fetch_report(secret_url).unwrap_err();
        assert!(!err.contains("Sup3rSecret"), "password leaked: {err}");
        assert!(!err.contains("password"), "param name leaked: {err}");
        assert!(!err.contains("lotw.example"), "host/URL leaked: {err}");
        assert!(err.starts_with("LoTW: "), "unexpected message: {err}");
    }

    /// A loopback server shaped like LoTW: 200 + `Transfer-Encoding: chunked` (measured
    /// against `lotw.arrl.org` on 2026-09-21 — it is chunked, and it sends no
    /// `Content-Encoding` even when gzip is offered), then `chunks` chunks of `chunk_len`
    /// bytes `gap` apart. `finish` closes the body properly; otherwise it goes quiet and
    /// holds the socket open, which is the shape of a report that stops arriving.
    ///
    /// Plain HTTP: what is under test is how the body-read loop treats bytes and silence, and
    /// reaching it over TLS would need a certificate this suite has no way to make. No
    /// internet is involved.
    fn body_server(chunks: usize, chunk_len: usize, gap: Duration, finish: bool) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                if s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n")
                    .is_err()
                {
                    return;
                }
                let _ = s.flush();
                let payload = vec![b'A'; chunk_len];
                for _ in 0..chunks {
                    std::thread::sleep(gap);
                    if s.write_all(format!("{chunk_len:x}\r\n").as_bytes())
                        .is_err()
                        || s.write_all(&payload).is_err()
                        || s.write_all(b"\r\n").is_err()
                    {
                        return;
                    }
                    let _ = s.flush();
                }
                if finish {
                    let _ = s.write_all(b"0\r\n\r\n");
                    let _ = s.flush();
                } else {
                    // Quiet, but still connected: the case an inactivity bound exists for.
                    std::thread::sleep(Duration::from_millis(1500));
                }
            }
        });
        port
    }

    /// The response whose body the tests drive, from a client carrying `idle` as its timeout —
    /// the same wiring [`fetch_report_with`] does, minus `https_only`, which no loopback
    /// server can satisfy and which is not what is under test here.
    fn stalled_response(port: u16, idle: Duration) -> reqwest::blocking::Response {
        let resp = reqwest::blocking::Client::builder()
            .timeout(idle)
            .build()
            .expect("client")
            .get(format!(
                "http://127.0.0.1:{port}/lotwuser/lotwreport.adi?login=ke3z&password=Sup3rSecret"
            ))
            .send()
            // The positive control for every test below: if this failed, the stall would be in
            // the HEADERS and the body-read path under test would never be reached.
            .expect("headers must arrive — only the body is under test");
        assert!(resp.status().is_success(), "the body must follow a 2xx");
        resp
    }

    /// Nothing on this path may echo the password-bearing URL.
    fn assert_redacted(err: &str) {
        assert!(!err.contains("Sup3rSecret"), "password leaked: {err}");
        assert!(!err.contains("password"), "param name leaked: {err}");
        assert!(!err.contains("127.0.0.1"), "host leaked: {err}");
        assert!(err.starts_with("LoTW: "), "unexpected message: {err}");
    }

    /// **#266 — THE DEFECT.** A report that keeps arriving must not be cut off.
    ///
    /// `Response::text()` put the whole body under ONE deadline, so a download that was
    /// progressing perfectly well failed the moment the transfer outlasted it — and since the
    /// sync cursor only advances on a complete body, every retry was the same doomed request.
    ///
    /// Here the body takes ~1.5 s to arrive in ten chunks 150 ms apart, with the inactivity
    /// bound at 400 ms: never reached, because bytes keep coming. Under the old semantics a
    /// 400 ms budget killed this outright, which is what makes it the repro rather than a
    /// restatement — flip `body_text`'s loop back to `resp.text()` and this reds on the
    /// timeout, while the two stall tests below stay green.
    #[test]
    fn a_report_that_keeps_arriving_is_not_cut_off_by_the_deadline() {
        let idle = Duration::from_millis(400);
        let port = body_server(10, 4096, Duration::from_millis(150), true);
        let body = body_text(
            stalled_response(port, idle),
            idle,
            Duration::from_secs(10), // the ceiling, far above this transfer
        )
        .expect("a body that keeps arriving must complete");
        assert_eq!(
            body.len(),
            10 * 4096,
            "every chunk must be kept, in order and whole"
        );
        assert!(body.bytes().all(|b| b == b'A'), "the body was corrupted");
    }

    #[test]
    fn a_report_that_never_starts_arriving_is_reported_as_lotw_still_assembling() {
        // The 2xx-then-silence case: this is what the reporter's "could not read the response
        // body" actually was, and the one case where waiting really is the answer.
        let idle = Duration::from_millis(400);
        let port = body_server(0, 0, Duration::ZERO, false);
        let err = body_text(stalled_response(port, idle), idle, Duration::from_secs(10))
            .expect_err("silence must not look like an empty report");
        assert!(
            err.contains("sent nothing"),
            "it must say nothing arrived: {err}"
        );
        assert!(
            err.contains("still assembling"),
            "the queued-report case is the one this is: {err}"
        );
        assert_redacted(&err);
    }

    /// The discriminator the old single-deadline wording could not make.
    ///
    /// A transfer that started and then stopped is not LoTW queueing a report, and telling
    /// that operator to wait for a queue sends them after the wrong thing. The byte count is
    /// what tells the two apart, so this asserts on the difference, not just on the words.
    #[test]
    fn a_report_that_stops_part_way_is_not_reported_as_a_queue() {
        let idle = Duration::from_millis(400);
        let port = body_server(2, 1024, Duration::from_millis(10), false);
        let err = body_text(stalled_response(port, idle), idle, Duration::from_secs(10))
            .expect_err("a body that stops half way is not a complete report");
        assert!(
            err.contains("stopped arriving after 2 kB"),
            "it must say how much arrived: {err}"
        );
        assert!(
            !err.contains("still assembling"),
            "bytes arrived, so this is not LoTW queueing: {err}"
        );
        assert_redacted(&err);
    }

    #[test]
    fn a_body_that_never_ends_is_stopped_by_the_overall_ceiling() {
        // The idle bound alone cannot end this — the trickle keeps resetting it — and the URL
        // carries the operator's password, so the fetch must always terminate.
        let idle = Duration::from_millis(400);
        let port = body_server(10_000, 64, Duration::from_millis(20), false);
        let err = body_text(
            stalled_response(port, idle),
            idle,
            Duration::from_millis(500),
        )
        .expect_err("an endless body must hit the ceiling");
        assert!(
            err.contains("still arriving"),
            "it must say the transfer was alive, not stalled: {err}"
        );
        assert!(
            err.contains("stopped waiting"),
            "it must say Nexus gave up, not that LoTW failed: {err}"
        );
        assert_redacted(&err);
    }
}
