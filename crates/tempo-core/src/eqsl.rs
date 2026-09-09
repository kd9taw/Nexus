//! Pure eQSL.cc InBox-download helpers — the offline, unit-testable core of the
//! eQSL confirmation sync (mirrors [`crate::lotw`]). No I/O.
//!
//! eQSL's `DownloadInBox.cfm` is a **two-step** flow: step 1 GET returns an HTML
//! page (marker "Your ADIF log file has been built") containing a link to an
//! ephemeral `.adi` file; step 2 GETs that file. This module builds the step-1
//! URL, validates the built-page, extracts + https-normalizes the `.adi` link,
//! validates the final ADIF and checks it is complete, and formats the
//! incremental cursor. The thin two-GET transport lives behind the `live`
//! feature elsewhere; the downloaded ADIF carries `EQSL_QSL_RCVD` (not
//! `QSL_RCVD`), so the existing source-aware [`crate::reconcile`] lands it
//! confirmed-but-NOT-award by construction — eQSL never credits ARRL DXCC/WAS.

/// The eQSL InBox download endpoint. Host + scheme are a hard-coded https
/// constant (never caller-supplied) so the password-bearing query string can only
/// ever go to eQSL over TLS.
pub const EQSL_INBOX_URL: &str = "https://www.eqsl.cc/qslcard/DownloadInBox.cfm";

/// Step-1 success marker (matched case-insensitively).
const BUILT_MARKER: &str = "your adif log file has been built";

/// Inputs for an InBox download. The fixed nature of the request lives in
/// [`build_inbox_url`]; only the operator-specific bits vary. `Debug` is manual to
/// **redact the password** — it must never reach a log, error string, or the UI.
#[derive(Clone)]
pub struct EqslQuery {
    /// eQSL account username (callsign or the account login).
    pub username: String,
    /// eQSL account password.
    pub password: String,
    /// Incremental cursor `RcvdSince=YYYYMMDDHHMM`. `None`/empty → full InBox.
    pub rcvd_since: Option<String>,
    /// The account's QTH Nickname. eQSL's own spec: "if not logged in, if multiple
    /// accounts with same callsign" the `QTHNickname` parameter disambiguates —
    /// without it, such an account's fresh (cookieless, which our transport always
    /// is) authentication simply fails. Field report, 2026-09-01. Not a secret;
    /// `None`/empty omits the parameter and single-profile accounts behave as before.
    pub qth_nickname: Option<String>,
}

impl std::fmt::Debug for EqslQuery {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EqslQuery")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .field("rcvd_since", &self.rcvd_since)
            .field("qth_nickname", &self.qth_nickname)
            .finish()
    }
}

/// Percent-encode a query value per RFC 3986 (keep only the unreserved set). Same
/// encoder as [`crate::lotw`]; kept local to avoid cross-module coupling. Byte-wise
/// (UTF-8 safe); critically encodes `&`, `=`, `?`, space in passwords/usernames.
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

/// Build the step-1 InBox URL. Always sends credentials (the cookie trap means a
/// stale session cookie would otherwise override them — the transport uses a fresh
/// cookieless client). The URL contains the password (encoded) — treat as secret.
pub fn build_inbox_url(q: &EqslQuery) -> String {
    let mut url = format!(
        "{EQSL_INBOX_URL}?UserName={}&Password={}",
        pct(&q.username),
        pct(&q.password),
    );
    if let Some(nick) = q.qth_nickname.as_deref() {
        let nick = nick.trim();
        if !nick.is_empty() {
            url.push_str("&QTHNickname=");
            url.push_str(&pct(nick));
        }
    }
    if let Some(since) = q.rcvd_since.as_deref() {
        let since = since.trim();
        if !since.is_empty() {
            url.push_str("&RcvdSince=");
            url.push_str(&pct(since));
        }
    }
    url
}

/// True iff the step-1 HTML reports the ADIF file was built (vs a bad-creds/error
/// page). Gate on this before looking for the download link.
pub fn is_built_page(html: &str) -> bool {
    html.to_ascii_lowercase().contains(BUILT_MARKER)
}

