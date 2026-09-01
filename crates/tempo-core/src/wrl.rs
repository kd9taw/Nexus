//! Pure World Radio League upload helpers — the offline, unit-testable core of the
//! WRL QSO-upload connector (mirrors the LoTW/eQSL/QRZ/ClubLog/HRDLog pure modules).
//! No I/O.
//!
//! WRL's public API (spec: `https://api.worldradioleague.com/v1/openapi.json`, and the
//! `/v1/llms.txt` summary) takes ONE contact per `POST /v1/contacts` as JSON, with
//! ADIF field names camelCased (`call`, `freq`, `rstRcvd`, `gridsquare`…). Contract
//! points that shaped this module, each verified against the LIVE API on 2026-09-01
//! (create → read-back → delete round trip):
//!
//!   * **Unknown fields are REJECTED, not ignored** — so this builder emits only the
//!     names the spec lists, and a test pins the exact key set.
//!   * `programId` is required on every contact (their ADIF PROGRAMID equivalent).
//!   * `mode` carries what ADIF splits across MODE and SUBMODE — "send the submode
//!     when you want to be specific". That is exactly [`crate::logbook::adif_submode`]'s
//!     split: a promoted submode (FT8-era ADIF parents, TempoFast → MFSK/TEMPOFAST)
//!     sends the SUBMODE; everything else sends the mode as logged.
//!   * `timestamp` is one ISO-8601 UTC instant (their one departure from ADIF).
//!   * Errors carry a stable `error.code` to branch on, never the message.
//!   * The auth key travels in a header, never in this body — so unlike the QRZ/HRDLog
//!     builders there is no secret to redact here; the transport owns that.

use crate::logbook::{QsoRecord, UploadOutcome};

/// The one-contact endpoint. Hard-coded https constant so the authenticated request
/// can only ever go to WRL over TLS.
pub const WRL_CONTACTS_URL: &str = "https://api.worldradioleague.com/v1/contacts";
/// Key validation + logbook resolution ("call `GET /v1/me` first").
pub const WRL_ME_URL: &str = "https://api.worldradioleague.com/v1/me";
/// Logbook listing — the fallback when the account has no default logbook.
pub const WRL_LOGBOOKS_URL: &str = "https://api.worldradioleague.com/v1/logbooks";

/// The `mode` string WRL wants: the SUBMODE when ADIF would split this mode into a
/// parent+submode pair, else the mode as logged. WRL folds MODE/SUBMODE into one
/// field ("send 'FT8', not 'MFSK' plus a submode").
fn wrl_mode(mode: &str) -> String {
    match crate::logbook::adif_submode(mode) {
        Some((_parent, sub)) => sub.to_string(),
        None => mode.to_uppercase(),
    }
}

