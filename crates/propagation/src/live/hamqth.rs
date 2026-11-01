//! HamQTH.com XML transport (the `live` feature) — a thin authenticated HTTPS GET,
//! used for BOTH steps of the session-id flow (login → id, then lookup). The free-
//! account fallback for [`qrz`](super::qrz).
//!
//! All HamQTH knowledge (URL shape, XML parsing, session handling) lives in the pure
//! [`tempo_core::hamqth`]; this module just moves bytes. The orchestration (cache the
//! session id, re-login on expiry) lives in the shell.
//!
//! ⚠️ BOTH request URLs are secret-bearing — the login URL carries the password
//! and the lookup URL carries the session id — so this is as strict as the
//! QRZ/LoTW/eQSL transports: HTTPS enforced, no redirect-following, and **redacted
//! errors** (a `reqwest::Error`'s `Display`/`source` can echo the request URL, so
//! we never stringify it — only fixed, category-based messages).
//!
//! Unlike QRZ, HamQTH lookup is a plain GET both ways — there is no logbook-push
//! POST, so this module has no `post_form`.

use super::neterr;
use std::time::Duration;

const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

/// Fetch a HamQTH XML URL (login or lookup, both built by [`tempo_core::hamqth`]).
/// Returns the raw XML body; the caller validates with `hamqth::is_hamqth_xml` and
/// parses with `hamqth::parse_session`/`parse_callsign`.
///
/// On any failure returns a **redacted** message that never contains the URL, the
/// password, the session id, or the raw transport error.
pub fn fetch(url: &str) -> Result<String, String> {
    let resp = client()?.get(url).send().map_err(redact)?;
    read_body(resp)
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "HamQTH: HTTP client initialization failed".to_string())
}

fn read_body(resp: reqwest::blocking::Response) -> Result<String, String> {
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HamQTH: server returned HTTP {}", status.as_u16()));
    }
    super::body::read_capped(resp, super::body::CALLBOOK_REPLY_BYTES, "HamQTH")
}

/// Map a transport error to a safe, category-only message. Uses ONLY boolean
/// predicates — never `Display`/`to_string`/`source`, any of which can leak the
/// password- or id-bearing URL.
fn redact(e: reqwest::Error) -> String {
    neterr::redact("HamQTH", &e)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer of a normal size is read whole; one past the callbook limit is refused, not
    /// read to the end.
    #[test]
    fn a_callbook_answer_past_the_limit_is_refused() {
        let normal = "<?xml version=\"1.0\"?>\n<HamQTH version=\"2.7\" xmlns=\"https://www.hamqth.com\">\n<session><session_id>09b0ae90050be03c452ad235a1f2915ad684393c</session_id></session>\n</HamQTH>";
        let huge = vec![b' '; super::super::body::CALLBOOK_REPLY_BYTES as usize + 1];
        let port = super::super::body::serve(vec![
            super::super::body::ok_reply(normal.as_bytes()),
            super::super::body::ok_reply(&huge),
        ]);
        let get =
            || reqwest::blocking::get(format!("http://127.0.0.1:{port}/")).expect("loopback reply");
        assert_eq!(read_body(get()).as_deref(), Ok(normal));
        let err = read_body(get()).unwrap_err();
        assert!(
            err.starts_with("HamQTH: ") && err.contains("larger than"),
            "{err}"
        );
    }

    #[test]
    fn non_https_url_is_rejected_without_leaking_secrets() {
        // `.https_only(true)` rejects http before network I/O, and `redact` must
        // scrub the URL/password/id from the message. A lookup URL carries the
        // session id; a login URL the password — neither may appear.
        let secret = "http://xml.hamqth.example/xml.php?id=SECRETID&callsign=AA7BQ&p=Sup3r";
        let err = fetch(secret).unwrap_err();
        assert!(!err.contains("SECRETID"), "session id leaked: {err}");
        assert!(!err.contains("Sup3r"), "password leaked: {err}");
        assert!(
            !err.contains("xml.hamqth.example"),
            "host/URL leaked: {err}"
        );
        assert!(err.starts_with("HamQTH: "), "unexpected message: {err}");
    }
}
