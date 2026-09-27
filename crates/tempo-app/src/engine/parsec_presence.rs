//! Parsec presence mode, the engine half: what a lost Parsec session STOPS, and how.
//!
//! The operator runs the shack remotely over Parsec, which gives Nexus no presence signal. A
//! watcher thread (`src-tauri/src/parsec_presence.rs`) reads Parsec's own host log OFF the radio
//! loop — [`crate::presence`] is that reading — and passes only its verdict in, once a second,
//! through [`Engine::apply_parsec_presence`]. This file decides what the verdict means for the
//! transmitter.
//!
//! ⛔ THE SIGN-OFF, AND ITS EXACT SCOPE (operator, 2026-09-27): "It only ever STOPS transmitting
//! on a missed heartbeat, and it's off unless you turn it on." So:
//!
//! - **Stop-only, through the EXISTING stop paths.** Nothing here keys, retunes, re-arms or
//!   changes timing. Each covered transmission ends exactly as its own Stop ends it — the path
//!   is named in [`PresenceStop`] and asserted by the tests below.
//! - **Covered: a latched Phone PTT, RTTY/PSK continuous TX, and Tune.** Those are the
//!   transmissions with no precomputed end, which is what a dropped link leaves keyed.
//! - **FT auto-sequencing is NOT covered.** An FT over ends by itself; FT QSO management sits
//!   behind its own gate, and the permit and the watchdog stay exactly as they are. So is
//!   everything else that ends by itself — a voice-keyer message, a CW macro, an SSTV image, an
//!   RTTY/PSK macro over. A foreign program's key through the CAT broker is that program's.
//! - **Off by default** ([`crate::settings::Settings::parsec_presence_stop`]), and with it off
//!   the machine is reset, so switching it on later starts from "unknown", never from a stale
//!   "connected".
//! - **No transition, no action.** Only the edge from a connected session to a lost one stops
//!   anything, and only a covered transmission that is up at that moment. An operator at the
//!   shack with no Parsec session is never touched.

use super::{now_unix_secs, Engine};
use crate::dto::ParsecPresenceDto;
use crate::presence::{Presence, PresenceMachine, Verdict};

/// A transmission presence mode ended, named by the EXISTING stop path it went through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresenceStop {
    /// Tune, through [`Engine::set_tune`]`(false)` — the Tune button's own off path, and the one
    /// the radio loop's `MAX_TUNE_MS` auto-release takes too.
    Tune,
    /// A latched mic key, through [`Engine::halt_tx`] — the Phone cockpit's Stop TX, whose job it
    /// already is to "drop any LATCHED key intent too (manual PTT-lock …)". It also lowers the TX
    /// enable, so the Phone PTT then reads "TX OFF — CLICK TO ENABLE" instead of "ON AIR" over an
    /// idle rig, and nothing re-keys until the operator is back and asks for it.
    Ptt,
    /// RTTY continuous TX, through [`Engine::rtty_stop`] — the RTTY cockpit's Esc/Stop.
    Rtty,
    /// PSK continuous TX, through [`Engine::psk_stop`] — the PSK cockpit's Esc/Stop.
    Psk,
}

impl PresenceStop {
    /// The token the snapshot carries; the UI owns the words.
    pub fn token(self) -> &'static str {
        match self {
            PresenceStop::Tune => "tune",
            PresenceStop::Ptt => "ptt",
            PresenceStop::Rtty => "rtty",
            PresenceStop::Psk => "psk",
        }
    }
}

/// Presence mode's state on the engine.
#[derive(Debug, Default)]
pub(crate) struct ParsecPresence {
    machine: PresenceMachine,
    /// The last verdict the watcher passed in, for the Settings readout. `None` until the first
    /// read after the mode was switched on.
    last: Option<Verdict>,
    /// The last stop this mode made — unix seconds and what it ended — until the operator
    /// transmits again.
    stopped: Option<(u64, Vec<PresenceStop>)>,
}

