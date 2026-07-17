//! Repeater-directory parsing + query planning for the "Program" section.
//!
//! Pure logic: normalize the two feeds (RepeaterBook JSON export, hearham.com
//! open API) into [`RepeaterRecord`]s, plan which RepeaterBook state exports a
//! radius query needs (their API has no lat/lng parameter — like CHIRP, we pull
//! whole states and haversine-filter client-side), filter/sort by distance, and
//! convert a picked repeater into a programmable [`Channel`]. HTTP lives in
//! `crate::live::{repeaterbook, hearham}`; caching/orchestration in the shell.

use crate::geo::{bearing_deg, haversine_km, latlon_to_maidenhead};
use crate::gridstate::state_for_grid;
use crate::memchan::{ChanMode, Channel, ChannelSource, Duplex, ToneMode};
use serde::{Deserialize, Serialize};

/// Which directory a record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeaterSource {
    Repeaterbook,
    Hearham,
}

/// One repeater, normalized across both feeds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepeaterRecord {
    pub source: RepeaterSource,
    pub source_id: String,
    pub callsign: String,
    /// Repeater OUTPUT in MHz — what you listen to.
    pub output_mhz: f64,
    /// Repeater INPUT in MHz — what you transmit on.
    pub input_mhz: f64,
    /// CTCSS required on the uplink ("PL"), Hz.
    pub ctcss_enc_hz: Option<f32>,
    /// CTCSS on the downlink (tone squelch), Hz.
    pub ctcss_dec_hz: Option<f32>,
    /// DCS code (uplink), when the machine uses digital code squelch.
    pub dcs: Option<u16>,
    pub lat: f64,
    pub lon: f64,
    pub city: String,
    pub county: String,
    pub state: String,
    pub fm: bool,
    pub dmr: bool,
    pub dstar: bool,
    pub fusion: bool,
    pub dmr_color_code: Option<u8>,
    pub bandwidth_khz: Option<f32>,
    /// On-air per the directory (RB "Operational Status", hearham `operational`).
    pub operational: bool,
    /// Open for general use (RB "Use" == OPEN; hearham has no field → true).
    pub open_use: bool,
    /// Filled by [`filter_sort`] — distance/bearing from the query origin.
    pub distance_km: f64,
    pub bearing_deg: f64,
}

// ── field-tolerant JSON helpers (feeds mix strings and numbers freely) ──────

