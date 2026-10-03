//! Repeater-directory parsing + query planning for the "Program" section.
//!
//! Pure logic: normalize the three feeds (the RSGB ETCC list for UK origins, the
//! RepeaterBook JSON export, the hearham.com open API) into [`RepeaterRecord`]s, plan
//! which RepeaterBook state exports and which RSGB locator squares a radius query
//! needs (RepeaterBook's API has no lat/lng parameter — like CHIRP, we pull whole
//! states and haversine-filter client-side), merge the sources into one row per
//! machine ([`merge_nearby`]: field by field from the highest-precedence source, every
//! source id kept, a disagreement shown and never silently resolved), filter/sort by
//! distance, and convert a picked repeater into a programmable [`Channel`]. A [`Route`]
//! (one place to another, a corridor either side) plans and merges the same way
//! ([`plan_route_states`], [`plan_rsgb_route`], [`merge_route`]), in route order. HTTP lives
//! in `crate::live::{rsgb, repeaterbook, hearham}`; caching/orchestration in the shell.

use crate::geo::{
    bearing_deg, haversine_km, interpolate, latlon_to_maidenhead, maidenhead_to_latlon,
};
use crate::gridstate::state_for_grid;
use crate::memchan::{ChanMode, Channel, ChannelSource, Duplex, ToneMode};
use serde::{Deserialize, Serialize};

/// Which directory a record came from, declared in PRECEDENCE order, highest first:
/// [`merge_nearby`] takes each field of a machine from the highest source that has it. The
/// national coordinator's own list, then RepeaterBook (the operator's personal token), then
/// hearham. The derived `Ord` IS that order, so the variants must not be reordered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RepeaterSource {
    /// The UK coordinator's list: RSGB ETCC (ukrepeater.net), for UK origins.
    Rsgb,
    Repeaterbook,
    Hearham,
}

/// A network a machine is reached through beyond its own coverage, as a directory names it.
/// Declared in the order a row lists its links.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LinkNetwork {
    /// An AllStar node.
    AllStar,
    /// An IRLP node.
    Irlp,
    /// The machine's DMR ID, the number a DMR network (BrandMeister, DMR+, …) knows it by.
    DmrId,
    /// A node number its directory gives without saying which network it is on.
    Node,
}

/// One way onto a machine beyond its own coverage: a [`LinkNetwork`] and the node number as the
/// directory writes it ("2462", "314158").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Link {
    pub network: LinkNetwork,
    pub node: String,
}

impl Link {
    /// The link as the exports write it: "AllStar 2462", "IRLP 3570", "DMR ID 314158",
    /// "node 7230" (tokens, like the colour code's "CC1" beside them).
    pub fn label(&self) -> String {
        let network = match self.network {
            LinkNetwork::AllStar => "AllStar",
            LinkNetwork::Irlp => "IRLP",
            LinkNetwork::DmrId => "DMR ID",
            LinkNetwork::Node => "node",
        };
        format!("{network} {}", self.node)
    }
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
    /// DCS code on the downlink, when the source gives one. The same code as `dcs` makes the
    /// channel DCS both ways; without it the code is sent only ([`to_channel`]).
    pub dcs_dec: Option<u16>,
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
    /// The source's own date for this entry, as it writes it: RepeaterBook's "Last Update"
    /// (`2026-05-14`). `None` when it gives none — hearham and the RSGB API have no date per
    /// machine, and the proxy's narrowed RepeaterBook rows drop it.
    #[serde(default)]
    pub updated: Option<String>,
    /// How the machine links beyond its own coverage ([`Link`]): hearham's `internet_node`
    /// ([`hearham_link`]). Neither other directory gives one Program reads (see their parsers).
    /// Left out of the JSON when there is none.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<Link>,
    /// Filled by [`filter_sort`] — distance/bearing from the query origin. On a route
    /// ([`merge_route`]), from the machine's nearest point of the line.
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
                dcs_dec: if tsq.starts_with(['D', 'd']) {
                    dcs_code(&tsq)
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
                updated: Some(jstr(v, "Last Update")).filter(|d| !d.is_empty()),
                // The export's own node columns ("AllStar Node", "EchoLink Node", "IRLP Node",
                // "Wires Node", "DMR ID") are left unread on purpose: a channel's links are saved
                // with it, where the Remote's programming view can show them, and RepeaterBook's
                // rows never leave this PC.
                links: Vec::new(),
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

/// The link a hearham row gives: its `internet_node`, on the network its `group` names. The
/// group is the list hearham imported the row from, and three of them are the networks
/// themselves: "Allstar" (an AllStar node), "IRLP" (an IRLP node) and "DMR" (the machine's DMR ID:
/// 3,072 of those rows' descriptions link its BrandMeister page by that number). A node in any
/// other group names no network, so it stays a plain node: 90 rows on 2026-10-01, 83 of them one
/// regional directory's, whose free text names IRLP on most, and free text is not read.
///
/// Measured on the whole list (22,696 rows, 2026-10-01): 10,297 rows give a node (DMR 5,480,
/// Allstar 2,875, IRLP 1,850), each a number once trimmed ("8728\t" once) but two, which hold a
/// callsign. Anything that is not a number is not read as a node.
fn hearham_link(v: &serde_json::Value) -> Option<Link> {
    let node = jstr(v, "internet_node");
    if node.is_empty() || !node.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let network = match jstr(v, "group").to_ascii_lowercase().as_str() {
        "allstar" => LinkNetwork::AllStar,
        "irlp" => LinkNetwork::Irlp,
        "dmr" => LinkNetwork::DmrId,
        _ => LinkNetwork::Node,
    };
    Some(Link { network, node })
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
///
/// **A payload that is not the list is an `Err`, never an empty list.** It is the whole
/// worldwide directory (22,699 rows on 2026-10-02), so a body that is not a JSON array, or
/// whose rows give no repeater at all (no frequency and position on any of them), is not
/// hearham's list: a proxy's or a server's error page answered as a success, say. Read as no
/// rows it made Program say "No FM repeaters within 50 mi." for a week, the cache's life, with
/// nothing to show why (2026-10-02). The shell keeps the list it has instead, or says the list
/// could not be read.
pub fn parse_hearham_json(json: &str) -> Result<Vec<RepeaterRecord>, String> {
    let rows: Vec<serde_json::Value> = serde_json::from_str(json)
        .map_err(|e| format!("hearham: the list it sent could not be read ({e})"))?;
    let records: Vec<RepeaterRecord> = rows
        .iter()
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
            // DCS is the uplink's (`encode`). The downlink confirms it only by giving the SAME
            // code; any other downlink (none, a tone, another code, one Nexus cannot read) leaves
            // it send-only, with the receiver open (`to_channel`), never guessed at.
            let dcs = match up {
                Some(Squelch::Dcs(code)) => Some(code),
                _ => None,
            };
            let dcs_dec = match down {
                Some(Squelch::Dcs(code)) if dcs == Some(code) => Some(code),
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
                dcs_dec,
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
                updated: None,
                links: hearham_link(v).into_iter().collect(),
                distance_km: 0.0,
                bearing_deg: 0.0,
            })
        })
        .collect();
    if records.is_empty() {
        return Err(format!(
            "hearham: the list it sent has no repeaters in it ({} rows)",
            rows.len()
        ));
    }
    Ok(records)
}

// ── RSGB ETCC (the UK coordinator) ──────────────────────────────────────────

/// The RSGB listing types Program keeps: voice repeaters (analogue `AV`, digital `DV`, both
/// `DM`) and the simplex internet gateways hams program like any other channel (`AG`, `DG`).
/// The rest of the list is not a voice channel: packet mailboxes and nodes (`PX`, `PN`), APRS
/// (`AP`), regenerative nodes (`RN`), ATV (`TV`) and link transmitters (`RL`, "GB3MN-L", which
/// carry a repeater's link rather than its users). The API page documents the mode flags but
/// not these codes; they were read off the listings (three squares, 372 rows, 2026-10-01),
/// where every `AV` row is a GB3 analogue duplex machine and every `AG` an MB7 simplex one.
const RSGB_VOICE_TYPES: [&str; 5] = ["AV", "DV", "DM", "AG", "DG"];

/// Parse one `api-beta.rsgb.online/locator/<square>` payload: `{"data": [ … ]}`, one object per
/// listing with `repeater` (the callsign), `type`, `status`, `town`, `modeCodes` (`["A","F"]`; a
/// DMR machine is `"M:<colour code>"`), `tx`/`rx` in Hz (`tx` is the machine's OUTPUT, what you
/// listen to, `rx` its input), `ctcss` in Hz, `txbw` in kHz, `locator` (4, 6 or 8 characters)
/// and `extraDetails.ngr`, a 1 km grid reference ([`rsgb_position`]).
///
/// `Err` when the payload is not that shape: not an object with a `data` list, or a listing
/// without a string `repeater` and `type`, numeric `tx` and `rx` and a `modeCodes` list. One such
/// listing fails the whole payload: the endpoint is a BETA, and a renamed field would otherwise
/// read as a square with nothing in it. Listings of a type Program does not program
/// ([`RSGB_VOICE_TYPES`]), or with no output frequency or no position, are skipped.
///
/// A `ctcss` of 0 is read as no tone listed, not as "this machine takes no tone", so a lower
/// source's tone still programs ([`merge_nearby`]): a tone sent to a machine that needs none
/// does no harm, and a missing one cannot open a machine that needs it. A `status` of
/// "NOT OPERATIONAL" is off the air; "REDUCED OUTPUT" is on it. `fac` is undocumented and unread.
pub fn parse_rsgb_json(json: &str) -> Result<Vec<RepeaterRecord>, String> {
    let root: serde_json::Value =
        serde_json::from_str(json).map_err(|_| "the RSGB payload is not JSON".to_string())?;
    let rows = root
        .get("data")
        .and_then(|d| d.as_array())
        .ok_or_else(|| "the RSGB payload has no \"data\" list".to_string())?;
    let mut out = Vec::new();
    for v in rows {
        let call = v.get("repeater").and_then(|x| x.as_str());
        let kind = v.get("type").and_then(|x| x.as_str());
        let tx = v.get("tx").and_then(|x| x.as_f64());
        let rx = v.get("rx").and_then(|x| x.as_f64());
        let codes = v.get("modeCodes").and_then(|x| x.as_array());
        let (Some(call), Some(kind), Some(tx), Some(rx), Some(codes)) = (call, kind, tx, rx, codes)
        else {
            return Err("an RSGB listing has no callsign, type, frequencies or mode codes".into());
        };
        if !RSGB_VOICE_TYPES.contains(&kind.trim().to_ascii_uppercase().as_str()) || tx <= 0.0 {
            continue;
        }
        let ngr = v
            .get("extraDetails")
            .map(|e| jstr(e, "ngr"))
            .unwrap_or_default();
        let Some((lat, lon)) = rsgb_position(&jstr(v, "locator"), &ngr) else {
            continue;
        };
        let (mut fm, mut dmr, mut dstar, mut fusion, mut colour) =
            (false, false, false, false, None);
        for code in codes.iter().filter_map(|c| c.as_str()) {
            let (flag, access) = match code.split_once(':') {
                Some((f, a)) => (f.trim(), Some(a.trim())),
                None => (code.trim(), None),
            };
            match flag {
                "A" => fm = true,
                "D" => dstar = true,
                "F" => fusion = true,
                "M" => {
                    dmr = true;
                    colour = access
                        .and_then(|a| a.parse::<u8>().ok())
                        .filter(|cc| *cc <= 15);
                }
                // Tetra, P25, NXDN, M17 and the packet flags: the record has no field for them.
                _ => {}
            }
        }
        out.push(RepeaterRecord {
            source: RepeaterSource::Rsgb,
            source_id: jstr(v, "id"),
            callsign: call.trim().to_ascii_uppercase(),
            output_mhz: tx / 1e6,
            input_mhz: if rx > 0.0 { rx / 1e6 } else { tx / 1e6 },
            ctcss_enc_hz: jf64(v, "ctcss")
                .map(|hz| hz as f32)
                .filter(|hz| (60.0..300.0).contains(hz)),
            ctcss_dec_hz: None,
            dcs: None,
            dcs_dec: None,
            lat,
            lon,
            city: jstr(v, "town")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" "),
            county: String::new(),
            state: String::new(),
            fm,
            dmr,
            dstar,
            fusion,
            dmr_color_code: colour,
            bandwidth_khz: jkhz(v, "txbw").filter(|khz| *khz > 0.0),
            operational: !jstr(v, "status").eq_ignore_ascii_case("not operational"),
            open_use: true,
            updated: None,
            // The API has no node or network field, and the only access code its listings carry
            // is DMR's colour code (`M:<code>`, above): 85 Fusion flags in 372 listings, none
            // with a DG-ID (three squares, 2026-10-01).
            links: Vec::new(),
            distance_km: 0.0,
            bearing_deg: 0.0,
        });
    }
    Ok(out)
}

/// Where an RSGB listing is. A 6- or 8-character locator places it to within a few km and is
/// used as it stands. When the list gives only the 4-character square (it truncates some sites:
/// 105 of 372 listings on 2026-10-01) the square's centre can be 75 km from the machine, so the
/// 1 km grid reference beside it is used instead, but only when it lands inside that square:
/// a reference that contradicts the coordinator's own locator never moves a machine out of it.
/// All 105 did land inside.
///
/// On a few 6-character listings the two disagree (11 of 267, by up to 88 km, both ways round:
/// GB3OA's reference is in Blackpool for a Southport machine, GB7MN's locator on the Wirral for
/// one in Stoke). The locator is kept: it is the field the API itself is searched by.
fn rsgb_position(locator: &str, ngr: &str) -> Option<(f64, f64)> {
    let loc = locator.trim().to_ascii_uppercase();
    if loc.len() >= 6 {
        if let Some(p) = maidenhead_to_latlon(&loc) {
            return Some(p);
        }
    }
    let square = loc.get(..4).filter(|sq| maidenhead_to_latlon(sq).is_some());
    match (square, grid_ref_to_latlon(ngr)) {
        (Some(sq), Some(p)) if latlon_to_maidenhead(p.0, p.1) == sq => Some(p),
        (Some(sq), _) => maidenhead_to_latlon(sq),
        (None, p) => p,
    }
}

