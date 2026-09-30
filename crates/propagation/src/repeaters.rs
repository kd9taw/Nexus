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
    /// Channel width, kHz, when the source says: RepeaterBook's "FM Bandwidth"; hearham's
    /// "NFM" is recorded as the narrowband 12.5. A narrow machine programs as NFM.
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

/// A bandwidth in kHz: a number, or a string of one, with or without the unit RepeaterBook's
/// own pages print ("12.5", 12.5, "12.5 kHz"). Anything else is not known.
fn jkhz(v: &serde_json::Value, k: &str) -> Option<f32> {
    match v.get(k) {
        Some(serde_json::Value::String(s)) => {
            let t = s.trim().to_ascii_lowercase();
            t.strip_suffix("khz").unwrap_or(&t).trim().parse().ok()
        }
        _ => jf64(v, k).map(|b| b as f32),
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
                bandwidth_khz: jkhz(v, "FM Bandwidth"),
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

/// The mode words a hearham `mode` value is made of. The directory joins them with `/`
/// ("YSF/FM", "P25/NFM") or `+` ("FM+YSF"), and a few values run them together with no
/// separator ("D-STARDMR", "P25YSFD-STARNXDNDMR/FM"): every distinct value in the
/// directory, 29 of them, was counted on 2026-09-30. No word is a prefix of another, so a
/// scan takes the one word that fits. A piece holding anything else is dropped whole
/// rather than guessed at. Expects the value already upper-cased.
fn hearham_mode_words(mode: &str) -> Vec<&'static str> {
    const WORDS: [&str; 14] = [
        "FM", "NFM", "DMR", "D-STAR", "DSTAR", "YSF", "C4FM", "FUSION", "P25", "NXDN", "M17",
        "AX25", "ATV", "TV",
    ];
    let mut out = Vec::new();
    for piece in mode.split(['/', '+']) {
        let mut rest = piece.trim();
        let mut words = Vec::new();
        while let Some(w) = WORDS.iter().find(|w| rest.starts_with(**w)) {
            words.push(*w);
            rest = &rest[w.len()..];
        }
        if rest.is_empty() {
            out.extend(words);
        }
    }
    out
}

/// The 104 standard DCS codes (the set CHIRP's generic CSV accepts), each written as its
/// three octal digits read as a decimal number, the way [`Channel::dtcs_code`] holds one.
const DCS_CODES: [u16; 104] = [
    23, 25, 26, 31, 32, 36, 43, 47, 51, 53, 54, 65, 71, 72, 73, 74, 114, 115, 116, 122, 125, 131,
    132, 134, 143, 145, 152, 155, 156, 162, 165, 172, 174, 205, 212, 223, 225, 226, 243, 244, 245,
    246, 251, 252, 255, 261, 263, 265, 266, 271, 274, 306, 311, 315, 325, 331, 332, 343, 346, 351,
    356, 364, 365, 371, 411, 412, 413, 423, 431, 432, 445, 446, 452, 454, 455, 462, 464, 465, 466,
    503, 506, 516, 523, 526, 532, 546, 565, 606, 612, 624, 627, 631, 632, 654, 662, 664, 703, 712,
    723, 731, 732, 734, 743, 754,
];

/// A DCS code in one part of a hearham tone field. Every shape the directory used on
/// 2026-09-30 is read: "DCS023", "DCS 043", "DCS411N", "D023", "DPL411", "DPL 432" and the
/// two-digit "DCS51" (code 051). Only a standard code is taken ("DCS 740" is none), and a
/// bare number, a trailing "*" or ".0" is not, since what those mean is not written down.
fn hearham_dcs_part(part: &str) -> Option<u16> {
    let p = part.trim().to_ascii_uppercase();
    let rest = p
        .strip_prefix("DCS")
        .or_else(|| p.strip_prefix("DPL"))
        .or_else(|| p.strip_prefix('D'))?;
    let digits = rest.trim_start();
    let digits = digits.strip_suffix('N').unwrap_or(digits);
    if !(2..=3).contains(&digits.len()) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let code: u16 = digits.parse().ok()?;
    DCS_CODES.contains(&code).then_some(code)
}

/// The squelch a hearham tone field asks for: a CTCSS tone or a DCS code.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Squelch {
    Ctcss(f32),
    Dcs(u16),
}