/// Build the `POST /v1/contacts` JSON body for one logged QSO.
///
/// `station_callsign` is the operator's own call (their `stationCallsign`);
/// `logbook_id` is the resolved destination — `None` omits the field and lets the
/// account's default logbook take it (the no-configuration path; resolution happens
/// once at credential-save time, not per QSO).
///
/// Only fields the record actually carries are emitted: WRL rejects unknown NAMES,
/// and an absent optional field is simply omitted (never sent as null/empty).
pub fn build_contact_json(
    r: &QsoRecord,
    station_callsign: &str,
    logbook_id: Option<&str>,
) -> String {
    let mut o = serde_json::Map::new();
    let mut put = |k: &str, v: serde_json::Value| {
        o.insert(k.to_string(), v);
    };
    put("programId", "Nexus".into());
    put("call", r.call.clone().into());
    // ISO-8601 UTC from the epoch stamp — WRL treats a zoneless string as UTC, but
    // sending the explicit Z costs nothing and survives their parser changing.
    put("timestamp", iso8601_utc(r.when_unix).into());
    put("freq", freq_number(r.freq_mhz));
    put("band", r.band.clone().into());
    put("mode", wrl_mode(&r.mode).into());
    put("stationCallsign", station_callsign.to_string().into());
    if let Some(id) = logbook_id {
        put("logbookId", id.to_string().into());
    }
    if let Some(v) = r.rst_sent.as_deref().filter(|s| !s.is_empty()) {
        put("rstSent", v.to_string().into());
    }
    if let Some(v) = r.rst_rcvd.as_deref().filter(|s| !s.is_empty()) {
        put("rstRcvd", v.to_string().into());
    }
    if let Some(v) = r.grid.as_deref().filter(|s| !s.is_empty()) {
        put("gridsquare", v.to_string().into());
    }
    if let Some(v) = r.state.as_deref().filter(|s| !s.is_empty()) {
        put("state", v.to_string().into());
    }
    if let Some(v) = r.name.as_deref().filter(|s| !s.is_empty()) {
        put("name", v.to_string().into());
    }
    if let Some(v) = r.qth.as_deref().filter(|s| !s.is_empty()) {
        put("qth", v.to_string().into());
    }
    if let Some(v) = r.comment.as_deref().filter(|s| !s.is_empty()) {
        put("notes", v.to_string().into());
    }
    if let Some(p) = r.tx_power {
        put("txPwr", freq_number(p));
    }
    serde_json::Value::Object(o).to_string()
}

/// `freq`/`txPwr` as a JSON number (their spec types them numeric). Falls back to 0
/// for a non-finite value rather than emitting invalid JSON.
fn freq_number(v: f64) -> serde_json::Value {
    serde_json::Number::from_f64(v)
        .map(serde_json::Value::Number)
        .unwrap_or_else(|| 0.into())
}

