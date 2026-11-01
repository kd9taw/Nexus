//! Pure ClubLog realtime-push helpers — the offline, unit-testable core of the
//! ClubLog QSO-upload connector (mirrors the LoTW/eQSL/QRZ pure modules). No I/O.
//!
//! ClubLog's `realtime.php` takes FOUR credential params + one ADIF record and
//! signals the outcome via the **HTTP status code** (with a plain-text body
//! message). This module builds the form body and classifies a (status, body)
//! pair; the thin POST transport lives behind the `live` feature elsewhere.
//!
//! ⚠️ The body carries the app-password AND the developer api key — both secret —
//! so [`ClubLogQuery`]'s `Debug` redacts them and the transport must redact errors.

/// The ClubLog realtime per-QSO upload endpoint. Hard-coded https constant.
pub const CLUBLOG_REALTIME_URL: &str = "https://clublog.org/realtime.php";

/// Inputs for one realtime push. `email`/`callsign`/`api_key` identify the
/// account+logbook+app; `adif` is exactly one record ending in `<eor>`. `Debug`
/// redacts the password and api key.
#[derive(Clone)]
pub struct ClubLogQuery {
    /// ClubLog account email (NOT a callsign).
    pub email: String,
    /// A ClubLog **Application Password** (not the main account password).
    pub password: String,
    /// The own logbook callsign to upload into.
    pub callsign: String,
    /// The per-developer app API key.
    pub api_key: String,
    /// Exactly one ADIF record ending in `<eor>`.
    pub adif: String,
}

impl std::fmt::Debug for ClubLogQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClubLogQuery")
            .field("email", &self.email)
            .field("password", &"<redacted>")
            .field("callsign", &self.callsign)
            .field("api_key", &"<redacted>")
            .field("adif", &self.adif)
            .finish()
    }
}

/// Outcome of a realtime push, derived from the HTTP status (+ body keyword).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClubLogResult {
    /// 200 "QSO OK" — accepted.
    Ok,
    /// 200 "QSO Modified" — accepted, ClubLog adjusted it.
    Modified,
    /// 200 "QSO Duplicate" — already known; idempotent/benign.
    Duplicate,
    /// 400 "QSO Rejected" — bad ADIF/QSO; don't blindly retry.
    Rejected,
    /// 403 Access denied — bad credentials/prereqs; STOP (IP-block risk).
    AuthFail,
    /// 500 — transient (parser/maintenance); retry later.
    ServerError,
    /// Any other status.
    Unknown,
}

impl ClubLogResult {
    /// Map a ClubLog push outcome to the generic per-QSO [`UploadOutcome`] for the
    /// logbook's `upload.clublog` cursor. `None` for the transient cases
    /// (`ServerError`/`Unknown`) so the QSO stays unstamped for a clean retry,
    /// matching the LoTW network-error convention; `AuthFail`/`Rejected` are bounces
    /// the diagnostics surface as R9.
    pub fn to_upload_outcome(self) -> Option<crate::logbook::UploadOutcome> {
        use crate::logbook::UploadOutcome as U;
        match self {
            ClubLogResult::Ok | ClubLogResult::Modified => Some(U::Accepted),
            ClubLogResult::Duplicate => Some(U::Duplicate),
            ClubLogResult::Rejected => Some(U::Rejected),
            ClubLogResult::AuthFail => Some(U::AuthFail),
            ClubLogResult::ServerError | ClubLogResult::Unknown => None,
        }
    }

    /// WHY, for the per-QSO stamp — the CLASS, off the HTTP status line.
    ///
    /// ⛔ Deliberately not [`ClubLogPush::message`]. That is ClubLog's own body, ClubLog
    /// echoes the failing request back, and the request body carries the app-password and
    /// the API key — and this value is written into `log.adi`, which is signed by TQSL and
    /// uploaded to ARRL. See [`crate::logbook::UploadDetail`]. The body still reaches the
    /// operator through the connection log and the toast, and dies with the session.
    pub fn to_upload_detail(self) -> Option<crate::logbook::UploadDetail> {
        use crate::logbook::UploadDetail as D;
        match self {
            ClubLogResult::Ok | ClubLogResult::Modified | ClubLogResult::Duplicate => None,
            ClubLogResult::Rejected => Some(D::RecordRefused),
            ClubLogResult::AuthFail => Some(D::Credentials),
            // Transient — `to_upload_outcome` stamps nothing, so there is nothing to explain.
            ClubLogResult::ServerError | ClubLogResult::Unknown => None,
        }
    }
}

/// A classified realtime response.
#[derive(Debug, Clone, PartialEq)]
pub struct ClubLogPush {
    pub result: ClubLogResult,
    /// The server's plain-text body message, if any.
    pub message: Option<String>,
}

