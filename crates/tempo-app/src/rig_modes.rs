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