impl Engine {
    /// Take one verdict from the Parsec watcher — `None` while presence mode is off — and, on
    /// the edge where a connected session is lost, stop each covered transmission that is up
    /// through its own stop path. Returns what was stopped: empty on every call that is not
    /// such an edge, and on an edge with nothing covered keyed.
    ///
    /// Cheap and lock-friendly by design: the watcher does the file reading before it takes the
    /// engine lock, so this is a few comparisons, never I/O.
    pub fn apply_parsec_presence(&mut self, verdict: Option<Verdict>) -> Vec<PresenceStop> {
        // Off — by the watcher's word or by the setting itself, whichever is newer — forgets
        // everything, so a session seen before the switch went off can never make an edge after
        // it comes back on.
        let Some(verdict) = verdict.filter(|_| self.settings.parsec_presence_stop) else {
            self.parsec = ParsecPresence::default();
            return Vec::new();
        };
        self.parsec.last = Some(verdict);
        let before = self.parsec.machine.state();
        let lost = self.parsec.machine.observe(verdict);
        let after = self.parsec.machine.state();
        if after != before {
            // One line per change, never per poll: the timestamps are the bench's evidence of
            // how long Parsec took to notice.
            tempo_core::applog::info("parsec", &format!("presence {before:?} -> {after:?}"));
        }
        if !lost {
            // The report of the last stop stands until the operator transmits again.
            if self.parsec.stopped.is_some() && !self.presence_covered().is_empty() {
                self.parsec.stopped = None;
            }
            return Vec::new();
        }
        let stopped = self.presence_covered();
        for stop in &stopped {
            match stop {
                PresenceStop::Tune => self.set_tune(false),
                PresenceStop::Ptt => self.halt_tx(),
                PresenceStop::Rtty => self.rtty_stop(),
                PresenceStop::Psk => self.psk_stop(),
            }
        }
        if stopped.is_empty() {
            tempo_core::applog::info("parsec", "session lost; nothing latched to stop");
        } else {
            let what: Vec<&str> = stopped.iter().map(|s| s.token()).collect();
            tempo_core::applog::warn(
                "parsec",
                &format!("session lost; transmission stopped: {}", what.join(", ")),
            );
            self.parsec.stopped = Some((now_unix_secs(), stopped.clone()));
        }
        stopped
    }

    /// The covered transmissions up right now, in the order a drop stops them. The engine's own
    /// state, read directly rather than through [`Self::tx_owner`]: that answers "who holds the
    /// transmitter" with ONE owner, and this must see each covered one.
    ///
    /// - `tuning` — the raw hold, not the privilege-masked [`Self::tuning`]: a carrier masked
    ///   by a QSY into a locked segment keys again the moment the dial comes back.
    /// - `manual_ptt` — the operator's OWN latched key, raw for the same reason (re-arming TX
    ///   would re-key it). Not `broker_ptt`: that is a foreign program's key.
    /// - `rtty_streaming` / `psk_streaming` — the latch up, or what was typed still draining.
    fn presence_covered(&self) -> Vec<PresenceStop> {
        let mut up = Vec::new();
        if self.tuning {
            up.push(PresenceStop::Tune);
        }
        if self.manual_ptt {
            up.push(PresenceStop::Ptt);
        }
        if self.rtty_streaming() {
            up.push(PresenceStop::Rtty);
        }
        if self.psk_streaming() {
            up.push(PresenceStop::Psk);
        }
        up
    }

