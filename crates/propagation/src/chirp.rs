//! CHIRP generic-CSV writer — the universal memory-channel interchange format
//! (CHIRP imports it and flashes ~1,000 radio models). All CHIRP schema
//! knowledge lives HERE so format drift is a one-file fix.
//!
//! Schema (chirpmyradio.com CSV_HowTo, verified 2026-07): header row required;
//! `Location` must be the FIRST column and starts at 1; `Duplex` ∈
//! {'', '+', '-', 'split'}; `Tone` ∈ {'', 'Tone', 'TSQL', 'DTCS', 'Cross'};
//! frequencies with 6 decimals. CHIRP itself clamps names/fields a given radio
//! can't hold when the operator copies rows into a radio image — so this CSV is
//! safe for every model. A `# comment` attribution line is appended after the
//! rows; CHIRP ignores lines it can't parse.

use crate::memchan::{csv_field, sanitize_name, ChanMode, Channel, Duplex, ToneMode};

/// The exact CHIRP generic-CSV header.
pub const CHIRP_HEADER: &str = "Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,DtcsPolarity,Mode,TStep,Skip,Comment,URCALL,RPT1CALL,RPT2CALL,DVCODE";

/// Render channels as a CHIRP generic CSV. Only analog channels are written —
/// v1 programs FM; digital rows are the UI's responsibility to exclude (this
/// filter is a safety net so a digital channel can never corrupt an import).
/// `name_cap` = the per-radio display limit chosen in the UI (CHIRP would clamp
/// at copy time anyway; capping here makes the file match the preview exactly).
/// `attribution` ("" = none) becomes a trailing comment line.
pub fn to_chirp_csv(channels: &[Channel], name_cap: usize, attribution: &str) -> String {
    let mut out = String::from(CHIRP_HEADER);
    out.push('\n');
    let mut loc = 1usize;
    for c in channels.iter().filter(|c| c.mode.is_analog()) {
        let duplex = match c.duplex {
            Duplex::Simplex => "",
            Duplex::Plus => "+",
            Duplex::Minus => "-",
            Duplex::Split => "split",
        };
        let tone = match c.tone_mode {
            ToneMode::None => "",
            ToneMode::Tone => "Tone",
            ToneMode::TSql => "TSQL",
            ToneMode::Dtcs => "DTCS",
        };
        let mode = match c.mode {
            ChanMode::Nfm => "NFM",
            ChanMode::Am => "AM",
            _ => "FM",
        };
        // In CHIRP's model `Offset` is the split TX frequency when Duplex=split,
        // the offset magnitude otherwise — identical to our Channel semantics.
        out.push_str(&format!(
            "{},{},{:.6},{},{:.6},{},{:.1},{:.1},{:03},NN,{},5.00,,{},,,,\n",
            loc,
            csv_field(&sanitize_name(&c.name, name_cap)),
            c.rx_mhz,
            duplex,
            c.offset_mhz,
            tone,
            c.rtone_hz,
            c.ctone_hz,
            c.dtcs_code,
            mode,
            csv_field(&c.comment),
        ));
        loc += 1;
    }
    if !attribution.is_empty() {
        out.push_str(&format!("# {attribution}\n"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memchan::Channel;

    fn fm(name: &str, rx: f64, duplex: Duplex, off: f64, tone: ToneMode, pl: f32) -> Channel {
        Channel {
            id: name.into(),
            name: name.into(),
            rx_mhz: rx,
            duplex,
            offset_mhz: off,
            tone_mode: tone,
            rtone_hz: pl,
            ..Channel::default()
        }
    }

    /// Golden-file shape test — one row of every duplex/tone flavor, frozen.
    /// (The full file was verified by importing into CHIRP-next by hand; if
    /// this test needs updating, re-verify an import before freezing again.)
    #[test]
    fn chirp_csv_golden() {
        let chans = vec![
            fm("W9ABC", 146.94, Duplex::Minus, 0.6, ToneMode::Tone, 103.5),
            fm("K9XYZ", 147.255, Duplex::Plus, 0.6, ToneMode::TSql, 91.5),
            fm("SIMPLX", 146.52, Duplex::Simplex, 0.0, ToneMode::None, 88.5),
            fm(
                "ODDSPL",
                145.11,
                Duplex::Split,
                147.885,
                ToneMode::Tone,
                114.8,
            ),
        ];
        let csv = to_chirp_csv(&chans, 7, "Data courtesy of RepeaterBook.com");
        let expect = "\
Location,Name,Frequency,Duplex,Offset,Tone,rToneFreq,cToneFreq,DtcsCode,DtcsPolarity,Mode,TStep,Skip,Comment,URCALL,RPT1CALL,RPT2CALL,DVCODE
1,W9ABC,146.940000,-,0.600000,Tone,103.5,88.5,023,NN,FM,5.00,,,,,,
2,K9XYZ,147.255000,+,0.600000,TSQL,91.5,88.5,023,NN,FM,5.00,,,,,,
3,SIMPLX,146.520000,,0.000000,,88.5,88.5,023,NN,FM,5.00,,,,,,
4,ODDSPL,145.110000,split,147.885000,Tone,114.8,88.5,023,NN,FM,5.00,,,,,,
# Data courtesy of RepeaterBook.com
";
        assert_eq!(csv, expect);
    }

    #[test]
    fn digital_rows_never_exported() {
        let mut dmr = fm("N9DMR", 443.1, Duplex::Plus, 5.0, ToneMode::None, 88.5);
        dmr.mode = ChanMode::Dmr;
        let ok = fm("W9ABC", 146.94, Duplex::Minus, 0.6, ToneMode::Tone, 103.5);
        let csv = to_chirp_csv(&[dmr, ok], 7, "");
        // The DMR row is skipped AND Location renumbers from 1 contiguously.
        assert!(!csv.contains("N9DMR"));
        assert!(csv.contains("\n1,W9ABC,"));
    }

    #[test]
    fn name_cap_applied_at_export() {
        let long = fm(
            "W9ABC ROCKFORD WIDE",
            146.94,
            Duplex::Minus,
            0.6,
            ToneMode::Tone,
            103.5,
        );
        let csv = to_chirp_csv(&[long], 7, "");
        assert!(csv.contains("1,W9ABC R,146.940000"), "{csv}");
    }
}