/// The squelch in a hearham tone field. A machine that runs several modes can have one
/// parameter per mode joined with `/`: "CC1/146.2" (the DMR colour code, then the FM tone),
/// "NAC293/100.0", "67.0/CC9/RAN1/NAC293/C/CAN0", "NAC353/D244" (58 rows on 2026-09-30). The
/// squelch is the part that reads as a tone or a DCS code. Parts that disagree, two different
/// tones ("88.5/71.9") or a tone and a code ("77.0/D454"), are left unknown rather than
/// guessed at. A field with no `/` reads as [`tone_hz`] or [`hearham_dcs_part`] reads it.
fn hearham_squelch(field: &str) -> Option<Squelch> {
    let mut found = field.split('/').filter_map(|part| {
        tone_hz(part)
            .map(Squelch::Ctcss)
            .or_else(|| hearham_dcs_part(part).map(Squelch::Dcs))
    });
    let first = found.next()?;
    found.all(|s| s == first).then_some(first)
}

/// The DMR colour code in a hearham tone field: its `"CCn"` part, alone or joined with
/// `/` as in [`hearham_squelch`].
fn hearham_cc(field: &str) -> Option<u8> {
    field.split('/').find_map(cc_code)
}

/// Parse the hearham.com `/api/repeaters/v1` payload (bare array; `frequency` +
/// `offset` in Hz as integers; tones as strings — `"0.00"`/`""` = none, and DMR
/// rows carry the color code as `"CC2"` in `encode`, joined with `/` to the FM tone
/// on a machine that runs both, see [`hearham_squelch`]).
///
/// `mode` can name several modes ([`hearham_mode_words`]). A machine is FM when any
/// of them is FM or NFM (or the mode is empty), and each digital flag comes from its
/// own word, so "YSF/FM" is an FM machine that is also Fusion. A mode the record has
/// no field for (P25, NXDN, M17, …) sets nothing and never makes a machine FM.
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
            let words = hearham_mode_words(&mode);
            let names = |any: &[&str]| words.iter().any(|w| any.contains(w));
            let enc = jstr(v, "encode");
            let dec = jstr(v, "decode");
            let is_dmr = names(&["DMR"]) || hearham_cc(&enc).is_some();
            let up = hearham_squelch(&enc);
            let down = hearham_squelch(&dec);
            let ctcss = |s: Option<Squelch>| match s {
                Some(Squelch::Ctcss(hz)) => Some(hz),
                _ => None,
            };
            // DCS is the uplink's (`encode`). A channel carries ONE code for both directions,
            // so a downlink naming a different tone or code (a cross-mode machine) leaves the
            // code unread rather than guessed at.
            let dcs = match (up, down) {
                (Some(Squelch::Dcs(code)), None) => Some(code),
                (Some(Squelch::Dcs(code)), Some(Squelch::Dcs(d))) if d == code => Some(code),
                _ => None,
            };
            Some(RepeaterRecord {
                source: RepeaterSource::Hearham,
                source_id: jstr(v, "id"),
                callsign: jstr(v, "callsign"),
                output_mhz,
                input_mhz: (freq_hz + offset_hz) / 1e6,
                ctcss_enc_hz: ctcss(up),
                ctcss_dec_hz: ctcss(down),
                dcs,
                lat,
                lon,
                city: jstr(v, "city"),
                county: String::new(),
                state: String::new(),
                fm: mode.is_empty() || names(&["FM", "NFM"]),
                dmr: is_dmr,
                dstar: names(&["D-STAR", "DSTAR"]),
                fusion: names(&["YSF", "C4FM", "FUSION"]),
                dmr_color_code: hearham_cc(&enc),
                bandwidth_khz: names(&["NFM"]).then_some(NARROW_FM_KHZ),
                operational: jf64(v, "operational").unwrap_or(1.0) != 0.0,
                open_use: jstr(v, "restriction").is_empty(),
                distance_km: 0.0,
                bearing_deg: 0.0,
            })
        })
        .collect()
}

// ── query planning ──────────────────────────────────────────────────────────

/// US state 2-letter code ↔ RepeaterBook `state_id` (US FIPS, zero-padded).
///
/// ONE table, read in both directions, so they cannot drift: [`state_id_for`] goes
/// forwards to plan the fetch, [`state_code_for_id`] comes back so a coverage report
/// can name the state an operator recognizes instead of a FIPS number.
const STATE_IDS: &[(&str, &str)] = &[
    ("AL", "01"),
    ("AK", "02"),
    ("AZ", "04"),
    ("AR", "05"),
    ("CA", "06"),
    ("CO", "08"),
    ("CT", "09"),
    ("DE", "10"),
    ("FL", "12"),
    ("GA", "13"),
    ("HI", "15"),
    ("ID", "16"),
    ("IL", "17"),
    ("IN", "18"),
    ("IA", "19"),
    ("KS", "20"),
    ("KY", "21"),
    ("LA", "22"),
    ("ME", "23"),
    ("MD", "24"),
    ("MA", "25"),
    ("MI", "26"),
    ("MN", "27"),
    ("MS", "28"),
    ("MO", "29"),
    ("MT", "30"),
    ("NE", "31"),
    ("NV", "32"),
    ("NH", "33"),
    ("NJ", "34"),
    ("NM", "35"),
    ("NY", "36"),
    ("NC", "37"),
    ("ND", "38"),
    ("OH", "39"),
    ("OK", "40"),
    ("OR", "41"),
    ("PA", "42"),
    ("RI", "44"),
    ("SC", "45"),
    ("SD", "46"),
    ("TN", "47"),
    ("TX", "48"),
    ("UT", "49"),
    ("VT", "50"),
    ("VA", "51"),
    ("WA", "53"),
    ("WV", "54"),
    ("WI", "55"),
    ("WY", "56"),
];