    /// Presence mode for the snapshot; `None` while it is off.
    pub fn parsec_presence_dto(&self) -> Option<ParsecPresenceDto> {
        if !self.settings.parsec_presence_stop {
            return None;
        }
        // The MACHINE's view, not the raw last read: during the one unreadable poll that is
        // retried, the session still counts — the next miss is what drops it — so "connected"
        // is the truth, and "unreadable" would promise nothing will be stopped a poll before
        // something is.
        let status = match (self.parsec.last, self.parsec.machine.state()) {
            (None, _) => "starting",
            (Some(_), Presence::Connected) => "connected",
            (Some(_), Presence::Disconnected) => "notConnected",
            (Some(_), Presence::Unknown) => "unreadable",
        };
        let (stopped_at, stopped) = match &self.parsec.stopped {
            Some((at, what)) => (
                Some(*at),
                what.iter().map(|s| s.token().to_string()).collect(),
            ),
            None => (None, Vec::new()),
        };
        Some(ParsecPresenceDto {
            status: status.to_string(),
            stopped_at,
            stopped,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::Tier;
    use crate::engine::RttyStreamTick;
    use Verdict::{Connected, Disconnected, Unreadable};

    /// An engine with presence mode switched on.
    fn watching(e: &mut Engine) {
        e.settings.parsec_presence_stop = true;
    }

    /// Connected, then `lost` — the drop under test. Returns what the drop stopped.
    fn drop_session(e: &mut Engine, lost: &[Verdict]) -> Vec<PresenceStop> {
        assert!(e.apply_parsec_presence(Some(Connected)).is_empty());
        let mut stopped = Vec::new();
        for v in lost {
            stopped.extend(e.apply_parsec_presence(Some(*v)));
        }
        stopped
    }

    /// Phone with the mic key LATCHED (the Lock toggle) — the transmission a dropped link leaves
    /// keyed.
    fn phone_latched() -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        watching(&mut e);
        e.set_operating_mode("phone", false);
        e.set_ptt(true);
        assert!(e.manual_ptt(), "baseline: keyed");
        e
    }

    fn rtty_latched() -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        watching(&mut e);
        e.set_operating_mode("rtty", false);
        e.set_rtty_latched(true).unwrap();
        e.rtty_type("CQ CQ DE W9XYZ").unwrap();
        assert!(e.rtty_latched(), "baseline: latched");
        e
    }

    fn psk_latched() -> Engine {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        watching(&mut e);
        e.set_operating_mode("keyboard", false);
        e.set_psk_latched(true).unwrap();
        assert!(e.psk_latched(), "baseline: latched");
        e
    }

    // ----- each covered transmission, released within one verdict, through its own path -----

    #[test]
    fn a_latched_phone_ptt_is_released_through_halt_tx() {
        let mut e = phone_latched();
        let stopped = drop_session(&mut e, &[Disconnected]);
        assert_eq!(stopped, vec![PresenceStop::Ptt]);
        // What the radio loop reads on its next pass: no key.
        assert!(!e.manual_ptt(), "the mic key survived the drop");
        // HALT_TX'S FINGERPRINT, which a bare `set_ptt(false)` does not leave: the TX enable is
        // down (stopped stays stopped) and the one-shot slot abort is armed.
        assert!(!e.tx_enabled(), "halt_tx disarms — a bare unkey would not");
        assert!(e.take_slot_tx_abort(), "halt_tx arms the immediate kill");
        // …and re-arming does not re-key: the latched INTENT is gone, not merely masked.
        e.set_tx_enabled(true);
        assert!(!e.manual_ptt(), "re-arming TX re-keyed the rig");
    }

    #[test]
    fn rtty_continuous_tx_is_released_through_rtty_stop() {
        let mut e = rtty_latched();
        let stopped = drop_session(&mut e, &[Disconnected]);
        assert_eq!(stopped, vec![PresenceStop::Rtty]);
        assert!(!e.rtty_latched(), "the latch survived");
        assert!(
            e.take_rtty_abort(),
            "no abort armed — the loop would key to the end of the ring"
        );
        assert_eq!(
            e.poll_rtty_stream(2),
            RttyStreamTick::Idle,
            "the stream kept feeding"
        );
        // RTTY_STOP'S FINGERPRINT: the transmitter is stopped and TX stays ARMED — halt_tx would
        // have lowered it.
        assert!(
            e.tx_enabled(),
            "rtty_stop leaves TX armed; this went through halt_tx"
        );
    }

    #[test]
    fn psk_continuous_tx_is_released_through_psk_stop() {
        let mut e = psk_latched();
        let stopped = drop_session(&mut e, &[Disconnected]);
        assert_eq!(stopped, vec![PresenceStop::Psk]);
        assert!(!e.psk_latched(), "the latch survived");
        assert!(e.take_psk_abort(), "no abort armed");
        assert!(
            e.tx_enabled(),
            "psk_stop leaves TX armed; this went through halt_tx"
        );
    }

    #[test]
    fn tune_is_released_through_the_tune_off_path() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        watching(&mut e);
        e.set_operating_mode("phone", false);
        e.set_tune(true);
        assert!(e.tuning(), "baseline: tuning");
        let stopped = drop_session(&mut e, &[Disconnected]);
        assert_eq!(stopped, vec![PresenceStop::Tune]);
        assert!(!e.tuning(), "the carrier survived the drop");
        // SET_TUNE(FALSE)'S FINGERPRINT in a manual mode: the carrier is down and TX stays armed
        // (halt_tx would have lowered it, and armed the slot abort).
        assert!(e.tx_enabled(), "the Tune-off path leaves Phone armed");
        assert!(!e.take_slot_tx_abort(), "no halt ran");
    }

