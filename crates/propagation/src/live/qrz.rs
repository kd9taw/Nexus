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

/// Fetch a QRZ XML URL (login or lookup, both built by [`tempo_core::qrz`]).
/// Returns the raw XML body; the caller validates with `qrz::is_qrz_xml` and
/// parses with `qrz::parse_session`/`parse_callsign`.
///
/// On any failure returns a **redacted** message that never contains the URL, the
/// password, the session key, or the raw transport error. `app_version` names the Nexus build in
/// the User-Agent ([`user_agent`]).
pub fn fetch(url: &str, app_version: &str) -> Result<String, String> {
    let client = client()?;
    let request = get_request(&client, url, app_version)?;
    read_body(client.execute(request).map_err(redact)?)
}

/// POST a `name=value` form body — the QRZ Logbook push (`ACTION=INSERT`). Same
/// HTTPS + redacted discipline as [`fetch`]. ⚠️ The `body` carries the per-logbook
/// API key, so it must NEVER be logged; transport errors echo the URL (no secret)
/// only, and are redacted regardless.
pub fn post_form(url: &str, app_version: &str, body: String) -> Result<String, String> {
    let client = client()?;
    let request = post_request(&client, url, app_version, body)?;
    read_body(client.execute(request).map_err(redact)?)
}

/// What Nexus calls itself to QRZ: `Nexus/<version>`, the app's own name and version and nothing
/// else. QRZ asks every application for an identifiable User-Agent and may restrict a generic one;
/// its suggested form adds the user's callsign, which Nexus does not send.
fn user_agent(app_version: &str) -> String {
    super::hrdlog::product(app_version)
}

/// The lookup or login request, built and not sent, so its User-Agent is testable offline. Set on
/// the request itself: a client's default headers are only merged in when it executes.
fn get_request(
    client: &reqwest::blocking::Client,
    url: &str,
    app_version: &str,
) -> Result<reqwest::blocking::Request, String> {
    client
        .get(url)
        .header(reqwest::header::USER_AGENT, user_agent(app_version))
        .build()
        .map_err(redact)
}

/// The Logbook form post, built and not sent, as [`get_request`]. The body carries the API key.
fn post_request(
    client: &reqwest::blocking::Client,
    url: &str,
    app_version: &str,
    body: String,
) -> Result<reqwest::blocking::Request, String> {
    client
        .post(url)
        .header(reqwest::header::USER_AGENT, user_agent(app_version))
        .header(
            reqwest::header::CONTENT_TYPE,
            "application/x-www-form-urlencoded",
        )
        .body(body)
        .build()
        .map_err(redact)
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
pub struct LogbookApi {
    /// The Nexus build, for the User-Agent ([`user_agent`]).
    pub app_version: &'static str,
}

impl tempo_core::qrz_correct::QrzLogbook for LogbookApi {
    fn status(&self, api_key: &str) -> Result<String, String> {
        post_form(
            tempo_core::qrz::QRZ_LOGBOOK_URL,
            self.app_version,
            tempo_core::qrz::build_status_body(api_key),
        )
    }

    fn fetch_callsign(&self, api_key: &str, callsign: &str) -> Result<String, String> {
        post_form(
            tempo_core::qrz::QRZ_LOGBOOK_URL,
            self.app_version,
            tempo_core::qrz::build_fetch_call_body(api_key, callsign),
        )
    }

    fn replace(&self, api_key: &str, adif: &str) -> Result<String, String> {
        post_form(
            tempo_core::qrz::QRZ_LOGBOOK_URL,
            self.app_version,
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
            self.app_version,
            tempo_core::qrz::build_delete_body(api_key, recovery),
        )
    }
}

fn client() -> Result<reqwest::blocking::Client, String> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
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
    super::body::read_capped(resp, super::body::CALLBOOK_REPLY_BYTES, "QRZ")
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

    /// An answer of a normal size is read whole; one past the callbook limit is refused, not
    /// read to the end.
    #[test]
    fn a_callbook_answer_past_the_limit_is_refused() {
        let normal = tempo_core::qrz::fixtures::LOOKUP_FULL;
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
            err.starts_with("QRZ: ") && err.contains("larger than"),
            "{err}"
        );
    }

    /// Every QRZ request, the callbook lookup and login and each Logbook post, names Nexus and its
    /// version, and nothing that identifies the operator.
    #[test]
    fn every_qrz_request_names_nexus_and_its_version_only() {
        let client = client().expect("client");
        let lookup = get_request(
            &client,
            "https://xmldata.qrz.com/xml/current/?s=KEY&callsign=AA7BQ",
            "1.19.0",
        )
        .expect("request");
        let post = post_request(
            &client,
            tempo_core::qrz::QRZ_LOGBOOK_URL,
            "1.19.0",
            "KEY=K&ACTION=STATUS".into(),
        )
        .expect("request");
        for request in [&lookup, &post] {
            let agents: Vec<_> = request
                .headers()
                .get_all(reqwest::header::USER_AGENT)
                .iter()
                .collect();
            assert_eq!(agents, ["Nexus/1.19.0"], "{}", request.method());
        }
        // A version can never end the header early.
        assert_eq!(user_agent("1.19.0\r\nX-Evil: 1"), "Nexus/1.19.0X-Evil1");
    }

    #[test]
    fn non_https_url_is_rejected_without_leaking_secrets() {
        // `.https_only(true)` rejects http before network I/O, and `redact` must
        // scrub the URL/password/key from the message. A lookup URL carries the
        // session key; a login URL the password — neither may appear.
        let secret =
            "http://xmldata.qrz.example/xml/current/?s=SECRETKEY&callsign=AA7BQ&password=Sup3r";
        let err = fetch(secret, "1.19.0").unwrap_err();
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
            "1.19.0",
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
        let err = post_form("http://logbook.qrz.example/api", "1.19.0", body).unwrap_err();
        assert!(!err.contains("SECRETKEY-1234"), "API key leaked: {err}");
        assert!(!err.contains("130877825"), "log id leaked: {err}");
        assert!(err.starts_with("QRZ: "), "unexpected message: {err}");
    }
}
