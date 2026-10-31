//! callook.info JSON transport (the `live` feature) — a plain HTTPS GET for one US call, the
//! third callbook after [`qrz`](super::qrz) and [`hamqth`](super::hamqth).
//!
//! All callook knowledge (URL shape, which calls to ask, JSON parsing, its terms) lives in the
//! pure [`tempo_core::callook`]; this module just moves bytes. The request carries no secret —
//! only the call — but it keeps the other callbook transports' discipline anyway: HTTPS only,
//! no redirects, and category-only errors.

use super::neterr;
use std::time::Duration;

const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

/// Fetch a callook.info lookup URL (built by `tempo_core::callook::build_lookup_url`) and return
/// the raw JSON body; the caller parses it with `tempo_core::callook::parse_callsign`.
pub fn fetch(url: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
        .user_agent(UA)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "callook.info: HTTP client initialization failed".to_string())?;
    let resp = client
        .get(url)
        .send()
        .map_err(|e| neterr::redact("callook.info", &e))?;
    read_body(resp)
}

fn read_body(resp: reqwest::blocking::Response) -> Result<String, String> {
    let status = resp.status();
    if !status.is_success() {
        return Err(format!(
            "callook.info: server returned HTTP {}",
            status.as_u16()
        ));
    }
    super::body::read_capped(resp, super::body::CALLBOOK_REPLY_BYTES, "callook.info")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An answer of a normal size is read whole; one past the callbook limit is refused, not
    /// read to the end.
    #[test]
    fn a_callbook_answer_past_the_limit_is_refused() {
        let normal = r#"{"status":"VALID","type":"PERSON","current":{"callsign":"W1AW","operClass":""},"name":"ARRL HQ OPERATORS CLUB"}"#;
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
            err.starts_with("callook.info: ") && err.contains("larger than"),
            "{err}"
        );
    }

    #[test]
    fn non_https_url_is_rejected_without_naming_the_url() {
        let err = fetch("http://callook.example/W1AW/json").unwrap_err();
        assert!(!err.contains("callook.example"), "URL leaked: {err}");
        assert!(
            err.starts_with("callook.info: "),
            "unexpected message: {err}"
        );
    }
}
