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
    /// DCS/DTCS digital code squelch.
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
    /// "repeaterbook" | "hearham".
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
    pub mode: ChanMode,
    pub comment: String,
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
            mode: ChanMode::Fm,
            comment: String::new(),
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
/// [`crate::chirp`]). `attribution` becomes a trailing comment line ("" = none).
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
            csv_field(&c.comment),
        ));
    }
    if !attribution.is_empty() {
        out.push_str(&format!("# {attribution}\n"));
    }
    out
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
}
