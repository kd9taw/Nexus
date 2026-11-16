//! The SSB DATA submodes a radio's CAT link has shown it does not take, judged by what happened on
//! the wire, for the connection in use.
//!
//! # Why
//!
//! Nexus commands a DATA submode (`PKTUSB`/`PKTLSB`) for every sound-card mode, and sends it to
//! every radio. Through Hamlib's SDR Console backend (2056, a TS-2000-family one) that word goes
//! out as `MDD;`, which is not a TS-2000 mode: SDR Console raised an error window at every FT8
//! key-down, because the mode set goes out on the slot boundary just before `TX;`. Hamlib reports
//! that set as taken, and SDR Console stays in USB.
//!
//! # Judged by the wire, not by a table or a caps list
//!
//! Hamlib's printed mode list cannot decide this. It under-reports, and the real answer is often
//! settled only at open, from the radio's own identity: a TS-2000-family backend whose radio
//! answers `ID900`/`ID904`–`ID909` (PowerSDR, DDUtil, SmartSDR emulations) takes `PKTUSB` as
//! DIGU, and so do piHPSDR and the QCX/QDX; Icom takes it as USB-D where the radio has a data
//! flag; the FT-1000MP takes `PKTLSB` as DATA-LSB. None of those lists name a PKT mode.
//!
//! So the radio loop (`tempo-audio`, `RadioLoop::judge_data_word`) judges the DATA word it
//! ALREADY sends, while receiving, and records a word here only when the link has shown it does
//! not take it: the loop's mode ladder gave up on it after an explicit refusal, or Hamlib answered
//! the set and the radio then read back as the plain twin (`USB` for `PKTUSB`) on two receive-time
//! reads in a row. A recorded word becomes its plain sideband (`Settings::plain_ssb_if_configured`,
//! the same path as the operator's "Data modes use plain SSB" switch). Nothing is sent for the
//! judgement itself, and nothing is sent sooner or later: only which word goes out next changes.
//!
//! # Runtime only, per connection
//!
//! It lives in the engine's own `Settings` (`Settings::link_refusals`), so every copy of the
//! settings agrees about one link, and it is never saved, never compared, and never sent to the UI.
//! It names the model it was judged on, so a model change or another radio's settings never see it,
//! and the loop forgets it at every reconnect and radio handoff, so each connection is judged
//! afresh. Losing it (settings replaced from somewhere that does not carry it over) only means the
//! link is judged again. Nothing recorded means everything is commanded as before.
//!
//! # The one table here, and why it is not a judgement
//!
//! [`NO_DATA_SUBMODE_MODELS`] names the radios that have no DATA submode at all (a TS-480, an
//! IC-706). It decides nothing on the wire: it is only where the operator's own "Data modes use
//! plain SSB" switch STARTS for a radio that has never had it saved (`Settings` applies it to a
//! radio's first model and to a settings file from before the switch). A saved choice always
//! stands, and the judgement above still runs for every link. Only radios on which two sources
//! agree are in it, and the emulated models the paragraph above warns about are left out.
//! [`OMNIRIG_NO_DATA_SUBMODE_RIG_TYPES`] is the same radios' OmniRig rig files, which the OmniRig
//! shim reads for a different reason: those files send a DATA word as the radio's FSK mode.

/// The one spelling of an SSB DATA submode, or `None` for any other mode: only these two are ever
/// judged (`PKTFM` and the rest are commanded as before).
pub fn data_word(mode: &str) -> Option<&'static str> {
    match mode.trim().to_ascii_uppercase().as_str() {
        "PKTUSB" | "USB-D" | "DATA-U" | "PKT-U" => Some("PKTUSB"),
        "PKTLSB" | "LSB-D" | "DATA-L" | "PKT-L" => Some("PKTLSB"),
        _ => None,
    }
}

/// The plain sideband a DATA word's radio reads back as when it did not take it.
pub fn plain_twin(mode: &str) -> Option<&'static str> {
    match data_word(mode)? {
        "PKTUSB" => Some("USB"),
        _ => Some("LSB"),
    }
}

