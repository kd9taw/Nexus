//! NOAA SWPC space-weather "scales" + alerts adapter (pure parser half).
//!
//! Two public SWPC products, normalized for a glanceable space-weather strip:
//! - `products/noaa-scales.json` — the R (radio blackout) / S (solar radiation)
//!   / G (geomagnetic) NOAA scales. It is an OBJECT of day blocks, each with a
//!   `DateStamp` ("YYYY-MM-DD", UTC) and `R`/`S`/`G` objects whose `Scale` is a
//!   STRING digit "0".."5" (or `null`: forecast R/S carry probabilities instead).
//!   "0" is observed now; the KEYS ARE NOT DAYS AHEAD — "1" is TODAY's forecast,
//!   dated the same day as "0", and tomorrow is "2". We read today's R/S/G from
//!   "0" and tomorrow's G from the block DATED the day after "0", by its
//!   `DateStamp`; no such block (or no parseable date) → no tomorrow value.
//! - `products/alerts.json` — an ARRAY of issued SWPC alert/watch/warning
//!   bulletins; we sort newest-first and keep the most recent `max`.
//!
//! Pure (`&Value` in) so it is unit-testable offline; the fetchers are in
//! `live::swpc_scales`.

use serde::Serialize;
use serde_json::Value;

/// Today's R/S/G NOAA scales plus tomorrow's geomagnetic (G) forecast, each
/// 0..5. Today's default to zero ("quiet"), an honest neutral when data is absent
/// — never a fabricated elevated level. Tomorrow's is `None` when NOAA's file
/// has no block dated tomorrow: unknown, never today's value standing in.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoaaScalesView {
    pub r: u8,
    pub s: u8,
    pub g: u8,
    pub g_tomorrow: Option<u8>,
    /// When these scales were actually FETCHED (unix). `None` on the cold-cache
    /// default — so the UI can tell "measured quiet (R0 S0 G0)" from "never
    /// loaded" instead of rendering an offline feed as calm chips.
    #[serde(default)]
    pub as_of: Option<i64>,
}

/// One issued SWPC bulletin, distilled for a notifications strip.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AlertView {
    pub product_id: String,
    pub issued: i64,
    pub kind: String,
    pub message: String,
}

/// Parse `noaa-scales.json` into today's R/S/G + tomorrow's G.
///
/// `Scale` is read as an OPTIONAL string then parsed (forecast blocks carry
/// `Scale: null`); for today a missing block, a `null`, or a non-digit all degrade
/// to `0`, for tomorrow to `None` — we never deserialize `Scale` straight into a
/// number (that panics on null).
pub fn parse_noaa_scales(v: &Value) -> NoaaScalesView {
    NoaaScalesView {
        r: scale_at(v, "0", "R"),
        s: scale_at(v, "0", "S"),
        g: scale_at(v, "0", "G"),
        g_tomorrow: g_tomorrow(v),
        as_of: None, // the COMMAND stamps this on a real fetch, never the parser
    }
}

/// Read `v[day][scale].Scale` as a 0..=5 digit, defaulting to 0. `day` is the block
/// key ("0"=observed now); `scale` is the R/S/G letter.
fn scale_at(v: &Value, day: &str, scale: &str) -> u8 {
    v.get(day).and_then(|b| block_scale(b, scale)).unwrap_or(0)
}

/// One block's `[scale].Scale` as a 0..=5 digit; `None` on null/number/missing/out of range.
fn block_scale(block: &Value, scale: &str) -> Option<u8> {
    block
        .get(scale)
        .and_then(|g| g.get("Scale"))
        .and_then(|s| s.as_str()) // STRING-or-null; None on null/number/missing.
        .and_then(|s| s.trim().parse::<u8>().ok())
        .filter(|n| *n <= 5) // the documented 0..=5 domain (a stray "6"/"9" is no reading)
}