/// A Transverse Mercator national grid: its ellipsoid (semi-axes, m), central scale factor,
/// true origin (degrees) and false origin (m).
struct TmGrid {
    a: f64,
    b: f64,
    f0: f64,
    lat0: f64,
    lon0: f64,
    e0: f64,
    n0: f64,
}

/// The British National Grid: OSGB36 on the Airy 1830 ellipsoid.
const BRITISH_GRID: TmGrid = TmGrid {
    a: 6_377_563.396,
    b: 6_356_256.909,
    f0: 0.999_601_271_7,
    lat0: 49.0,
    lon0: -2.0,
    e0: 400_000.0,
    n0: -100_000.0,
};

/// The Irish Grid: Ireland 1965 on the modified Airy ellipsoid.
const IRISH_GRID: TmGrid = TmGrid {
    a: 6_377_340.189,
    b: 6_356_034.447,
    f0: 1.000_035,
    lat0: 53.5,
    lon0: -8.0,
    e0: 200_000.0,
    n0: 250_000.0,
};

/// Easting/northing (m) on `g` → (lat, lon) in degrees on its own datum, by the Ordnance
/// Survey's series ("A guide to coordinate systems in Great Britain", annex C, whose worked
/// example the tests reproduce).
fn tm_inverse(g: &TmGrid, east: f64, north: f64) -> (f64, f64) {
    let (a, b, f0) = (g.a, g.b, g.f0);
    let (lat0, lon0) = (g.lat0.to_radians(), g.lon0.to_radians());
    let e2 = 1.0 - (b * b) / (a * a);
    let n = (a - b) / (a + b);
    let (n2, n3) = (n * n, n * n * n);
    let meridian_arc = |phi: f64| {
        let (d, s) = (phi - lat0, phi + lat0);
        b * f0
            * ((1.0 + n + 1.25 * n2 + 1.25 * n3) * d
                - (3.0 * n + 3.0 * n2 + 2.625 * n3) * d.sin() * s.cos()
                + (1.875 * n2 + 1.875 * n3) * (2.0 * d).sin() * (2.0 * s).cos()
                - (35.0 / 24.0) * n3 * (3.0 * d).sin() * (3.0 * s).cos())
    };
    let mut phi = lat0 + (north - g.n0) / (a * f0);
    for _ in 0..20 {
        let gap = north - g.n0 - meridian_arc(phi);
        if gap.abs() < 1e-5 {
            break;
        }
        phi += gap / (a * f0);
    }
    let (s, c, t) = (phi.sin(), phi.cos(), phi.tan());
    let nu = a * f0 / (1.0 - e2 * s * s).sqrt();
    let rho = a * f0 * (1.0 - e2) / (1.0 - e2 * s * s).powf(1.5);
    let eta2 = nu / rho - 1.0;
    let (t2, t4, t6) = (t * t, t.powi(4), t.powi(6));
    let vii = t / (2.0 * rho * nu);
    let viii = t / (24.0 * rho * nu.powi(3)) * (5.0 + 3.0 * t2 + eta2 - 9.0 * t2 * eta2);
    let ix = t / (720.0 * rho * nu.powi(5)) * (61.0 + 90.0 * t2 + 45.0 * t4);
    let x = 1.0 / (c * nu);
    let xi = 1.0 / (c * 6.0 * nu.powi(3)) * (nu / rho + 2.0 * t2);
    let xii = 1.0 / (c * 120.0 * nu.powi(5)) * (5.0 + 28.0 * t2 + 24.0 * t4);
    let xiia = 1.0 / (c * 5040.0 * nu.powi(7)) * (61.0 + 662.0 * t2 + 1320.0 * t4 + 720.0 * t6);
    let de = east - g.e0;
    let lat = phi - vii * de.powi(2) + viii * de.powi(4) - ix * de.powi(6);
    let lon = lon0 + x * de - xi * de.powi(3) + xii * de.powi(5) - xiia * de.powi(7);
    (lat.to_degrees(), lon.to_degrees())
}

/// A British or Irish grid reference → (lat, lon) at the centre of the square it names.
///
/// British National Grid: two letters and an even number of digits ("SJ2957" is a 1 km square).
/// Irish Grid, which the RSGB list writes with an `I` before its one letter ("IJ4076" is
/// J 40 76, Belfast). Both grids letter their 100 km squares A–Z without I, five to a row from
/// the top. The latitude and longitude come out on the grid's own datum (OSGB36, Ireland 1965),
/// within about 120 m of WGS84 here, far inside the kilometre the reference itself carries, so
/// no datum shift is applied. Anything else (no reference, another grid) is `None`.
fn grid_ref_to_latlon(reference: &str) -> Option<(f64, f64)> {
    const LETTERS: &[u8; 25] = b"ABCDEFGHJKLMNOPQRSTUVWXYZ";
    let r: String = reference
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_uppercase();
    let b = r.as_bytes();
    let letter = |c: u8| LETTERS.iter().position(|&l| l == c);
    let (grid, e100, n100) = match b {
        [b'I', l, ..] => {
            let l = letter(*l)?;
            (&IRISH_GRID, l % 5, 4 - l / 5)
        }
        [l1 @ (b'H' | b'J' | b'N' | b'O' | b'S' | b'T'), l2, ..] => {
            let (l1, l2) = (letter(*l1)?, letter(*l2)?);
            (
                &BRITISH_GRID,
                ((l1 + 3) % 5) * 5 + l2 % 5,
                19 - (l1 / 5) * 5 - l2 / 5,
            )
        }
        _ => return None,
    };
    let digits = &r[2..];
    if !(2..=10).contains(&digits.len())
        || !digits.len().is_multiple_of(2)
        || !digits.bytes().all(|d| d.is_ascii_digit())
    {
        return None;
    }
    let half = digits.len() / 2;
    let unit = 10f64.powi(5 - half as i32);
    let east = e100 as f64 * 100_000.0 + digits[..half].parse::<f64>().ok()? * unit + unit / 2.0;
    let north = n100 as f64 * 100_000.0 + digits[half..].parse::<f64>().ok()? * unit + unit / 2.0;
    Some(tm_inverse(grid, east, north))
}

/// The 4-character locator squares holding UK land (England, Scotland, Wales, Northern Ireland,
/// the Isle of Man and the Channel Islands), which the RSGB list covers. An origin in one is a
/// UK origin, and only these are ever asked about. Listed from the coastline and checked against
/// hearham: every square it places a GB- or MB-prefixed machine in (2,023 machines, 2026-10-01)
/// is here. Four are shared with a neighbour (IO64 and IO65 with Ireland, IN89 and JO00 with
/// France); an origin there asks RSGB too, which only adds the UK machines in range of it.
const RSGB_SQUARES: [&str; 41] = [
    "IN69", "IN79", "IN89", "IO64", "IO65", "IO66", "IO67", "IO68", "IO70", "IO71", "IO72", "IO73",
    "IO74", "IO75", "IO76", "IO77", "IO78", "IO80", "IO81", "IO82", "IO83", "IO84", "IO85", "IO86",
    "IO87", "IO88", "IO89", "IO90", "IO91", "IO92", "IO93", "IO94", "IO95", "IO97", "IO99", "IP80",
    "IP90", "JO00", "JO01", "JO02", "JO03",
];

/// The most RSGB squares one search asks about, nearest first. Nine is a 3×3 block, which holds
/// all of the default 50 mi radius anywhere in the UK; a wider search asks about the nine
/// nearest and names the rest ([`RsgbPlan::beyond`]), where the rows are hearham's alone. Each
/// square is one request, cached for a week, so a search costs at most nine.
pub const RSGB_SQUARES_PER_SEARCH: usize = 9;

/// Which RSGB locator squares a radius search asks about.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RsgbPlan {
    /// The squares to ask about, nearest first. Empty for an origin outside the UK.
    pub ask: Vec<String>,
    /// UK squares the radius reaches that are NOT asked about, nearest first.
    pub beyond: Vec<String>,
}

/// Plan the RSGB requests for a radius search: none unless the origin is in a UK square
/// ([`RSGB_SQUARES`]); otherwise every UK square the radius reaches, nearest first, at most
/// [`RSGB_SQUARES_PER_SEARCH`] of them.
pub fn plan_rsgb_squares(origin: (f64, f64), radius_km: f64) -> RsgbPlan {
    let home = latlon_to_maidenhead(origin.0, origin.1);
    if !RSGB_SQUARES.contains(&home.as_str()) {
        return RsgbPlan::default();
    }
    let mut near: Vec<(f64, &str)> = RSGB_SQUARES
        .iter()
        .filter_map(|sq| {
            let d = km_to_square(origin, sq)?;
            (d <= radius_km).then_some((d, *sq))
        })
        .collect();
    near.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.1.cmp(b.1))
    });
    let mut squares = near.into_iter().map(|(_, sq)| sq.to_string());
    RsgbPlan {
        ask: squares.by_ref().take(RSGB_SQUARES_PER_SEARCH).collect(),
        beyond: squares.collect(),
    }
}

/// Distance from `origin` to the nearest point of a 2°×1° locator square (0 inside it).
fn km_to_square(origin: (f64, f64), square: &str) -> Option<f64> {
    let (lat, lon) = maidenhead_to_latlon(square)?;
    let nearest = (
        origin.0.clamp(lat - 0.5, lat + 0.5),
        origin.1.clamp(lon - 1.0, lon + 1.0),
    );
    Some(haversine_km(origin, nearest))
}

/// What ONE planned square's RSGB fetch gave a search: [`StateFetch`]'s counterpart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SquareFetch {
    /// The 4-character locator square, as [`plan_rsgb_squares`] produced it.
    pub square: String,
    /// The payload, fresh or cached. `None` = nothing: the fetch failed with no cache to serve.
    pub body: Option<String>,
    /// When `body` was obtained (unix secs).
    pub fetched_utc: i64,
    /// `body` came from cache because the fetch failed or was throttled.
    pub stale: bool,
}

/// The RSGB list behind a search, when every square it asked about was read.
#[derive(Debug, Clone, PartialEq)]
pub struct RsgbLayer {
    pub records: Vec<RepeaterRecord>,
    /// Oldest payload behind `records`: the list's "as of" stamp.
    pub oldest_utc: i64,
    /// At least one square was served from stale cache.
    pub stale: bool,
}

/// Fold the squares' fetches into the RSGB layer of a search, or say why there is none. ANY
/// square that gave nothing, or gave a payload [`parse_rsgb_json`] cannot read, drops the whole
/// layer: the endpoint is a BETA, a list with a square missing would read as complete, and the
/// shell then shows hearham's rows alone with a plain notice, never an empty list. The reason
/// goes to the connection log, not the screen.
pub fn fold_rsgb_squares(fetches: &[SquareFetch]) -> Result<RsgbLayer, String> {
    if fetches.is_empty() {
        return Err("no RSGB square was asked about".into());
    }
    let mut layer = RsgbLayer {
        records: Vec::new(),
        oldest_utc: i64::MAX,
        stale: false,
    };
    for f in fetches {
        let body = f
            .body
            .as_deref()
            .ok_or_else(|| format!("RSGB gave nothing for {}", f.square))?;
        let rows = parse_rsgb_json(body).map_err(|e| format!("{e} ({})", f.square))?;
        layer.records.extend(rows);
        layer.oldest_utc = layer.oldest_utc.min(f.fetched_utc);
        layer.stale |= f.stale;
    }
    Ok(layer)
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

// ── one row per machine ─────────────────────────────────────────────────────

/// Outputs (and, between rows without a callsign, inputs) at most this far apart are the same
/// channel. Under half of the narrowest 6.25 kHz raster, so neighbouring channels never merge.
const SAME_CHANNEL_MHZ: f64 = 0.0025;
/// Rows without a callsign are one machine only when closer together than this.
const SAME_SITE_KM: f64 = 5.0;
/// How far past the radius each source is read before merging, so a machine its sources place
/// either side of the edge still becomes one row (which is then judged by its own position).
const MERGE_MARGIN_KM: f64 = 10.0;

/// One directory row behind a merged machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceRef {
    pub source: RepeaterSource,
    pub source_id: String,
    /// The channel id this row gives ([`to_channel`]). A channel saved from any of a machine's
    /// rows finds the machine by it, whichever source the machine now programs from.
    pub channel_id: String,
    /// The row's own date ([`RepeaterRecord::updated`]).
    pub updated: Option<String>,
}

/// A field a machine's sources disagree on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum DisputedField {
    /// The access tone or DCS code on the uplink.
    Tone,
    /// Where you transmit: the input, and so the shift.
    Input,
    /// The modes: a lower source names one the machine's top source does not.
    Mode,
    /// The DMR colour code.
    ColorCode,
}

/// What one source said about a disputed field, as the row shows it ("88.5", "D023", "438.525",
/// "FM+DMR", "CC1": tokens, never words).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Said {
    pub source: RepeaterSource,
    pub value: String,
}

/// A disagreement between a machine's sources. Shown, never silently resolved: the row programs
/// the first value (the highest-precedence source's) and shows every other one beside it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Disagreement {
    pub field: DisputedField,
    pub said: Vec<Said>,
}

