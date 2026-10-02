//! Memory-channel model for the radio-programming ("Program") section.
//!
//! A [`Channel`] is one radio memory slot: the repeater output you listen on,
//! the duplex/offset that derives your transmit frequency, and the tone that
//! opens the machine. The model is a superset: analog FM is fully supported in
//! v1; the DMR/D-STAR fields are persisted now so saved projects survive the
//! v2 digital work unchanged. Pure data + formatting only — fetching lives in
//! `crate::live`, orchestration in the Tauri shell. CHIRP CSV export is in
//! [`crate::chirp`] (schema knowledge isolated there).

use serde::{Deserialize, Serialize};

/// Repeater shift: how the TX frequency derives from the RX (output) frequency.
/// Serialized lowercase to match CHIRP's `Duplex` column vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Duplex {
    /// TX = RX (simplex, or talk-around).
    Simplex,
    /// TX = RX + offset.
    Plus,
    /// TX = RX − offset.
    Minus,
    /// Non-standard split: `offset_mhz` holds the ABSOLUTE TX frequency.
    Split,
}

/// Tone squelch mode, CHIRP `Tone` column vocabulary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToneMode {
    /// Carrier squelch — no tone.
    None,
    /// CTCSS encode only (TX a PL tone; RX open) — the common repeater case.
    Tone,
    /// CTCSS encode + decode (tone squelch both ways).
    TSql,
    /// DCS/DTCS digital code squelch: both ways, or on transmit only when
    /// [`Channel::dtcs_tx_only`] is set.
    Dtcs,
}

/// Channel operating mode. v1 exports FM/NFM; the digital modes are carried
/// for display + forward-compat (a DMR channel saved today programs in v2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChanMode {
    Fm,
    Nfm,
    Am,
    Dmr,
    Dstar,
    Fusion,
}

impl ChanMode {
    /// Programmable in the v1 analog path?
    pub fn is_analog(self) -> bool {
        matches!(self, ChanMode::Fm | ChanMode::Nfm | ChanMode::Am)
    }
}

/// Where a channel came from (provenance for refresh/dedupe and attribution).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelSource {
    /// "rsgb" | "repeaterbook" | "hearham".
    pub source: String,
    /// The source's repeater id.
    pub source_id: String,
    pub callsign: String,
}

/// One memory channel. Crosses the Tauri boundary and persists in
/// `radioprog.json` as-is — every field has a default so old saves load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Channel {
    /// Stable row id (rename/delete/reorder target in the UI).
    pub id: String,
    /// Operator label. NEVER truncated in the model — per-radio caps apply at
    /// export/preview time only (see [`sanitize_name`]).
    pub name: String,
    /// RX frequency in MHz — the repeater OUTPUT (what you listen to).
    pub rx_mhz: f64,
    pub duplex: Duplex,
    /// Offset magnitude in MHz; when `duplex == Split` this is the absolute TX
    /// frequency instead.
    pub offset_mhz: f64,
    pub tone_mode: ToneMode,
    /// CTCSS encode (TX tone / "PL"), Hz.
    pub rtone_hz: f32,
    /// CTCSS decode (RX tone squelch / "TSQ"), Hz.
    pub ctone_hz: f32,
    /// DCS code (when `tone_mode == Dtcs`).
    pub dtcs_code: u16,
    /// With `tone_mode == Dtcs`: the code goes out on transmit only and the receiver stays
    /// open (CHIRP's Cross mode "DTCS->"), the DCS counterpart of [`ToneMode::Tone`], for a
    /// machine whose source gives its code on the uplink alone. A FIELD rather than another
    /// tone mode on purpose: a build that predates it ignores it (reading DCS both ways),
    /// where an unknown tone mode would fail the whole `radioprog.json` parse, and that
    /// loader falls back to an empty file.
    pub dtcs_tx_only: bool,
    pub mode: ChanMode,
    pub comment: String,
    /// How the machine links beyond its own coverage, as its directory gives it ("IRLP 3570",
    /// "AllStar 2462", "DMR ID 314158"): tokens the exports write after the comment
    /// ([`Channel::export_comment`]). Apart from `comment` because Program's naming engine reads
    /// that as the town a callsign-less channel is named from. Not written when there are none, so
    /// a list without links saves exactly as before.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    // ── forward-compat (persisted now, exported in v2) ──
    pub dmr_color_code: Option<u8>,
    pub dmr_timeslot: Option<u8>,
    pub dmr_talkgroup: Option<u32>,
    pub dstar_rpt1: Option<String>,
    pub dstar_rpt2: Option<String>,
    pub source: Option<ChannelSource>,
}