/// Tomorrow's G: the one block whose `DateStamp` is the UTC day after block "0"'s.
/// `None` when "0" has no parseable date, when no block (or more than one) carries
/// tomorrow's date, or when that block's G is not a 0..=5 digit.
fn g_tomorrow(v: &Value) -> Option<u8> {
    let today = block_day(v.get("0")?)?;
    let mut dated_tomorrow = v
        .as_object()?
        .values()
        .filter(|b| block_day(b) == Some(today + 1));
    let block = dated_tomorrow.next()?;
    if dated_tomorrow.next().is_some() {
        return None; // two blocks claim tomorrow: no honest single answer
    }
    block_scale(block, "G")
}

/// A block's `DateStamp` ("YYYY-MM-DD") as days since the Unix epoch; `None` when
/// missing, malformed, or not a real calendar date (2026-02-30 is refused).
fn block_day(block: &Value) -> Option<i64> {
    let s = block.get("DateStamp")?.as_str()?.trim();
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return None;
    }
    let y: i64 = s[0..4].parse().ok()?;
    let m: u32 = s[5..7].parse().ok()?;
    let d: u32 = s[8..10].parse().ok()?;
    let days = crate::geo::days_from_civil(y, m, d);
    // Round-trip rejects a month or day the calendar does not have (2026-02-29 would
    // otherwise roll into 1 March, 2026-13-01 into the next year).
    (crate::geo::civil_from_days(days) == (y, m, d)).then_some(days)
}

/// Parse `alerts.json` into the most recent `max` bulletins, newest-first.
///
/// `issue_datetime` is a space-separated, timezone-naive UTC stamp
/// ("YYYY-MM-DD hh:mm:ss.fff", no 'T'/'Z') parsed via the shared
/// [`crate::kc2g::parse_naive_utc_unix`]; an unparseable stamp sorts last (0).
pub fn parse_alerts(v: &Value, max: usize) -> Vec<AlertView> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    let mut out: Vec<AlertView> = arr
        .iter()
        .filter_map(|item| {
            if !item.is_object() {
                return None;
            }
            let product_id = item
                .get("product_id")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let issued = item
                .get("issue_datetime")
                .and_then(|x| x.as_str())
                .and_then(crate::kc2g::parse_naive_utc_unix)
                .unwrap_or(0);
            let message = item
                .get("message")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string();
            let kind = alert_kind(&product_id, &message).to_string();
            Some(AlertView {
                product_id,
                issued,
                kind,
                message,
            })
        })
        .collect();
    out.sort_by(|a, b| b.issued.cmp(&a.issued));
    out.truncate(max);
    out
}

