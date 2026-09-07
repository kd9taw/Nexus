//! Cloudlog / Wavelog QSO upload (HTTP JSON). Cloudlog (and its Wavelog fork) are self-hosted
//! web logbooks with an identical QSO API: `POST {base}/index.php/api/qso` with the instance
//! API key + station-profile id + one ADIF record. The URL + JSON builders are pure (unit-
//! tested); [`upload`] does the blocking POST.

/// How much of the server's own words to show, in characters.
///
/// A Cloudlog/Wavelog `reason` is a short sentence; what else can arrive on this socket is a
/// reverse proxy's HTML error page or a PHP notice. The bound is not about screen space —
/// the panel row already truncates with `text-overflow: ellipsis` (`ui/src/styles.css`
/// `.conn-when`) — it is about where the string GOES: `src-tauri/src/lib.rs` puts it in the
/// 200-entry in-memory connection log and writes it into `conn-health.json` on every change,
/// so an unbounded body is an unbounded repeated disk write. 160 leaves room for a real
/// sentence and cuts a web page off at its title, which is the part that identifies it.
///
/// Measured against a 50 000-character HTML error page: the whole operator-facing message
/// comes out at **201 characters** (this bound, the ellipsis, and the longest prefix
/// `classify` builds). A genuine Cloudlog reason lands well inside it — the reported
/// station-profile rejection measures 96.
const REASON_MAX_CHARS: usize = 160;

/// Build the QSO API endpoint from a user-entered base URL. Tolerant of a trailing slash, an
/// already-present `/index.php`, or the full `/index.php/api/qso` path.
pub fn api_url(base: &str) -> String {
    let b = base.trim().trim_end_matches('/');
    if b.ends_with("/api/qso") {
        b.to_string()
    } else if b.contains("/index.php") {
        format!("{b}/api/qso")
    } else {
        format!("{b}/index.php/api/qso")
    }
}

/// Escape a string for embedding in a JSON string literal.
fn json_escape(s: &str) -> String {
    let mut o = String::with_capacity(s.len() + 8);
    for c in s.chars() {
        match c {
            '"' => o.push_str("\\\""),
            '\\' => o.push_str("\\\\"),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c if (c as u32) < 0x20 => o.push(' '),
            c => o.push(c),
        }
    }
    o
}

/// Build the Cloudlog/Wavelog JSON request body for one ADIF record.
pub fn build_body(key: &str, station_id: &str, adif: &str) -> String {
    format!(
        "{{\"key\":\"{}\",\"station_profile_id\":\"{}\",\"type\":\"adif\",\"string\":\"{}\"}}",
        json_escape(key),
        json_escape(station_id),
        json_escape(adif)
    )
}

/// Classify a Cloudlog/Wavelog 2xx response. The per-record import result is carried IN the
/// body (`{"status":"created"}` on success; `{"status":"failed"|"error",...}` on a rejected or
/// misfiled record), so an HTTP 2xx alone does not mean the QSO was filed. Lenient on unknown
/// body shapes so a Wavelog variant with a different success payload isn't reported as failed.
fn classify_body(text: &str, reason: Option<&str>) -> Result<String, String> {
    let t = text.to_ascii_lowercase().replace(' ', "");
    if t.contains("\"status\":\"failed\"") || t.contains("\"status\":\"error\"") {
        return Err(match reason {
            Some(r) => format!("Cloudlog rejected the QSO: {r}"),
            None => "Cloudlog rejected the QSO — check the instance log".to_string(),
        });
    }
    Ok(text.to_string())
}

