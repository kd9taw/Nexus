//! QRZ XML transport (the `live` feature) — a thin authenticated HTTPS GET, used
//! for BOTH steps of the session-key flow (login → key, then lookup).
//!
//! All QRZ knowledge (URL shape, XML parsing, session handling) lives in the pure
//! [`tempo_core::qrz`]; this module just moves bytes. The orchestration (cache the
//! session key, re-login on expiry) lives in the shell.
//!
//! ⚠️ BOTH request URLs are secret-bearing — the login URL carries the password
//! and the lookup URL carries the session key — so this is as strict as the
//! LoTW/eQSL transports: HTTPS enforced, no redirect-following, and **redacted
//! errors** (a `reqwest::Error`'s `Display`/`source` can echo the request URL, so
//! we never stringify it — only fixed, category-based messages).

use super::neterr;
use std::time::Duration;

const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

/// Fetch a QRZ XML URL (login or lookup, both built by [`tempo_core::qrz`]).
/// Returns the raw XML body; the caller validates with `qrz::is_qrz_xml` and
/// parses with `qrz::parse_session`/`parse_callsign`.
///
/// On any failure returns a **redacted** message that never contains the URL, the
/// password, the session key, or the raw transport error.
pub fn fetch(url: &str) -> Result<String, String> {
    let resp = client()?.get(url).send().map_err(redact)?;
    read_body(resp)
}

/// POST a `name=value` form body — the QRZ Logbook push (`ACTION=INSERT`). Same
/// HTTPS + redacted discipline as [`fetch`]. ⚠️ The `body` carries the per-logbook
/// API key, so it must NEVER be logged; transport errors echo the URL (no secret)
/// only, and are redacted regardless.
pub fn post_form(url: &str, body: String) -> Result<String, String> {
    let resp = client()?
        .post(url)
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .send()
        .map_err(redact)?;
    read_body(resp)
}

/// The live QRZ Logbook API for the operator-invoked **"correct at QRZ"** action.
///
/// It exists so that the pure orchestration in [`tempo_core::qrz_correct`] can name the four
/// calls it is allowed to make instead of posting an arbitrary body: a STATUS, a SINGLE-CALLSIGN
/// FETCH, an INSERT carrying `OPTION=REPLACE`, and a SINGLE-RECORD DELETE. Two of those are new
/// to Nexus's QRZ surface, and they are the read-and-undo half of the feature — they exist
/// BEFORE the first replace is ever sent, not after.
///
/// ⚠️ **The DELETE is destructive at a third party and has no undo** ("Deleted records cannot be
/// recovered" — QRZ's own spec). It is reachable only from the miss-recovery path, and that is
/// enforced by type, not by prose: [`delete_missed_replace`](QrzLogbook::delete_missed_replace)
/// takes a [`tempo_core::qrz::QrzMissRecovery`], whose only constructor is a `RESULT=OK` answer
/// to a replace — QRZ saying its matcher missed and it inserted a duplicate. The value also
/// carries the record's operator-readable name, so nothing can delete a record it cannot name;
/// the name is what the caller logs.
///
/// Every body carries the per-logbook API key, so the same rule as [`post_form`] holds: never
/// log a body, and let [`redact`] own the error text.
pub struct LogbookApi;

impl tempo_core::qrz_correct::QrzLogbook for LogbookApi {
    fn status(&self, api_key: &str) -> Result<String, String> {
        post_form(
            tempo_core::qrz::QRZ_LOGBOOK_URL,
            tempo_core::qrz::build_status_body(api_key),
        )
    }

    fn fetch_callsign(&self, api_key: &str, callsign: &str) -> Result<String, String> {
        post_form(
            tempo_core::qrz::QRZ_LOGBOOK_URL,
            tempo_core::qrz::build_fetch_call_body(api_key, callsign),
        )
    }

