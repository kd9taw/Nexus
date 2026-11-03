//! Diagnostic-log words about transmit: who holds the transmitter, and why the TX watchdog
//! tripped. LOG ONLY. Nothing here keys, unkeys, arms, disarms or refuses anything: every
//! function reads the engine and writes (at most) one line to the diagnostic log.
//!
//! An operator's log once could not answer the one question it was sent to answer: did anything
//! outside Nexus key the radio? It recorded arms, halts and each over as it was released, but no
//! PTT edge, no PTT method and no watchdog trip (the trip sites set the flag silently). The radio
//! loop logs the PTT edges and method (`tempo-audio`'s `service/ptt_diag.rs`) and asks
//! [`Engine::ptt_keyer`] who holds the transmitter at the key edge; the watchdog trips are logged
//! here, where the engine knows the reason.
//!
//! Each line is one per EVENT, never per tick: a trip disarms TX, so a trip site cannot fire twice
//! without an operator act in between, and [`Engine::note_tx_watchdog_trip`] says nothing while a
//! trip already stands.

use super::{Engine, TxOwner};

#[cfg(test)]
thread_local! {
    /// Every line [`Engine::note_tx_watchdog_trip`] wrote on this thread, for the tests: the
    /// diagnostic log itself is inert in a test (`tempo_core::applog` before `init`).
    static LINES: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// The watchdog lines written on this thread since the last call (tests only).
#[cfg(test)]
pub(super) fn take_watchdog_lines() -> Vec<String> {
    LINES.with(|l| std::mem::take(&mut *l.borrow_mut()))
}

impl Engine {
    /// Who holds the transmitter right now, in the diagnostic log's words: `None` when nothing in
    /// Nexus does ([`Engine::tx_owner`]). Read by the radio loop at a PTT key edge only.
    pub fn ptt_keyer(&self) -> Option<String> {
        Some(match self.tx_owner()? {
            TxOwner::Slot => format!("{} over", self.app.tier().label()),
            TxOwner::Tune => "Tune".to_string(),
            // The CAT share's client rides the same latch as the operator's held PTT.
            TxOwner::ManualPtt
                if self.broker_ptt && !self.manual_ptt && !self.pc_mic.over.active() =>
            {
                "a program on Nexus's CAT share".to_string()
            }
            TxOwner::ManualPtt => "manual PTT".to_string(),
            TxOwner::Mic => "the Remote microphone".to_string(),
            TxOwner::Voice => "the voice keyer".to_string(),
            TxOwner::Cw => "the CW keyer".to_string(),
            TxOwner::Rtty => "an RTTY over".to_string(),
            TxOwner::Psk => "a PSK over".to_string(),
            TxOwner::Sstv => "an SSTV image".to_string(),
            TxOwner::Jtty => "JTTY".to_string(),
            TxOwner::Tci => "a program on Nexus's TCI server".to_string(),
        })
    }

    /// Log a TX watchdog trip: `what` names the transmission it ended and why. Called at each trip
    /// site just BEFORE it sets `tx_watchdog`, so a trip that already stands writes nothing. Reads
    /// only: the trip itself is the caller's, unchanged.
    pub(super) fn note_tx_watchdog_trip(&self, what: &str) {
        if self.tx_watchdog {
            return;
        }
        let line = format!(
            "TX watchdog tripped: {what} (limit {} min since the last operator action) — \
             transmit disarmed",
            self.settings.tx_watchdog_min
        );
        #[cfg(test)]
        LINES.with(|l| l.borrow_mut().push(line.clone()));
        tempo_core::applog::warn("tx", &line);
    }
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::take_watchdog_lines;

    /// ⭐ A TRIP WAS SILENT. Every trip site set `tx_watchdog` and disarmed with no line, so a log
    /// read later could not tell "the watchdog ended it" from "the operator stopped it". One line
    /// per trip, naming what it ended, and nothing while a trip still stands.
    #[test]
    fn an_ft_watchdog_trip_writes_one_line_naming_the_over() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tier(Tier::Ft8);
        let mut s = e.settings().clone();
        s.tx_watchdog_min = 1;
        e.apply_settings(s);
        e.set_tx_enabled(true);
        e.set_mode("qso-run").unwrap();
        take_watchdog_lines();
        e.tx_watchdog_start = Some(now_unix_secs().saturating_sub(61));
        assert!(e.poll_tx(0).is_empty(), "control: the trip keys nothing");
        assert!(e.tx_watchdog && !e.tx_enabled(), "control: tripped, TX off");
        let lines = take_watchdog_lines();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("TX watchdog tripped"), "{lines:?}");
        assert!(lines[0].contains("FT8"), "{lines:?}");
        assert!(lines[0].contains("limit 1 min"), "{lines:?}");
        // A trip that stands: the next slot writes nothing more.
        assert!(e.poll_tx(2).is_empty());
        assert!(take_watchdog_lines().is_empty(), "one line per trip");
    }

    #[test]
    fn an_over_longer_than_the_limit_says_so() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tier(Tier::Fst4);
        let mut s = e.settings().clone();
        s.fst4_period_s = 1800;
        s.tx_watchdog_min = 6;
        e.apply_settings(s);
        e.set_tx_enabled(true);
        e.set_mode("qso-run").unwrap();
        take_watchdog_lines();
        assert!(e.poll_tx(0).is_empty(), "control: refused, not keyed");
        let lines = take_watchdog_lines();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(
            lines[0].contains("longer than the whole limit"),
            "{lines:?}"
        );
    }

    #[test]
    fn an_rtty_watchdog_trip_writes_one_line() {
        let mut e = Engine::new("K1ABC", "EN61", 0);
        e.set_license_class("general");
        e.set_operating_mode("rtty", false);
        e.set_frequency(14.080, "20m", "LSB");
        e.settings.tx_watchdog_min = 1;
        e.set_tx_enabled_by_operator(true);
        e.rtty_send_text("TEST").unwrap();
        take_watchdog_lines();
        e.tx_watchdog_start = Some(now_unix_secs().saturating_sub(61));
        assert!(
            e.poll_rtty_one().is_none(),
            "control: the trip keys nothing"
        );
        assert!(e.tx_watchdog && !e.tx_enabled(), "control: tripped, TX off");
        let lines = take_watchdog_lines();
        assert_eq!(lines.len(), 1, "{lines:?}");
        assert!(lines[0].contains("RTTY"), "{lines:?}");
    }

    /// THE CONTROL: an armed station inside its limit writes no watchdog line and keys exactly as
    /// before; the new words change nothing about whether an over goes out.
    #[test]
    fn no_trip_no_line_and_the_over_still_keys() {
        let mut e = Engine::new("W9XYZ", "EN37", 0);
        e.set_tier(Tier::Ft8);
        let mut s = e.settings().clone();
        s.tx_watchdog_min = 6;
        e.apply_settings(s);
        e.set_tx_enabled(true);
        e.set_mode("qso-run").unwrap();
        take_watchdog_lines();
        assert!(!e.poll_tx(0).is_empty(), "an over inside the limit keys");
        assert!(!e.tx_watchdog && e.tx_enabled());
        assert!(take_watchdog_lines().is_empty());
    }

    /// Who keyed, as the radio loop's PTT line names it: the engine's own arbiter, in words.
    #[test]
    fn the_keyer_is_named_from_the_owner_of_the_transmitter() {
        let mut e = Engine::new("K1ABC", "EN61", 0);
        e.set_license_class("extra");
        assert_eq!(e.ptt_keyer(), None, "nothing holds the transmitter");

        e.set_operating_mode("phone", false);
        e.set_frequency(14.250, "20m", "USB");
        e.set_tx_enabled(true);
        e.set_ptt(true);
        assert!(e.manual_ptt(), "premise: the operator's PTT is held");
        assert_eq!(e.ptt_keyer().as_deref(), Some("manual PTT"));
        e.set_ptt(false);
        assert_eq!(e.ptt_keyer(), None, "released");

        // A program on the CAT share keying through Nexus.
        assert!(e.broker_ptt(true), "premise: the share's key is granted");
        assert_eq!(
            e.ptt_keyer().as_deref(),
            Some("a program on Nexus's CAT share")
        );
        e.broker_ptt(false);

        e.app.set_transmitting(true);
        e.set_tier(Tier::Ft8);
        assert_eq!(e.ptt_keyer().as_deref(), Some("FT8 over"));
        e.app.set_transmitting(false);

        e.tuning = true;
        assert_eq!(e.ptt_keyer().as_deref(), Some("Tune"));
    }
}