impl Default for Channel {
    fn default() -> Self {
        Channel {
            id: String::new(),
            name: String::new(),
            rx_mhz: 0.0,
            duplex: Duplex::Simplex,
            offset_mhz: 0.0,
            tone_mode: ToneMode::None,
            rtone_hz: 88.5,
            ctone_hz: 88.5,
            dtcs_code: 23,
            dtcs_tx_only: false,
            mode: ChanMode::Fm,
            comment: String::new(),
            links: Vec::new(),
            dmr_color_code: None,
            dmr_timeslot: None,
            dmr_talkgroup: None,
            dstar_rpt1: None,
            dstar_rpt2: None,
            source: None,
        }
    }
}

impl Channel {
    /// The transmit frequency this channel keys up on (MHz).
    pub fn tx_mhz(&self) -> f64 {
        match self.duplex {
            Duplex::Simplex => self.rx_mhz,
            Duplex::Plus => self.rx_mhz + self.offset_mhz,
            Duplex::Minus => self.rx_mhz - self.offset_mhz,
            Duplex::Split => self.offset_mhz,
        }
    }

    /// The comment both exports write: the channel's own, then its links and its DMR colour code,
    /// the parts there are joined with "; " ("Seattle; IRLP 3570; AllStar 2462; CC1"). Neither
    /// file has a column for those two, and a radio's DMR side is set up from them by hand.
    pub fn export_comment(&self) -> String {
        let colour = self.dmr_color_code.map(|cc| format!("CC{cc}"));
        std::iter::once(self.comment.clone())
            .chain(self.links.iter().cloned())
            .chain(colour)
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>()
            .join("; ")
    }
}

/// Drop channels that program the SAME thing: identical RX (to the Hz), duplex,
/// offset and tone. Keeps the first occurrence (list order = operator's order).
/// Real feeds carry duplicates (hearham lists one machine per linked node).
pub fn dedupe(channels: &[Channel]) -> Vec<Channel> {
    let mut seen: Vec<(i64, Duplex, i64, ToneMode, u32)> = Vec::new();
    let mut out = Vec::new();
    for c in channels {
        let key = (
            (c.rx_mhz * 1e6).round() as i64,
            c.duplex,
            (c.offset_mhz * 1e6).round() as i64,
            c.tone_mode,
            (c.rtone_hz * 10.0).round() as u32,
        );
        if !seen.contains(&key) {
            seen.push(key);
            out.push(c.clone());
        }
    }
    out
}

/// Clamp a channel name to a radio's display cap: uppercase, strip characters
/// radios can't show (keep A–Z 0–9 space `/` `-`), squeeze doubled spaces, cut
/// at `max_len`. Used by the export writers and the UI's live preview — the
/// stored name is never modified.
pub fn sanitize_name(name: &str, max_len: usize) -> String {
    let mut cleaned = String::with_capacity(name.len());
    let mut last_space = false;
    for ch in name.trim().chars() {
        let up = ch.to_ascii_uppercase();
        let ok = up.is_ascii_alphanumeric() || up == ' ' || up == '/' || up == '-';
        if !ok {
            continue;
        }
        if up == ' ' {
            if last_space {
                continue;
            }
            last_space = true;
        } else {
            last_space = false;
        }
        cleaned.push(up);
    }
    cleaned.trim().chars().take(max_len).collect()
}