/// Extract the `.adi` download href from the built-page HTML. eQSL serves two
/// links (`.adi` + identical `.txt`); we pick the `.adi`. Returns the raw href
/// (original case) for [`resolve_download_url`]. Robust to single/double quotes;
/// the built page is simple enough not to need a full HTML parser.
pub fn extract_adi_href(html: &str) -> Option<String> {
    for quote in ['"', '\''] {
        for (i, seg) in html.split(quote).enumerate() {
            // Quoted attribute values are the odd-indexed segments.
            if i % 2 == 1 {
                let v = seg.trim();
                if v.to_ascii_lowercase().ends_with(".adi") {
                    return Some(v.to_string());
                }
            }
        }
    }
    None
}

/// Resolve a download `href` against the step-1 `request_url`, **force https**, and
/// **pin the host to eqsl.cc**. Handles eQSL's historical forms: absolute
/// `http://`/`https://`, protocol-relative `//host/...`, root-relative `/...`, and
/// relative `../downloadedfiles/...` (the 2019 move put DownloadedFiles above
/// QSLCard). Forcing https avoids the transport's `https_only` rejecting a
/// legitimate absolute-`http` href; pinning the host keeps step 2 inside the same
/// "only ever talk to eqsl.cc" guarantee step 1 maintains (defense-in-depth — a
/// hostile/MITM'd built-page can't redirect the fetch to an attacker host serving
/// forged confirmations). Returns `None` for a cross-host href.
pub fn resolve_download_url(request_url: &str, href: &str) -> Option<String> {
    const HOST: &str = "https://www.eqsl.cc";
    let h = href.trim();
    let joined = if h.starts_with("https://") {
        h.to_string()
    } else if let Some(r) = h.strip_prefix("http://") {
        format!("https://{r}")
    } else if let Some(r) = h.strip_prefix("//") {
        format!("https://{r}")
    } else if h.starts_with('/') {
        format!("{HOST}{h}")
    } else {
        let base = request_url.split('?').next().unwrap_or(request_url);
        let dir = base.rsplit_once('/').map(|(d, _)| d).unwrap_or(HOST);
        resolve_relative(dir, h)
    };
    let resolved = force_https(&joined);
    if is_eqsl_host(&resolved) {
        Some(resolved)
    } else {
        None
    }
}

/// True iff `url`'s host is `eqsl.cc` or a `*.eqsl.cc` subdomain.
fn is_eqsl_host(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or(rest);
    let host = authority
        .rsplit('@')
        .next()
        .unwrap_or(authority) // strip any userinfo
        .split(':')
        .next()
        .unwrap_or(authority) // strip any port
        .to_ascii_lowercase();
    host == "eqsl.cc" || host.ends_with(".eqsl.cc")
}

/// Resolve a relative path against a base directory, honoring leading `../`/`./`
/// without ever popping past the `scheme://host`.
fn resolve_relative(base_dir: &str, rel: &str) -> String {
    let mut dir = base_dir.trim_end_matches('/').to_string();
    let mut rest = rel;
    loop {
        if let Some(r) = rest.strip_prefix("../") {
            if let Some(idx) = dir.rfind('/') {
                if idx >= "https://".len() {
                    dir.truncate(idx);
                }
            }
            rest = r;
        } else if let Some(r) = rest.strip_prefix("./") {
            rest = r;
        } else {
            break;
        }
    }
    format!("{dir}/{rest}")
}

fn force_https(url: &str) -> String {
    match url.strip_prefix("http://") {
        Some(rest) => format!("https://{rest}"),
        None => url.to_string(),
    }
}

/// True iff `body` is a genuine eQSL ADIF, so an HTML/error page is never parsed as
/// ADIF. eQSL's human-readable preamble is not stable: older InBox exports began
/// `ADIF 3 Export from eQSL.cc`, while current exports may begin `Received eQSLs
/// for ...`. Validate the eQSL-specific PROGRAMID + `<eoh>` in the header instead,
/// while retaining the leading-HTML rejection as defense in depth. Only the header
/// region is inspected.
pub fn is_eqsl_adif(body: &str) -> bool {
    let head: String = body
        .chars()
        .take(8192)
        .collect::<String>()
        .to_ascii_lowercase();
    let trimmed = head.trim_start();
    !trimmed.starts_with('<')
        && head.contains("<programid:")
        && head.contains(">eqsl.cc downloadinbox")
        && head.contains("<eoh>")
}