/// Where Program's map may put a machine, and all it may say there: its hearham row's own
/// position, callsign, output and town ([`Machine::map`]).
///
/// The map is hearham's alone (the operator, 2026-09-30): hearham invites map use, RepeaterBook's
/// terms forbid putting its rows on one, and a coordinator's list is not mapped for now. So the
/// point is never the merged [`Machine::record`], whose position and town can be RepeaterBook's or
/// the coordinator's.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MapPoint {
    pub lat: f64,
    pub lon: f64,
    pub callsign: String,
    pub output_mhz: f64,
    pub city: String,
}

/// One machine after the merge.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Machine {
    /// What Program shows and programs: field by field from the highest-precedence row that
    /// has the field. Its `source`/`source_id` are that top row's.
    pub record: RepeaterRecord,
    /// Every row merged in, the top one first.
    pub sources: Vec<SourceRef>,
    pub disagreements: Vec<Disagreement>,
    /// On a route ([`merge_route`]): how far along the line the machine's nearest point of it is,
    /// km from the start. `None` on a radius search.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub along_km: Option<f64>,
    /// The machine on the map ([`MapPoint`]): `None` when no hearham row lists it, and then it is
    /// left off the map. Left out of the JSON then.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub map: Option<MapPoint>,
}

/// Merge every source's rows near `origin` into one row per machine within `radius_km`, nearest
/// first (ties by output).
///
/// **One machine.** Two rows are the same machine when their outputs are within
/// [`SAME_CHANNEL_MHZ`] and their callsigns, without link decoration ([`machine_call`]), match;
/// with no callsign on either, their inputs must be within [`SAME_CHANNEL_MHZ`] too and their
/// sites within [`SAME_SITE_KM`]. Rows of one source merge as well: hearham lists a mixed machine
/// once per mode and a linked one once per node.
///
/// **Precedence** follows [`RepeaterSource`]: the coordinator, then RepeaterBook, then hearham.
/// Each field comes from the highest row that has it (a coordinator listing with no tone takes
/// hearham's), and the tone fields (CTCSS both ways, DCS) come together from one row, never
/// mixed. The modes are the top source's, all its rows together, so a mixed machine that a
/// coordinator lists with FM is ONE programmable FM row carrying its CTCSS and its colour code,
/// even where hearham lists only its DMR side.
///
/// **Disagreements** on the tone, the input, the modes and the colour code stay on the machine
/// ([`Disagreement`]). A row that only lacks a field disagrees with nothing.
///
/// **Links** ([`Link`]) are every one any of its rows gives, each once: hearham lists a linked
/// machine once per node, so a machine on AllStar and IRLP shows both.
pub fn merge_nearby(
    layers: &[&[RepeaterRecord]],
    origin: (f64, f64),
    radius_km: f64,
) -> Vec<Machine> {
    let mut rows: Vec<RepeaterRecord> = layers
        .iter()
        .flat_map(|layer| filter_sort(layer, origin, radius_km + MERGE_MARGIN_KM))
        .collect();
    // Precedence first; within one source, nearest first (filter_sort's order, kept by a
    // stable sort), so the top row of a machine is its best source's nearest listing.
    rows.sort_by_key(|r| r.source);
    let mut out: Vec<Machine> = group_machines(rows)
        .into_iter()
        .map(|g| machine(g, origin))
        .filter(|m| m.record.distance_km <= radius_km)
        .collect();
    out.sort_by(|a, b| {
        a.record
            .distance_km
            .partial_cmp(&b.record.distance_km)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.record
                    .output_mhz
                    .partial_cmp(&b.record.output_mhz)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    out
}

/// `rows` (in the order they should lead their machine) grouped into machines: each row joins the
/// first group holding a row of the same machine ([`same_machine`]), else starts one.
fn group_machines(rows: Vec<RepeaterRecord>) -> Vec<Vec<RepeaterRecord>> {
    let mut groups: Vec<Vec<RepeaterRecord>> = Vec::new();
    for r in rows {
        match groups
            .iter_mut()
            .find(|g| g.iter().any(|x| same_machine(x, &r)))
        {
            Some(g) => g.push(r),
            None => groups.push(vec![r]),
        }
    }
    groups
}

/// The callsign a machine is known by, without the decoration a list adds for a link or a node
/// ("GB7DZ-L", "K1ABC-R", "W1XYZ/R"): upper case, up to the first `-`, `/` or space.
fn machine_call(call: &str) -> String {
    let up = call.trim().to_ascii_uppercase();
    up.split(['-', '/', ' '])
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Are `a` and `b` the same machine? See [`merge_nearby`].
fn same_machine(a: &RepeaterRecord, b: &RepeaterRecord) -> bool {
    let near = |x: f64, y: f64| (x - y).abs() <= SAME_CHANNEL_MHZ + 1e-9;
    if !near(a.output_mhz, b.output_mhz) {
        return false;
    }
    let (call_a, call_b) = (machine_call(&a.callsign), machine_call(&b.callsign));
    if !call_a.is_empty() && !call_b.is_empty() {
        return call_a == call_b;
    }
    near(a.input_mhz, b.input_mhz) && haversine_km((a.lat, a.lon), (b.lat, b.lon)) < SAME_SITE_KM
}

/// The modes a row names, as bits: FM, DMR, D-STAR, Fusion.
fn modes(r: &RepeaterRecord) -> u8 {
    u8::from(r.fm) | u8::from(r.dmr) << 1 | u8::from(r.dstar) << 2 | u8::from(r.fusion) << 3
}

/// "FM+DMR" for a set of mode bits; "—" for a machine none of the four describes.
fn modes_label(bits: u8) -> String {
    let names = ["FM", "DMR", "D-STAR", "YSF"];
    let on: Vec<&str> = (0..4)
        .filter(|i| bits & (1 << i) != 0)
        .map(|i| names[i])
        .collect();
    if on.is_empty() {
        "—".into()
    } else {
        on.join("+")
    }
}

/// `MHz` as Program prints a frequency: four decimals, trailing zeros dropped.
fn mhz_label(mhz: f64) -> String {
    let s = format!("{mhz:.4}");
    let s = s.trim_end_matches('0');
    if s.ends_with('.') {
        format!("{s}0")
    } else {
        s.to_string()
    }
}

/// A disagreement over one field, when the values said (with a number to compare them by) are
/// not all `same` as the first. Repeats of a source's value are shown once.
fn disputed(
    field: DisputedField,
    said: Vec<(Said, f64)>,
    same: impl Fn(f64, f64) -> bool,
) -> Option<Disagreement> {
    let first = said.first()?.1;
    if said.iter().all(|(_, v)| same(first, *v)) {
        return None;
    }
    let mut out: Vec<Said> = Vec::new();
    for (s, _) in said {
        if !out.contains(&s) {
            out.push(s);
        }
    }
    Some(Disagreement { field, said: out })
}

/// Build one machine from its rows, in precedence order (`rows[0]` is the top row).
fn machine(rows: Vec<RepeaterRecord>, origin: (f64, f64)) -> Machine {
    let top = &rows[0];
    let mut rec = top.clone();
    let first_text = |get: fn(&RepeaterRecord) -> &String| {
        rows.iter()
            .map(get)
            .find(|s| !s.is_empty())
            .cloned()
            .unwrap_or_default()
    };
    rec.callsign = first_text(|r| &r.callsign);
    rec.city = first_text(|r| &r.city);
    rec.county = first_text(|r| &r.county);
    rec.state = first_text(|r| &r.state);
    // The tone fields travel together, from the first row that gives any of them.
    if let Some(t) = rows
        .iter()
        .find(|r| r.ctcss_enc_hz.is_some() || r.ctcss_dec_hz.is_some() || r.dcs.is_some())
    {
        rec.ctcss_enc_hz = t.ctcss_enc_hz;
        rec.ctcss_dec_hz = t.ctcss_dec_hz;
        rec.dcs = t.dcs;
        rec.dcs_dec = t.dcs_dec;
    }
    // The modes: everything the top source's rows name, together.
    let top_modes = rows
        .iter()
        .filter(|r| r.source == top.source)
        .fold(0, |bits, r| bits | modes(r));
    rec.fm = top_modes & 1 != 0;
    rec.dmr = top_modes & 2 != 0;
    rec.dstar = top_modes & 4 != 0;
    rec.fusion = top_modes & 8 != 0;
    rec.dmr_color_code = if rec.dmr {
        rows.iter().filter(|r| r.dmr).find_map(|r| r.dmr_color_code)
    } else {
        None
    };
    if rec.fm {
        rec.bandwidth_khz = rows.iter().filter(|r| r.fm).find_map(|r| r.bandwidth_khz);
    }
    // Every link any row gives, once, in the network order; one network's as the rows give them.
    rec.links = Vec::new();
    for l in rows.iter().flat_map(|r| &r.links) {
        if !rec.links.contains(l) {
            rec.links.push(l.clone());
        }
    }
    rec.links.sort_by_key(|l| l.network);
    rec.distance_km = haversine_km(origin, (rec.lat, rec.lon));
    rec.bearing_deg = bearing_deg(origin, (rec.lat, rec.lon));

    let said = |r: &RepeaterRecord, value: String| Said {
        source: r.source,
        value,
    };
    let tones = rows
        .iter()
        .filter_map(|r| match (r.dcs, r.ctcss_enc_hz) {
            // A DCS code compares as 10000 + code: never equal to a CTCSS tone.
            (Some(code), _) => Some((said(r, format!("D{code:03}")), 10_000.0 + f64::from(code))),
            (None, Some(hz)) => Some((said(r, format!("{hz:.1}")), f64::from(hz))),
            (None, None) => None,
        })
        .collect();
    let inputs = rows
        .iter()
        .map(|r| (said(r, mhz_label(r.input_mhz)), r.input_mhz))
        .collect();
    let colours = rows
        .iter()
        .filter(|r| r.dmr)
        .filter_map(|r| {
            r.dmr_color_code
                .map(|cc| (said(r, format!("CC{cc}")), f64::from(cc)))
        })
        .collect();
    let mut disagreements: Vec<Disagreement> = [
        disputed(DisputedField::Tone, tones, |a, b| (a - b).abs() < 0.05),
        disputed(DisputedField::Input, inputs, |a, b| {
            (a - b).abs() <= SAME_CHANNEL_MHZ + 1e-9
        }),
    ]
    .into_iter()
    .flatten()
    .collect();
    // Mode: a lower source naming a mode the top source does not list.
    let beyond: Vec<&RepeaterRecord> = rows
        .iter()
        .filter(|r| r.source != top.source && modes(r) & !top_modes != 0)
        .collect();
    if !beyond.is_empty() {
        let mut said_modes = vec![Said {
            source: top.source,
            value: modes_label(top_modes),
        }];
        for r in beyond {
            let s = said(r, modes_label(modes(r)));
            if !said_modes.contains(&s) {
                said_modes.push(s);
            }
        }
        disagreements.push(Disagreement {
            field: DisputedField::Mode,
            said: said_modes,
        });
    }
    disagreements.extend(disputed(DisputedField::ColorCode, colours, |a, b| a == b));

    let mut sources: Vec<SourceRef> = Vec::new();
    for r in &rows {
        if !sources
            .iter()
            .any(|s| s.source == r.source && s.source_id == r.source_id)
        {
            sources.push(SourceRef {
                source: r.source,
                source_id: r.source_id.clone(),
                channel_id: channel_id(r),
                updated: r.updated.clone(),
            });
        }
    }
    // The map's point is the machine's hearham row, never `rec`: hearham invites map use, and the
    // merged record's place and town can be RepeaterBook's (whose terms forbid a map) or the
    // coordinator's (not mapped for now). Rows of one source are nearest first, so this is
    // hearham's nearest listing of the machine.
    let map = rows
        .iter()
        .find(|r| r.source == RepeaterSource::Hearham)
        .map(|h| MapPoint {
            lat: h.lat,
            lon: h.lon,
            callsign: h.callsign.clone(),
            output_mhz: h.output_mhz,
            city: h.city.clone(),
        });
    Machine {
        record: rec,
        sources,
        disagreements,
        along_km: None,
        map,
    }
}

// ── a route: one place to another ───────────────────────────────────────────

/// A trip, for the route list: the great-circle line from one place to another, and a corridor
/// either side of it. Nexus has no road map, so the line is straight; the corridor (25 mi by
/// default in Program) is what keeps the machines along a road that bends away from it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Route {
    pub from: (f64, f64),
    pub to: (f64, f64),
    /// How far a machine may be from the line, km: either side of it and past either end.
    pub corridor_km: f64,
}

/// Where a point sits on a [`Route`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoutePlace {
    /// How far along the line its nearest point of the line is, km from the start: 0 for a point
    /// before the start, the route's length for one past the end.
    pub along_km: f64,
    /// How far it is from that point, km: off the line, or past an end.
    pub off_km: f64,
    /// That nearest point of the line.
    pub nearest: (f64, f64),
}

/// Plans sample the line at points this far apart: well inside the 4-character locator square
/// (1° by 2°, about 110 km by 150) that [`state_for_grid`] answers for, and the RSGB list is asked
/// about, so no square the corridor crosses falls between two samples.
const ROUTE_STEP_KM: f64 = 10.0;

impl Route {
    pub fn length_km(&self) -> f64 {
        haversine_km(self.from, self.to)
    }

    /// Where `p` sits: the point of the line nearest it (the foot of the perpendicular from it to
    /// the great circle, or the nearer end when that falls outside the line), and how far it is
    /// from there.
    pub fn place(&self, p: (f64, f64)) -> RoutePlace {
        const R: f64 = 6371.0;
        let length = self.length_km();
        // The along-track distance on the great circle through both ends (Napier's rule for the
        // right spherical triangle start, foot, point): negative behind the start.
        let to_p = haversine_km(self.from, p) / R;
        let turn = (bearing_deg(self.from, p) - bearing_deg(self.from, self.to)).to_radians();
        let along = (to_p.sin() * turn.cos()).atan2(to_p.cos()) * R;
        let (along_km, nearest) = if along <= 0.0 || length < 1e-9 {
            (0.0, self.from)
        } else if along >= length {
            (length, self.to)
        } else {
            (along, interpolate(self.from, self.to, along / length))
        };
        RoutePlace {
            along_km,
            off_km: haversine_km(nearest, p),
            nearest,
        }
    }

    /// Points along the line from the start to the end, both included, at most [`ROUTE_STEP_KM`]
    /// apart.
    fn samples(&self) -> Vec<(f64, f64)> {
        let steps = (self.length_km() / ROUTE_STEP_KM).ceil().max(1.0) as u32;
        (0..=steps)
            .map(|i| interpolate(self.from, self.to, f64::from(i) / f64::from(steps)))
            .collect()
    }
}

/// The most RepeaterBook states one route asks about: nine, the most a radius search can plan
/// ([`plan_states`]: its origin and eight compass points), so a route never costs more state
/// requests than a radius search can, and each one goes through the same per-state cache and
/// retry throttle in the shell. The states past the ninth are named ([`StatePlan::beyond`]).
pub const RB_STATES_PER_SEARCH: usize = 9;

/// Which RepeaterBook state exports a route asks about, and which it leaves out.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatePlan {
    /// RepeaterBook `state_id`s (US FIPS) to ask about, in the order the route reaches them; at
    /// most [`RB_STATES_PER_SEARCH`].
    pub ask: Vec<String>,
    /// The states the corridor crosses after those, as 2-letter codes, in route order.
    pub beyond: Vec<String>,
}