/// Percent-encode a form value (RFC 3986 unreserved set). Same encoder as the
/// sibling connectors; kept local. Encodes `&`/`=`/space/`<`/`>` so the
/// password/key/ADIF can't break the form body.
fn pct(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

/// Build the `application/x-www-form-urlencoded` body for a realtime push. Carries
/// the app-password + api key — never log it.
pub fn build_realtime_body(q: &ClubLogQuery) -> String {
    format!(
        "email={}&password={}&callsign={}&api={}&adif={}",
        pct(&q.email),
        pct(&q.password),
        pct(&q.callsign),
        pct(&q.api_key),
        pct(&q.adif),
    )
}

/// Classify a realtime response by its HTTP status (+ the 200 body keyword). The
/// status IS the result; the body just disambiguates the three 200 variants.
pub fn classify_response(status: u16, body: &str) -> ClubLogPush {
    let msg = body.trim();
    let lower = msg.to_ascii_lowercase();
    let result = match status {
        200 => {
            if lower.contains("duplicate") {
                ClubLogResult::Duplicate
            } else if lower.contains("modified") {
                ClubLogResult::Modified
            } else {
                ClubLogResult::Ok
            }
        }
        400 => ClubLogResult::Rejected,
        403 => ClubLogResult::AuthFail,
        500..=599 => ClubLogResult::ServerError,
        _ => ClubLogResult::Unknown,
    };
    ClubLogPush {
        result,
        message: if msg.is_empty() {
            None
        } else {
            Some(msg.to_string())
        },
    }
}

// ----- Settings ▸ Test: a read-only sign-in ----------------------------------

/// ClubLog's log-matches endpoint (GET). Hard-coded https constant, as the realtime one.
pub const CLUBLOG_MATCHES_URL: &str = "https://clublog.org/getmatches.php";

/// Build the URL of a Test of the ClubLog credentials: `getmatches.php`, signed in with the
/// four values a realtime upload sends (email, Application Password, callsign, application key),
/// asked for the matches made from 31 December 2078.
///
/// ClubLog's "Checking for Log Matches" page documents it as a GET of `api`, `email`,
/// `password` (the Application Password), `callsign` ("it must be owned by the account") and an
/// optional `startyear`/`startmonth`/`startday` (the date ClubLog made the match), answering a
/// JSON array. So it proves what an upload needs — the login and that the callsign is the
/// account's — and reads only: no `adif`, nothing stored. ClubLog documents no read-only "login"
/// call. The returned URL carries the password and the key — never log it.
pub fn build_login_test_url(email: &str, password: &str, callsign: &str, api_key: &str) -> String {
    format!(
        "{CLUBLOG_MATCHES_URL}?api={}&email={}&password={}&callsign={}\
         &startyear=2078&startmonth=12&startday=31",
        pct(api_key.trim()),
        pct(email.trim()),
        pct(password),
        pct(callsign.trim()),
    )
}

/// What ClubLog's answer (`status`, `body`) to [`build_login_test_url`] says about the login:
/// a JSON array is a good one; a 403 is ClubLog's refusal ("Authentication failed", in the
/// realtime page's words for the same status), in its words when it gave any. ClubLog's words
/// may echo the request, so they come back whole: the caller masks the secrets in all of them
/// first, and only then cuts them to length (`login_check::capped`).
pub fn classify_login_test(status: u16, body: &str) -> crate::login_check::LoginCheck {
    use crate::login_check::LoginCheck;
    let words = Some(body.trim().to_string()).filter(|w| !w.is_empty());
    match status {
        200 if body.trim_start().starts_with('[') => LoginCheck::Accepted,
        403 => LoginCheck::Refused(words.unwrap_or_else(|| "HTTP 403 Forbidden".to_string())),
        _ => LoginCheck::Unrecognised(words),
    }
}

#[cfg(test)]
mod login_test_tests {
    use super::*;
    use crate::login_check::LoginCheck;

    /// The Test carries the four values the upload sends, to the read-only matches endpoint,
    /// and no record.
    #[test]
    fn the_test_signs_in_to_the_matches_endpoint_with_the_uploads_credentials() {
        let url = build_login_test_url(" a@b.com ", "app pw&1", " W9TST ", "DEVKEY");
        assert_eq!(
            url,
            "https://clublog.org/getmatches.php?api=DEVKEY&email=a%40b.com&password=app%20pw%261\
             &callsign=W9TST&startyear=2078&startmonth=12&startday=31"
        );
        assert!(!url.contains("realtime.php") && !url.contains("putlogs"));
        assert!(!url.contains("adif"), "no record goes with a Test");
        // Control: the upload's body does carry one, so the check above can fail.
        let upload = build_realtime_body(&ClubLogQuery {
            email: "a@b.com".into(),
            password: "app pw&1".into(),
            callsign: "W9TST".into(),
            api_key: "DEVKEY".into(),
            adif: "<call:4>W1AW<eor>".into(),
        });
        assert!(upload.contains("adif="));
    }

    #[test]
    fn the_test_reads_the_status_clublog_answers() {
        assert_eq!(classify_login_test(200, "[]"), LoginCheck::Accepted);
        assert_eq!(
            classify_login_test(200, " [[\"W1AW\",291,\"2026-01-02\",20,\"FT8\"]]"),
            LoginCheck::Accepted
        );
        assert_eq!(
            classify_login_test(403, "Invalid login\n"),
            LoginCheck::Refused("Invalid login".into())
        );
        assert_eq!(
            classify_login_test(403, ""),
            LoginCheck::Refused("HTTP 403 Forbidden".into())
        );
        // Controls: a 200 that is not the array, and any other status, are not called a good
        // login nor a bad one.
        assert_eq!(
            classify_login_test(200, "Maintenance"),
            LoginCheck::Unrecognised(Some("Maintenance".into()))
        );
        assert_eq!(classify_login_test(500, ""), LoginCheck::Unrecognised(None));
    }

    /// A long answer comes back whole, so the caller can mask a secret anywhere in it before
    /// cutting it.
    #[test]
    fn the_test_passes_clublogs_words_on_uncut() {
        let long = format!("Invalid login {}", "x".repeat(1000));
        assert_eq!(
            classify_login_test(403, &long),
            LoginCheck::Refused(long.clone())
        );
        assert_eq!(
            classify_login_test(200, &long),
            LoginCheck::Unrecognised(Some(long))
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q() -> ClubLogQuery {
        ClubLogQuery {
            email: "a@b.com".into(),
            password: "app pw&1".into(),
            callsign: "KD9TAW".into(),
            api_key: "DEVKEY".into(),
            adif: "<call:4>W1AW<eor>".into(),
        }
    }

    #[test]
    fn body_has_all_params_encoded() {
        let body = build_realtime_body(&q());
        assert!(body.contains("email=a%40b.com"));
        assert!(body.contains("password=app%20pw%261")); // space + & encoded
        assert!(body.contains("callsign=KD9TAW"));
        assert!(body.contains("api=DEVKEY"));
        assert!(body.contains("adif=%3Ccall%3A4%3EW1AW%3Ceor%3E")); // ADIF tags encoded
        assert!(!body.contains("app pw&1"));
    }

    #[test]
    fn debug_redacts_password_and_api_key() {
        let dbg = format!("{:?}", q());
        assert!(dbg.contains("<redacted>"));
        assert!(!dbg.contains("app pw&1"));
        assert!(!dbg.contains("DEVKEY"));
    }

    #[test]
    fn classify_200_variants() {
        assert_eq!(classify_response(200, "QSO OK").result, ClubLogResult::Ok);
        assert_eq!(
            classify_response(200, "QSO Modified").result,
            ClubLogResult::Modified
        );
        assert_eq!(
            classify_response(200, "QSO Duplicate").result,
            ClubLogResult::Duplicate
        );
    }

    #[test]
    fn classify_error_statuses() {
        assert_eq!(
            classify_response(400, "QSO Rejected").result,
            ClubLogResult::Rejected
        );
        assert_eq!(
            classify_response(403, "Access denied").result,
            ClubLogResult::AuthFail
        );
        assert_eq!(
            classify_response(500, "oops").result,
            ClubLogResult::ServerError
        );
        assert_eq!(classify_response(418, "?").result, ClubLogResult::Unknown);
    }

    #[test]
    fn classify_keeps_message() {
        assert_eq!(
            classify_response(403, "Access denied").message.as_deref(),
            Some("Access denied")
        );
        assert!(classify_response(200, "   ").message.is_none());
    }

    #[test]
    fn result_maps_to_upload_outcome() {
        use crate::logbook::UploadOutcome as U;
        assert_eq!(ClubLogResult::Ok.to_upload_outcome(), Some(U::Accepted));
        assert_eq!(
            ClubLogResult::Modified.to_upload_outcome(),
            Some(U::Accepted)
        );
        assert_eq!(
            ClubLogResult::Duplicate.to_upload_outcome(),
            Some(U::Duplicate)
        );
        assert_eq!(
            ClubLogResult::Rejected.to_upload_outcome(),
            Some(U::Rejected)
        );
        assert_eq!(
            ClubLogResult::AuthFail.to_upload_outcome(),
            Some(U::AuthFail)
        );
        // Transient → no stamp (clean retry).
        assert_eq!(ClubLogResult::ServerError.to_upload_outcome(), None);
        assert_eq!(ClubLogResult::Unknown.to_upload_outcome(), None);
    }
}