fn jstr(v: &serde_json::Value, k: &str) -> String {
    match v.get(k) {
        Some(serde_json::Value::String(s)) => s.trim().to_string(),
        Some(serde_json::Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

fn jf64(v: &serde_json::Value, k: &str) -> Option<f64> {
    match v.get(k) {
        Some(serde_json::Value::Number(n)) => n.as_f64(),
        Some(serde_json::Value::String(s)) => s.trim().parse().ok(),
        _ => None,
    }
}

/// RB truthy flags arrive as "Yes"/"No", 1/0, or true/false.
fn jyes(v: &serde_json::Value, k: &str) -> bool {
    match v.get(k) {
        Some(serde_json::Value::String(s)) => {
            let s = s.trim();
            s.eq_ignore_ascii_case("yes") || s == "1"
        }
        Some(serde_json::Value::Number(n)) => n.as_i64().unwrap_or(0) != 0,
        Some(serde_json::Value::Bool(b)) => *b,
        _ => false,
    }
}

/// Parse a tone field: `"103.5"` → CTCSS Hz; `"D023"`/`"023"` style DCS handled
/// by the caller; empty / `"CSQ"` / zero → None.
fn tone_hz(s: &str) -> Option<f32> {
    let t = s.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("csq") {
        return None;
    }
    let hz: f32 = t.parse().ok()?;
    // CTCSS tones live in 60–260 Hz; anything else (0.00, garbage) is "none".
    (60.0..300.0).contains(&hz).then_some(hz)
}

/// A DCS code like `"D023"` / `"D023N"` → 23.
fn dcs_code(s: &str) -> Option<u16> {
    let t = s.trim().trim_start_matches(['D', 'd']);
    let digits: String = t.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    digits.parse().ok()
}

/// hearham stuffs DMR color codes into the tone fields as `"CC2"`.
fn cc_code(s: &str) -> Option<u8> {
    let t = s.trim();
    let rest = t.strip_prefix("CC").or_else(|| t.strip_prefix("cc"))?;
    rest.parse().ok()
}

// ── RepeaterBook ────────────────────────────────────────────────────────────

/// Parse a RepeaterBook `export.php` JSON payload (either the `{count, results:
/// [...]}` wrapper or a bare array). Rows missing frequency or coordinates are
/// skipped — without them the record can neither program nor rank. Malformed
/// JSON → empty (transport reports the error separately).
pub fn parse_repeaterbook_json(json: &str) -> Vec<RepeaterRecord> {
    let root: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let rows = match root.get("results") {
        Some(serde_json::Value::Array(a)) => a.clone(),
        _ => match root {
            serde_json::Value::Array(a) => a,
            _ => Vec::new(),
        },
    };
    rows.iter()
        .filter_map(|v| {
            let output_mhz = jf64(v, "Frequency")?;
            let input_mhz = jf64(v, "Input Freq").unwrap_or(output_mhz);
            let lat = jf64(v, "Lat")?;
            let lon = jf64(v, "Long")?;
            if output_mhz <= 0.0 || (lat == 0.0 && lon == 0.0) {
                return None;
            }
            let pl = jstr(v, "PL");
            let tsq = jstr(v, "TSQ");
            let status = jstr(v, "Operational Status");
            Some(RepeaterRecord {
                source: RepeaterSource::Repeaterbook,
                source_id: format!("{}-{}", jstr(v, "State ID"), jstr(v, "Rptr ID")),
                callsign: jstr(v, "Callsign"),
                output_mhz,
                input_mhz,
                ctcss_enc_hz: tone_hz(&pl),
                ctcss_dec_hz: tone_hz(&tsq),
                dcs: if pl.starts_with(['D', 'd']) {
                    dcs_code(&pl)
                } else {
                    None
                },
                lat,
                lon,
                city: jstr(v, "Nearest City"),
                county: jstr(v, "County"),
                state: jstr(v, "State"),
                fm: jyes(v, "FM Analog"),
                dmr: jyes(v, "DMR"),
                dstar: jyes(v, "D-Star"),
                fusion: jyes(v, "System Fusion"),
                dmr_color_code: jf64(v, "DMR Color Code").map(|c| c as u8),
                bandwidth_khz: jf64(v, "FM Bandwidth").map(|b| b as f32),
                // RB reports "On-air" / "Off-air" / "Unknown"; only a positive
                // off-air marks it down (unknown machines still get programmed).
                operational: !status.eq_ignore_ascii_case("off-air"),
                open_use: jstr(v, "Use").is_empty() || jstr(v, "Use").eq_ignore_ascii_case("open"),
                distance_km: 0.0,
                bearing_deg: 0.0,
            })
        })
        .collect()
}

// ── hearham ─────────────────────────────────────────────────────────────────

/// Parse the hearham.com `/api/repeaters/v1` payload (bare array; `frequency` +
/// `offset` in Hz as integers; tones as strings — `"0.00"`/`""` = none, and DMR
/// rows carry the color code as `"CC2"` in `encode`).
pub fn parse_hearham_json(json: &str) -> Vec<RepeaterRecord> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(json).unwrap_or_default();
    rows.iter()
        .filter_map(|v| {
            let freq_hz = jf64(v, "frequency")?;
            let lat = jf64(v, "latitude")?;
            let lon = jf64(v, "longitude")?;
            if freq_hz <= 0.0 || (lat == 0.0 && lon == 0.0) {
                return None;
            }
            let offset_hz = jf64(v, "offset").unwrap_or(0.0);
            let output_mhz = freq_hz / 1e6;
            let mode = jstr(v, "mode").to_ascii_uppercase();
            let enc = jstr(v, "encode");
            let dec = jstr(v, "decode");
            let is_dmr = mode == "DMR" || cc_code(&enc).is_some();
            Some(RepeaterRecord {
                source: RepeaterSource::Hearham,
                source_id: jstr(v, "id"),
                callsign: jstr(v, "callsign"),
                output_mhz,
                input_mhz: (freq_hz + offset_hz) / 1e6,
                ctcss_enc_hz: tone_hz(&enc),
                ctcss_dec_hz: tone_hz(&dec),
                dcs: None,
                lat,
                lon,
                city: jstr(v, "city"),
                county: String::new(),
                state: String::new(),
                fm: mode == "FM" || mode.is_empty(),
                dmr: is_dmr,
                dstar: mode == "D-STAR" || mode == "DSTAR",
                fusion: mode == "FUSION" || mode == "YSF" || mode == "C4FM",
                dmr_color_code: cc_code(&enc),
                bandwidth_khz: None,
                operational: jf64(v, "operational").unwrap_or(1.0) != 0.0,
                open_use: jstr(v, "restriction").is_empty(),
                distance_km: 0.0,
                bearing_deg: 0.0,
            })
        })
        .collect()
}

// ── query planning ──────────────────────────────────────────────────────────

/// US state 2-letter code → RepeaterBook `state_id` (US FIPS, zero-padded).
pub fn state_id_for(code: &str) -> Option<&'static str> {
    Some(match code {
        "AL" => "01",
        "AK" => "02",
        "AZ" => "04",
        "AR" => "05",
        "CA" => "06",
        "CO" => "08",
        "CT" => "09",
        "DE" => "10",
        "FL" => "12",
        "GA" => "13",
        "HI" => "15",
        "ID" => "16",
        "IL" => "17",
        "IN" => "18",
        "IA" => "19",
        "KS" => "20",
        "KY" => "21",
        "LA" => "22",
        "ME" => "23",
        "MD" => "24",
        "MA" => "25",
        "MI" => "26",
        "MN" => "27",
        "MS" => "28",
        "MO" => "29",
        "MT" => "30",
        "NE" => "31",
        "NV" => "32",
        "NH" => "33",
        "NJ" => "34",
        "NM" => "35",
        "NY" => "36",
        "NC" => "37",
        "ND" => "38",
        "OH" => "39",
        "OK" => "40",
        "OR" => "41",
        "PA" => "42",
        "RI" => "44",
        "SC" => "45",
        "SD" => "46",
        "TN" => "47",
        "TX" => "48",
        "UT" => "49",
        "VT" => "50",
        "VA" => "51",
        "WA" => "53",
        "WV" => "54",
        "WI" => "55",
        "WY" => "56",
        _ => return None,
    })
}

/// Which RepeaterBook state exports cover a radius query. The origin's state
/// plus the states under 8 compass points at the radius — so a query near a
/// border pulls the neighbor too instead of silently dropping half the circle.
/// Empty ⇒ the origin isn't resolvable to a US state (non-US → hearham path).
pub fn plan_states(origin: (f64, f64), radius_km: f64) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut push = |lat: f64, lon: f64| {
        let grid = latlon_to_maidenhead(lat, lon);
        if let Some(code) = state_for_grid(&grid) {
            if let Some(id) = state_id_for(code) {
                if !out.iter().any(|s| s == id) {
                    out.push(id.to_string());
                }
            }
        }
    };
    push(origin.0, origin.1);
    for i in 0..8 {
        let b = f64::from(i) * 45.0;
        let p = crate::geo::destination_point(origin, b, radius_km);
        push(p.0, p.1);
    }
    out
}