/// Every state the corridor crosses, in the order the route reaches them: [`plan_states`] at each
/// sample point of the line with the corridor as its radius, so the corridor's edges and both of
/// its ends count. The first [`RB_STATES_PER_SEARCH`] are asked about. Empty when the corridor
/// reaches no US state.
pub fn plan_route_states(route: &Route) -> StatePlan {
    let mut ask: Vec<String> = Vec::new();
    for p in route.samples() {
        for st in plan_states(p, route.corridor_km) {
            if !ask.contains(&st) {
                ask.push(st);
            }
        }
    }
    let beyond = ask.split_off(ask.len().min(RB_STATES_PER_SEARCH));
    StatePlan {
        ask,
        beyond: beyond
            .iter()
            .map(|id| state_code_for_id(id).unwrap_or(id).to_string())
            .collect(),
    }
}

/// Which RSGB locator squares a route asks about: every UK square within the corridor of a point
/// of the line that is itself in the UK ([`plan_rsgb_squares`] at each sample point), in the order
/// the route reaches them; the first [`RSGB_SQUARES_PER_SEARCH`] are asked about and the rest are
/// named, as on a radius search.
pub fn plan_rsgb_route(route: &Route) -> RsgbPlan {
    let mut ask: Vec<String> = Vec::new();
    for p in route.samples() {
        let near = plan_rsgb_squares(p, route.corridor_km);
        for sq in near.ask.into_iter().chain(near.beyond) {
            if !ask.contains(&sq) {
                ask.push(sq);
            }
        }
    }
    let beyond = ask.split_off(ask.len().min(RSGB_SQUARES_PER_SEARCH));
    RsgbPlan { ask, beyond }
}