fn iso8601_utc(unix: u64) -> String {
    // Days-since-epoch → civil date, the same arithmetic logbook.rs uses for ADIF
    // dates (Howard Hinnant's algorithm) — no chrono dependency in this crate.
    let secs = unix % 86_400;
    let days = (unix / 86_400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        (secs % 3600) / 60,
        secs % 60
    )
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Classify a WRL response. `status` is the HTTP status; `body` is the response JSON
/// (the `{data, meta, error}` envelope; `error.code` is the stable contract).
pub fn classify_response(status: u16, body: &str) -> UploadOutcome {
    if status == 201 || status == 200 {
        return UploadOutcome::Accepted;
    }
    let code = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|v| {
            v.get("error")
                .and_then(|e| e.get("code"))
                .and_then(|c| c.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    match code.as_str() {
        // Credential problems: fix the key, then re-send.
        "MISSING_CREDENTIALS"
        | "INVALID_KEY"
        | "KEY_REVOKED"
        | "INSUFFICIENT_SCOPE"
        | "MEMBERSHIP_REQUIRED"
        | "IP_NOT_ALLOWED" => UploadOutcome::AuthFail,
        // The payload is wrong (or the logbook routing is): a definitive bounce that a
        // retry cannot fix. LOGBOOK_REQUIRED lands here so the operator sees a real
        // error and re-saves the key (which re-runs logbook resolution).
        "VALIDATION_ERROR" | "MALFORMED_JSON" | "LOGBOOK_REQUIRED" | "PAYLOAD_TOO_LARGE"
        | "BATCH_TOO_LARGE" | "NOT_FOUND" | "CONFLICT" => UploadOutcome::Rejected,
        // Rate limit / server trouble / anything unrecognized: the record is fine,
        // the moment was not — leave it pending for a later manual re-push.
        _ => UploadOutcome::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::QsoRecord;

    /// Built through the crate's own ADIF reader rather than a struct literal:
    /// `QsoRecord` deliberately has no `Default` (a defaulted record is how half-empty
    /// rows reach a logbook), and the reader is the same path every import uses.
    fn rec() -> QsoRecord {
        let adif = "<CALL:4>W1AW<GRIDSQUARE:4>FN31<BAND:3>20m<FREQ:6>14.074\
                    <MODE:3>FT8<RST_SENT:3>-05<RST_RCVD:3>-10\
                    <QSO_DATE:8>20260831<TIME_ON:6>200000<eor>";
        let mut lb = crate::logbook::Logbook::new();
        let (recs, _, _) = lb.import_adif(adif);
        recs.into_iter().next().expect("fixture parses")
    }

    /// ⚠️ WRL REJECTS UNKNOWN FIELD NAMES (verified live: `grid_square` → 400), so the
    /// exact key set IS the contract. A new field must be added here deliberately,
    /// with its spec name, never guessed.
    #[test]
    fn emits_only_the_spec_field_names() {
        let v: serde_json::Value =
            serde_json::from_str(&build_contact_json(&rec(), "KD9TAW", Some("lb-1"))).unwrap();
        let mut keys: Vec<&str> = v.as_object().unwrap().keys().map(|s| s.as_str()).collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "band",
                "call",
                "freq",
                "gridsquare",
                "logbookId",
                "mode",
                "programId",
                "rstRcvd",
                "rstSent",
                "stationCallsign",
                "timestamp",
            ]
        );
        assert_eq!(v["programId"], "Nexus");
        assert_eq!(v["freq"], 14.074);
        assert_eq!(v["timestamp"], "2026-08-31T20:00:00Z");
    }

    /// WRL folds MODE/SUBMODE into one field: "send 'FT8', not 'MFSK' plus a
    /// submode" — and for our own tiers that means the SUBMODE (TEMPOFAST), the
    /// same split `adif_submode` already owns for the ADIF writers.
    #[test]
    fn mode_sends_the_submode_when_adif_would_split() {
        assert_eq!(wrl_mode("FT8"), "FT8");
        assert_eq!(wrl_mode("TempoFast"), "TEMPOFAST");
        assert_eq!(wrl_mode("cw"), "CW");
    }

    /// Absent optionals are OMITTED — never null, never "" (an empty rstSent is a
    /// validation error waiting to happen).
    #[test]
    fn absent_fields_are_omitted_not_nulled() {
        let mut r = rec();
        r.rst_sent = None;
        r.rst_rcvd = Some(String::new());
        r.grid = None;
        let v: serde_json::Value =
            serde_json::from_str(&build_contact_json(&r, "KD9TAW", None)).unwrap();
        let o = v.as_object().unwrap();
        for gone in ["rstSent", "rstRcvd", "gridsquare", "logbookId"] {
            assert!(!o.contains_key(gone), "{gone} was emitted");
        }
    }

    /// The stable error codes → the funnel's outcomes. AuthFail re-prompts for the
    /// key; Rejected is a definitive bounce; anything else stays Pending for re-push.
    #[test]
    fn classifies_the_stable_error_codes() {
        let err = |code: &str| {
            format!(r#"{{"data":null,"meta":null,"error":{{"code":"{code}","message":"x"}}}}"#)
        };
        assert_eq!(classify_response(201, "{}"), UploadOutcome::Accepted);
        assert_eq!(
            classify_response(401, &err("INVALID_KEY")),
            UploadOutcome::AuthFail
        );
        assert_eq!(
            classify_response(403, &err("KEY_REVOKED")),
            UploadOutcome::AuthFail
        );
        assert_eq!(
            classify_response(422, &err("VALIDATION_ERROR")),
            UploadOutcome::Rejected
        );
        assert_eq!(
            classify_response(422, &err("LOGBOOK_REQUIRED")),
            UploadOutcome::Rejected
        );
        assert_eq!(
            classify_response(429, &err("RATE_LIMITED")),
            UploadOutcome::Pending
        );
        assert_eq!(
            classify_response(500, &err("INTERNAL_ERROR")),
            UploadOutcome::Pending
        );
        // Unrecognized body (proxy HTML, truncation) — never a definitive verdict.
        assert_eq!(
            classify_response(502, "<html>bad gateway</html>"),
            UploadOutcome::Pending
        );
    }
}