/// The Hamlib models of radios that have NO SSB DATA submode at all: their sound-card modes can
/// only go out in plain USB/LSB, through the rear accessory jack, so "Data modes use plain SSB"
/// starts ON for them ([`plain_ssb_by_default`]).
///
/// Each is in only on two agreeing sources: its Hamlib backend lists no `RIG_MODE_PKT*` mode at
/// all (so a `PKTUSB` set is refused, and the radio loop's mode ladder ends on plain USB anyway,
/// after up to 30 refused sets), and OmniRig's stock rig file for it has no real data mode to map
/// its `pmDIG_*` to (it maps them to the radio's FSK/RTTY mode, or to nothing). Read off Hamlib
/// `rigs/kenwood/ts480.c`, `ts570.c`, `ts690.c`, `ts870s.c` and `rigs/icom/ic706.c`, `ic703.c`,
/// `ic718.c`, `ic7000.c` (model numbers from `include/hamlib/riglist.h`).
///
/// ⚠️ LEFT OUT ON PURPOSE, and each one matters:
/// - TS-2000 (2014): its number is what the PowerSDR, SmartSDR and DDUtil emulations are run as,
///   and Hamlib turns `PKTUSB` into their DIGU there (`kenwood.c`, `is_emulation`).
/// - FT-817/818/857/897: their DIG mode is a data mode, and Hamlib sends `PKTUSB` as it.
/// - FT-450/450D: USER-U is their data mode (Hamlib `ft450.h`: "USER-U == RIG_MODE_PKTUSB").
/// - TS-590S/SG and FT-991/991A: real DATA modes (`DA1`, DATA-U).
pub const NO_DATA_SUBMODE_MODELS: &[u32] = &[
    2004, // Kenwood TS-570D: AM CW SSB FM RTTY only (ts570.c TS570_ALL_MODES)
    2005, // Kenwood TS-690S (ts690.c TS690_ALL_MODES)
    2010, // Kenwood TS-870S (ts870s.c TS870S_ALL_MODES)
    2016, // Kenwood TS-570S (ts570.c TS570_ALL_MODES)
    2028, // Kenwood TS-480 SAT/HX (ts480.c TS480_ALL_MODES)
    3009, // Icom IC-706 (ic706.c IC706_ALL_RX_MODES)
    3010, // Icom IC-706MKII (ic706.c)
    3011, // Icom IC-706MKIIG (ic706.c)
    3013, // Icom IC-718 (ic718.c IC718_ALL_RX_MODES)
    3055, // Icom IC-703 (ic703.c IC703_ALL_RX_MODES)
    3060, // Icom IC-7000 (ic7000.c IC7000_ALL_RX_MODES)
];

/// Does "Data modes use plain SSB" start ON for a radio of this Hamlib model? Only for a radio
/// with no DATA submode ([`NO_DATA_SUBMODE_MODELS`]); `false` for model 0 (none chosen yet).
pub fn plain_ssb_by_default(rig_model: u32) -> bool {
    NO_DATA_SUBMODE_MODELS.contains(&rig_model)
}

/// OmniRig's stock rig files (its `RigType`, the file name without `.ini`) for the radios of
/// [`NO_DATA_SUBMODE_MODELS`]. Each maps `pmDIG_U`/`pmDIG_L` to the radio's FSK or RTTY mode, or
/// to nothing ("not supported"): the TS-480, TS-690 and TS-870 files send `MD9;` (FSK-R) and
/// `MD6;` (FSK), the Icom files CI-V mode `04`/`08` (RTTY/RTTY-R). So a DATA word sent as
/// `pmDIG_*` puts the radio in FSK, where sound-card audio radiates nothing, which is why the
/// OmniRig shim sends plain SSB for a DATA word on these (`tempo_audio::omnirig`).
///
/// Only the stock names: a rig file the operator renamed or wrote is theirs, and is left alone.
/// OmniRig's TS-2000 file is out for the TS-2000's reason above: an SDR's emulation can sit
/// behind it, and FSK-R is its DIGU.
pub const OMNIRIG_NO_DATA_SUBMODE_RIG_TYPES: &[&str] = &[
    "TS-480",
    "TS-690",
    "TS-870",
    "IC-706",
    "IC-706 MKII",
    "IC-706 MKIIG",
    "IC-703",
    "IC-703a",
    "IC-718",
    "IC-7000",
    "IC-7000v2",
];