    /// Unreadable after a session was seen counts as lost — on the second unreadable poll, one
    /// verdict after the first.
    #[test]
    fn an_unreadable_log_after_a_session_releases_the_ptt() {
        let mut e = phone_latched();
        assert!(e.apply_parsec_presence(Some(Connected)).is_empty());
        assert!(
            e.apply_parsec_presence(Some(Unreadable)).is_empty(),
            "one miss is retried"
        );
        assert!(e.manual_ptt(), "…and changes nothing yet");
        assert_eq!(
            e.apply_parsec_presence(Some(Unreadable)),
            vec![PresenceStop::Ptt]
        );
        assert!(!e.manual_ptt());
    }

    // ----- what it must leave alone -----

    /// ⛔ FT auto-sequencing is not covered. A CQ run with an over in flight rides through a
    /// dropped session untouched: TX stays armed, no abort is armed, the run is still a run.
    #[test]
    fn an_ft_run_is_left_alone() {
        let mut e = Engine::new("W9XYZ", "EN52", 0);
        watching(&mut e);
        e.set_tier(Tier::Ft8);
        e.set_mode("qso-run").unwrap();
        assert!(!e.poll_tx(0).is_empty(), "baseline: the CQ keys");
        e.app.set_transmitting(true); // the over is on the air
        let mode_before = e.snapshot().mode;
        let stopped = drop_session(&mut e, &[Disconnected]);
        assert!(
            stopped.is_empty(),
            "presence mode stopped an FT over: {stopped:?}"
        );
        assert!(e.tx_enabled(), "the FT run was disarmed");
        assert!(!e.take_slot_tx_abort(), "the FT over in flight was cut");
        assert!(e.app.radio.transmitting, "the over in flight was cleared");
        assert_eq!(e.snapshot().mode, mode_before, "the run changed mode");
    }

    /// Nothing covered is up: a drop is noted and nothing is touched.
    #[test]
    fn an_unkeyed_station_is_left_alone() {
        let mut e = Engine::new("W9XYZ", "EN61", 0);
        watching(&mut e);
        e.set_operating_mode("phone", false);
        let stopped = drop_session(&mut e, &[Disconnected]);
        assert!(stopped.is_empty());
        assert!(e.tx_enabled(), "an idle Phone station was disarmed");
        assert!(!e.take_slot_tx_abort());
        assert!(
            e.parsec_presence_dto().unwrap().stopped.is_empty(),
            "nothing stopped, nothing reported"
        );
    }

    /// Never connected: the operator is at the shack. However the log reads, a latched key
    /// stays keyed.
    #[test]
    fn never_connected_does_nothing() {
        let mut e = phone_latched();
        for v in [
            Unreadable,
            Unreadable,
            Disconnected,
            Unreadable,
            Unreadable,
            Disconnected,
        ] {
            assert!(e.apply_parsec_presence(Some(v)).is_empty(), "{v:?}");
        }
        assert!(e.manual_ptt(), "a local operator's key was released");
        assert!(e.tx_enabled());
    }

    /// No transition, no action: an established session that stays connected changes nothing,
    /// and a drop stops once — not again on every poll after it.
    #[test]
    fn no_transition_no_action() {
        let mut e = phone_latched();
        for _ in 0..10 {
            assert!(e.apply_parsec_presence(Some(Connected)).is_empty());
        }
        assert!(e.manual_ptt());
        assert_eq!(
            e.apply_parsec_presence(Some(Disconnected)),
            vec![PresenceStop::Ptt]
        );
        // The operator (or anyone at the shack) keys up again while the session is still gone.
        e.set_tx_enabled(true);
        e.set_ptt(true);
        for v in [Disconnected, Disconnected, Unreadable, Unreadable] {
            assert!(
                e.apply_parsec_presence(Some(v)).is_empty(),
                "{v:?} stopped twice"
            );
        }
        assert!(
            e.manual_ptt(),
            "a later key was released without a new drop"
        );
    }