/// The route list: every source's rows within the corridor, merged into one row per machine
/// exactly as [`merge_nearby`] merges them, in the order the route passes them: along the line,
/// then nearer the line first, then by output. Each machine's [`Machine::along_km`] is set, and
/// its distance and bearing are from its nearest point of the line.
pub fn merge_route(layers: &[&[RepeaterRecord]], route: &Route) -> Vec<Machine> {
    let reach = route.corridor_km + MERGE_MARGIN_KM;
    let mut rows: Vec<(f64, RepeaterRecord)> = layers
        .iter()
        .flat_map(|layer| layer.iter())
        .filter_map(|r| {
            let off = route.place((r.lat, r.lon)).off_km;
            (off <= reach).then(|| (off, r.clone()))
        })
        .collect();
    // As merge_nearby: precedence first and, within one source, nearest the line first, so the
    // top row of a machine is its best source's nearest listing.
    rows.sort_by(|a, b| {
        a.0.partial_cmp(&b.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.1.output_mhz
                    .partial_cmp(&b.1.output_mhz)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    rows.sort_by_key(|(_, r)| r.source);
    let mut out: Vec<(RoutePlace, Machine)> =
        group_machines(rows.into_iter().map(|(_, r)| r).collect())
            .into_iter()
            .map(|g| {
                // The machine stands where its top row places it ([`machine`]).
                let at = route.place((g[0].lat, g[0].lon));
                let mut m = machine(g, at.nearest);
                m.along_km = Some(at.along_km);
                (at, m)
            })
            .filter(|(at, _)| at.off_km <= route.corridor_km)
            .collect();
    out.sort_by(|(a, x), (b, y)| {
        a.along_km
            .partial_cmp(&b.along_km)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(
                a.off_km
                    .partial_cmp(&b.off_km)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
            .then(
                x.record
                    .output_mhz
                    .partial_cmp(&y.record.output_mhz)
                    .unwrap_or(std::cmp::Ordering::Equal),
            )
    });
    out.into_iter().map(|(_, m)| m).collect()
}

// ── record → channel ────────────────────────────────────────────────────────

/// The id a record's channel takes: `<source>:<source id>` ("rsgb:4808", "rb:42-1", "hh:15279").
fn channel_id(r: &RepeaterRecord) -> String {
    let prefix = match r.source {
        RepeaterSource::Rsgb => "rsgb",
        RepeaterSource::Repeaterbook => "rb",
        RepeaterSource::Hearham => "hh",
    };
    format!("{prefix}:{}", r.source_id)
}

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
/// `TSql`; DCS ⇒ `Dtcs`, both ways only when the downlink gives the same code and
/// otherwise send-only ([`Channel::dtcs_tx_only`], the DCS counterpart of `Tone`, for the
/// same reason). Mode: FM, or NFM for a machine its source marks narrow
/// ([`NARROW_FM_KHZ`]), unless the record is digital-only. The machine's links go with it as
/// [`Link::label`] tokens, for the exports' comment ([`Channel::export_comment`]).
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
        id: channel_id(r),
        name,
        rx_mhz: r.output_mhz,
        duplex,
        offset_mhz,
        tone_mode,
        rtone_hz: rtone,
        ctone_hz: ctone,
        dtcs_code: r.dcs.unwrap_or(23),
        dtcs_tx_only: r.dcs.is_some() && r.dcs_dec != r.dcs,
        mode,
        comment: if r.city.is_empty() {
            r.callsign.clone()
        } else {
            r.city.clone()
        },
        links: r.links.iter().map(Link::label).collect(),
        dmr_color_code: r.dmr_color_code,
        source: Some(ChannelSource {
            source: match r.source {
                RepeaterSource::Rsgb => "rsgb".into(),
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

    /// A payload that is not hearham's list is an error, never an empty list: read as "no rows",
    /// it was a week of "No FM repeaters within 50 mi." with nothing to show why (2026-10-02). The
    /// shapes are what a server or a proxy answers as a success: an error page, an error object,
    /// an empty array, rows with no position. Each case is paired with the real list reading.
    #[test]
    fn a_payload_that_is_not_the_list_is_an_error_never_an_empty_list() {
        for (what, body) in [
            ("an HTML page", "<html><body>502 Bad Gateway</body></html>"),
            ("an error object", r#"{"message":"Server Error"}"#),
            ("nothing", ""),
            ("an empty list", "[]"),
            (
                "rows that give no repeater",
                r#"[{"id":1,"callsign":"W9TST","frequency":146940000},{"id":2}]"#,
            ),
        ] {
            let err = parse_hearham_json(body).expect_err(what);
            assert!(
                err.starts_with("hearham: the list it sent"),
                "{what}: {err}"
            );
        }
        // CONTROL: the real list reads, so the errors above are about the payloads.
        assert!(parse_hearham_json(HEARHAM_FIXTURE).is_ok_and(|r| !r.is_empty()));
    }

    #[test]
    fn hearham_fixture_parses() {
        let recs = parse_hearham_json(HEARHAM_FIXTURE).unwrap();
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
        let mut recs = parse_hearham_json(&json).unwrap();
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

    /// The uplink's DCS code is the machine's, and the downlink only confirms it: the same
    /// code there makes the channel DCS both ways, anything else (a tone on a cross-mode
    /// machine, another code, "D031*" that Nexus cannot read) leaves it send-only
    /// ([`a_dcs_code_on_the_uplink_only_exports_as_send_only_dcs`]). A field whose own parts
    /// disagree, a tone and a code ("77.0/D454"), gives neither. Every pair below is one the
    /// directory writes.
    #[test]
    fn a_hearham_dcs_code_is_the_uplinks_and_only_the_same_downlink_code_confirms_it() {
        // (encode, decode, the uplink's code, the downlink's code, the uplink's CTCSS)
        type Case<'a> = (&'a str, &'a str, Option<u16>, Option<u16>, Option<f32>);
        let cases: [Case; 7] = [
            ("DCS023", "DCS023", Some(23), Some(23), None),
            ("D031", "D031*", Some(31), None, None),
            ("DCS172", "DCS172.0", Some(172), None, None),
            ("77.0/D454", "", None, None, None),
            ("DCS365", "DCS364", Some(365), None, None),
            ("D311", "100", Some(311), None, None),
            ("D244/NAC293", "131.8/NAC293", Some(244), None, None),
        ];
        let wrong: Vec<String> = cases
            .iter()
            .filter_map(|&(encode, decode, dcs, dcs_dec, tone)| {
                let r = hh_row_tones("NFM", encode, decode);
                let got = (r.dcs, r.dcs_dec, r.ctcss_enc_hz);
                (got != (dcs, dcs_dec, tone)).then(|| {
                    format!(
                        "{encode:?}/{decode:?}: got {got:?}, want {:?}",
                        (dcs, dcs_dec, tone)
                    )
                })
            })
            .collect();
        assert_eq!(wrong, Vec::<String>::new(), "uplink and downlink DCS codes");
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

    /// A one-row RepeaterBook export with its uplink (`PL`) and downlink (`TSQ`) tone fields.
    fn rb_row_tones(pl: &str, tsq: &str) -> RepeaterRecord {
        let json = format!(
            r#"{{"results":[{{"Callsign":"W9DCS","Frequency":"146.9400","Input Freq":"146.3400","PL":"{pl}","TSQ":"{tsq}","Lat":"42.5","Long":"-89.0","State ID":"55","Rptr ID":"8","FM Analog":"Yes"}}]}}"#
        );
        let mut recs = parse_repeaterbook_json(&json);
        assert_eq!(recs.len(), 1, "the {pl}/{tsq} row did not parse");
        recs.remove(0)
    }

    /// One CSV line split into its fields, honouring the quotes `csv_field` puts around a
    /// value with a comma in it (a city such as "Rockford, Illinois" in the comment).
    fn csv_fields(line: &str) -> Vec<String> {
        let (mut out, mut cur, mut quoted) = (Vec::new(), String::new(), false);
        let mut chars = line.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '"' if quoted && chars.peek() == Some(&'"') => {
                    cur.push('"');
                    chars.next();
                }
                '"' => quoted = !quoted,
                ',' if !quoted => out.push(std::mem::take(&mut cur)),
                _ => cur.push(ch),
            }
        }
        out.push(cur);
        out
    }

    /// The columns a record's CHIRP row carries, read back by CHIRP's own column names.
    fn chirp_columns(r: &RepeaterRecord, names: &[&str]) -> Vec<String> {
        let csv = crate::chirp::to_chirp_csv(&[to_channel(r)], 8, "");
        let mut lines = csv.lines();
        let header = csv_fields(lines.next().expect("a header"));
        let row = csv_fields(lines.next().expect("a CHIRP row"));
        names
            .iter()
            .map(|name| {
                header
                    .iter()
                    .position(|h| h == name)
                    .and_then(|i| row.get(i))
                    .map_or_else(|| format!("<no {name} column>"), |v| v.to_string())
            })
            .collect()
    }

    /// A machine whose source gives its DCS code on the uplink only is send-only DCS: CHIRP's
    /// Cross mode "DTCS->", the code on transmit and the receiver open, which is the DCS
    /// counterpart of `Tone`. As DTCS both ways, a machine whose output carries no code would
    /// keep the radio squelched. hearham's uplink-only rows (32 on 2026-09-30) and
    /// RepeaterBook's PL-only rows alike. A downlink that gives the same code stays DTCS both
    /// ways; a downlink with a tone or another code (a cross-mode machine) is send-only too.
    #[test]
    fn a_dcs_code_on_the_uplink_only_exports_as_send_only_dcs() {
        let cases = [
            (
                "hearham, uplink only",
                hh_row_tones("FM", "DCS023", ""),
                ["Cross", "023", "DTCS->"],
            ),
            (
                "hearham, both ways",
                hh_row_tones("FM", "DCS023", "DCS023"),
                ["DTCS", "023", "Tone->Tone"],
            ),
            (
                "hearham, tone down",
                hh_row_tones("NFM", "D311", "100"),
                ["Cross", "311", "DTCS->"],
            ),
            (
                "RepeaterBook, PL only",
                rb_row_tones("D023", ""),
                ["Cross", "023", "DTCS->"],
            ),
            (
                "RepeaterBook, PL and TSQ",
                rb_row_tones("D023", "D023"),
                ["DTCS", "023", "Tone->Tone"],
            ),
            (
                "RepeaterBook, tone down",
                rb_row_tones("D023", "100.0"),
                ["Cross", "023", "DTCS->"],
            ),
            (
                "a CTCSS machine",
                hh_row_tones("FM", "100.0", ""),
                ["Tone", "023", "Tone->Tone"],
            ),
        ];
        let wrong: Vec<String> = cases
            .iter()
            .filter_map(|(what, r, want)| {
                let got = chirp_columns(r, &["Tone", "DtcsCode", "CrossMode"]);
                (got != want).then(|| format!("{what}: got {got:?}, want {want:?}"))
            })
            .collect();
        assert_eq!(
            wrong,
            Vec::<String>::new(),
            "send-only DCS in the CHIRP export"
        );
    }

    /// The generic CSV names send-only DCS in its Tone Mode column with CHIRP's word for it.
    #[test]
    fn send_only_dcs_is_named_in_the_generic_csv() {
        let c = to_channel(&hh_row_tones("FM", "DCS023", ""));
        let csv = crate::memchan::to_generic_csv(std::slice::from_ref(&c), "");
        let mut lines = csv.lines();
        let header = csv_fields(lines.next().expect("a header"));
        let row = csv_fields(lines.next().expect("a CSV row"));
        let at = |name: &str| {
            header
                .iter()
                .position(|h| h == name)
                .map(|i| row[i].as_str())
        };
        assert_eq!((at("Tone Mode"), at("DCS")), (Some("DTCS->"), Some("023")));
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
        // Except the row's date: "Last Update" is not among the proxy's 20 fields, so a row
        // that came through it says "no date" (the list's fetch date stands in) — by design.
        assert!(full.iter().any(|r| r.updated.is_some()));
        assert!(narrowed.iter().all(|r| r.updated.is_none()));
        let undated: Vec<RepeaterRecord> = full
            .into_iter()
            .map(|r| RepeaterRecord { updated: None, ..r })
            .collect();
        assert_eq!(narrowed, undated, "narrowing the export changed a record");
    }

    /// The measured cases from the hearham directory that motivated this check.
    #[test]
    fn missing_major_band_flags_a_gap_not_empty_country() {
        let base = parse_hearham_json(HEARHAM_FIXTURE).unwrap()[0].clone();
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
        assert_eq!(
            w9abc.updated.as_deref(),
            Some("2026-05-14"),
            "the row's own date"
        );
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
        let recs = parse_hearham_json(HEARHAM_FIXTURE).unwrap();
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

    // ── RSGB + the merge (N46 P1) ───────────────────────────────────────────

    /// Real listings, kept small: RSGB rows from `api-beta.rsgb.online/locator/` IO83, IO93 and
    /// IO74, and their hearham twins, both read 2026-10-01 (keeper callsigns and hearham's free
    /// text blanked). Repeater data: RSGB ETCC (ukrepeater.net), and hearham.com.
    const RSGB_FIXTURE: &str = include_str!("../tests/fixtures/rsgb_locator_sample.json");
    const HEARHAM_UK_FIXTURE: &str = include_str!("../tests/fixtures/hearham_uk_sample.json");
    /// Manchester: the middle of the comparison the merge was designed from.
    const MANCHESTER: (f64, f64) = (53.48, -2.24);

    fn rsgb(call: &str) -> RepeaterRecord {
        parse_rsgb_json(RSGB_FIXTURE)
            .expect("the fixture is the real shape")
            .into_iter()
            .find(|r| r.callsign == call)
            .unwrap_or_else(|| panic!("{call} is not in the RSGB fixture"))
    }

    #[test]
    fn an_rsgb_payload_reads_its_voice_listings_with_their_modes_and_tone() {
        let recs = parse_rsgb_json(RSGB_FIXTURE).expect("the real shape parses");
        let calls: Vec<&str> = recs.iter().map(|r| r.callsign.as_str()).collect();
        // ATV (GB3UD), a packet mailbox (GB7VAX) and a link transmitter (GB7DZ-L) are not
        // voice channels; the twelve voice listings are all kept, Tetra-only GB7PR included.
        for gone in ["GB3UD", "GB7VAX", "GB7DZ-L"] {
            assert!(!calls.contains(&gone), "{gone} is not a channel to program");
        }
        assert_eq!(recs.len(), 12, "{calls:?}");

        let pp = rsgb("GB3PP");
        assert_eq!(pp.source, RepeaterSource::Rsgb);
        assert_eq!(pp.source_id, "4356");
        assert!((pp.output_mhz - 433.375).abs() < 1e-9, "tx is the output");
        assert!((pp.input_mhz - 434.975).abs() < 1e-9, "rx is the input");
        assert_eq!(pp.ctcss_enc_hz, Some(82.5));
        assert_eq!(pp.ctcss_dec_hz, None, "the list gives the access tone only");
        assert!(pp.fm && pp.dmr && pp.fusion && !pp.dstar, "A, M:10, F");
        assert_eq!(
            pp.dmr_color_code,
            Some(10),
            "\"M:10\" is DMR on colour code 10"
        );
        assert_eq!(pp.bandwidth_khz, Some(12.5));
        assert_eq!(pp.updated, None, "the API has no date per listing");

        let jb = rsgb("MB6JB");
        assert!(jb.dmr && !jb.fm, "a bare \"M\" is DMR");
        assert_eq!(jb.dmr_color_code, None, "with no colour code given");
        assert_eq!(jb.ctcss_enc_hz, None, "a ctcss of 0 is no tone listed");

        assert!(!rsgb("GB3XL").operational, "NOT OPERATIONAL");
        assert!(
            rsgb("GB3MN").operational,
            "REDUCED OUTPUT is still on the air"
        );
        let pr = rsgb("GB7PR");
        assert!(
            !pr.fm && !pr.dmr && !pr.dstar && !pr.fusion,
            "Tetra has no field"
        );
        assert_eq!(rsgb("MB7INI").city, "CARRICKFERGUS");
        assert_eq!(rsgb("GB3BW").city, "WAKEFIELD");
    }

    #[test]
    fn an_rsgb_payload_in_another_shape_is_refused_never_read_as_empty() {
        assert_eq!(
            parse_rsgb_json(r#"{"data":[]}"#),
            Ok(vec![]),
            "an empty square is fine"
        );
        let row = r#"{"id":1,"type":"AV","status":"OPERATIONAL","repeater":"GB3XX","town":"X","modeCodes":["A"],"tx":145600000,"rx":145000000,"ctcss":77,"txbw":12.5,"locator":"IO83LP"}"#;
        assert_eq!(
            parse_rsgb_json(&format!(r#"{{"data":[{row}]}}"#)).map(|r| r.len()),
            Ok(1),
            "CONTROL: the one-row payload the cases below each break"
        );
        let shapes = [
            ("a bare list", format!("[{row}]")),
            ("another wrapper", format!(r#"{{"results":[{row}]}}"#)),
            (
                "tx renamed",
                format!(r#"{{"data":[{}]}}"#, row.replace("\"tx\"", "\"output\"")),
            ),
            (
                "modeCodes as text",
                format!(r#"{{"data":[{}]}}"#, row.replace(r#"["A"]"#, r#""A""#)),
            ),
            (
                "no callsign",
                format!(
                    r#"{{"data":[{}]}}"#,
                    row.replace("\"repeater\"", "\"call\"")
                ),
            ),
            (
                "an HTML error page",
                "<html><body>502 Bad Gateway</body></html>".to_string(),
            ),
        ];
        for (what, body) in shapes {
            assert!(
                parse_rsgb_json(&body).is_err(),
                "{what} must be refused, not read as empty"
            );
        }
    }

    #[test]
    fn an_rsgb_position_is_its_locator_or_a_grid_reference_inside_its_square() {
        // The Ordnance Survey's own worked example (annex C): E 651409.903, N 313177.270 is
        // 52°39′27.2531″N 1°43′4.5177″E on OSGB36.
        let (lat, lon) = tm_inverse(&BRITISH_GRID, 651_409.903, 313_177.270);
        assert!(
            (lat - (52.0 + 39.0 / 60.0 + 27.2531 / 3600.0)).abs() < 1e-7,
            "{lat}"
        );
        assert!(
            (lon - (1.0 + 43.0 / 60.0 + 4.5177 / 3600.0)).abs() < 1e-7,
            "{lon}"
        );
        // The same point as a 10-figure reference (TG, 1 m squares, so half a metre off).
        let (lat, lon) = grid_ref_to_latlon("TG 51409 13177").expect("a GB reference");
        assert!((lat - 52.657_570).abs() < 2e-5 && (lon - 1.717_922).abs() < 2e-5);
        // Irish Grid, as the list writes it: GB3NI's IJ4076 lands inside IO74CO, the
        // coordinator's own locator for the same site (54.583–54.625 N, 5.833–5.750 W).
        let (lat, lon) = grid_ref_to_latlon("IJ4076").expect("an Irish reference");
        assert!(
            (54.583..54.625).contains(&lat) && (-5.834..-5.75).contains(&lon),
            "{lat},{lon}"
        );
        for bad in ["", "IJ", "SJ295", "XX1234", "J4076", "SJ29X7", "IÉ4076"] {
            assert_eq!(grid_ref_to_latlon(bad), None, "{bad:?}");
        }

        // A 6-character locator is used as it stands, even where the reference disagrees
        // (GB3OA: Southport by its locator, Blackpool by its reference).
        assert_eq!(
            rsgb_position("IO83LP", "SD3341"),
            maidenhead_to_latlon("IO83LP")
        );
        // A 4-character square takes the reference inside it (GB3CR, Caergwrle), which is
        // NOT the square's centre 50 km away…
        let cr = rsgb_position("IO83", "SJ2957").expect("placed");
        assert_eq!(Some(cr), grid_ref_to_latlon("SJ2957"));
        assert_eq!(latlon_to_maidenhead(cr.0, cr.1), "IO83");
        assert!(haversine_km(cr, maidenhead_to_latlon("IO83").unwrap()) > 40.0);
        // …but never one outside it (a London reference): the square's centre then.
        assert_eq!(
            rsgb_position("IO83", "TQ3080"),
            maidenhead_to_latlon("IO83")
        );
        assert_eq!(rsgb_position("", "SJ2957"), grid_ref_to_latlon("SJ2957"));
        assert_eq!(rsgb_position("", ""), None);
        // Through the parser: MB7INI's 4-character IO74 and Irish IJ3988 (Carrickfergus).
        let ini = rsgb("MB7INI");
        assert!(
            (ini.lat - 54.72).abs() < 0.03 && (ini.lon + 5.81).abs() < 0.03,
            "{ini:?}"
        );
    }

    #[test]
    fn a_uk_origin_asks_rsgb_about_the_nearest_squares_and_names_the_rest() {
        // Manchester sits in IO83, 16 km from IO93 and over 50 km from IO82/IO84.
        let near = plan_rsgb_squares(MANCHESTER, 40.0);
        assert_eq!(near.ask, vec!["IO83", "IO93"]);
        assert!(near.beyond.is_empty());

        let wide = plan_rsgb_squares(MANCHESTER, 322.0);
        assert_eq!(wide.ask.len(), RSGB_SQUARES_PER_SEARCH);
        assert_eq!(wide.ask[0], "IO83");
        assert!(!wide.beyond.is_empty(), "200 mi reaches past nine squares");
        let km = |sq: &String| km_to_square(MANCHESTER, sq).unwrap();
        let farthest_asked = wide.ask.iter().map(km).fold(0.0, f64::max);
        assert!(
            wide.beyond.iter().all(|sq| km(sq) >= farthest_asked),
            "nearest first"
        );
        assert!(wide
            .ask
            .iter()
            .chain(&wide.beyond)
            .all(|sq| RSGB_SQUARES.contains(&sq.as_str())));

        // Belfast is a UK origin (IO74); Dublin (IO63), Paris and Chicago are not.
        assert_eq!(plan_rsgb_squares((54.60, -5.93), 25.0).ask[0], "IO74");
        for elsewhere in [(53.35, -6.26), (48.86, 2.35), (41.88, -87.63)] {
            assert_eq!(plan_rsgb_squares(elsewhere, 100.0), RsgbPlan::default());
        }
    }

    #[test]
    fn the_rsgb_layer_is_every_square_it_asked_about_or_none() {
        let one = |sq: &str, body: Option<&str>, at: i64, stale: bool| SquareFetch {
            square: sq.into(),
            body: body.map(str::to_string),
            fetched_utc: at,
            stale,
        };
        let ok = fold_rsgb_squares(&[
            one("IO83", Some(RSGB_FIXTURE), 900, false),
            one("IO93", Some(r#"{"data":[]}"#), 1000, true),
        ])
        .expect("both squares answered");
        assert_eq!(ok.records.len(), 12);
        assert_eq!(ok.oldest_utc, 900, "the older square's stamp");
        assert!(ok.stale);

        let silent = fold_rsgb_squares(&[
            one("IO83", Some(RSGB_FIXTURE), 900, false),
            one("IO93", None, 0, false),
        ]);
        assert!(
            silent.as_ref().is_err_and(|e| e.contains("IO93")),
            "{silent:?}"
        );
        let changed = fold_rsgb_squares(&[
            one("IO83", Some(RSGB_FIXTURE), 900, false),
            one("IO93", Some("[]"), 1000, false),
        ]);
        assert!(
            changed.is_err(),
            "a payload in another shape drops the layer"
        );
        assert!(fold_rsgb_squares(&[]).is_err());
    }

    /// One merged machine as a line, by value: what it programs, where it came from and what its
    /// sources disagree on.
    fn golden_line(m: &Machine) -> String {
        let r = &m.record;
        let tone = match (r.dcs, r.ctcss_enc_hz) {
            (Some(c), _) => format!("D{c:03}"),
            (None, Some(hz)) => format!("{hz:.1}"),
            (None, None) => "-".into(),
        };
        let cc = r.dmr_color_code.map_or("-".to_string(), |c| c.to_string());
        let sources: Vec<&str> = m.sources.iter().map(|s| s.channel_id.as_str()).collect();
        let differ: Vec<String> = m
            .disagreements
            .iter()
            .map(|d| {
                let said: Vec<String> = d
                    .said
                    .iter()
                    .map(|s| format!("{:?} {}", s.source, s.value))
                    .collect();
                format!("{:?}({})", d.field, said.join("|"))
            })
            .collect();
        format!(
            "{} {} in {} {} {} cc{} {} [{}] {}",
            r.callsign,
            mhz_label(r.output_mhz),
            mhz_label(r.input_mhz),
            tone,
            modes_label(modes(r)),
            cc,
            if r.operational { "on" } else { "off" },
            sources.join(" "),
            differ.join(" ")
        )
        .trim_end()
        .to_string()
    }

    /// THE GOLDEN: the coordinator's listings and hearham's around Manchester, merged. Each line
    /// was reasoned from the two fixtures before the merge first ran:
    /// - GB3BW: RSGB says 88.5 and input 438.4125; hearham lists it twice at 82.5, once with
    ///   that input and once as simplex. Both disagreements shown, RSGB's values programmed.
    /// - GB3XN: hearham has it simplex; RSGB gives the 438.525 input (a radio programmed from
    ///   hearham alone could not open it).
    /// - GB3XL, GB3PP: hearham lists only the DMR side; RSGB says FM too, so each is ONE
    ///   programmable FM row with its CTCSS and colour code (GB3PP's colour codes disagree).
    /// - GB7SJ, MB6JB, GB3OA: hearham names a mode RSGB does not list.
    /// - GB3CR: hearham places its twin in County Fermanagh, outside the radius, so RSGB's row
    ///   stands alone; MB7ITW is hearham's alone; GB7PR (Tetra) is RSGB's alone.
    #[test]
    fn the_uk_merge_golden() {
        let coordinator = parse_rsgb_json(RSGB_FIXTURE).unwrap();
        let hearham = parse_hearham_json(HEARHAM_UK_FIXTURE).unwrap();
        let got: Vec<String> = merge_nearby(&[&coordinator, &[], &hearham], MANCHESTER, 100.0)
            .iter()
            .map(golden_line)
            .collect();
        let want = [
            "MB7IGF 430.0125 in 430.0125 82.5 FM cc- on [rsgb:355 hh:15527]",
            "GB3MN 145.65 in 145.05 82.5 FM+YSF cc- on [rsgb:2293 hh:14327]",
            "MB6JB 431.1375 in 431.1375 - DMR cc- on [rsgb:6531 hh:18598] Mode(Rsgb DMR|Hearham FM)",
            "GB7SJ 433.175 in 434.775 103.5 FM cc- on [rsgb:2437 hh:4042] Mode(Rsgb FM|Hearham DMR)",
            "GB3PP 433.375 in 434.975 82.5 FM+DMR+YSF cc10 on [rsgb:4356 hh:3959] ColorCode(Rsgb CC10|Hearham CC9)",
            "GB7PR 434.05 in 434.05 - — cc- on [rsgb:5190]",
            "GB3XL 430.8875 in 438.4875 82.5 FM+DMR cc1 off [rsgb:6863 hh:4017]",
            "MB7ITW 50.53 in 50.53 71.9 FM cc- off [hh:671]",
            "GB3OA 145.6125 in 145.0125 82.5 FM+YSF cc- on [rsgb:207 hh:692 hh:4092] Mode(Rsgb FM+YSF|Hearham DMR)",
            "GB3BW 430.8125 in 438.4125 88.5 FM cc- on [rsgb:4808 hh:12141 hh:12268] Tone(Rsgb 88.5|Hearham 82.5) Input(Rsgb 438.4125|Hearham 438.4125|Hearham 430.8125)",
            "GB3CR 433.15 in 434.75 110.9 FM cc- on [rsgb:744]",
            "GB3XN 430.925 in 438.525 71.9 FM cc- on [rsgb:1031 hh:721] Input(Rsgb 438.525|Hearham 430.925)",
        ];
        assert_eq!(got, want);
    }

    /// A hand-built row for the rule tests (no parser involved): FM on 146.94, input 146.34,
    /// no tone, near Hartford; `source`, id and callsign given.
    fn row(source: RepeaterSource, id: &str, call: &str) -> RepeaterRecord {
        RepeaterRecord {
            source,
            source_id: id.into(),
            callsign: call.into(),
            output_mhz: 146.94,
            input_mhz: 146.34,
            ctcss_enc_hz: None,
            ctcss_dec_hz: None,
            dcs: None,
            dcs_dec: None,
            lat: 41.73,
            lon: -72.71,
            city: String::new(),
            county: String::new(),
            state: String::new(),
            fm: true,
            dmr: false,
            dstar: false,
            fusion: false,
            dmr_color_code: None,
            bandwidth_khz: None,
            operational: true,
            open_use: true,
            updated: None,
            links: Vec::new(),
            distance_km: 0.0,
            bearing_deg: 0.0,
        }
    }
    const HARTFORD: (f64, f64) = (41.76, -72.69);
    fn merged(rows: &[RepeaterRecord]) -> Vec<Machine> {
        merge_nearby(&[rows], HARTFORD, 50.0)
    }

    #[test]
    fn one_machine_is_one_callsign_on_one_output() {
        let a = row(RepeaterSource::Hearham, "1", "W1AW");
        let b = row(RepeaterSource::Repeaterbook, "2", "w1aw-r");
        assert_eq!(
            merged(&[a.clone(), b.clone()]).len(),
            1,
            "the base callsign, any case"
        );
        let near = RepeaterRecord {
            output_mhz: 146.9425,
            ..b.clone()
        };
        assert_eq!(
            merged(&[a.clone(), near]).len(),
            1,
            "2.5 kHz apart: one channel"
        );
        let next = RepeaterRecord {
            output_mhz: 146.9431,
            ..b.clone()
        };
        assert_eq!(
            merged(&[a.clone(), next]).len(),
            2,
            "3.1 kHz apart: two channels"
        );
        let other = row(RepeaterSource::Repeaterbook, "3", "W1XYZ");
        assert_eq!(
            merged(&[a, other]).len(),
            2,
            "two callsigns on one site are two machines"
        );
    }

    #[test]
    fn without_a_callsign_it_takes_both_frequencies_and_one_site() {
        let a = row(RepeaterSource::Hearham, "1", "");
        let b = row(RepeaterSource::Repeaterbook, "2", "W1AW");
        let at = |r: &RepeaterRecord, north_km: f64| RepeaterRecord {
            lat: r.lat + north_km / 111.2,
            ..r.clone()
        };
        assert_eq!(
            merged(&[a.clone(), at(&b, 4.0)]).len(),
            1,
            "4 km apart: one machine"
        );
        assert_eq!(
            merged(&[a.clone(), at(&b, 6.0)]).len(),
            2,
            "6 km apart: two"
        );
        let other_input = RepeaterRecord {
            input_mhz: b.input_mhz + 0.005,
            ..b.clone()
        };
        assert_eq!(
            merged(&[a.clone(), other_input]).len(),
            2,
            "another input: two"
        );
        let m = &merged(&[a, b])[0];
        assert_eq!(
            m.record.callsign, "W1AW",
            "the callsign comes from the row that has one"
        );
        assert_eq!(m.sources.len(), 2, "every source id kept");
    }

    #[test]
    fn each_field_comes_from_the_coordinator_then_repeaterbook_then_hearham() {
        let tone = |r: RepeaterRecord, enc: Option<f32>, dec: Option<f32>| RepeaterRecord {
            ctcss_enc_hz: enc,
            ctcss_dec_hz: dec,
            ..r
        };
        let coord = row(RepeaterSource::Rsgb, "1", "W1AW");
        let rb = RepeaterRecord {
            city: "Newington".into(),
            ..tone(
                row(RepeaterSource::Repeaterbook, "2", "W1AW"),
                Some(123.0),
                None,
            )
        };
        let hh = tone(
            row(RepeaterSource::Hearham, "3", "W1AW"),
            Some(100.0),
            Some(100.0),
        );

        // The coordinator gives no tone: RepeaterBook's, and the disagreement with hearham shown.
        // The argument order does not matter; the source does.
        for rows in [
            vec![hh.clone(), rb.clone(), coord.clone()],
            vec![coord.clone(), rb.clone(), hh.clone()],
        ] {
            let m = &merged(&rows)[0];
            assert_eq!(m.record.ctcss_enc_hz, Some(123.0));
            assert_eq!(
                m.record.ctcss_dec_hz, None,
                "the tone fields come from ONE row"
            );
            assert_eq!(m.record.city, "Newington", "a field the coordinator lacks");
            assert_eq!(
                m.record.source,
                RepeaterSource::Rsgb,
                "the top row is the coordinator's"
            );
            let ids: Vec<&str> = m.sources.iter().map(|s| s.channel_id.as_str()).collect();
            assert_eq!(ids, ["rsgb:1", "rb:2", "hh:3"]);
            assert_eq!(
                m.disagreements,
                vec![Disagreement {
                    field: DisputedField::Tone,
                    said: vec![
                        Said {
                            source: RepeaterSource::Repeaterbook,
                            value: "123.0".into()
                        },
                        Said {
                            source: RepeaterSource::Hearham,
                            value: "100.0".into()
                        },
                    ],
                }]
            );
        }
        // The coordinator's own tone wins over both.
        let m = &merged(&[tone(coord, Some(88.5), None), rb, hh])[0];
        assert_eq!(m.record.ctcss_enc_hz, Some(88.5));
        assert_eq!(m.disagreements[0].said.len(), 3);
    }

    #[test]
    fn a_mixed_machine_the_coordinator_lists_with_fm_programs_as_one_fm_row() {
        let coordinator = parse_rsgb_json(RSGB_FIXTURE).unwrap();
        let hearham = parse_hearham_json(HEARHAM_UK_FIXTURE).unwrap();
        // hearham alone: GB3XL is a DMR row, nothing to program.
        let alone = merge_nearby(&[&hearham], MANCHESTER, 100.0);
        let xl = alone.iter().find(|m| m.record.callsign == "GB3XL").unwrap();
        assert!(!xl.record.fm);
        assert_eq!(to_channel(&xl.record).mode, ChanMode::Dmr);
        // With the coordinator: one FM row, its CTCSS and its colour code.
        let both = merge_nearby(&[&coordinator, &hearham], MANCHESTER, 100.0);
        let xl: Vec<&Machine> = both
            .iter()
            .filter(|m| m.record.callsign == "GB3XL")
            .collect();
        assert_eq!(xl.len(), 1, "one row per machine");
        let c = to_channel(&xl[0].record);
        assert_eq!(
            (c.mode, c.tone_mode, c.rtone_hz),
            (ChanMode::Nfm, ToneMode::Tone, 82.5)
        );
        assert_eq!(c.dmr_color_code, Some(1));
        assert_eq!((c.duplex, c.id.as_str()), (Duplex::Plus, "rsgb:6863"));
        assert!((c.offset_mhz - 7.6).abs() < 1e-9);
        let src = c.source.expect("provenance");
        assert_eq!(
            (src.source.as_str(), src.source_id.as_str()),
            ("rsgb", "6863")
        );
    }

    // ── links ───────────────────────────────────────────────────────────────

    /// One hearham row with a `group` and an `internet_node` (written as JSON: a string, or null).
    fn hh_node(group: &str, node: &str) -> RepeaterRecord {
        let json = format!(
            r#"[{{"id":1,"callsign":"W9TST","latitude":42.3,"longitude":-89.0,"city":"Rockford, Illinois","group":"{group}","internet_node":{node},"mode":"FM","encode":"","decode":"","frequency":146940000,"offset":-600000,"description":"","power":"unknown","operational":1,"restriction":""}}]"#
        );
        let mut recs = parse_hearham_json(&json).unwrap();
        assert_eq!(recs.len(), 1, "the {group:?} {node} row did not parse");
        recs.remove(0)
    }
    fn link(network: LinkNetwork, node: &str) -> Link {
        Link {
            network,
            node: node.into(),
        }
    }

    /// hearham's `internet_node` is a node on the network its `group` names (AllStar, IRLP, or the
    /// DMR ID of a row the DMR registry gave), and a plain node where the group names none. The
    /// shapes are the directory's own on 2026-10-01: a string, one with a tab after it, null, and
    /// a callsign where a number belongs.
    #[test]
    fn a_hearham_internet_node_is_a_link_on_the_network_its_group_names() {
        let recs = parse_hearham_json(HEARHAM_FIXTURE).unwrap();
        let links = |id: &str| {
            recs.iter()
                .find(|r| r.source_id == id)
                .unwrap()
                .links
                .clone()
        };
        assert_eq!(
            links("15279"),
            [link(LinkNetwork::DmrId, "314158")],
            "the DMR registry's row"
        );
        assert_eq!(links("1166"), [link(LinkNetwork::Irlp, "8625")]);
        assert_eq!(links("18545"), [link(LinkNetwork::AllStar, "569394")]);
        assert!(links("16091").is_empty(), "a null node is none");

        assert_eq!(
            hh_node("", r#""7230""#).links,
            [link(LinkNetwork::Node, "7230")],
            "a group that names no network"
        );
        assert_eq!(
            hh_node("PALS", r#""4100""#).links,
            [link(LinkNetwork::Node, "4100")]
        );
        assert_eq!(
            hh_node("IRLP", "\"8728\\t\"").links,
            [link(LinkNetwork::Irlp, "8728")],
            "trimmed"
        );
        assert!(
            hh_node("w0eno", r#""w0eno""#).links.is_empty(),
            "a callsign is not a node"
        );
        assert!(hh_node("Allstar", r#""""#).links.is_empty());
        assert!(hh_node("Allstar", "null").links.is_empty());
    }

    /// A machine keeps every link its rows give, once each, in the network order (AllStar, IRLP,
    /// DMR ID, plain node) and, within one network, as its rows give them, nearest first. hearham
    /// lists a linked machine once per node: GB3BW twice on IRLP, GB3OA once on IRLP and once as
    /// the DMR registry's. The coordinator's rows give none.
    #[test]
    fn a_machine_keeps_every_link_its_rows_give_once() {
        let coordinator = parse_rsgb_json(RSGB_FIXTURE).unwrap();
        let hearham = parse_hearham_json(HEARHAM_UK_FIXTURE).unwrap();
        let both = merge_nearby(&[&coordinator, &[], &hearham], MANCHESTER, 100.0);
        let links = |call: &str| {
            both.iter()
                .find(|m| m.record.callsign == call)
                .unwrap()
                .record
                .links
                .clone()
        };
        assert_eq!(
            links("GB3BW"),
            [
                link(LinkNetwork::Irlp, "5775"),
                link(LinkNetwork::Irlp, "5201")
            ]
        );
        assert_eq!(
            links("GB3OA"),
            [
                link(LinkNetwork::Irlp, "5302"),
                link(LinkNetwork::DmrId, "235239")
            ]
        );
        assert!(links("GB7PR").is_empty(), "the coordinator's row alone");
        assert!(coordinator.iter().all(|r| r.links.is_empty()));

        let on = |id: &str, l: Link| RepeaterRecord {
            links: vec![l],
            ..row(RepeaterSource::Hearham, id, "W1AW")
        };
        let m = &merged(&[
            on("1", link(LinkNetwork::Node, "7230")),
            on("2", link(LinkNetwork::DmrId, "310123")),
            on("3", link(LinkNetwork::Irlp, "3570")),
            on("4", link(LinkNetwork::AllStar, "2462")),
            on("5", link(LinkNetwork::Irlp, "3570")),
        ])[0];
        assert_eq!(
            m.record.links,
            [
                link(LinkNetwork::AllStar, "2462"),
                link(LinkNetwork::Irlp, "3570"),
                link(LinkNetwork::DmrId, "310123"),
                link(LinkNetwork::Node, "7230"),
            ]
        );
    }

    /// A machine's links and its DMR colour code ride into its channel and out in both files'
    /// comment, after the town, which stays the channel's own comment.
    #[test]
    fn a_machines_links_and_colour_code_ride_into_both_exports() {
        let coordinator = parse_rsgb_json(RSGB_FIXTURE).unwrap();
        let hearham = parse_hearham_json(HEARHAM_UK_FIXTURE).unwrap();
        let both = merge_nearby(&[&coordinator, &[], &hearham], MANCHESTER, 100.0);
        let pp = both.iter().find(|m| m.record.callsign == "GB3PP").unwrap();
        let c = to_channel(&pp.record);
        assert_eq!(c.links, ["DMR ID 234109"]);
        assert_eq!(c.dmr_color_code, Some(10));
        assert!(
            !c.comment.is_empty() && !c.comment.contains(';'),
            "the comment is still the town: {:?}",
            c.comment
        );
        let want = format!("{}; DMR ID 234109; CC10", c.comment);
        assert_eq!(c.export_comment(), want);
        let chirp = crate::chirp::to_chirp_csv(std::slice::from_ref(&c), 7, "");
        assert!(chirp.lines().nth(1).unwrap().contains(&want), "{chirp}");
        let csv = crate::memchan::to_generic_csv(std::slice::from_ref(&c), "");
        assert!(csv.lines().nth(1).unwrap().contains(&want), "{csv}");

        let labels: Vec<String> = [
            link(LinkNetwork::AllStar, "2462"),
            link(LinkNetwork::Irlp, "3570"),
            link(LinkNetwork::DmrId, "310123"),
            link(LinkNetwork::Node, "7230"),
        ]
        .iter()
        .map(Link::label)
        .collect();
        assert_eq!(
            labels,
            ["AllStar 2462", "IRLP 3570", "DMR ID 310123", "node 7230"]
        );
    }

    /// RepeaterBook's own node columns are never read: a channel's links are saved with it, where
    /// the Remote's programming view can show them, and RepeaterBook's rows never leave this PC.
    #[test]
    fn repeaterbook_node_columns_are_never_read() {
        let json = r#"{"results":[{"Callsign":"W9NOD","Frequency":"146.9400","Input Freq":"146.3400","PL":"103.5","Lat":"42.5","Long":"-89.0","State ID":"55","Rptr ID":"8","FM Analog":"Yes","AllStar Node":"2462","EchoLink Node":"12345","IRLP Node":"3570","Wires Node":"54321","DMR":"Yes","DMR ID":"310123","DMR Color Code":"1"}]}"#;
        let recs = parse_repeaterbook_json(json);
        assert_eq!(recs.len(), 1);
        assert!(recs[0].links.is_empty());
        assert!(to_channel(&recs[0]).links.is_empty());
        assert!(parse_repeaterbook_json(RB_FIXTURE)
            .iter()
            .all(|r| r.links.is_empty()));
    }
    // ── the route list ──────────────────────────────────────────────────────

    /// A route due north up the 72.7° W meridian, from south of Hartford to Massachusetts.
    const ROUTE_START: (f64, f64) = (41.0, -72.7);
    const ROUTE_END: (f64, f64) = (42.5, -72.7);
    /// Liverpool to Leeds, the M62 across the Pennines.
    const LIVERPOOL: (f64, f64) = (53.41, -2.98);
    const LEEDS: (f64, f64) = (53.80, -1.55);
    /// Program's default corridor, 25 mi.
    const CORRIDOR_KM: f64 = 40.2336;

    fn up_the_meridian(corridor_km: f64) -> Route {
        Route {
            from: ROUTE_START,
            to: ROUTE_END,
            corridor_km,
        }
    }

    /// A point `along` km up the route's meridian (south of the start when negative) and `east` km
    /// east of it (west when negative). The great circle due east from a point of a meridian
    /// crosses it at a right angle, so the point's foot on the line is exactly `along` up it.
    fn on_route(along: f64, east: f64) -> (f64, f64) {
        let foot = if along < 0.0 {
            crate::geo::destination_point(ROUTE_START, 180.0, -along)
        } else {
            crate::geo::destination_point(ROUTE_START, 0.0, along)
        };
        if east < 0.0 {
            crate::geo::destination_point(foot, 270.0, -east)
        } else {
            crate::geo::destination_point(foot, 90.0, east)
        }
    }

    /// A hearham FM row at `p`, on its own output.
    fn route_row(call: &str, mhz: f64, p: (f64, f64)) -> RepeaterRecord {
        RepeaterRecord {
            output_mhz: mhz,
            input_mhz: mhz + 0.6,
            lat: p.0,
            lon: p.1,
            ..row(RepeaterSource::Hearham, call, call)
        }
    }

    #[test]
    fn a_route_places_a_point_by_how_far_along_the_line_and_how_far_off_it() {
        let route = up_the_meridian(40.0);
        let length = route.length_km();
        assert!((length - 166.8).abs() < 0.1, "{length}");
        let near = |a: f64, b: f64| (a - b).abs() < 0.05;
        // Beside the line, either side of it, and on it.
        for (along, east) in [(50.0, 30.0), (55.0, 0.0), (120.0, -39.0), (0.0, 10.0)] {
            let at = route.place(on_route(along, east));
            assert!(
                near(at.along_km, along) && near(at.off_km, east.abs()),
                "{along} {east}: {at:?}"
            );
            assert!(
                haversine_km(at.nearest, on_route(along, 0.0)) < 0.05,
                "{at:?}"
            );
        }
        // Before the start and past the end, the nearer end is the nearest point of the line.
        let before = route.place(on_route(-20.0, 0.0));
        assert!(
            near(before.along_km, 0.0) && near(before.off_km, 20.0),
            "{before:?}"
        );
        assert!(haversine_km(before.nearest, ROUTE_START) < 0.05);
        let beyond = on_route(length + 30.0, 10.0);
        let past = route.place(beyond);
        assert!(near(past.along_km, length), "{past:?}");
        assert!(
            near(past.off_km, haversine_km(ROUTE_END, beyond)),
            "{past:?}"
        );
        assert!(haversine_km(past.nearest, ROUTE_END) < 0.05);
        // A route that ends where it starts is a circle around the place.
        let circle = Route {
            from: ROUTE_START,
            to: ROUTE_START,
            corridor_km: 40.0,
        };
        let p = on_route(30.0, 20.0);
        let at = circle.place(p);
        assert!(near(at.along_km, 0.0) && near(at.off_km, haversine_km(ROUTE_START, p)));
    }

    /// The route list is the corridor, in the order the route passes it: along the line, never by
    /// distance from the start, which would put W1BBB (55 km up the line, on it) before W1AAA (50 km
    /// up, 30 km off, 58 km from the start). A machine before the start or past the end is in it
    /// while it is within the corridor of that end.
    #[test]
    fn the_route_list_is_the_corridor_in_the_order_the_route_passes_it() {
        let route = up_the_meridian(40.0);
        let length = route.length_km();
        let rows = [
            route_row("W1BBB", 146.61, on_route(55.0, 0.0)),
            route_row("W1AAA", 146.67, on_route(50.0, 30.0)),
            // 45 km off, and 41 km: outside, the second inside the merge's margin.
            route_row("W1OUT", 146.73, on_route(100.0, 45.0)),
            route_row("W1EDG", 146.79, on_route(100.0, 41.0)),
            route_row("W1GGG", 146.85, on_route(120.0, -39.0)),
            route_row("W1DDD", 146.91, on_route(-20.0, 0.0)),
            route_row("W1EEE", 146.97, on_route(length + 30.0, 0.0)),
            route_row("W1FAR", 147.03, on_route(length + 50.0, 0.0)),
            // Behind the start too, nearer it: both are at the start, the nearer first.
            route_row("W1HHH", 147.09, on_route(-8.0, 3.0)),
        ];
        let got = merge_route(&[&rows], &route);
        let calls: Vec<&str> = got.iter().map(|m| m.record.callsign.as_str()).collect();
        assert_eq!(
            calls,
            ["W1HHH", "W1DDD", "W1AAA", "W1BBB", "W1GGG", "W1EEE"]
        );
        // Each one's place: how far along the line, and how far off it (its distance).
        let want = [
            (0.0, haversine_km(ROUTE_START, on_route(-8.0, 3.0))),
            (0.0, 20.0),
            (50.0, 30.0),
            (55.0, 0.0),
            (120.0, 39.0),
            (length, 30.0),
        ];
        for (m, (along, off)) in got.iter().zip(want) {
            let (a, d) = (m.along_km.unwrap(), m.record.distance_km);
            assert!(
                (a - along).abs() < 0.05 && (d - off).abs() < 0.05,
                "{}: {a} {d}",
                m.record.callsign
            );
        }
        // Its bearing is from the line: W1AAA is east of it and W1GGG west.
        let bearing = |call: &str| {
            got.iter()
                .find(|m| m.record.callsign == call)
                .unwrap()
                .record
                .bearing_deg
        };
        assert!(
            (bearing("W1AAA") - 90.0).abs() < 1.0,
            "{}",
            bearing("W1AAA")
        );
        assert!(
            (bearing("W1GGG") - 270.0).abs() < 1.0,
            "{}",
            bearing("W1GGG")
        );
        // A radius search has no route position.
        assert!(merge_nearby(&[&rows], ROUTE_START, 40.0)
            .iter()
            .all(|m| m.along_km.is_none()));
    }

    /// A route merges exactly as a radius search does: one machine is one row, programmed from its
    /// highest source (here the one farther from the line), every source kept.
    #[test]
    fn a_machine_on_a_route_is_one_row_from_its_best_source() {
        let route = up_the_meridian(40.0);
        let hh = RepeaterRecord {
            ctcss_enc_hz: Some(77.0),
            ..route_row("W1MCH", 147.18, on_route(80.0, 10.0))
        };
        let rb = RepeaterRecord {
            source: RepeaterSource::Repeaterbook,
            source_id: "42-1".into(),
            callsign: "W1MCH-R".into(),
            ctcss_enc_hz: Some(88.5),
            ..route_row("W1MCH", 147.18, on_route(80.0, 12.0))
        };
        let got = merge_route(&[&[], &[rb], &[hh]], &route);
        assert_eq!(got.len(), 1, "{got:?}");
        let ids: Vec<&str> = got[0]
            .sources
            .iter()
            .map(|s| s.channel_id.as_str())
            .collect();
        assert_eq!(ids, ["rb:42-1", "hh:W1MCH"]);
        assert_eq!(got[0].record.ctcss_enc_hz, Some(88.5));
        assert_eq!(got[0].disagreements[0].field, DisputedField::Tone);
    }

    /// A route that ends where it starts is the radius search around that place: the same machines
    /// in the same order (the real Manchester listings).
    #[test]
    fn a_route_to_where_it_starts_is_the_radius_search() {
        let coordinator = parse_rsgb_json(RSGB_FIXTURE).unwrap();
        let hearham = parse_hearham_json(HEARHAM_UK_FIXTURE).unwrap();
        let layers: [&[RepeaterRecord]; 3] = [&coordinator, &[], &hearham];
        let circle = Route {
            from: MANCHESTER,
            to: MANCHESTER,
            corridor_km: 100.0,
        };
        let lines = |ms: Vec<Machine>| ms.iter().map(golden_line).collect::<Vec<_>>();
        let around = lines(merge_nearby(&layers, MANCHESTER, 100.0));
        assert_eq!(around.len(), 12);
        assert_eq!(lines(merge_route(&layers, &circle)), around);
    }

    /// THE ROUTE GOLDEN: Liverpool to Leeds with the 25 mi corridor, from the real Manchester
    /// listings. The order and each machine's place were worked out before the route merge first
    /// ran, by a separate program that finds each machine's nearest point of the line by sampling
    /// it every 5 m (none of this module's geometry), and every machine is the radius search's
    /// own merged line, but for one thing the route changes: a machine's rows from one directory are
    /// listed nearest the line first, so GB3OA's hearham rows swap (hh:4092 is 24.57 km off the
    /// line, hh:692 24.73 km). GB3CR, south of Liverpool, is placed before the start; GB3BW, past
    /// Leeds, after the end; MB7ITW (Sheffield, 42.9 km off the line) and GB3XN (53.0 km) are
    /// outside.
    #[test]
    fn the_route_golden() {
        let coordinator = parse_rsgb_json(RSGB_FIXTURE).unwrap();
        let hearham = parse_hearham_json(HEARHAM_UK_FIXTURE).unwrap();
        let route = Route {
            from: LIVERPOOL,
            to: LEEDS,
            corridor_km: CORRIDOR_KM,
        };
        let got = merge_route(&[&coordinator, &[], &hearham], &route);
        let want = [
            ("GB3CR 433.15 in 434.75 110.9 FM cc- on [rsgb:744]", 0.000, 33.743),
            ("GB3OA 145.6125 in 145.0125 82.5 FM+YSF cc- on [rsgb:207 hh:4092 hh:692] Mode(Rsgb FM+YSF|Hearham DMR)", 7.512, 25.452),
            ("GB7SJ 433.175 in 434.775 103.5 FM cc- on [rsgb:2437 hh:4042] Mode(Rsgb FM|Hearham DMR)", 17.844, 30.554),
            ("MB6JB 431.1375 in 431.1375 - DMR cc- on [rsgb:6531 hh:18598] Mode(Rsgb DMR|Hearham FM)", 32.376, 13.784),
            ("GB3PP 433.375 in 434.975 82.5 FM+DMR+YSF cc10 on [rsgb:4356 hh:3959] ColorCode(Rsgb CC10|Hearham CC9)", 34.821, 24.423),
            ("GB7PR 434.05 in 434.05 - — cc- on [rsgb:5190]", 47.115, 32.478),
            ("GB3MN 145.65 in 145.05 82.5 FM+YSF cc- on [rsgb:2293 hh:14327]", 51.922, 36.034),
            ("MB7IGF 430.0125 in 430.0125 82.5 FM cc- on [rsgb:355 hh:15527]", 61.532, 14.955),
            ("GB3XL 430.8875 in 438.4875 82.5 FM+DMR cc1 off [rsgb:6863 hh:4017]", 89.931, 7.774),
            ("GB3BW 430.8125 in 438.4125 88.5 FM cc- on [rsgb:4808 hh:12141 hh:12268] Tone(Rsgb 88.5|Hearham 82.5) Input(Rsgb 438.4125|Hearham 438.4125|Hearham 430.8125)", 103.834, 24.635),
        ];
        let lines: Vec<String> = got.iter().map(golden_line).collect();
        assert_eq!(lines, want.map(|(line, _, _)| line));
        for (m, (_, along, off)) in got.iter().zip(want) {
            let (a, d) = (m.along_km.unwrap(), m.record.distance_km);
            assert!(
                (a - along).abs() < 0.05 && (d - off).abs() < 0.05,
                "{}: {a} {d}",
                m.record.callsign
            );
        }
    }

    #[test]
    fn a_route_asks_repeaterbook_about_every_state_its_corridor_crosses_in_route_order() {
        // The states come from the 4-character locator square a point is in ([`state_for_grid`]:
        // the square's main state), so a route starts with the state its first square names:
        // Chicago's EN61 is Indiana's, Rockford's EN52 Wisconsin's.
        let start = |p: (f64, f64)| plan_states(p, CORRIDOR_KM)[0].clone();
        // Rockford IL to Madison WI: both states, nothing left out.
        let mut short = plan_route_states(&Route {
            from: (42.27, -89.09),
            to: (43.07, -89.40),
            corridor_km: CORRIDOR_KM,
        });
        assert!(short.beyond.is_empty(), "{short:?}");
        short.ask.sort();
        assert_eq!(short.ask, ["17", "55"]);
        // Chicago to Denver: from its start, through Illinois, Iowa and Nebraska to Colorado, in
        // that order.
        let chicago = (41.88, -87.63);
        let west = plan_route_states(&Route {
            from: chicago,
            to: (39.74, -104.99),
            corridor_km: CORRIDOR_KM,
        });
        let at = |id: &str| {
            west.ask
                .iter()
                .position(|s| s == id)
                .unwrap_or_else(|| panic!("{id} not asked: {west:?}"))
        };
        assert_eq!(west.ask[0], start(chicago), "{west:?}");
        assert!(
            at("17") < at("19") && at("19") < at("31") && at("31") < at("08"),
            "{west:?}"
        );
        assert!(west.beyond.is_empty(), "{west:?}");
        // Coast to coast crosses more than nine: the nine the route reaches first are asked
        // about, the rest named, each state once.
        let new_york = (40.71, -74.01);
        let across = plan_route_states(&Route {
            from: new_york,
            to: (34.05, -118.24),
            corridor_km: CORRIDOR_KM,
        });
        assert_eq!(across.ask.len(), RB_STATES_PER_SEARCH, "{across:?}");
        assert_eq!(across.ask[0], start(new_york), "{across:?}");
        assert_eq!(
            across.beyond.last().map(String::as_str),
            Some("CA"),
            "{across:?}"
        );
        let mut every: Vec<&str> = across
            .ask
            .iter()
            .map(|id| state_code_for_id(id).unwrap())
            .chain(across.beyond.iter().map(String::as_str))
            .collect();
        let crossed = every.len();
        every.sort_unstable();
        every.dedup();
        assert_eq!(every.len(), crossed, "{across:?}");
        // Outside the US, none; a route that ends where it starts plans the radius search's.
        let europe = Route {
            from: (48.86, 2.35),
            to: (50.85, 4.35),
            corridor_km: CORRIDOR_KM,
        };
        assert_eq!(plan_route_states(&europe), StatePlan::default());
        let circle = Route {
            from: EN52,
            to: EN52,
            corridor_km: 60.0,
        };
        assert_eq!(plan_route_states(&circle).ask, plan_states(EN52, 60.0));
    }

    #[test]
    fn a_uk_route_asks_rsgb_about_the_squares_along_it_and_names_the_rest() {
        // Liverpool to Leeds: Liverpool's IO83 first, Leeds's IO93 on the way.
        let m62 = plan_rsgb_route(&Route {
            from: LIVERPOOL,
            to: LEEDS,
            corridor_km: CORRIDOR_KM,
        });
        assert_eq!(m62.ask.first().map(String::as_str), Some("IO83"), "{m62:?}");
        assert!(m62.ask.contains(&"IO93".to_string()), "{m62:?}");
        assert!(m62.beyond.is_empty(), "{m62:?}");
        // London to Edinburgh reaches more than nine: those it reaches first are asked about, and
        // the rest named, up to IO86 across the Forth (5 km north of Edinburgh).
        let north = plan_rsgb_route(&Route {
            from: (51.51, -0.13),
            to: (55.95, -3.19),
            corridor_km: CORRIDOR_KM,
        });
        assert_eq!(north.ask.len(), RSGB_SQUARES_PER_SEARCH, "{north:?}");
        assert_eq!(north.ask[0], "IO91", "{north:?}");
        assert_eq!(
            north.beyond.last().map(String::as_str),
            Some("IO86"),
            "{north:?}"
        );
        // Outside the UK, none: Dunkirk to Brussels passes 26 km from the UK square JO01, but as
        // on a radius search a search from outside the UK asks RSGB nothing.
        for (from, to) in [
            ((51.03, 2.38), (50.85, 4.35)),
            ((41.88, -87.63), (39.74, -104.99)),
        ] {
            let route = Route {
                from,
                to,
                corridor_km: CORRIDOR_KM,
            };
            assert_eq!(plan_rsgb_route(&route), RsgbPlan::default(), "{from:?}");
        }
    }

    // ── the map: hearham's rows alone ────────────────────────────────────────

    /// A machine is mapped from its hearham row (the operator, 2026-09-30: hearham invites map
    /// use; RepeaterBook's terms forbid a map; a coordinator's list is not mapped for now): the
    /// position, callsign, output and town hearham gives, never the merged record's, which here
    /// are RepeaterBook's or the coordinator's.
    #[test]
    fn a_machine_is_mapped_from_its_hearham_row_alone() {
        let rb = RepeaterRecord {
            city: "Hartford, Talcott Mountain".into(),
            ..row(RepeaterSource::Repeaterbook, "77", "W1ABC")
        };
        let hh = RepeaterRecord {
            lat: 41.80,
            lon: -72.80,
            output_mhz: 146.9415,
            city: "Avon".into(),
            ..row(RepeaterSource::Hearham, "5", "W1ABC-R")
        };
        // hearham's second listing of it, farther out: the map takes hearham's nearest.
        let far = RepeaterRecord {
            lat: 42.0,
            lon: -72.6,
            city: "Somers".into(),
            ..row(RepeaterSource::Hearham, "6", "W1ABC")
        };
        let ms = merged(&[rb.clone(), far, hh.clone()]);
        assert_eq!(ms.len(), 1, "{ms:?}");
        assert_eq!(ms[0].sources.len(), 3, "{ms:?}");
        let m = &ms[0];
        // The list places and programs it from RepeaterBook's row, as before...
        assert_eq!(
            (m.record.lat, m.record.city.as_str()),
            (41.73, "Hartford, Talcott Mountain")
        );
        // ...and the map from hearham's.
        assert_eq!(
            m.map,
            Some(MapPoint {
                lat: 41.80,
                lon: -72.80,
                callsign: "W1ABC-R".into(),
                output_mhz: 146.9415,
                city: "Avon".into(),
            })
        );
        // The same beside the coordinator's row.
        let rsgb = RepeaterRecord {
            source: RepeaterSource::Rsgb,
            ..rb
        };
        let m = &merged(&[rsgb, hh])[0];
        assert_eq!(m.record.source, RepeaterSource::Rsgb);
        assert_eq!(
            m.map.as_ref().map(|p| (p.lat, p.lon, p.city.as_str())),
            Some((41.80, -72.80, "Avon"))
        );
        // In the JSON Program reads.
        let json = serde_json::to_value(m).unwrap();
        assert_eq!(json["map"]["outputMhz"], 146.9415, "{json}");
        assert_eq!(json["map"]["callsign"], "W1ABC-R", "{json}");
    }

    /// A machine no hearham row lists is left off the map: RepeaterBook's alone never goes on one,
    /// the coordinator's alone does not for now, and the two together are still neither hearham's.
    #[test]
    fn a_machine_no_hearham_row_lists_is_left_off_the_map() {
        let ms = merged(&[
            row(RepeaterSource::Repeaterbook, "77", "W1ABC"),
            row(RepeaterSource::Rsgb, "8", "W1ABC"),
            row(RepeaterSource::Rsgb, "9", "W1XYZ"),
            row(RepeaterSource::Hearham, "5", "W1HHH"),
        ]);
        assert_eq!(ms.len(), 3, "{ms:?}");
        let mapped = |call: &str| {
            ms.iter()
                .find(|m| m.record.callsign == call)
                .unwrap()
                .map
                .is_some()
        };
        assert!(!mapped("W1ABC"), "RepeaterBook's and the coordinator's");
        assert!(!mapped("W1XYZ"), "the coordinator's alone");
        assert!(mapped("W1HHH"), "hearham's");
        let json = serde_json::to_value(ms.iter().find(|m| m.record.callsign == "W1ABC")).unwrap();
        assert!(json.get("map").is_none(), "{json}");
    }

    /// On the real Manchester listings, around a place and along a route alike: exactly the
    /// machines with a hearham row are mapped, each where that row (hearham's nearest listing of
    /// the machine) places it, so GB3CR and GB7PR, the coordinator's alone, are not.
    #[test]
    fn the_uk_merges_map_hearhams_listings_only() {
        let coordinator = parse_rsgb_json(RSGB_FIXTURE).unwrap();
        let hearham = parse_hearham_json(HEARHAM_UK_FIXTURE).unwrap();
        let layers: [&[RepeaterRecord]; 3] = [&coordinator, &[], &hearham];
        let around = merge_nearby(&layers, MANCHESTER, 100.0);
        let along = merge_route(
            &layers,
            &Route {
                from: LIVERPOOL,
                to: LEEDS,
                corridor_km: CORRIDOR_KM,
            },
        );
        for ms in [&around, &along] {
            let mut unmapped = Vec::new();
            for m in ms.iter() {
                let first = m
                    .sources
                    .iter()
                    .find(|s| s.source == RepeaterSource::Hearham);
                match (first, &m.map) {
                    (Some(s), Some(p)) => {
                        let h = hearham.iter().find(|r| r.source_id == s.source_id).unwrap();
                        let want = MapPoint {
                            lat: h.lat,
                            lon: h.lon,
                            callsign: h.callsign.clone(),
                            output_mhz: h.output_mhz,
                            city: h.city.clone(),
                        };
                        assert_eq!(p, &want, "{}", m.record.callsign);
                    }
                    (None, None) => unmapped.push(m.record.callsign.as_str()),
                    (first, map) => panic!("{}: {first:?} {map:?}", m.record.callsign),
                }
            }
            unmapped.sort_unstable();
            assert_eq!(unmapped, ["GB3CR", "GB7PR"]);
        }
    }
}