/// Generic CSV export — a plain spreadsheet-friendly dump (Anytone CPS / RT
/// Systems users copy columns from it; it is NOT the CHIRP format, see
/// [`crate::chirp`]). `attribution` becomes trailing comment lines ("" = none), one per line
/// of it ([`attribution_lines`]). `Comment` is [`Channel::export_comment`], as in CHIRP's file.
pub fn to_generic_csv(channels: &[Channel], attribution: &str) -> String {
    let mut out = String::from(
        "Channel,Name,RX Frequency (MHz),TX Frequency (MHz),Duplex,Offset (MHz),\
         Tone Mode,CTCSS Encode (Hz),CTCSS Decode (Hz),DCS,Mode,Comment\n",
    );
    for (i, c) in channels.iter().enumerate() {
        let duplex = match c.duplex {
            Duplex::Simplex => "",
            Duplex::Plus => "+",
            Duplex::Minus => "-",
            Duplex::Split => "split",
        };
        let tone = match c.tone_mode {
            ToneMode::None => "None",
            ToneMode::Tone => "Tone",
            ToneMode::TSql => "TSQL",
            // CHIRP's word for DCS on transmit only.
            ToneMode::Dtcs if c.dtcs_tx_only => "DTCS->",
            ToneMode::Dtcs => "DTCS",
        };
        let mode = match c.mode {
            ChanMode::Fm => "FM",
            ChanMode::Nfm => "NFM",
            ChanMode::Am => "AM",
            ChanMode::Dmr => "DMR",
            ChanMode::Dstar => "D-STAR",
            ChanMode::Fusion => "Fusion",
        };
        out.push_str(&format!(
            "{},{},{:.6},{:.6},{},{:.6},{},{:.1},{:.1},{:03},{},{}\n",
            i + 1,
            csv_field(&c.name),
            c.rx_mhz,
            c.tx_mhz(),
            duplex,
            c.offset_mhz,
            tone,
            c.rtone_hz,
            c.ctone_hz,
            c.dtcs_code,
            mode,
            csv_field(&c.export_comment()),
        ));
    }
    out.push_str(&attribution_lines(attribution));
    out
}

/// The attribution as comment lines, `# ` before each of its lines: the directories a list came
/// from each require their own ("Repeater data: RSGB ETCC (ukrepeater.net)" beside hearham's),
/// so it arrives as several lines, and a line written without its `#` would read as a channel.
/// Blank lines are dropped; "" is no attribution at all.
pub(crate) fn attribution_lines(attribution: &str) -> String {
    attribution
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(|line| format!("# {line}\n"))
        .collect()
}