    /// Off (the default) means off: no verdict is acted on, and switching it on later starts
    /// from "unknown", never from a session seen while it was off.
    #[test]
    fn switched_off_it_does_nothing_and_forgets() {
        let mut e = phone_latched();
        e.settings.parsec_presence_stop = false;
        for v in [Some(Connected), Some(Disconnected), None] {
            let stopped = e.apply_parsec_presence(v);
            assert!(
                stopped.is_empty(),
                "presence mode acted while switched off: {v:?} stopped {stopped:?}"
            );
        }
        assert!(e.manual_ptt(), "presence mode acted while switched off");
        assert_eq!(e.parsec_presence_dto(), None);
        // A session seen while ON, then the switch goes off and on again with no new read of
        // "connected": a disconnect now is not an edge.
        e.settings.parsec_presence_stop = true;
        assert!(e.apply_parsec_presence(Some(Connected)).is_empty());
        assert!(e.apply_parsec_presence(None).is_empty()); // the watcher's "off"
        let stopped = e.apply_parsec_presence(Some(Disconnected));
        assert!(
            stopped.is_empty() && e.manual_ptt(),
            "a session from before the switch went off was remembered: stopped {stopped:?}"
        );
    }

    // ----- what it says -----

    #[test]
    fn the_snapshot_reports_status_and_the_last_stop_until_the_operator_transmits_again() {
        let mut e = phone_latched();
        assert_eq!(
            e.parsec_presence_dto(),
            Some(ParsecPresenceDto {
                status: "starting".into(),
                stopped_at: None,
                stopped: vec![],
            })
        );
        e.apply_parsec_presence(Some(Connected));
        assert_eq!(e.parsec_presence_dto().unwrap().status, "connected");
        e.apply_parsec_presence(Some(Disconnected));
        let dto = e.snapshot().parsec_presence.expect("on → reported");
        assert_eq!(dto.status, "notConnected");
        assert_eq!(dto.stopped, vec!["ptt".to_string()]);
        assert!(dto.stopped_at.is_some_and(|t| t > 0));
        // Still reported on the polls after — the operator reconnects and reads it.
        e.apply_parsec_presence(Some(Connected));
        e.apply_parsec_presence(Some(Connected));
        assert_eq!(
            e.parsec_presence_dto().unwrap().stopped,
            vec!["ptt".to_string()]
        );
        // Transmitting again is what retires it.
        e.set_tx_enabled(true);
        e.set_ptt(true);
        e.apply_parsec_presence(Some(Connected));
        let dto = e.parsec_presence_dto().unwrap();
        assert!(
            dto.stopped.is_empty() && dto.stopped_at.is_none(),
            "{dto:?}"
        );
        // The readout follows the MACHINE: one retried miss is still a session — the next miss
        // is the one that drops it — so it must not yet read "unreadable, nothing will be
        // stopped" a poll before something is.
        e.apply_parsec_presence(Some(Unreadable));
        assert_eq!(
            e.parsec_presence_dto().unwrap().status,
            "connected",
            "one retried miss already read as a lost log"
        );
        e.apply_parsec_presence(Some(Unreadable));
        assert_eq!(e.parsec_presence_dto().unwrap().status, "unreadable");
    }

    #[test]
    fn the_stop_tokens_are_the_ones_the_ui_translates() {
        let tokens: Vec<&str> = [
            PresenceStop::Tune,
            PresenceStop::Ptt,
            PresenceStop::Rtty,
            PresenceStop::Psk,
        ]
        .into_iter()
        .map(PresenceStop::token)
        .collect();
        assert_eq!(tokens, ["tune", "ptt", "rtty", "psk"]);
        // Each has its words in the English catalog, under the key the lane builds.
        let en = include_str!("../../../../ui/src/i18n/en.ts");
        for t in tokens {
            let key = format!("'shell.lane.parsecStop.what.{t}'");
            assert!(en.contains(&key), "ui/src/i18n/en.ts has no {key}");
        }
    }
}
