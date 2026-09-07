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
use std::time::Duration;

const UA: &str = "nexus-propagation/0.1 (+ham radio propagation nowcast)";

/// The whole-request deadline. reqwest 0.12's blocking `ClientBuilder::timeout` is "a
/// timeout for connect, **read** and write operations"
/// (`reqwest-0.12.28/src/blocking/client.rs`), so this bounds the body download too —
/// which is why [`body_text`] has to classify a timeout of its own. Named rather than
/// inlined so the operator-facing message below cannot drift from the real number.
const TIMEOUT_SECS: u64 = 60; // LoTW is a slow queue; a full pull is large.

/// Fetch a LoTW report given a fully-built report URL (which carries the
/// credentials). Returns the raw response body (ADIF on success); the caller
/// validates it with `tempo_core::lotw::is_lotw_adif` before parsing.
///
/// On any failure returns a **redacted** message that never contains the URL,
/// the password, or the raw transport error.
pub fn fetch_report(url: &str) -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(TIMEOUT_SECS))
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
    body_text(resp)
}

/// Read the report body off a response LoTW has already answered 2xx to.
///
/// ⚠️ #266. This used to flatten every failure here into "could not read the response
/// body", which named the *step* and threw away the *category* — the one thing the
/// operator could have acted on. Reaching this point already proves the request got
/// through and LoTW answered 2xx, so the categories mean something different than they do
/// around [`fetch_report`]'s `send()`: a timeout here is not an unreachable server, and
/// telling the operator to "check your network" would send them after something that
/// demonstrably works. What it IS, this code cannot tell — see
/// [`download_timeout_message`], which says so rather than guessing.
///
/// Same redaction discipline as [`redact`] and for the same reason — `reqwest::Error`'s
/// `Display`/`source` can echo the password-bearing URL, so this classifies by boolean
/// predicate and never stringifies the error.
///
/// The only production caller is [`fetch_report`], whose client carries [`TIMEOUT_SECS`];
/// that is where the number in the message comes from.
fn body_text(resp: reqwest::blocking::Response) -> Result<String, String> {
    resp.text().map_err(|e| {
        if e.is_timeout() {
            download_timeout_message()
        } else {
            format!(
                "{} while downloading the report",
                neterr::redact("LoTW", &e)
            )
        }
    })
}

/// What the operator is told when the report did not finish downloading in time.
///
/// ⚠️ It names **two** possibilities and asserts neither, and that is deliberate. The first
/// wording said the cause was "LoTW queueing the report at its end. Try again shortly." — but
/// [`TIMEOUT_SECS`] is a whole-request deadline, so a report that is arriving steadily and is
/// simply too big to finish inside it busts exactly the same deadline and arrives as exactly
/// the same error, and for that operator trying again shortly is the one thing that never
/// helps. Nothing at this point can tell the two apart: `blocking::Response::text` wraps the
/// entire body read in ONE `wait::timeout`, so all that comes back is "the budget ran out"
/// (`reqwest-0.12.28/src/blocking/response.rs:296`).
///
/// Nor is reading the body by hand a way to find out, though it looks like one: `impl Read for
/// Response` re-applies `self.timeout` to EVERY `read` call (same file, :439), so counting
/// bytes as they arrive would replace the whole-request deadline with a per-chunk one and
/// leave a credential-bearing fetch with no overall bound at all. Diagnosing this properly
/// costs the deadline; saying honestly what is known costs nothing.
fn download_timeout_message() -> String {
    format!(
        "LoTW: timed out after {TIMEOUT_SECS}s while downloading the report — LoTW accepted \
         the request and answered, so the connection is fine. Either it is still assembling \
         the report at its end, or the report is too large to finish arriving inside the \
         deadline. Try again; if it keeps timing out, ask for a shorter date range."
    )
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

    /// A loopback server that answers 200 with a `Content-Length` it never satisfies, then
    /// holds the socket open — the exact shape of LoTW answering and then queueing the
    /// report server-side. Plain HTTP: what is under test is the error class reqwest
    /// reports for a stalled body, and reaching it over TLS would need a certificate this
    /// suite has no way to make. No internet is involved.
    fn stalled_body_server() -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = l.local_addr().expect("addr").port();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let _ = s.set_read_timeout(Some(Duration::from_millis(500)));
                let mut buf = [0u8; 1024];
                let _ = s.read(&mut buf);
                // Headers only. The body the length promises never comes.
                let _ = s.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 4096\r\n\r\n");
                let _ = s.flush();
                std::thread::sleep(Duration::from_millis(1500));
            }
        });
        port
    }

    /// #266 follow-up. The message must not name a cause it cannot tell apart.
    ///
    /// A report LoTW has not begun sending and a report that is arriving too slowly bust the
    /// SAME whole-request deadline and arrive as the same `reqwest` error class, so the arm
    /// cannot know which it has. The first wording asserted the first one — "which is LoTW
    /// queueing the report at its end. Try again shortly." — and for the second, retrying is
    /// exactly what does not help.
    ///
    /// Coupled to the wording on purpose: the wording IS the defect here, so this is what
    /// stops it regressing quietly.
    #[test]
    fn the_download_timeout_does_not_diagnose_a_cause_it_cannot_see() {
        let m = download_timeout_message();
        // Controls: it must still say what happened and how long it waited, or "stop naming a
        // cause" would be satisfied by saying nothing.
        assert!(
            m.contains("timed out"),
            "it must still name the failure: {m}"
        );
        assert!(
            m.contains(&TIMEOUT_SECS.to_string()),
            "it must still name the deadline: {m}"
        );
        // Both possibilities named, neither asserted.
        assert!(
            m.contains("still assembling"),
            "the queued-report case must still be offered: {m}"
        );
        assert!(
            m.contains("too large"),
            "the slow-download case is the one the old wording denied: {m}"
        );
        // …and an action for the case where trying again never helps.
        assert!(
            m.contains("date range"),
            "an operator whose pull is simply too big is told nothing to do: {m}"
        );
    }

    #[test]
    fn a_stall_while_downloading_the_report_is_classified_as_a_timeout() {
        // #266: the reporter's "LoTW: could not read the response body" proves LoTW
        // answered 2xx and then the BODY read failed — and the old wording threw the
        // category away, so a 60 s stall and a dropped stream read identically.
        let port = stalled_body_server();
        let resp = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(400))
            .build()
            .expect("client")
            .get(format!(
                "http://127.0.0.1:{port}/lotwuser/lotwreport.adi?login=ke3z&password=Sup3rSecret"
            ))
            .send()
            // The positive control for the whole test: if this failed, the stall would be
            // in the HEADERS and the body-read path under test would never be reached.
            .expect("headers must arrive — only the body stalls");
        assert!(resp.status().is_success(), "the stall must follow a 2xx");

        let err = body_text(resp).unwrap_err();
        assert!(
            err.contains("timed out"),
            "a stalled download must be reported as a timeout, not as an unreadable body: {err}"
        );
        // Same redaction invariant as `send()`: the URL carries the password.
        assert!(!err.contains("Sup3rSecret"), "password leaked: {err}");
        assert!(!err.contains("password"), "param name leaked: {err}");
        assert!(!err.contains("127.0.0.1"), "host leaked: {err}");
        assert!(err.starts_with("LoTW: "), "unexpected message: {err}");
    }
}