/// Is this OmniRig `RigType` one of [`OMNIRIG_NO_DATA_SUBMODE_RIG_TYPES`]? Exact, after trimming.
pub fn omnirig_rig_type_has_no_data_submode(rig_type: &str) -> bool {
    OMNIRIG_NO_DATA_SUBMODE_RIG_TYPES.contains(&rig_type.trim())
}

/// The DATA words one radio model's current link has shown it does not take.
///
/// ⚠️ `PartialEq` IS ALWAYS EQUAL, ON PURPOSE. This is not configuration: two `Settings` that
/// differ only here are the same settings, so a judgement can never mark them changed, dirty, or
/// in need of a rebuild.
#[derive(Debug, Clone, Default)]
pub struct LinkRefusals {
    model: u32,
    words: Vec<&'static str>,
}

impl PartialEq for LinkRefusals {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl LinkRefusals {
    /// Record that `model`'s link does not take the DATA word `mode`. `true` when that is news.
    /// Ignored for any other mode. A different model starts a new record.
    pub fn refuse(&mut self, model: u32, mode: &str) -> bool {
        let Some(word) = data_word(mode) else {
            return false;
        };
        if self.model != model {
            *self = Self {
                model,
                words: Vec::new(),
            };
        }
        if self.words.contains(&word) {
            return false;
        }
        self.words.push(word);
        true
    }

    /// Forget everything: a new connection is judged afresh. `true` when anything was recorded.
    pub fn forget(&mut self) -> bool {
        let had = !self.words.is_empty();
        *self = Self::default();
        had
    }

    /// Has `model`'s link shown it does not take `mode`? `false` for anything not recorded.
    pub fn refused(&self, model: u32, mode: &str) -> bool {
        self.model == model && data_word(mode).is_some_and(|word| self.words.contains(&word))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nothing_recorded_refuses_nothing() {
        let r = LinkRefusals::default();
        assert!(!r.refused(2056, "PKTUSB"));
        assert!(!r.refused(0, "PKTLSB"));
    }

    #[test]
    fn a_refused_word_is_that_word_on_that_model_only_until_forgotten() {
        let mut r = LinkRefusals::default();
        assert!(r.refuse(2056, "PKTUSB"));
        assert!(r.refused(2056, "PKTUSB") && r.refused(2056, "USB-D"));
        assert!(!r.refused(2056, "PKTLSB"), "each word is judged on its own");
        assert!(!r.refused(2014, "PKTUSB"), "another model never sees it");
        assert!(!r.refuse(2056, "pktusb"), "the same word again is no news");
        assert!(!r.refuse(2056, "PKTFM") && !r.refuse(2056, "CWR"));
        assert!(!r.refused(2056, "PKTFM") && !r.refused(2056, "CWR"));
        assert!(r.forget());
        assert!(
            !r.refused(2056, "PKTUSB"),
            "a new connection starts unjudged"
        );
        assert!(!r.forget(), "nothing left to forget");
        assert_eq!(LinkRefusals::default(), r, "never configuration");
    }

    #[test]
    fn the_plain_twin_is_the_sideband_underneath() {
        assert_eq!(plain_twin("PKTUSB"), Some("USB"));
        assert_eq!(plain_twin("pktlsb"), Some("LSB"));
        assert_eq!(plain_twin("PKTFM"), None);
        assert_eq!(plain_twin("USB"), None);
    }
}