/// US state 2-letter code → RepeaterBook `state_id` (US FIPS, zero-padded).
pub fn state_id_for(code: &str) -> Option<&'static str> {
    STATE_IDS
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, id)| *id)
}

/// RepeaterBook `state_id` → US state 2-letter code — the inverse of [`state_id_for`],
/// for naming a state that a search planned but could not fetch.
pub fn state_code_for_id(state_id: &str) -> Option<&'static str> {
    STATE_IDS
        .iter()
        .find(|(_, id)| *id == state_id)
        .map(|(c, _)| *c)
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

/// What ONE planned state contributed to a multi-state search.
///
/// A [`plan_states`] entry becomes one of these. `body` is `None` when the state
/// yielded nothing at all — the fetch failed with no cache to fall back on, or the
/// per-state throttle blocked it — which is the case that used to vanish.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StateFetch {
    /// RepeaterBook `state_id` (US FIPS), exactly as [`plan_states`] produced it.
    pub state_id: String,
    /// The export payload, from a fresh fetch or from cache. `None` = nothing.
    pub body: Option<String>,
    /// When `body` was obtained (unix secs). Meaningless when `body` is `None`.
    pub fetched_utc: i64,
    /// `body` came from cache because the fetch failed or was rate-limited.
    pub stale: bool,
}

/// What a multi-state search actually covered — the records it found AND the states
/// it never heard from.
///
/// **The whole point of `missing`.** A search that plans two states and hears from one
/// used to be reported exactly like a search that heard from both: the caller kept a
/// single `any` flag, so "there are no repeaters near you" and "we could not fetch your
/// state" produced the same screen (issue #241). The states are carried as 2-letter
/// codes because that is what an operator recognizes — a FIPS number names nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct StateCoverage {
    /// Every record parsed out of the states that did answer.
    pub records: Vec<RepeaterRecord>,
    /// Oldest payload timestamp behind `records` — the "as of" stamp.
    pub oldest_utc: i64,
    /// At least one served body was stale cache.
    pub stale: bool,
    /// Planned states that returned NOTHING, as 2-letter codes, in plan order.
    /// An unrecognized `state_id` is passed through verbatim rather than dropped —
    /// losing it here would be the same silence this type exists to end.
    pub missing: Vec<String>,
}

impl StateCoverage {
    /// True when at least one planned state answered. A search where NO state answered
    /// has no RepeaterBook result at all and falls through to the other directory.
    pub fn any_served(&self) -> bool {
        self.oldest_utc != i64::MAX
    }
}

/// Fold each planned state's fetch into the records plus the coverage report.
pub fn fold_state_fetches(fetches: &[StateFetch]) -> StateCoverage {
    let mut records = Vec::new();
    let mut oldest = i64::MAX;
    let mut stale = false;
    let mut missing = Vec::new();
    for f in fetches {
        match &f.body {
            Some(body) => {
                records.extend(parse_repeaterbook_json(body));
                oldest = oldest.min(f.fetched_utc);
                stale |= f.stale;
            }
            None => missing.push(
                state_code_for_id(&f.state_id)
                    .unwrap_or(&f.state_id)
                    .to_string(),
            ),
        }
    }
    StateCoverage {
        records,
        oldest_utc: oldest,
        stale,
        missing,
    }
}

/// A major band the source lists NO repeater on, in an area where it lists
/// some — a signal the directory is incomplete here rather than the country
/// being genuinely empty. `None` when both bands are represented, or when
/// there is nothing to judge.
///
/// Band absence is the discriminator, not a low count. Measured against
/// hearham: it has nine machines within 50 mi of Bozeman MT and not one on
/// 2 m, which is not a true description of Montana. A genuinely thin area
/// stays band-balanced (Amarillo TX: three on 2 m, three on 70 cm), so a raw
/// count would flag real empty country while missing the actual data gap.
pub fn missing_major_band(records: &[RepeaterRecord]) -> Option<&'static str> {
    if records.is_empty() {
        return None;
    }
    let two_m = records
        .iter()
        .any(|r| (144.0..=148.0).contains(&r.output_mhz));
    let seventy_cm = records
        .iter()
        .any(|r| (420.0..=450.0).contains(&r.output_mhz));
    match (two_m, seventy_cm) {
        (true, true) => None,
        (false, true) => Some("2 m"),
        (true, false) => Some("70 cm"),
        (false, false) => Some("2 m or 70 cm"),
    }
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