/// A short, glanceable category derived from the SWPC product code (with the
/// bulletin text as a fallback signal): "watch" / "warning" / "summary" /
/// "alert" (the default).
fn alert_kind(product_id: &str, message: &str) -> &'static str {
    let id = product_id.to_uppercase();
    let hay = message.to_uppercase();
    if id.starts_with("WAT") || hay.contains("WATCH") {
        "watch"
    } else if id.starts_with("WAR") || hay.contains("WARNING") {
        "warning"
    } else if id.starts_with("SUM") || hay.contains("SUMMARY") {
        "summary"
    } else {
        // ALT… and everything else read as a plain alert.
        "alert"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn scales_reads_observed_and_null_forecast_blocks() {
        let v = json!({
            "0": { "R": {"Scale": "2"}, "S": {"Scale": "0"}, "G": {"Scale": "1"} },
            "1": { "R": {"Scale": null}, "S": {"Scale": null}, "G": {"Scale": null} }
        });
        let s = parse_noaa_scales(&v);
        assert_eq!(s.r, 2);
        assert_eq!(s.s, 0);
        assert_eq!(s.g, 1);
        assert_eq!(
            s.g_tomorrow, None,
            "no dated tomorrow block and a null forecast Scale → no value, never a panic"
        );
    }

    #[test]
    fn scales_missing_blocks_default_to_zero() {
        let s = parse_noaa_scales(&json!({}));
        assert_eq!((s.r, s.s, s.g, s.g_tomorrow), (0, 0, 0, None));
    }

    /// `noaa-scales.json` as SWPC served it on 2026-10-31 (04:14Z): "-1" = yesterday,
    /// "0" = observed today, "1" = TODAY's forecast (same DateStamp as "0"),
    /// "2" = tomorrow (2026-11-01), "3" = the day after.
    const SCALES_2026_10_31: &str =
        include_str!("../tests/fixtures/swpc-noaa-scales-2026-10-31.json");

    fn fixture() -> Value {
        serde_json::from_str(SCALES_2026_10_31).expect("fixture parses")
    }

    fn set_g(v: &mut Value, day: &str, scale: &str) {
        v[day]["G"]["Scale"] = json!(scale);
    }

    fn tmrw(s: &NoaaScalesView) -> Option<u8> {
        s.g_tomorrow
    }

    #[test]
    fn tomorrow_is_the_block_dated_the_day_after_today_not_key_1() {
        // The real file: "1" and "2" are both G1, so it reads right by luck. Make
        // today's forecast ("1") a storm and tomorrow ("2", 2026-11-01) quiet: the
        // "tomorrow" value must be tomorrow's G0, never today's G3.
        let mut v = fixture();
        set_g(&mut v, "1", "3");
        set_g(&mut v, "2", "0");
        assert_eq!(tmrw(&parse_noaa_scales(&v)), Some(0));
        // And the reverse: quiet today, a G2 tomorrow is shown, not hidden.
        set_g(&mut v, "1", "0");
        set_g(&mut v, "2", "2");
        assert_eq!(tmrw(&parse_noaa_scales(&v)), Some(2));
        // The untouched real file: today's R/S/G observed, tomorrow G1.
        let s = parse_noaa_scales(&fixture());
        assert_eq!((s.r, s.s, s.g), (0, 0, 0));
        assert_eq!(tmrw(&s), Some(1));
    }

    #[test]
    fn tomorrow_is_found_by_date_whatever_the_keys() {
        // Same blocks under shuffled keys: the date picks tomorrow, not the key.
        let v = fixture();
        let mut shuffled = json!({
            "0": v["0"],
            "3": v["1"],              // today's forecast (2026-10-31), G3
            "1": v["3"],              // 2026-11-02, G0
            "-1": v["-1"],
            "7": v["2"],              // tomorrow (2026-11-01)
        });
        set_g(&mut shuffled, "3", "3");
        set_g(&mut shuffled, "7", "2");
        assert_eq!(tmrw(&parse_noaa_scales(&shuffled)), Some(2));
        // Two blocks both dated tomorrow, disagreeing: no single honest answer.
        shuffled["9"] = shuffled["7"].clone();
        set_g(&mut shuffled, "9", "4");
        assert_eq!(tmrw(&parse_noaa_scales(&shuffled)), None);
    }

    #[test]
    fn no_block_dated_tomorrow_gives_no_tomorrow_value() {
        // Tomorrow's block missing: today's forecast ("1", G1) must not stand in.
        let mut v = fixture();
        v.as_object_mut().unwrap().remove("2");
        set_g(&mut v, "1", "3");
        let s = parse_noaa_scales(&v);
        assert_eq!(tmrw(&s), None);
        assert_eq!(s.g, 0, "today's observed G still reads");
    }

    #[test]
    fn missing_or_bad_dates_give_no_tomorrow_value() {
        // Today's date missing → nothing to count "tomorrow" from.
        let mut v = fixture();
        v["0"].as_object_mut().unwrap().remove("DateStamp");
        assert_eq!(tmrw(&parse_noaa_scales(&v)), None);
        // Unparseable / impossible dates.
        for bad in [
            "31 Oct 2026",
            "2026-10-32",
            "2026-13-01",
            "2026-02-30",
            "",
            "2026-1-1",
        ] {
            let mut v = fixture();
            v["0"]["DateStamp"] = json!(bad);
            assert_eq!(tmrw(&parse_noaa_scales(&v)), None, "today dated {bad:?}");
        }
        // Tomorrow's block dated nonsense → not tomorrow.
        let mut v = fixture();
        v["2"]["DateStamp"] = json!("tomorrow");
        assert_eq!(tmrw(&parse_noaa_scales(&v)), None);
        // A date the calendar does not have is not tomorrow, even where it would roll
        // over onto tomorrow's day number (2026 has no 29 February).
        let v = json!({
            "0": { "DateStamp": "2026-02-28", "G": { "Scale": "0" } },
            "2": { "DateStamp": "2026-02-29", "G": { "Scale": "2" } },
        });
        assert_eq!(tmrw(&parse_noaa_scales(&v)), None);
        // Tomorrow's block found but its G is null/garbage/out of range → unknown, not quiet.
        for bad in [Value::Null, json!("6"), json!(2), json!("G2")] {
            let mut v = fixture();
            v["2"]["G"]["Scale"] = bad.clone();
            assert_eq!(tmrw(&parse_noaa_scales(&v)), None, "tomorrow's G {bad}");
        }
    }

    #[test]
    fn tomorrow_crosses_utc_month_year_and_leap_day_boundaries() {
        let day = |date: &str, g: &str| json!({ "DateStamp": date, "G": { "Scale": g } });
        for (today, tomorrow) in [
            ("2026-12-31", "2027-01-01"),
            ("2026-10-31", "2026-11-01"),
            ("2028-02-28", "2028-02-29"),
            ("2028-02-29", "2028-03-01"),
            ("2027-02-28", "2027-03-01"),
        ] {
            let v = json!({
                "0": day(today, "0"),
                "1": day(today, "4"),       // today's forecast, same date
                "2": day(tomorrow, "2"),
            });
            assert_eq!(
                tmrw(&parse_noaa_scales(&v)),
                Some(2),
                "{today} → {tomorrow}"
            );
        }
        // A gap: no block dated the day after today. The block two days on must not
        // stand in for tomorrow.
        let v = json!({
            "0": day("2026-10-31", "0"),
            "1": day("2026-10-31", "1"),
            "2": day("2026-11-02", "3"),   // gap: no 11-01 block
        });
        assert_eq!(tmrw(&parse_noaa_scales(&v)), None);
    }

    #[test]
    fn alerts_parse_datetime_and_sort_newest_first() {
        let v = json!([
            { "product_id": "ALTK04", "issue_datetime": "2024-06-01 00:00:00.000",
              "message": "ALERT: Geomagnetic K-index of 4" },
            { "product_id": "WATA50", "issue_datetime": "2024-06-01 01:00:00.000",
              "message": "WATCH: Geomagnetic Storm Category G3 Predicted" }
        ]);
        let out = parse_alerts(&v, 10);
        assert_eq!(out.len(), 2);
        // Newest (01:00) sorts first.
        assert_eq!(out[0].product_id, "WATA50");
        assert_eq!(out[0].issued, 1_717_203_600);
        assert_eq!(out[0].kind, "watch");
        assert_eq!(out[1].issued, 1_717_200_000);
        assert_eq!(out[1].kind, "alert");
    }

    #[test]
    fn alerts_respects_max_and_non_array() {
        let v = json!([
            { "product_id": "A", "issue_datetime": "2024-06-01 00:00:00.000", "message": "" },
            { "product_id": "B", "issue_datetime": "2024-06-02 00:00:00.000", "message": "" }
        ]);
        assert_eq!(parse_alerts(&v, 1).len(), 1, "max caps the result");
        assert!(parse_alerts(&json!({}), 5).is_empty());
    }
}