/// Quote a CSV field only when it needs it.
pub(crate) fn csv_field(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chan(rx: f64, duplex: Duplex, off: f64) -> Channel {
        Channel {
            id: format!("{rx}"),
            name: "TEST".into(),
            rx_mhz: rx,
            duplex,
            offset_mhz: off,
            tone_mode: ToneMode::Tone,
            rtone_hz: 103.5,
            ..Channel::default()
        }
    }

    #[test]
    fn tx_mhz_by_duplex() {
        assert_eq!(chan(146.94, Duplex::Minus, 0.6).tx_mhz(), 146.34);
        assert_eq!(chan(147.255, Duplex::Plus, 0.6).tx_mhz(), 147.855);
        assert_eq!(chan(146.52, Duplex::Simplex, 0.0).tx_mhz(), 146.52);
        // Split: offset_mhz IS the TX frequency.
        assert_eq!(chan(145.0, Duplex::Split, 147.0).tx_mhz(), 147.0);
    }

    #[test]
    fn dedupe_keeps_first_drops_identical() {
        let a = chan(146.94, Duplex::Minus, 0.6);
        let mut b = a.clone();
        b.id = "other".into();
        let c = chan(147.255, Duplex::Plus, 0.6);
        let out = dedupe(&[a.clone(), b, c.clone()]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].id, a.id);
        assert_eq!(out[1].id, c.id);
    }

    #[test]
    fn dedupe_tone_differs_kept() {
        let a = chan(146.94, Duplex::Minus, 0.6);
        let mut b = a.clone();
        b.rtone_hz = 91.5; // different machine on the same pair elsewhere
        assert_eq!(dedupe(&[a, b]).len(), 2);
    }

    #[test]
    fn sanitize_name_rules() {
        assert_eq!(sanitize_name("w9abc", 7), "W9ABC");
        assert_eq!(sanitize_name("W9ABC Rockford", 7), "W9ABC R");
        assert_eq!(sanitize_name("WB9COW/R  HUB", 12), "WB9COW/R HUB");
        assert_eq!(sanitize_name("Café—Tower", 8), "CAFTOWER");
        assert_eq!(sanitize_name("  K9ESV  ", 7), "K9ESV");
    }

    #[test]
    fn generic_csv_shape() {
        let csv = to_generic_csv(
            &[chan(146.94, Duplex::Minus, 0.6)],
            "Data courtesy of RepeaterBook.com",
        );
        let mut lines = csv.lines();
        assert!(lines
            .next()
            .unwrap()
            .starts_with("Channel,Name,RX Frequency"));
        let row = lines.next().unwrap();
        assert!(row.starts_with("1,TEST,146.940000,146.340000,-,0.600000,Tone,103.5,"));
        assert_eq!(lines.next().unwrap(), "# Data courtesy of RepeaterBook.com");
    }

    #[test]
    fn generic_csv_writes_each_attribution_line_as_a_comment() {
        let csv = to_generic_csv(
            &[chan(146.94, Duplex::Minus, 0.6)],
            "Repeater data: RSGB ETCC (ukrepeater.net)\nRepeater data from hearham.com",
        );
        let tail: Vec<&str> = csv.lines().skip(2).collect();
        assert_eq!(
            tail,
            [
                "# Repeater data: RSGB ETCC (ukrepeater.net)",
                "# Repeater data from hearham.com"
            ]
        );
        assert_eq!(attribution_lines(""), "", "no attribution, no line");
        assert_eq!(attribution_lines(" \n\n"), "", "blank lines are dropped");
    }

    #[test]
    fn channel_serde_roundtrip_camel_case() {
        let c = chan(146.94, Duplex::Minus, 0.6);
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("\"rxMhz\":146.94"), "camelCase keys: {json}");
        assert!(json.contains("\"toneMode\":\"tone\""));
        let back: Channel = serde_json::from_str(&json).unwrap();
        assert_eq!(back, c);
        // Old/partial saves still load (every field defaulted).
        let sparse: Channel = serde_json::from_str(r#"{"name":"X","rxMhz":146.52}"#).unwrap();
        assert_eq!(sparse.rx_mhz, 146.52);
        assert_eq!(sparse.duplex, Duplex::Simplex);
    }

    /// A field this build does not know is ignored, not an error: that is what lets a later
    /// build add one (send-only DCS was added as a field, not a new tone mode) without an
    /// earlier build failing to read `radioprog.json`, whose loader falls back to an EMPTY
    /// file on any parse error and would then save over every project.
    #[test]
    fn a_channel_field_this_build_does_not_know_is_ignored() {
        let later: Channel = serde_json::from_str(
            r#"{"name":"X","rxMhz":146.94,"toneMode":"dtcs","dtcsCode":23,"aFieldFromLater":true}"#,
        )
        .expect("an unknown field must not fail the parse");
        assert_eq!((later.tone_mode, later.dtcs_code), (ToneMode::Dtcs, 23));

        // And send-only DCS survives the save the Remote's export reads back.
        let send_only = Channel {
            tone_mode: ToneMode::Dtcs,
            dtcs_tx_only: true,
            ..chan(146.94, Duplex::Minus, 0.6)
        };
        let json = serde_json::to_string(&send_only).unwrap();
        assert!(json.contains("\"dtcsTxOnly\":true"), "{json}");
        assert_eq!(serde_json::from_str::<Channel>(&json).unwrap(), send_only);
    }

    /// The exports' comment: the channel's own, then its links and its colour code, the parts
    /// there are joined with "; ". A channel with neither writes its comment unchanged.
    #[test]
    fn the_export_comment_adds_links_and_the_colour_code_after_the_comment() {
        let ch = |comment: &str, links: &[&str], cc: Option<u8>| Channel {
            comment: comment.into(),
            links: links.iter().map(|l| l.to_string()).collect(),
            dmr_color_code: cc,
            ..Channel::default()
        };
        assert_eq!(
            ch("Seattle", &["IRLP 3570", "AllStar 2462"], Some(1)).export_comment(),
            "Seattle; IRLP 3570; AllStar 2462; CC1"
        );
        assert_eq!(ch("", &["IRLP 3570"], None).export_comment(), "IRLP 3570");
        assert_eq!(ch("", &[], Some(2)).export_comment(), "CC2");
        assert_eq!(ch("Seattle", &[], None).export_comment(), "Seattle");
        assert_eq!(ch("", &[], None).export_comment(), "");
    }

    /// A channel without links saves exactly as before: no `links` key, and a file written before
    /// links existed reads with none.
    #[test]
    fn a_channel_without_links_saves_as_before() {
        let plain = serde_json::to_value(Channel::default()).unwrap();
        assert!(plain.get("links").is_none(), "{plain}");
        let old: Channel = serde_json::from_value(plain).unwrap();
        assert!(old.links.is_empty());
        let linked = Channel {
            links: vec!["IRLP 3570".into()],
            ..Channel::default()
        };
        let v = serde_json::to_value(&linked).unwrap();
        assert_eq!(v["links"], serde_json::json!(["IRLP 3570"]));
        assert_eq!(serde_json::from_value::<Channel>(v).unwrap(), linked);
    }
}