/// The narrowband FM channel width. A machine whose source gives this bandwidth or less
/// (RepeaterBook's "FM Bandwidth" 12.5; hearham's "NFM" is recorded as it) is an NFM channel.
const NARROW_FM_KHZ: f32 = 12.5;

/// Convert a picked repeater into a programmable [`Channel`].
///
/// Duplex/offset derive from `input − output`: within ±1 Hz ⇒ simplex; a
/// conventional magnitude ⇒ Plus/Minus with the EXACT magnitude (odd splits
/// like +1.0 MHz on 2m stay correct); beyond [`MAX_SHIFT_MHZ`] ⇒ `Split` with
/// the absolute input frequency. Tone: uplink PL ⇒ `Tone` (the safe default —
/// TSQL would mute a machine that doesn't transmit tone); downlink-only tone ⇒
/// `TSql`; DCS ⇒ `Dtcs`. Mode: FM, or NFM for a machine its source marks narrow
/// ([`NARROW_FM_KHZ`]), unless the record is digital-only.
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
        if r.bandwidth_khz.is_some_and(|khz| khz <= NARROW_FM_KHZ) {
            ChanMode::Nfm
        } else {
            ChanMode::Fm
        }
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
    /// RB_FIXTURE after the rb.hamradiotools.io proxy narrows each row to the
    /// fields this parser reads. Generated by running RB_FIXTURE through the
    /// Worker, so it is the real output shape and not a hand-written guess.
    const RB_NARROWED_FIXTURE: &str =
        include_str!("../tests/fixtures/repeaterbook_proxy_narrowed.json");

    // EN52 center — the WI/IL border area (exercises multi-state planning).
    const EN52: (f64, f64) = (42.5, -89.0);

    /// A one-row RepeaterBook export, in the `{results:[…]}` wrapper shape.
    fn rb_row(callsign: &str, mhz: f64, lat: f64, lon: f64) -> String {
        format!(
            r#"{{"results":[{{"Callsign":"{callsign}","Frequency":"{mhz}","Lat":"{lat}","Long":"{lon}","State ID":"42","Rptr ID":"1","FM Analog":"Yes"}}]}}"#
        )
    }

    /// Issue #241 (swinn): a search that hears from ONE of two planned states must be
    /// DISTINGUISHABLE from one that hears from both.
    ///
    /// The discriminator is the coverage report BY VALUE. A row count is NOT one: a
    /// genuinely thin area and a state that failed to fetch both produce a short list,
    /// which is exactly why the old `any` flag could not tell the operator apart from
    /// "there are no repeaters near you".
    #[test]
    fn a_state_that_answered_nothing_is_named_in_the_coverage() {
        let pa = StateFetch {
            state_id: "42".into(),
            body: Some(rb_row("W3ZGD", 146.865, 39.9, -76.6)),
            fetched_utc: 1000,
            stale: false,
        };
        let md_served = StateFetch {
            state_id: "24".into(),
            body: Some(rb_row("K3AE", 146.895, 39.7, -76.7)),
            fetched_utc: 900,
            stale: false,
        };
        let md_silent = StateFetch {
            state_id: "24".into(),
            body: None,
            fetched_utc: 0,
            stale: false,
        };

        // CONTROL — both states answered. Nothing is missing, and the stamp is the
        // OLDER of the two payloads.
        let both = fold_state_fetches(&[pa.clone(), md_served]);
        assert_eq!(both.records.len(), 2);
        assert_eq!(both.missing, Vec::<String>::new());
        assert!(both.any_served());
        assert_eq!(both.oldest_utc, 900);

        // THE DEFECT — Maryland answered nothing. Same call shape, and the observable
        // MUST differ: the absent state is named, not merely absent.
        let partial = fold_state_fetches(&[pa, md_silent]);
        assert_eq!(partial.records.len(), 1);
        assert_eq!(
            partial.missing,
            vec!["MD".to_string()],
            "a state that returned nothing must be named"
        );
        assert!(partial.any_served());
        assert_eq!(partial.oldest_utc, 1000);
    }

    /// No state answered at all: there is no RepeaterBook result to report, and every
    /// planned state is named. An unrecognized id is passed through rather than dropped.
    #[test]
    fn no_state_answering_reports_every_planned_state() {
        let silent = |id: &str| StateFetch {
            state_id: id.into(),
            body: None,
            fetched_utc: 0,
            stale: false,
        };
        let cov = fold_state_fetches(&[silent("42"), silent("24"), silent("99")]);
        assert!(!cov.any_served(), "nothing answered");
        assert!(cov.records.is_empty());
        assert_eq!(cov.missing, vec!["PA", "MD", "99"]);
    }

    /// The two directions read ONE table, so a code that plans a fetch must name
    /// itself again when that fetch is the one that failed.
    #[test]
    fn state_code_and_id_round_trip() {
        for (code, id) in STATE_IDS {
            assert_eq!(state_id_for(code), Some(*id));
            assert_eq!(state_code_for_id(id), Some(*code));
        }
        assert_eq!(STATE_IDS.len(), 50);
        assert_eq!(state_id_for("ZZ"), None);
        assert_eq!(state_code_for_id("99"), None);
    }

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

    /// One hearham row in the `/api/repeaters/v1` shape (the fixture's fields), carrying
    /// `mode` and `encode` exactly as the directory writes them. The mode strings in the tests
    /// below are the directory's own (every distinct value, counted 2026-09-30); the rest of the
    /// row is filler.
    fn hh_row(mode: &str, encode: &str) -> RepeaterRecord {
        hh_row_tones(mode, encode, "")
    }

    /// [`hh_row`] with the downlink tone field (`decode`) too.
    fn hh_row_tones(mode: &str, encode: &str, decode: &str) -> RepeaterRecord {
        let json = format!(
            r#"[{{"id":1,"callsign":"W9TST","latitude":42.3,"longitude":-89.0,"city":"Rockford, Illinois","group":"","internet_node":"","mode":"{mode}","encode":"{encode}","decode":"{decode}","frequency":146940000,"offset":-600000,"description":"","power":"unknown","operational":1,"restriction":""}}]"#
        );
        let mut recs = parse_hearham_json(&json);
        assert_eq!(recs.len(), 1, "the {mode:?} row did not parse");
        recs.remove(0)
    }

    /// The flags a record carries, by name — what Program reads: `fm` decides whether the
    /// machine is listed by default and can be added, the others pick its badge.
    fn flag_names(r: &RepeaterRecord) -> Vec<&'static str> {
        [
            (r.fm, "FM"),
            (r.dmr, "DMR"),
            (r.dstar, "D-STAR"),
            (r.fusion, "YSF"),
        ]
        .into_iter()
        .filter_map(|(on, name)| on.then_some(name))
        .collect()
    }

    /// Every case whose parsed flags differ from the expected ones, so a failure names each
    /// mode string at once rather than stopping at the first.
    fn wrong_flags(cases: &[(&str, &[&str])]) -> Vec<String> {
        cases
            .iter()
            .filter_map(|&(mode, want)| {
                let got = flag_names(&hh_row(mode, "100.0"));
                (got != want).then(|| format!("{mode:?}: got {got:?}, want {want:?}"))
            })
            .collect()
    }

    /// A hearham mode that names FM beside a digital mode is an FM machine that ALSO keeps the
    /// digital mode. Reading only an exact "FM" left these with no flag at all, so Program hid
    /// them and could not add them (315 "YSF/FM", 32 "DMR/FM", 18 "D-STAR/FM" rows, and the
    /// run-together forms, on 2026-09-30).
    #[test]
    fn a_hearham_mode_naming_fm_beside_a_digital_mode_is_fm_and_keeps_it() {
        let wrong = wrong_flags(&[
            ("YSF/FM", &["FM", "YSF"]),
            ("FM/YSF", &["FM", "YSF"]),
            ("FM+YSF", &["FM", "YSF"]),
            ("DMR/FM", &["FM", "DMR"]),
            ("D-STAR/FM", &["FM", "D-STAR"]),
            ("YSFD-STAR/FM", &["FM", "D-STAR", "YSF"]),
            ("P25YSFD-STARNXDNDMR/FM", &["FM", "DMR", "D-STAR", "YSF"]),
            ("P25YSFD-STARM17NXDNDMR/FM", &["FM", "DMR", "D-STAR", "YSF"]),
        ]);
        assert_eq!(wrong, Vec::<String>::new(), "FM beside a digital mode");

        // The DMR colour code still arrives in `encode` on such a row: it stays a colour code
        // (never a CTCSS tone), and the machine programs as FM.
        let r = hh_row("DMR/FM", "CC1");
        assert!(r.fm && r.dmr, "DMR/FM with CC1: {:?}", flag_names(&r));
        assert_eq!(r.dmr_color_code, Some(1));
        assert_eq!(r.ctcss_enc_hz, None);
        assert_eq!(to_channel(&r).mode, ChanMode::Fm);
    }

    /// "NFM" is narrow FM, so an NFM machine is an FM machine: listed, and addable. It programs
    /// as a narrow channel ([`a_machine_its_source_marks_narrow_is_an_nfm_channel`]).
    #[test]
    fn a_hearham_nfm_machine_is_fm() {
        let wrong = wrong_flags(&[
            ("NFM", &["FM"]),
            ("P25/NFM", &["FM"]),
            ("DMR/NFM", &["FM", "DMR"]),
            ("NXDN/NFM", &["FM"]),
        ]);
        assert_eq!(wrong, Vec::<String>::new(), "NFM");
        assert_eq!(to_channel(&hh_row("NFM", "100.0")).mode, ChanMode::Nfm);
    }

    /// FM beside a mode the record has no field for (P25, NXDN, M17) is still FM.
    #[test]
    fn a_hearham_mode_naming_fm_beside_an_unrecorded_mode_is_fm() {
        let wrong = wrong_flags(&[
            ("P25/FM", &["FM"]),
            ("NXDN/FM", &["FM"]),
            ("M17/FM", &["FM"]),
        ]);
        assert_eq!(wrong, Vec::<String>::new(), "FM beside P25/NXDN/M17");
    }

    /// A mode without FM in it stays digital-only (not listed by default, not addable), with
    /// each digital mode it names read, the run-together forms included. "C4FM" is Fusion: the
    /// letters FM inside a token are not FM.
    #[test]
    fn a_hearham_mode_without_fm_stays_digital_and_names_each_mode() {
        let wrong = wrong_flags(&[
            ("DMR", &["DMR"]),
            ("D-STAR", &["D-STAR"]),
            ("DSTAR", &["D-STAR"]),
            ("YSF", &["YSF"]),
            ("C4FM", &["YSF"]),
            ("FUSION", &["YSF"]),
            ("DMR/DSTAR", &["DMR", "D-STAR"]),
            ("P25/D-STAR", &["D-STAR"]),
            ("D-STARDMR", &["DMR", "D-STAR"]),
            ("P25YSFD-STARNXDNDMR", &["DMR", "D-STAR", "YSF"]),
        ]);
        assert_eq!(wrong, Vec::<String>::new(), "digital-only modes");
    }

    /// A tone field that joins one parameter per mode with `/` gives the FM tone and the DMR
    /// colour code in it. The machines in the tests above write theirs this way ("CC1/146.2",
    /// "NAC293/100.0"), and read as one value that was no tone at all, so they would program
    /// with no tone. Every tone field below is one the directory writes.
    #[test]
    fn a_hearham_tone_field_joining_several_modes_gives_its_tone_and_colour_code() {
        let cases: [(&str, &str, Option<f32>, Option<u8>); 9] = [
            ("DMR/FM", "CC1/146.2", Some(146.2), Some(1)),
            ("P25/FM", "NAC293/100.0", Some(100.0), None),
            ("P25/FM", "131.8/NAC293", Some(131.8), None),
            ("YSF/FM", "67.0/CC9/RAN1/NAC293/C/CAN0", Some(67.0), Some(9)),
            ("DMR/FM", "B/CC1", None, Some(1)),
            // Two different tones: which one opens the machine is not written, so neither.
            ("YSF/FM", "88.5/71.9", None, None),
            ("FM", "B/71.9/88.5", None, None),
            // One value reads as before.
            ("FM", "100.0", Some(100.0), None),
            ("DMR", "CC2", None, Some(2)),
        ];
        let wrong: Vec<String> = cases
            .iter()
            .filter_map(|&(mode, encode, tone, cc)| {
                let r = hh_row(mode, encode);
                let got = (r.ctcss_enc_hz, r.dmr_color_code);
                (got != (tone, cc))
                    .then(|| format!("{encode:?}: got {got:?}, want {:?}", (tone, cc)))
            })
            .collect();
        assert_eq!(wrong, Vec::<String>::new(), "joined tone fields");

        // What the operator exports: an FM channel with the tone that opens the machine.
        let c = to_channel(&hh_row("DMR/FM", "CC1/146.2"));
        assert_eq!(
            (c.mode, c.tone_mode, c.rtone_hz),
            (ChanMode::Fm, ToneMode::Tone, 146.2)
        );
    }

    /// hearham writes a DCS code in its tone field in several shapes, and every shape in the
    /// directory on 2026-09-30 is here. Each reads as its code, so the machine exports DTCS with
    /// it; read as a tone it was no squelch at all. A code outside the 104 standard ones, a bare
    /// number, and a trailing "*" or ".0" (their meaning is not written down) are not read.
    #[test]
    fn a_hearham_dcs_code_in_any_of_the_directorys_shapes_is_read() {
        let cases: [(&str, Option<u16>); 15] = [
            ("DCS023", Some(23)),
            ("DCS 043", Some(43)),
            ("DCS411N", Some(411)),
            ("D023", Some(23)),
            ("DPL411", Some(411)),
            ("DPL 432", Some(432)),
            ("DCS51", Some(51)),
            ("NAC353/D244", Some(244)),
            ("D244/NAC293", Some(244)),
            ("DCS 740", None),
            ("DCS100", None),
            ("DCS017", None),
            ("D031*", None),
            ("DCS172.0", None),
            ("023", None),
        ];
        let wrong: Vec<String> = cases
            .iter()
            .filter_map(|&(encode, want)| {
                let got = hh_row("FM", encode).dcs;
                (got != want).then(|| format!("{encode:?}: got {got:?}, want {want:?}"))
            })
            .collect();
        assert_eq!(wrong, Vec::<String>::new(), "DCS codes");

        // What the operator exports: DTCS with the code, and no CTCSS read out of it.
        let r = hh_row("NFM", "DCS023");
        assert_eq!(r.ctcss_enc_hz, None);
        let c = to_channel(&r);
        assert_eq!((c.tone_mode, c.dtcs_code), (ToneMode::Dtcs, 23));
    }

    /// The channel carries ONE squelch setting for both directions, so a DCS code whose other
    /// settings contradict it is left unread rather than guessed at: a tone and a code in one
    /// field ("77.0/D454"), or a downlink that names a different tone or code (a cross-mode
    /// machine, "D311" up and "100" down). A downlink with the same code, or one Nexus cannot
    /// read ("D031*"), leaves the uplink's code standing. Every pair below is one the directory
    /// writes.
    #[test]
    fn a_hearham_dcs_code_that_its_other_settings_contradict_is_not_read() {
        let cases: [(&str, &str, Option<u16>, Option<f32>); 7] = [
            ("DCS023", "DCS023", Some(23), None),
            ("D031", "D031*", Some(31), None),
            ("DCS172", "DCS172.0", Some(172), None),
            ("77.0/D454", "", None, None),
            ("DCS365", "DCS364", None, None),
            ("D311", "100", None, None),
            ("D244/NAC293", "131.8/NAC293", None, None),
        ];
        let wrong: Vec<String> = cases
            .iter()
            .filter_map(|&(encode, decode, dcs, tone)| {
                let r = hh_row_tones("NFM", encode, decode);
                let got = (r.dcs, r.ctcss_enc_hz);
                (got != (dcs, tone))
                    .then(|| format!("{encode:?}/{decode:?}: got {got:?}, want {:?}", (dcs, tone)))
            })
            .collect();
        assert_eq!(wrong, Vec::<String>::new(), "contradicted DCS codes");
    }

    /// A one-row RepeaterBook export carrying `bandwidth` as the raw JSON value of its
    /// "FM Bandwidth" field (`null` = absent).
    fn rb_row_bw(bandwidth: &str) -> RepeaterRecord {
        let json = format!(
            r#"{{"results":[{{"Callsign":"W9NAR","Frequency":"146.9400","Input Freq":"146.3400","PL":"103.5","TSQ":"","Lat":"42.5","Long":"-89.0","State ID":"55","Rptr ID":"7","FM Analog":"Yes","FM Bandwidth":{bandwidth}}}]}}"#
        );
        let mut recs = parse_repeaterbook_json(&json);
        assert_eq!(recs.len(), 1, "the {bandwidth} row did not parse");
        recs.remove(0)
    }

    /// A machine its source marks narrow is a narrow (NFM) channel: hearham's "NFM" word (55
    /// rows on 2026-09-30) and RepeaterBook's "FM Bandwidth" of 12.5 kHz, written as a number or
    /// with the unit RepeaterBook's own pages print. Everything else stays wide FM.
    #[test]
    fn a_machine_its_source_marks_narrow_is_an_nfm_channel() {
        let hearham = [
            ("NFM", ChanMode::Nfm),
            ("P25/NFM", ChanMode::Nfm),
            ("DMR/NFM", ChanMode::Nfm),
            ("FM", ChanMode::Fm),
            ("YSF/FM", ChanMode::Fm),
        ];
        let repeaterbook = [
            (r#""12.5""#, ChanMode::Nfm),
            ("12.5", ChanMode::Nfm),
            (r#""12.5 kHz""#, ChanMode::Nfm),
            (r#""25""#, ChanMode::Fm),
            (r#""25 kHz""#, ChanMode::Fm),
            (r#""wide""#, ChanMode::Fm),
            ("null", ChanMode::Fm),
        ];
        let mut wrong: Vec<String> = hearham
            .iter()
            .filter_map(|&(mode, want)| {
                let got = to_channel(&hh_row(mode, "100.0")).mode;
                (got != want).then(|| format!("hearham {mode:?}: got {got:?}, want {want:?}"))
            })
            .collect();
        wrong.extend(repeaterbook.iter().filter_map(|&(bw, want)| {
            let got = to_channel(&rb_row_bw(bw)).mode;
            (got != want).then(|| format!("RepeaterBook {bw}: got {got:?}, want {want:?}"))
        }));
        assert_eq!(wrong, Vec::<String>::new(), "narrow channels");
    }

    /// What the operator's radio gets: both export files write a narrow machine's mode as NFM.
    #[test]
    fn a_narrow_machine_exports_as_nfm_in_both_files() {
        let c = to_channel(&hh_row("NFM", "100.0"));
        let chirp = crate::chirp::to_chirp_csv(std::slice::from_ref(&c), 8, "");
        let row = chirp.lines().nth(1).expect("a CHIRP row");
        assert_eq!(
            row.split(',').nth(10),
            Some("NFM"),
            "CHIRP Mode column: {row}"
        );
        let csv = crate::memchan::to_generic_csv(std::slice::from_ref(&c), "");
        let row = csv.lines().nth(1).expect("a CSV row");
        assert_eq!(
            row.split(',').nth(10),
            Some("NFM"),
            "generic CSV Mode column: {row}"
        );
    }

    /// A mode the record has no field for is never FM on its own, and a plain or empty mode is
    /// FM, as before.
    #[test]
    fn a_hearham_mode_with_no_field_is_never_fm_on_its_own() {
        let wrong = wrong_flags(&[
            ("P25", &[]),
            ("NXDN", &[]),
            ("AX25", &[]),
            ("ATV", &[]),
            ("TV", &[]),
            ("FM", &["FM"]),
            ("", &["FM"]),
        ]);
        assert_eq!(wrong, Vec::<String>::new(), "modes with no field");
    }

    /// The shared-access path serves rows narrowed to the fields below, while
    /// a user on their own `rbuapp_` token gets the full export straight from
    /// RepeaterBook. Both must produce identical records, or the two paths
    /// disagree about what a repeater is. If this fails after adding a field
    /// here, KEEP_FIELDS in hamradiotools/rb-proxy/worker.js needs it too.
    #[test]
    fn narrowed_proxy_rows_parse_identically_to_the_full_export() {
        let full = parse_repeaterbook_json(RB_FIXTURE);
        let narrowed = parse_repeaterbook_json(RB_NARROWED_FIXTURE);
        assert!(!full.is_empty(), "fixture parsed nothing");
        assert_eq!(narrowed, full, "narrowing the export changed a record");
    }

    /// The measured cases from the hearham directory that motivated this check.
    #[test]
    fn missing_major_band_flags_a_gap_not_empty_country() {
        let base = parse_hearham_json(HEARHAM_FIXTURE)[0].clone();
        let at = |mhz: f64| RepeaterRecord {
            output_mhz: mhz,
            ..base.clone()
        };

        // Bozeman MT as hearham actually has it: nine machines, all 70 cm.
        let bozeman: Vec<_> = [
            434.40, 444.10, 446.46, 446.66, 447.00, 447.95, 448.35, 449.30, 449.90,
        ]
        .iter()
        .map(|f| at(*f))
        .collect();
        assert_eq!(missing_major_band(&bozeman), Some("2 m"));

        // Amarillo TX: genuinely sparse, but band-balanced — not a gap.
        let amarillo: Vec<_> = [145.31, 146.72, 147.10, 442.75, 444.30, 447.55]
            .iter()
            .map(|f| at(*f))
            .collect();
        assert_eq!(missing_major_band(&amarillo), None);

        // Fairbanks AK: hearham lists 2 m machines here, both marked off-air.
        // The directory knows about the band, so that is an outage and not a
        // hole — counting listings rather than live ones keeps this quiet.
        let fairbanks = vec![
            RepeaterRecord {
                output_mhz: 147.25,
                operational: false,
                ..base.clone()
            },
            RepeaterRecord {
                output_mhz: 147.55,
                operational: false,
                ..base.clone()
            },
            RepeaterRecord {
                output_mhz: 443.30,
                operational: true,
                ..base.clone()
            },
        ];
        assert_eq!(missing_major_band(&fairbanks), None);

        assert_eq!(missing_major_band(&[at(146.94)]), Some("70 cm"));
        assert_eq!(
            missing_major_band(&[at(53.10), at(224.5)]),
            Some("2 m or 70 cm")
        );
        // Nothing found at all is the caller's empty state, not a coverage gap.
        assert_eq!(missing_major_band(&[]), None);
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