/// Fill distance/bearing from `origin`, drop records outside `radius_km`, and
/// sort nearest-first (ties by output frequency for a stable order).
pub fn filter_sort(
    records: &[RepeaterRecord],
    origin: (f64, f64),
    radius_km: f64,
) -> Vec<RepeaterRecord> {
    let mut out: Vec<RepeaterRecord> = records
        .iter()
        .filter_map(|r| {
            let d = haversine_km(origin, (r.lat, r.lon));
            if d > radius_km {
                return None;
            }
            let mut r = r.clone();
            r.distance_km = d;
            r.bearing_deg = bearing_deg(origin, (r.lat, r.lon));
            Some(r)
        })
        .collect();
    out.sort_by(|a, b| {
        a.distance_km
            .partial_cmp(&b.distance_km)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.output_mhz
                    .partial_cmp(&b.output_mhz)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    out
}

// ── record → channel ────────────────────────────────────────────────────────

/// Offsets bigger than this are treated as a true split (cross-band or exotic)
/// rather than a +/- shift. The largest conventional shift is 23cm's 12–20 MHz.
const MAX_SHIFT_MHZ: f64 = 30.0;

/// Convert a picked repeater into a programmable [`Channel`].
///
/// Duplex/offset derive from `input − output`: within ±1 Hz ⇒ simplex; a
/// conventional magnitude ⇒ Plus/Minus with the EXACT magnitude (odd splits
/// like +1.0 MHz on 2m stay correct); beyond [`MAX_SHIFT_MHZ`] ⇒ `Split` with
/// the absolute input frequency. Tone: uplink PL ⇒ `Tone` (the safe default —
/// TSQL would mute a machine that doesn't transmit tone); downlink-only tone ⇒
/// `TSql`; DCS ⇒ `Dtcs`. Mode: FM unless the record is digital-only.
pub fn to_channel(r: &RepeaterRecord) -> Channel {
    let diff = r.input_mhz - r.output_mhz;
    let (duplex, offset_mhz) = if diff.abs() < 1e-6 {
        (Duplex::Simplex, 0.0)
    } else if diff.abs() > MAX_SHIFT_MHZ {
        (Duplex::Split, r.input_mhz)
    } else if diff > 0.0 {
        (Duplex::Plus, diff)
    } else {
        (Duplex::Minus, -diff)
    };
    let (tone_mode, rtone, ctone) = match (r.dcs, r.ctcss_enc_hz, r.ctcss_dec_hz) {
        (Some(_), _, _) => (ToneMode::Dtcs, 88.5, 88.5),
        (None, Some(enc), dec) => (ToneMode::Tone, enc, dec.unwrap_or(enc)),
        (None, None, Some(dec)) => (ToneMode::TSql, dec, dec),
        (None, None, None) => (ToneMode::None, 88.5, 88.5),
    };
    let mode = if r.fm {
        ChanMode::Fm
    } else if r.dmr {
        ChanMode::Dmr
    } else if r.dstar {
        ChanMode::Dstar
    } else if r.fusion {
        ChanMode::Fusion
    } else {
        ChanMode::Fm
    };
    let name = if r.callsign.is_empty() {
        r.city.clone()
    } else {
        r.callsign.clone()
    };
    Channel {
        id: format!(
            "{}:{}",
            match r.source {
                RepeaterSource::Repeaterbook => "rb",
                RepeaterSource::Hearham => "hh",
            },
            r.source_id
        ),
        name,
        rx_mhz: r.output_mhz,
        duplex,
        offset_mhz,
        tone_mode,
        rtone_hz: rtone,
        ctone_hz: ctone,
        dtcs_code: r.dcs.unwrap_or(23),
        mode,
        comment: if r.city.is_empty() {
            r.callsign.clone()
        } else {
            r.city.clone()
        },
        dmr_color_code: r.dmr_color_code,
        source: Some(ChannelSource {
            source: match r.source {
                RepeaterSource::Repeaterbook => "repeaterbook".into(),
                RepeaterSource::Hearham => "hearham".into(),
            },
            source_id: r.source_id.clone(),
            callsign: r.callsign.clone(),
        }),
        ..Channel::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEARHAM_FIXTURE: &str = include_str!("../tests/fixtures/hearham_v1.json");
    const RB_FIXTURE: &str = include_str!("../tests/fixtures/repeaterbook_export.json");

    // EN52 center — the WI/IL border area (exercises multi-state planning).
    const EN52: (f64, f64) = (42.5, -89.0);

    #[test]
    fn hearham_fixture_parses() {
        let recs = parse_hearham_json(HEARHAM_FIXTURE);
        assert!(recs.len() >= 15, "parsed {}", recs.len());
        // VE7RHS: 441.975 MHz +5 MHz, 100.0 enc/dec, FM, operational (Hz→MHz).
        let r = recs.iter().find(|r| r.callsign == "VE7RHS").unwrap();
        assert!((r.output_mhz - 441.975).abs() < 1e-9);
        assert!((r.input_mhz - 446.975).abs() < 1e-9);
        assert_eq!(r.ctcss_enc_hz, Some(100.0));
        assert!(r.fm && r.operational);
        // hearham "CC1" tone → DMR color code, NOT a CTCSS tone.
        let dmr = recs.iter().find(|r| r.dmr).unwrap();
        assert!(dmr.ctcss_enc_hz.is_none());
        assert!(dmr.dmr_color_code.is_some());
        // A "123.0" plain FM tone parses as CTCSS.
        let we9com = recs.iter().find(|r| r.callsign == "WE9COM").unwrap();
        assert_eq!(we9com.ctcss_enc_hz, Some(123.0));
        // A blank-callsign row still parses (name falls back to city later).
        assert!(recs.iter().any(|r| r.callsign.is_empty()));
        // Non-operational rows are flagged, not dropped.
        assert!(recs.iter().any(|r| !r.operational));
    }

    #[test]
    fn repeaterbook_fixture_parses() {
        let recs = parse_repeaterbook_json(RB_FIXTURE);
        assert_eq!(
            recs.len(),
            4,
            "one row is missing coords and must be skipped"
        );
        let w9abc = recs.iter().find(|r| r.callsign == "W9ABC").unwrap();
        assert!((w9abc.output_mhz - 146.94).abs() < 1e-9);
        assert!((w9abc.input_mhz - 146.34).abs() < 1e-9);
        assert_eq!(w9abc.ctcss_enc_hz, Some(103.5));
        assert!(w9abc.fm && w9abc.operational && w9abc.open_use);
        // Off-air row parses but is flagged.
        let off = recs.iter().find(|r| r.callsign == "K9OFF").unwrap();
        assert!(!off.operational);
        // DMR-only machine.
        let dmr = recs.iter().find(|r| r.callsign == "N9DMR").unwrap();
        assert!(dmr.dmr && !dmr.fm);
        assert_eq!(dmr.dmr_color_code, Some(1));
    }

    #[test]
    fn plan_states_border_union() {
        // EN52 sits on the WI/IL line: a 60 km circle must plan BOTH states.
        let states = plan_states(EN52, 60.0);
        assert!(states.contains(&"55".to_string()), "WI missing: {states:?}");
        assert!(states.contains(&"17".to_string()), "IL missing: {states:?}");
        // Non-US origin plans nothing (falls to the hearham path).
        assert!(plan_states((48.85, 2.35), 50.0).is_empty());
    }

    #[test]
    fn filter_sort_radius_and_order() {
        let recs = parse_hearham_json(HEARHAM_FIXTURE);
        let near = filter_sort(&recs, EN52, 80.0);
        assert!(!near.is_empty());
        assert!(near
            .windows(2)
            .all(|w| w[0].distance_km <= w[1].distance_km));
        assert!(near.iter().all(|r| r.distance_km <= 80.0));
        // Vancouver machines are ~2,900 km out — never inside an 80 km circle.
        assert!(near.iter().all(|r| r.callsign != "VE7RHS"));
        let far = filter_sort(&recs, EN52, 25.0);
        assert!(far.len() <= near.len());
    }

    #[test]
    fn to_channel_duplex_tone_mapping() {
        let recs = parse_repeaterbook_json(RB_FIXTURE);
        let w9abc = to_channel(recs.iter().find(|r| r.callsign == "W9ABC").unwrap());
        assert_eq!(w9abc.duplex, Duplex::Minus);
        assert!((w9abc.offset_mhz - 0.6).abs() < 1e-9);
        assert_eq!(w9abc.tone_mode, ToneMode::Tone);
        assert_eq!(w9abc.rtone_hz, 103.5);
        assert_eq!(w9abc.mode, ChanMode::Fm);
        assert!((w9abc.tx_mhz() - 146.34).abs() < 1e-9);
        // Odd split (cross-band input) → Split with absolute TX.
        let odd = RepeaterRecord {
            input_mhz: 445.5,
            ..recs.iter().find(|r| r.callsign == "W9ABC").unwrap().clone()
        };
        let c = to_channel(&odd);
        assert_eq!(c.duplex, Duplex::Split);
        assert!((c.offset_mhz - 445.5).abs() < 1e-9);
        assert!((c.tx_mhz() - 445.5).abs() < 1e-9);
    }

    #[test]
    fn state_id_table_complete() {
        for code in crate::awards::WAS_STATES {
            assert!(state_id_for(code).is_some(), "missing FIPS for {code}");
        }
        assert_eq!(state_id_for("WI"), Some("55"));
        assert_eq!(state_id_for("XX"), None);
    }
}