/// The server's own explanation of a failure, made safe to show — or `None` when it said
/// nothing an operator can use.
///
/// ⚠️ #226, and this is the whole point of the issue. Cloudlog and Wavelog answer a rejected
/// upload with a body naming what they rejected: a missing or read-only API key, a station
/// profile id not linked to that key, a malformed record. Nexus read that body and threw it
/// away, so all of them arrived as "check the API key" — the one thing the reporter had
/// already checked, on two independent instances, with a key four other clients accept.
///
/// Two things are done to the body before any of it is shown:
///
/// 1. **The API key is scrubbed.** It rides in the REQUEST body, and a debug-mode PHP notice
///    or a WAF page can echo a request straight back. This string does not stop at the panel
///    — `src-tauri/src/lib.rs` keeps it in the connection log and writes it into
///    `conn-health.json` — so an echoed key would land on disk in cleartext. The scrub is a
///    literal match and therefore best-effort: a key the server re-encodes (HTML entities,
///    say) would not be caught, which is the second reason for the bound below.
/// 2. **It is flattened to one line and cut to [`REASON_MAX_CHARS`].**
///
/// A JSON answer's explanation is read from its named field, because the object as a whole is
/// machine shape rather than words for an operator. A body that is not JSON at all — a
/// reverse proxy's HTML page, a PHP notice — IS the message, and a bounded slice of it is
/// worth showing: knowing a proxy answered instead of Cloudlog is the actionable half.
fn server_reason(text: &str, key: &str) -> Option<String> {
    let scrubbed = match key.trim() {
        "" => text.to_string(),
        k => text.replace(k, "[api key]"),
    };
    let words = match serde_json::from_str::<serde_json::Value>(&scrubbed) {
        Ok(v) => ["reason", "message", "error"]
            .iter()
            .find_map(|f| v.get(f).and_then(serde_json::Value::as_str))
            .or_else(|| v.as_str())
            .map(str::to_string)?,
        Err(_) => scrubbed,
    };
    // Control characters (newlines included) become spaces, then runs of whitespace collapse:
    // an HTML page is otherwise 40 blank lines in a tooltip.
    let flat = words
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>();
    let mut out: String = flat.split_whitespace().collect::<Vec<_>>().join(" ");
    if out.is_empty() {
        return None;
    }
    if out.chars().count() > REASON_MAX_CHARS {
        out = out.chars().take(REASON_MAX_CHARS).collect::<String>() + "…";
    }
    Some(out)
}

/// POST one ADIF record to a Cloudlog/Wavelog instance. `Ok(body)` when the record is actually
/// filed; a redacted error otherwise (the API key is in the REQUEST body — never echoed into an
/// error string). Enforces HTTPS + no redirects so a credential-bearing request can't be
/// downgraded onto cleartext, matching every sibling connector.
pub fn upload(base_url: &str, key: &str, station_id: &str, adif: &str) -> Result<String, String> {
    if key.trim().is_empty() {
        return Err("Cloudlog API key is empty — set it in Settings".to_string());
    }
    if station_id.trim().is_empty() {
        return Err("Cloudlog station profile id is empty — set it in Settings".to_string());
    }
    let url = api_url(base_url);
    let body = build_body(key, station_id, adif);
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "couldn't build HTTP client".to_string())?;
    let resp = client
        .post(&url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send()
        .map_err(|_| {
            "Cloudlog/Wavelog unreachable — check the URL (must be https://)".to_string()
        })?;
    let status = resp.status();
    let text = resp.text().unwrap_or_default();
    classify(status.as_u16(), &text, key)
}