/// True iff the body is a structurally **complete** download: after the header
/// there are either no records (empty InBox — valid and complete) or the last
/// record is terminated with `<eor>`. A truncated-but-HTTP-200 body (partial final
/// record) is detected as incomplete so the caller does NOT advance the sync
/// cursor over records it never received.
pub fn is_complete_eqsl_body(body: &str) -> bool {
    let lower = body.to_ascii_lowercase();
    match lower.split_once("<eoh>") {
        Some((_, after)) => {
            let after = after.trim();
            after.is_empty() || after.ends_with("<eor>")
        }
        None => false,
    }
}

/// Format a Unix time as `RcvdSince`'s `YYYYMMDDHHMM`, **flooring to the minute**
/// (seconds dropped, never rounded up). Flooring is load-bearing: the persisted
/// cursor must be `<=` the captured sync time so an inclusive, minute-granular
/// `RcvdSince` re-includes every card at/after it (no server cursor can recover a
/// skipped one). The caller subtracts the timezone safety margin before calling.
pub fn format_rcvd_since(unix_secs: i64) -> String {
    let (y, mo, d, h, mi, _s) = crate::logbook::datetime_utc(unix_secs.max(0) as u64);
    format!("{y:04}{mo:02}{d:02}{h:02}{mi:02}")
}

// ----- eQSL ADIF UPLOAD (ImportADIF.cfm) -----------------------------------

/// The eQSL ADIF-upload endpoint (POST). Hard-coded https constant so the
/// credential-bearing body can only ever go to eQSL over TLS.
pub const EQSL_IMPORT_URL: &str = "https://www.eqsl.cc/qslcard/ImportADIF.cfm";

/// Build the `application/x-www-form-urlencoded` POST body to upload ONE QSO via
/// `ADIFData`. Per the eQSL spec the credentials travel as ADIF header tags
/// (`EQSL_USER`/`EQSL_PSWD`) inside the payload; the whole thing is percent-encoded
/// into the `ADIFData` field. `record_adif` is a single `<…>…<eor>` record (e.g.
/// from [`crate::logbook::adif_record`]). The body carries the password — never log
/// it. ADIF length prefixes are BYTE lengths (ASCII calls/passwords).
pub fn build_upload_body(
    user: &str,
    pswd: &str,
    record_adif: &str,
    qth_nickname: Option<&str>,
) -> String {
    // The QTH Nickname goes IN THE RECORD — eQSL's ImportADIF spec places
    // `APP_EQSL_QTH_NICKNAME` "at the record level", not the header — inserted just
    // before this record's `<eor>`. Without it, an account with several QTH profiles
    // under one callsign refuses the upload outright (field report, 2026-09-01).
    let record = record_adif.trim_end();
    let record = match qth_nickname.map(str::trim).filter(|n| !n.is_empty()) {
        Some(nick) => {
            let lower = record.to_ascii_lowercase();
            match lower.rfind("<eor>") {
                Some(i) => format!(
                    "{}<APP_EQSL_QTH_NICKNAME:{}>{}{}",
                    &record[..i],
                    nick.len(),
                    nick,
                    &record[i..]
                ),
                // A record without <eor> is malformed anyway; append rather than lose
                // the nickname.
                None => format!("{record}<APP_EQSL_QTH_NICKNAME:{}>{nick}", nick.len()),
            }
        }
        None => record.to_string(),
    };
    let payload = format!(
        "Nexus eQSL upload\n<EQSL_USER:{}>{}\n<EQSL_PSWD:{}>{}\n<EOH>\n{}\n",
        user.len(),
        user,
        pswd.len(),
        pswd,
        record,
    );
    format!("ADIFData={}", pct(&payload))
}