    fn replace(&self, api_key: &str, adif: &str) -> Result<String, String> {
        post_form(
            tempo_core::qrz::QRZ_LOGBOOK_URL,
            tempo_core::qrz::build_insert_body(api_key, adif, true),
        )
    }

    fn delete_missed_replace(
        &self,
        api_key: &str,
        recovery: &tempo_core::qrz::QrzMissRecovery,
    ) -> Result<String, String> {
        post_form(
            tempo_core::qrz::QRZ_LOGBOOK_URL,
            tempo_core::qrz::build_delete_body(api_key, recovery),
        )
    }
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(UA)
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "QRZ: HTTP client initialization failed".to_string())
}

fn read_body(resp: reqwest::blocking::Response) -> Result<String, String> {
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("QRZ: server returned HTTP {}", status.as_u16()));
    }
    resp.text()
        .map_err(|_| "QRZ: could not read the response body".to_string())
}

/// Map a transport error to a safe, category-only message. Uses ONLY boolean
/// predicates — never `Display`/`to_string`/`source`, any of which can leak the
/// password- or key-bearing URL.
fn redact(e: reqwest::Error) -> String {
    neterr::redact("QRZ", &e)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn non_https_url_is_rejected_without_leaking_secrets() {
        // `.https_only(true)` rejects http before network I/O, and `redact` must
        // scrub the URL/password/key from the message. A lookup URL carries the
        // session key; a login URL the password — neither may appear.
        let secret =
            "http://xmldata.qrz.example/xml/current/?s=SECRETKEY&callsign=AA7BQ&password=Sup3r";
        let err = fetch(secret).unwrap_err();
        assert!(!err.contains("SECRETKEY"), "session key leaked: {err}");
        assert!(!err.contains("Sup3r"), "password leaked: {err}");
        assert!(
            !err.contains("xmldata.qrz.example"),
            "host/URL leaked: {err}"
        );
        assert!(err.starts_with("QRZ: "), "unexpected message: {err}");
    }

    #[test]
    fn post_form_rejects_http_without_leaking_the_api_key() {
        // The POST body carries the per-logbook API key; a redacted error must not
        // surface it (nor the URL).
        let err = post_form(
            "http://logbook.qrz.example/api",
            "KEY=SECRETKEY-1234&ACTION=INSERT&ADIF=%3Ceor%3E".to_string(),
        )
        .unwrap_err();
        assert!(!err.contains("SECRETKEY-1234"), "API key leaked: {err}");
        assert!(
            !err.contains("logbook.qrz.example"),
            "host/URL leaked: {err}"
        );
        assert!(err.starts_with("QRZ: "), "unexpected message: {err}");
    }

    #[test]
    fn a_delete_body_never_reaches_an_error_message() {
        // The DELETE is the destructive call, so its body gets the same proof the others do.
        // No network: `.https_only(true)` refuses an http URL before any I/O, which is how
        // every test on this transport stays offline.
        let recovery = tempo_core::qrz::QrzMissRecovery::from_missed_replace(
            &tempo_core::qrz::QrzPush {
                result: tempo_core::qrz::QrzPushResult::Ok,
                logid: Some("130877825".into()),
                count: 1,
                reason: None,
            },
            "W1AW on 2024-03-01 at 1432Z",
        )
        .expect("a missed replace authorises the delete");
        let body = tempo_core::qrz::build_delete_body("SECRETKEY-1234", &recovery);
        // POSITIVE CONTROL: the body really does carry the key, so a clean error below is the
        // redaction working rather than a fixture with nothing in it.
        assert!(body.contains("SECRETKEY-1234"), "{body}");
        let err = post_form("http://logbook.qrz.example/api", body).unwrap_err();
        assert!(!err.contains("SECRETKEY-1234"), "API key leaked: {err}");
        assert!(!err.contains("130877825"), "log id leaked: {err}");
        assert!(err.starts_with("QRZ: "), "unexpected message: {err}");
    }
}
