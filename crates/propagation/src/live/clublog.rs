//! ClubLog realtime transport (the `live` feature) — a thin authenticated POST.
//!
//! Unlike the other connectors' transports, this returns the **(status, body) for
//! ANY HTTP response** (it does NOT early-return on non-2xx): ClubLog encodes the
//! outcome (OK / Duplicate / Rejected / auth-fail / transient) in the HTTP STATUS
//! CODE, so the caller (`tempo_core::clublog::classify_response`) needs it. `Err`
//! is only a transport failure.
//!
//! ⚠️ The body carries the app-password AND the developer api key — both secret —
//! so errors are redacted (boolean predicates only, never the URL/body/raw error),
//! and HTTPS is enforced. Mirrors the `qrz.rs` `client()`/`redact` discipline (NOT
//! the relaxed `dxped.rs` live client).

use super::neterr;
use std::time::Duration;

const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

/// POST a realtime form body and return `(http_status, body_text)` for any
/// response. The caller classifies the status. `Err` only on a transport failure
/// (timeout/connect/etc.), always **redacted** — never the URL, the
/// password/api-key body, or the raw error.
pub fn push_realtime(url: &str, body: String) -> Result<(u16, String), String> {
    let resp = client()?
        .post(url)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .map_err(redact)?;
    read_reply(resp)
}

/// GET a fully built URL and return `(http_status, body_text)` for any response — the Settings
/// Test of the ClubLog credentials (`url` from [`tempo_core::clublog::build_login_test_url`],
/// which carries the password and the key). Same discipline as [`push_realtime`]: HTTPS only, no
/// redirects, and `Err` only for a transport failure, always redacted.
pub fn get_status(url: &str) -> Result<(u16, String), String> {
    get_status_with(&client()?, url)
}

fn get_status_with(c: &reqwest::blocking::Client, url: &str) -> Result<(u16, String), String> {
    read_reply(c.get(url).send().map_err(redact)?)
}

/// The most a ClubLog answer is read to. Both are short: the realtime upload answers a word or a
/// sentence, and the login Test asks for matches from a date no log reaches, so a good login
/// answers an empty list.
const CLUBLOG_REPLY_BYTES: u64 = 256 * 1024;

/// `(status, body)` for any status, the body read only as far as [`CLUBLOG_REPLY_BYTES`].
fn read_reply(resp: reqwest::blocking::Response) -> Result<(u16, String), String> {
    let status = resp.status().as_u16();
    let text = super::body::read_capped(resp, CLUBLOG_REPLY_BYTES, "ClubLog")?;
    Ok((status, text))
}

/// The client both requests use: HTTPS only, on [`builder`]'s timeout and redirect policy.
fn client() -> Result<reqwest::blocking::Client, String> {
    builder()
        .https_only(true)
        .build()
        .map_err(|_| "ClubLog: HTTP client initialization failed".to_string())
}

/// A timeout, and no redirect followed. The loopback tests build on this (plain HTTP); the
/// requests themselves always go through [`client`].
fn builder() -> reqwest::blocking::ClientBuilder {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .redirect(reqwest::redirect::Policy::none())
}

/// Map a transport error to a safe, category-only message — never `Display`/
/// `to_string`/`source` (which can echo the URL/credentials).
fn redact(e: reqwest::Error) -> String {
    neterr::redact("ClubLog", &e)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Both requests read through `read_reply` on `client`: HTTPS only, no redirect followed
    /// (it comes back as its own status), at most `CLUBLOG_REPLY_BYTES`.
    #[test]
    fn the_wire_keeps_its_limits() {
        super::super::body::assert_wire_limits(
            builder,
            client,
            |c, u| match get_status_with(c, u)? {
                (200, body) => Ok(body),
                (status, _) => Err(format!("ClubLog: HTTP {status}")),
            },
            CLUBLOG_REPLY_BYTES,
            "ClubLog",
        );
    }

    #[test]
    fn http_url_rejected_without_leaking_the_credentials_body() {
        // `.https_only(true)` rejects http pre-network; the body (app-password +
        // api key) and URL must never appear in the redacted error.
        let err = push_realtime(
            "http://clublog.example/realtime.php",
            "email=a@b.com&password=SECRETPW&api=DEVKEY&adif=%3Ceor%3E".to_string(),
        )
        .unwrap_err();
        assert!(!err.contains("SECRETPW"), "password leaked: {err}");
        assert!(!err.contains("DEVKEY"), "api key leaked: {err}");
        assert!(!err.contains("clublog.example"), "host/URL leaked: {err}");
        assert!(err.starts_with("ClubLog: "), "unexpected message: {err}");
    }

    #[test]
    fn the_login_test_rejects_http_without_leaking_the_credentials() {
        let err = get_status(
            "http://clublog.example/getmatches.php?api=DEVKEY&email=a%40b.com&password=SECRETPW",
        )
        .unwrap_err();
        assert!(!err.contains("SECRETPW"), "password leaked: {err}");
        assert!(!err.contains("DEVKEY"), "api key leaked: {err}");
        assert!(!err.contains("clublog.example"), "host/URL leaked: {err}");
        assert!(err.starts_with("ClubLog: "), "unexpected message: {err}");
    }
}