/// Turn one answered response into the operator's result. Pure, so the whole
/// classification is unit-testable without a server (`upload` above is only the socket).
///
/// Where the server explained itself, its words lead and Nexus's guess is dropped: a guess
/// printed beside an answer is noise. Where it did not, the guess is all there is, so it
/// stays exactly as it was — and it names the station profile id as well as the key, because
/// #226's actual failure was the profile id and the old wording never mentioned it.
fn classify(status: u16, text: &str, key: &str) -> Result<String, String> {
    let reason = server_reason(text, key);
    if (200..300).contains(&status) {
        return classify_body(text, reason.as_deref());
    }
    let what = if status == 401 || status == 403 {
        "rejected the credentials"
    } else {
        "refused the upload"
    };
    Err(match reason {
        Some(r) => format!("Cloudlog HTTP {status} — {what}: {r}"),
        None if status == 401 || status == 403 => format!(
            "Cloudlog HTTP {status} — {what}, and said no more. Check the API key and the \
             station profile id (a key is scoped to one profile)."
        ),
        None => format!("Cloudlog HTTP {status} — {what}, and said no more."),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_url_tolerates_url_variants() {
        let want = "https://log.example.com/index.php/api/qso";
        assert_eq!(api_url("https://log.example.com"), want);
        assert_eq!(api_url("https://log.example.com/"), want);
        assert_eq!(api_url("https://log.example.com/index.php"), want);
        assert_eq!(api_url("https://log.example.com/index.php/api/qso"), want);
    }

    #[test]
    fn body_has_the_documented_shape_and_escapes() {
        let b = build_body("K3Y", "3", "<CALL:5>W1ABC \"x\" <EOR>");
        assert!(b.starts_with("{\"key\":\"K3Y\",\"station_profile_id\":\"3\",\"type\":\"adif\""));
        assert!(b.contains("W1ABC"));
        assert!(b.contains("\\\"x\\\""), "embedded quotes escaped: {b}");
    }

    const KEY: &str = "cl0udl0g-4pi-k3y-abcdef0123456789";

    #[test]
    fn an_auth_rejection_carries_the_servers_own_reason() {
        // #226: the same key works from GridTracker2 and WSJT-X-improved and fails against
        // two independent instances, because what the server actually rejected was NOT the
        // key. Collapsing every 401 into "check the API key" sends the operator back to the
        // one thing they have already checked, and Nexus has the answer in hand.
        let err = classify(
            401,
            r#"{"status":"failed","reason":"station_profile_id 7 is not linked to this API key"}"#,
            KEY,
        )
        .unwrap_err();
        assert!(
            err.contains("station_profile_id 7 is not linked to this API key"),
            "the server's own reason must reach the operator: {err}"
        );
    }

    #[test]
    fn a_2xx_that_rejects_the_record_carries_the_reason_too() {
        // Cloudlog files the per-record verdict IN the body, so this is the other half of
        // the same defect: HTTP 200 and the QSO still did not land.
        let err = classify(
            200,
            r#"{"status":"failed","reason":"ADIF field BAND is missing"}"#,
            KEY,
        )
        .unwrap_err();
        assert!(
            err.contains("ADIF field BAND is missing"),
            "a rejected record must say why: {err}"
        );
    }

    #[test]
    fn a_reason_that_echoes_the_api_key_never_reaches_the_message() {
        // The key rides in the REQUEST body, so a debug-mode PHP notice or a WAF page can
        // echo it straight back — and this string is persisted to conn-health.json and kept
        // in the connection log, so an echo would put the key on disk in cleartext.
        let body = format!(r#"{{"status":"failed","reason":"denied for key={KEY} (profile 7)"}}"#);
        let err = classify(403, &body, KEY).unwrap_err();
        // Positive control, and it has to be a word the OLD flattened message never used —
        // "rejected" would have passed against "auth rejected — check the API key" and this
        // test would have proved nothing.
        assert!(
            err.contains("(profile 7)"),
            "the reason must be surfaced: {err}"
        );
        assert!(!err.contains(KEY), "API key leaked into the message: {err}");
    }

    #[test]
    fn an_enormous_or_hostile_body_is_bounded_and_flattened_to_one_line() {
        // A reverse proxy answers with an HTML page, not a Cloudlog reason. Showing a slice
        // of it is useful (it tells the operator the URL reached a proxy), showing all of it
        // is not: the message is written to conn-health.json on every change.
        let hostile = format!(
            "<!DOCTYPE html>\n<html><head><title>502 Bad Gateway</title></head>\n{}",
            "A".repeat(50_000)
        );
        let err = classify(502, &hostile, KEY).unwrap_err();
        assert!(
            err.contains("502 Bad Gateway"),
            "the useful head of the page must survive: {err}"
        );
        // Measured at 201 characters for this 50 000-character page. Asserted tightly, so a
        // longer prefix or a raised REASON_MAX_CHARS has to be a deliberate edit here rather
        // than silent drift.
        assert!(
            err.chars().count() <= 201,
            "unbounded body reached the message ({} chars)",
            err.chars().count()
        );
        assert!(
            !err.contains('\n') && !err.contains('\r'),
            "the message must stay one line: {err}"
        );
    }

    #[test]
    fn a_server_that_says_nothing_still_gets_the_old_actionable_guess() {
        // The control for the three above: when there is no reason to surface, the message
        // must not become emptier than it was.
        let err = classify(401, "", KEY).unwrap_err();
        assert!(
            err.contains("API key"),
            "no-reason fallback lost its hint: {err}"
        );
        let err = classify(500, "", KEY).unwrap_err();
        assert!(err.contains("500"), "the status must still be named: {err}");
    }
}