/// Classify an `ImportADIF.cfm` response by its documented markers. `None` for a
/// transient "system is down" (leave the QSO unstamped for a clean retry, matching
/// the LoTW/ClubLog convention). Order matters: auth + transient are checked before
/// the duplicate/added markers.
pub fn classify_upload(html: &str) -> Option<crate::logbook::UploadOutcome> {
    use crate::logbook::UploadOutcome as U;
    let s = html.to_ascii_lowercase();
    // Auth failures (fatal): bad/missing credentials.
    if s.contains("no match on eqsl_user")
        || s.contains("missing eqsl_user")
        || s.contains("missing eqsl_pswd")
    {
        return Some(U::AuthFail);
    }
    // System down → transient; don't stamp so the next attempt retries cleanly.
    if s.contains("system is down") || (s.contains("error:") && s.contains("down")) {
        return None;
    }
    // Duplicate (benign — already on file at eQSL).
    if s.contains("duplicate") {
        return Some(U::Duplicate);
    }
    // "Result: x out of y records added" — accepted iff x > 0.
    if let Some(added) = parse_added_count(&s) {
        return Some(if added > 0 { U::Accepted } else { U::Rejected });
    }
    // Any other "Error:" or unrecognized body → a definitive bounce.
    Some(U::Rejected)
}

/// Pull the leading integer out of a lowercased `result: <x> out of …` line.
fn parse_added_count(lower: &str) -> Option<u32> {
    let after = &lower[lower.find("result:")? + "result:".len()..];
    let digits: String = after
        .trim_start()
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q() -> EqslQuery {
        EqslQuery {
            qth_nickname: None,
            username: "KD9TAW".into(),
            password: "p@ss w&rd?1".into(),
            rcvd_since: None,
        }
    }

    #[test]
    fn url_encodes_secrets_and_omits_blank_since() {
        let url = build_inbox_url(&q());
        assert!(url.starts_with("https://www.eqsl.cc/qslcard/DownloadInBox.cfm?"));
        assert!(url.contains("UserName=KD9TAW"));
        // & ? space must be percent-encoded so they can't break/inject params.
        assert!(url.contains("Password=p%40ss%20w%26rd%3F1"));
        assert!(!url.contains("p@ss w&rd?1"));
        assert!(!url.contains("RcvdSince="));
    }

    #[test]
    fn url_includes_rcvd_since_when_set() {
        let url = build_inbox_url(&EqslQuery {
            qth_nickname: None,
            rcvd_since: Some("202606050000".into()),
            ..q()
        });
        assert!(url.contains("RcvdSince=202606050000"));
    }

    #[test]
    fn debug_redacts_password() {
        let dbg = format!("{:?}", q());
        assert!(dbg.contains("<redacted>"));
        assert!(!dbg.contains("p@ss"));
    }

    #[test]
    fn built_page_detected_case_insensitively() {
        assert!(is_built_page(
            "<html>...Your ADIF log file has been built...</html>"
        ));
        assert!(!is_built_page(
            "<html>Error: No such Username/Password found</html>"
        ));
    }

    #[test]
    fn extracts_adi_link_not_txt() {
        let html = "<p>Your ADIF log file has been built</p>\
<A HREF=\"../downloadedfiles/AB1CD_5.txt\">.TXT file</A>\
<A HREF=\"../downloadedfiles/AB1CD_5.adi\">.ADI file</A>";
        assert_eq!(
            extract_adi_href(html).as_deref(),
            Some("../downloadedfiles/AB1CD_5.adi")
        );
        assert_eq!(extract_adi_href("<html>no links here</html>"), None);
    }

    const REQ: &str = "https://www.eqsl.cc/qslcard/DownloadInBox.cfm?UserName=AB1CD&Password=x";

    #[test]
    fn resolve_forces_https_on_absolute_http_href() {
        // The documented CQRLOG form: an absolute http:// download URL.
        assert_eq!(
            resolve_download_url(REQ, "http://www.eqsl.cc/qslcard/downloadedfiles/X.adi")
                .as_deref(),
            Some("https://www.eqsl.cc/qslcard/downloadedfiles/X.adi")
        );
    }

    #[test]
    fn resolve_handles_relative_dotdot_root_and_protocol_relative() {
        // ../ pops the qslcard segment → DownloadedFiles above QSLCard (2019 move).
        assert_eq!(
            resolve_download_url(REQ, "../downloadedfiles/X.adi").as_deref(),
            Some("https://www.eqsl.cc/downloadedfiles/X.adi")
        );
        // plain relative resolves against the request dir.
        assert_eq!(
            resolve_download_url(REQ, "downloadedfiles/X.adi").as_deref(),
            Some("https://www.eqsl.cc/qslcard/downloadedfiles/X.adi")
        );
        // root-relative.
        assert_eq!(
            resolve_download_url(REQ, "/downloadedfiles/X.adi").as_deref(),
            Some("https://www.eqsl.cc/downloadedfiles/X.adi")
        );
        // protocol-relative.
        assert_eq!(
            resolve_download_url(REQ, "//www.eqsl.cc/downloadedfiles/X.adi").as_deref(),
            Some("https://www.eqsl.cc/downloadedfiles/X.adi")
        );
    }

    #[test]
    fn resolve_rejects_cross_host_hrefs() {
        // Defense-in-depth: a hostile/MITM'd built-page href to another host must
        // NOT be fetched, in any form.
        assert_eq!(resolve_download_url(REQ, "http://evil.com/x.adi"), None);
        assert_eq!(resolve_download_url(REQ, "https://evil.com/x.adi"), None);
        assert_eq!(resolve_download_url(REQ, "//evil.com/x.adi"), None);
        assert_eq!(
            resolve_download_url(REQ, "https://eqsl.cc.evil.com/x.adi"),
            None
        );
        // A real eqsl.cc subdomain is allowed.
        assert_eq!(
            resolve_download_url(REQ, "https://www.eqsl.cc/x.adi").as_deref(),
            Some("https://www.eqsl.cc/x.adi")
        );
    }

    const EQSL_ADIF: &str = "ADIF 3 Export from eQSL.cc\n\
<PROGRAMID:21>eQSL.cc DownloadInBox <ADIF_Ver:5>3.1.6 <EOH>\n\
<CALL:5>W1AW/4 <BAND:3>20m <MODE:3>FT8 <EQSL_QSL_RCVD:1>Y <EOR>\n";

    const EQSL_ADIF_CURRENT: &str = "Received eQSLs for KR8MER\n\
for QSOs between 01-Jan-1999 and 31-Dec-2030\n\
Generated on Tuesday, August 25, 2026 at 22:21:51 PM UTC\n\
<PROGRAMID:21>eQSL.cc DownloadInBox\n\
<ADIF_Ver:5>3.1.6\n\
<EOH>\n\
<CALL:5>KC4WQ<QSO_DATE:8:D>20260808<BAND:3>40M<MODE:3>FT8<EQSL_QSL_RCVD:1>Y <EOR>\n";

    #[test]
    fn is_eqsl_adif_accepts_real_and_rejects_html() {
        assert!(is_eqsl_adif(EQSL_ADIF));
        assert!(is_eqsl_adif(EQSL_ADIF_CURRENT));
        // Reject an HTML page even when its chrome mentions the product name and
        // embeds ADIF-looking markers. Real eQSL exports begin with plain text;
        // HTML/error responses begin with '<'.
        assert!(!is_eqsl_adif(
            "<html><title>eQSL.cc DownloadInBox</title><body><PROGRAMID:21>eQSL.cc DownloadInBox <eoh> err</body></html>"
        ));
    }

    #[test]
    fn complete_body_detection() {
        assert!(is_complete_eqsl_body(EQSL_ADIF)); // ends in <EOR>
                                                   // Empty InBox (header only, no records) is valid + complete.
        let empty = "ADIF 3 Export from eQSL.cc\n<PROGRAMID:21>eQSL.cc DownloadInBox <EOH>\n";
        assert!(is_complete_eqsl_body(empty));
        // Truncated mid-record → incomplete (must not advance the cursor).
        let truncated = "<PROGRAMID:21>eQSL.cc DownloadInBox <EOH>\n<CALL:5>W1AW/4 <BAND:3>20m";
        assert!(!is_complete_eqsl_body(truncated));
        // No header at all → not complete.
        assert!(!is_complete_eqsl_body("<html>error</html>"));
    }

    #[test]
    fn upload_body_carries_creds_in_header_and_encodes() {
        let body = build_upload_body("KD9TAW", "p@ss&1", "<CALL:5>W1AW/4 <BAND:3>20m <EOR>", None);
        assert!(body.starts_with("ADIFData="));
        // Credentials are inside the encoded ADIF header (length-prefixed), never raw.
        assert!(body.contains("EQSL_USER%3A6%3EKD9TAW"));
        assert!(!body.contains("p@ss&1"), "password must be percent-encoded");
        assert!(body.contains("p%40ss%261"));
        // The record + EOH survive (encoded).
        assert!(body.contains("%3CEOH%3E"));
        assert!(body.to_uppercase().contains("W1AW")); // call present (unreserved)
    }

    #[test]
    fn classify_upload_covers_documented_markers() {
        use crate::logbook::UploadOutcome as U;
        assert_eq!(
            classify_upload("Result: 1 out of 1 records added"),
            Some(U::Accepted)
        );
        assert_eq!(
            classify_upload("Result: 0 out of 1 records added"),
            Some(U::Rejected)
        );
        assert_eq!(
            classify_upload("Warning: Y=2026 M=06 D=07 Bad record: Duplicate"),
            Some(U::Duplicate)
        );
        assert_eq!(
            classify_upload("Error: No match on eQSL_User/eQSL_Pswd"),
            Some(U::AuthFail)
        );
        assert_eq!(
            classify_upload("Error: Missing eQSL_Pswd"),
            Some(U::AuthFail)
        );
        // System down → transient, no stamp.
        assert_eq!(
            classify_upload("Error: The system is down until 1200Z"),
            None
        );
        // Unrecognized → a definitive bounce.
        assert_eq!(classify_upload("Error: something odd"), Some(U::Rejected));
    }

    #[test]
    fn rcvd_since_floors_to_the_minute() {
        // 1_780_617_600 = 2026-06-05T00:00:00Z (a minute boundary).
        assert_eq!(format_rcvd_since(1_780_617_600), "202606050000");
        // Flooring (epoch-independent): +0..59s stays in the same minute, never
        // rounds up; crossing 60s advances exactly one minute.
        let base = format_rcvd_since(1_780_617_600);
        assert_eq!(format_rcvd_since(1_780_617_600 + 59), base);
        assert_eq!(format_rcvd_since(1_780_617_600 + 60), "202606050001");
    }

    /// The QTH Nickname, both surfaces, spelled as eQSL's own specs spell them:
    /// `QTHNickname` on DownloadInBox.cfm ("if not logged in, if multiple accounts
    /// with same callsign"), and `APP_EQSL_QTH_NICKNAME` at the RECORD level on
    /// ImportADIF. An account with several QTH profiles fails outright without them
    /// (field report); an account without one must see byte-identical output.
    #[test]
    fn qth_nickname_reaches_both_surfaces_only_when_set() {
        let mut query = q();
        query.qth_nickname = Some("HomeQTH".into());
        let url = build_inbox_url(&query);
        assert!(url.contains("&QTHNickname=HomeQTH"), "{url}");
        // …and encoded when it needs it.
        query.qth_nickname = Some("My QTH".into());
        assert!(build_inbox_url(&query).contains("&QTHNickname=My%20QTH"));

        let body = build_upload_body("KD9TAW", "pw", "<CALL:4>W1AW <eor>", Some("HomeQTH"));
        let decoded = body
            .replace("%3C", "<")
            .replace("%3E", ">")
            .replace("%3A", ":");
        assert!(
            decoded.contains("<APP_EQSL_QTH_NICKNAME:7>HomeQTH<eor>"),
            "the nickname must sit at record level, before <eor>: {decoded}"
        );

        // Unset: byte-identical to the pre-nickname builder — single-profile
        // accounts must see no change at all.
        let plain = build_upload_body("KD9TAW", "pw", "<CALL:4>W1AW <eor>", None);
        assert!(!plain.to_ascii_lowercase().contains("nickname"));
    }
}
