//! A streamed Remote operator's microphone, on the engine: the over's latch ([`crate::mic`]),
//! ARMED by their held PTT, KEYED by their audio, and ended by every stop the station has.
//!
//! ## M1 with S7: the hold arms, audio keys, either gap unkeys
//!
//! The page's held PTT ([`crate::remote_control::ptt_hold::PttHold`]) used to key the station on
//! the hold alone, through the desktop's own PTT verb. Under M1 it ARMS a microphone over instead
//! ([`Engine::arm_remote_mic`]), and nothing goes on the air until the first frame of the page's
//! audio is accepted. The hold's own 200 ms gap ends the over as it always did, and so does 200 ms
//! with no audio (G6): either gap unkeys. An armed over with no audio keys nothing, however long
//! the hold is kept up.
//!
//! A press of the Phone cockpit's own PTT made through the stream's picture (Space over it, or its
//! button) arms THE SAME over, and its release ends it ([`Engine::release_remote_mic`]): the
//! operator's ruling "Arms your mic". That press is bounded by the stream's held-input dead-man
//! instead of the page's hold, and by every rule below.
//!
//! What the page is shown of the over (`micState`) is published to the feed by each tick of a live
//! over and by its end ([`MicStatus`]).
//!
//! ## Where each stop reaches it
//!
//! Every end goes through [`Engine::drop_mic_latch`]: the feed closes first (its epoch moves, M12),
//! the latch is dropped, and the one-shot abort is armed when the rig was keyed, which the radio
//! loop turns into `flush_output(); rig.ptt(false)`, as it does the voice keyer's.
//! - `halt_tx`, the universal stop (the desktop's Stop TX, the stream's own Stop, WSJT-X's HaltTx,
//!   a lapse of the stream's presence): unchanged, and it drops the over (M10).
//! - `set_tx_enabled(false)`: TX Off ends a microphone over in flight, as it does RTTY and SSTV
//!   (M10, the design's one deliberate change to what a shipped button does).
//! - The per-tick predicate ([`Engine::poll_mic`]): every gate, the ceiling, the watchdog, presence
//!   for THIS over, the transmit route, and the audio gap.
//! - The radio loop, when it does not own the operator's radio (`may_key`).
//!
//! ## One owner
//!
//! From the arm, the microphone owns the transmitter ([`super::TxOwner::Mic`]): the starters that
//! consult the arbiter (RTTY, PSK, SSTV, APRS, the ATU tune) refuse while it is up, and so does the
//! voice keyer, which otherwise consults none; a tune takes the transmitter from it (G3), as it does
//! from a latched RTTY stream. The arm is refused while any other source owns the transmitter.
use super::Engine;
use super::{now_unix_millis, now_unix_secs};

/// How much of the operator's voice must have arrived, with the rig reporting no power, before the
/// no-power warning is believed: two seconds, the same settle as the loop's own zero-power watch.
const MIC_NO_POWER_AFTER_MS: u64 = 2_000;
use crate::mic::{MicDrop, MicEnded, MicFeed, MicGates, MicStatus, MicTick, MIC};
use crate::settings::OperatingMode;
use std::time::Instant;

impl Engine {
    /// Share the stream's microphone feed with the engine. Installed once, when Remote is built;
    /// with none installed nothing can arm.
    pub fn set_remote_mic_feed(&mut self, feed: MicFeed) {
        self.mic_feed = Some(feed);
    }

    /// Arm a microphone over for the streamed operator's held PTT. Keys NOTHING (M1): the first
    /// frame of their audio does, on the radio loop. Returns whether an over is armed after.
    ///
    /// The up-front gate is the per-tick one, checked before anything is armed, plus the one owner:
    /// in Phone, transmit armed, inside the licence, no tune up, the stream's presence live, and
    /// nothing else holding the transmitter.
    pub fn arm_remote_mic(&mut self, now: Instant) -> bool {
        if self.mic.active() {
            return true;
        }
        let Some(feed) = self.mic_feed.clone() else {
            return false;
        };
        if self.settings.operating_mode != OperatingMode::Phone
            || !self.tx_enabled
            || !self.tx_allowed()
            || self.tuning
            || self.tx_owner().is_some()
            || self.clock_repair_holds_tx()
        {
            return false;
        }
        let Some(presence) = self.remote_presence.clone().filter(|p| p.valid(now)) else {
            return false;
        };
        self.mic.arm();
        feed.open();
        self.mic_presence = Some(presence);
        self.mic_rf_seen = false;
        self.mic_zero_seen = false;
        self.mic_ended = None;
        // Pressing PTT is an operator action, like a send: the unattended-transmit clock restarts
        // here, and the over is measured from its key.
        self.reset_tx_watchdog();
        true
    }

    /// The Phone cockpit let go of a press it armed through the stream's picture: the over ends,
    /// as the page's own PTT's does when it is let go.
    pub fn release_remote_mic(&mut self) {
        self.drop_mic_latch_for(MicEnded::Released);
    }

    /// Whether a microphone over is armed or keyed.
    pub fn mic_armed(&self) -> bool {
        self.mic.active()
    }

    /// Whether a microphone over has keyed the rig.
    pub fn mic_keyed(&self) -> bool {
        self.mic.keyed()
    }

    /// End the microphone over NOW: the ONE kill path. The feed closes FIRST, so nothing sent to
    /// the dead over can reach the next one (M12); the abort is armed only when the rig was keyed,
    /// because an armed over has put nothing on the air and a one-shot abort armed for nothing
    /// would cut an unrelated over riding the same PTT.
    pub fn drop_mic_latch(&mut self) {
        if let Some(feed) = &self.mic_feed {
            feed.close();
        }
        self.mic_presence = None;
        let why = self.mic_end.take().unwrap_or(MicEnded::Stopped);
        let was_active = self.mic.active();
        if self.mic.drop_latch() {
            self.mic_abort = true;
        }
        if was_active {
            self.mic_ended = Some(why);
            self.publish_mic_status();
        }
    }

    /// [`Self::drop_mic_latch`], saying why for the page.
    pub(super) fn drop_mic_latch_for(&mut self, why: MicEnded) {
        self.mic_end = Some(why);
        self.drop_mic_latch();
    }

    /// The over as the page is shown it (`micState`), published to the stream's feed: by every
    /// tick of a live over ([`Self::poll_mic`]), and by its end, after which no tick runs.
    fn publish_mic_status(&self) {
        if let Some(feed) = &self.mic_feed {
            feed.publish(MicStatus {
                armed: self.mic.active(),
                keyed: self.mic.keyed(),
                no_power_out: self.mic_no_power_out(),
                ended: self.mic_ended,
            });
        }
    }

    /// Fold one forward-power reading into the no-power warning's evidence. `None` (the rig did
    /// not answer, or has no such meter) is never evidence of anything.
    pub(super) fn observe_mic_power(&mut self, po_w: Option<f32>) {
        let Some(po) = po_w.filter(|_| self.mic.keyed()) else {
            return;
        };
        if po > 0.0 {
            self.mic_rf_seen = true; // the radio IS transmitting this over: settled
        } else if self.mic.voiced_ms() > 0 {
            self.mic_zero_seen = true;
        }
    }

    /// ⚠️ DISPLAY ONLY (the operator's ruling of 2026-09-27, "State it + warn"): the operator's
    /// voice has been arriving for about two seconds of this over and the rig reports no power
    /// out. Almost always the rig's SSB audio source is its own microphone, not the USB audio the
    /// over is played into, so what went out was the shack's microphone. It never keys, unkeys or
    /// refuses anything. It is never raised by a rig that reports no power reading, and once the
    /// rig has reported power this over it stays down.
    pub fn mic_no_power_out(&self) -> bool {
        self.mic.keyed()
            && self.mic.voiced_ms() >= MIC_NO_POWER_AFTER_MS
            && self.mic_zero_seen
            && !self.mic_rf_seen
    }

    /// Take + reset the one-shot microphone abort (the loop flushes output + unkeys).
    pub fn take_mic_abort(&mut self) -> bool {
        std::mem::take(&mut self.mic_abort)
    }

    /// One radio-loop tick of the microphone over: the ONLY path from the page's audio to the
    /// transmitter, and the per-tick predicate ([`crate::mic::MicLatch::tick`]).
    ///
    /// `budget_ms` is how much audio the loop's output ring can take this tick; the engine clamps
    /// it to [`crate::mic::MicMode::max_push_ms`] as well, so the bound does not rest on the
    /// caller alone. `route` is the loop's transmit-route generation (G7).
    pub fn poll_mic(&mut self, now: Instant, budget_ms: f64, route: u64) -> MicTick {
        if !self.mic.active() {
            return MicTick::Idle;
        }
        // G5: presence is live, and it is the session this over armed under. A new session (the
        // page reconnected) is not this over's: it must be pressed again.
        let presence = match (&self.remote_presence, &self.mic_presence) {
            (Some(live), Some(armed)) => live.valid(now) && live.same_session(armed),
            _ => false,
        };
        let gates = MicGates {
            tx_enabled: self.tx_enabled,
            tx_allowed: self.tx_allowed(),
            tuning: self.tuning,
            in_section: self.settings.operating_mode == OperatingMode::Phone,
            presence,
            route,
            now,
            now_ms: now_unix_millis(),
            now_secs: now_unix_secs(),
            watchdog_limit_secs: self.settings.tx_watchdog_min as u64 * 60,
            watchdog_start_secs: self.tx_watchdog_start,
        };
        let arrived = self
            .mic_feed
            .as_ref()
            .map(MicFeed::take)
            .unwrap_or_default();
        let budget_ms = budget_ms.clamp(0.0, MIC.max_push_ms as f64);
        let budget = (budget_ms * f64::from(MIC.rate_hz) / 1000.0) as usize;
        let res = self.mic.tick(gates, arrived, budget);
        if let Some(start) = res.watchdog_start {
            self.tx_watchdog_start = Some(start);
        }
        if let Some(why) = res.drop {
            if why == MicDrop::Watchdog {
                // A trip disarms TX so it stays stopped, exactly as the keyboard modes' does.
                self.tx_watchdog = true;
                self.tx_enabled = false;
            }
            self.drop_mic_latch_for(match why {
                MicDrop::GateDown => MicEnded::Stopped,
                MicDrop::Presence => MicEnded::Presence,
                MicDrop::DeviceChanged => MicEnded::RouteChanged,
                MicDrop::Ceiling => MicEnded::Ceiling,
                MicDrop::Watchdog => MicEnded::Watchdog,
                MicDrop::AudioGap => MicEnded::AudioGap,
            });
            // An unexplained unkey reads as a fault: say why.
            tempo_core::applog::info(
                "tx",
                &format!(
                    "{} over ended: {}",
                    MIC.name,
                    match why {
                        MicDrop::GateDown => "a transmit gate went down",
                        MicDrop::Presence => "the stream's transmit presence lapsed",
                        MicDrop::DeviceChanged => "the transmit audio route changed",
                        MicDrop::Ceiling => "it reached its 10-minute ceiling",
                        MicDrop::Watchdog => "the TX watchdog tripped",
                        MicDrop::AudioGap => "no audio arrived for 200 ms",
                    }
                ),
            );
        }
        self.publish_mic_status();
        res.tick
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mic::MicFrame;
    use crate::remote_control::ptt_hold::{HoldEnd, PttHold};
    use crate::remote_control::transmit::TransmitAuthority;
    use std::time::Duration;

    const PRESENCE: Duration = Duration::from_secs(5);
    const PRESS: &str = "10000000-0000-4000-8000-00000000000a";
    const TICK: f64 = 20.0;

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// A station a streamed operator controls, in Phone, as `Service::start` wires it: the held PTT
    /// and the microphone feed installed, presence held.
    struct Scene {
        e: Engine,
        hold: PttHold,
        feed: MicFeed,
        presence: TransmitAuthority,
        t0: Instant,
    }

    fn scene() -> Scene {
        let mut e = Engine::new("W9XYZ", "EN52", 0);
        let transmit = TransmitAuthority::default();
        e.set_remote_transmit_revocation(transmit.revocation());
        let hold = PttHold::default();
        e.set_remote_ptt_hold(hold.clone());
        let feed = MicFeed::default();
        e.set_remote_mic_feed(feed.clone());
        e.set_operating_mode("phone", false); // arms TX, as entering Phone does
        let presence = TransmitAuthority::default();
        let t0 = Instant::now();
        assert!(e.hold_remote_presence(presence.permit(t0 + PRESENCE).unwrap(), t0));
        Scene {
            e,
            hold,
            feed,
            presence,
            t0,
        }
    }

    fn frame(k: u64, at: Instant) -> MicFrame {
        MicFrame {
            seq: k + 1,
            media: MIC.samples(20) * k,
            arrived: at,
            samples: vec![0.5; MIC.samples(20) as usize],
        }
    }

    impl Scene {
        fn at(&self, n: u64) -> Instant {
            self.t0 + ms(n)
        }
        /// The page re-asserts its hold, and the radio loop ticks the engine once.
        fn held_tick(&mut self, n: u64) -> MicTick {
            self.hold.hold(PRESS, self.at(n));
            self.tick(n)
        }
        /// Arm with a press at 0 ms, then key on the page's first frame. Audio sent before the
        /// arm is refused (M5), so the press comes first, as it does from the page.
        fn key(&mut self) {
            assert_eq!(self.held_tick(0), MicTick::Armed, "premise: the press arms");
            self.feed.push(frame(0, self.at(0))).unwrap();
            assert_eq!(self.tick(0), MicTick::Key, "premise: the first frame keys");
        }
        fn tick(&mut self, n: u64) -> MicTick {
            let now = self.at(n);
            self.e.poll_remote_transmit(now);
            self.e.poll_mic(now, TICK, 1)
        }
    }

    // ── M1 with S7 ────────────────────────────────────────────────────────────────────────────

    /// ★ M1: the held PTT arms, and only the page's audio keys. `arming_without_audio_never_asserts_ptt`
    /// at the engine: a hundred ticks of a held PTT with no audio key nothing.
    #[test]
    fn a_held_ptt_arms_and_only_audio_keys() {
        let mut s = scene();
        for k in 0..100u64 {
            let tick = s.held_tick(20 * k);
            assert_eq!(
                tick,
                MicTick::Armed,
                "tick {k}: a hold with no audio did something"
            );
        }
        assert!(s.e.mic_armed(), "the hold did not arm");
        assert!(!s.e.mic_keyed(), "a hold with no audio keyed the rig");
        assert!(
            !s.e.manual_ptt(),
            "the hold keyed the desktop's PTT: that is not M1"
        );
        assert_eq!(
            s.e.tx_owner(),
            Some(super::super::TxOwner::Mic),
            "an armed over does not own TX"
        );
        // Positive control: the page's first frame keys.
        s.hold.hold(PRESS, s.at(2_000));
        s.feed.push(frame(0, s.at(2_000))).unwrap();
        assert_eq!(
            s.tick(2_000),
            MicTick::Key,
            "the first accepted frame did not key"
        );
        assert!(s.e.mic_keyed());
    }

    /// The page is told the press was taken as soon as it is armed; nothing more.
    #[test]
    fn the_page_hears_the_press_was_taken() {
        let mut s = scene();
        s.held_tick(0);
        let report = s.hold.reports().pop().expect("no report");
        assert!(
            report.keyed && report.end.is_none(),
            "the armed press was not reported: {report:?}"
        );
    }

    // ── Either gap unkeys ─────────────────────────────────────────────────────────────────────

    /// A6 under M1: the hold's own gap. No hold for 200 ms and the keyed over ends, with the abort
    /// armed; re-asserted every 100 ms (the page's rate), it does not.
    #[test]
    fn the_holds_gap_ends_a_keyed_over() {
        let mut s = scene();
        s.key();
        for k in 1..=50u64 {
            let n = 20 * k;
            if n.is_multiple_of(100) {
                s.hold.hold(PRESS, s.at(n));
            }
            s.feed.push(frame(k, s.at(n))).unwrap();
            s.tick(n);
            assert!(
                s.e.mic_keyed(),
                "the over ended at {n} ms with the hold re-asserted"
            );
        }
        // The last hold was at 1000 ms. The audio keeps coming; the hold does not.
        for k in 51..=59u64 {
            s.feed.push(frame(k, s.at(20 * k))).unwrap();
            s.tick(20 * k);
            assert!(
                s.e.mic_keyed(),
                "ended {} ms after the last hold",
                20 * k - 1_000
            );
        }
        s.feed.push(frame(60, s.at(1_200))).unwrap();
        s.tick(1_200);
        assert!(!s.e.mic_armed(), "the hold's gap did not end the over");
        assert!(
            s.e.take_mic_abort(),
            "the over ended without the abort that unkeys the rig"
        );
        assert!(
            s.e.tx_enabled(),
            "the gap disarmed TX: a release is not a halt"
        );
    }

    /// ★ M2 at the engine: the audio's gap ends a keyed over although the hold is still held, and
    /// the press ends with it (the page must press again). The positive control, a 180 ms pause, is
    /// the latch's own test.
    #[test]
    fn the_audios_gap_ends_a_keyed_over_under_a_held_ptt() {
        let mut s = scene();
        s.key();
        let mut ended = None;
        for k in 1..=40u64 {
            let n = 20 * k;
            s.hold.hold(PRESS, s.at(n)); // the hold never lapses
            if n <= 200 {
                s.feed.push(frame(k, s.at(n))).unwrap(); // audio stops after 200 ms
            }
            s.tick(n);
            if !s.e.mic_armed() {
                ended = Some(n);
                break;
            }
        }
        assert_eq!(
            ended,
            Some(200 + MIC.gap_ms),
            "the audio's gap did not end the over at 200 ms"
        );
        assert!(s.e.take_mic_abort(), "no abort armed");
        // The press is over too: a hold for it does not re-arm.
        s.hold.hold(PRESS, s.at(700));
        assert_eq!(
            s.tick(700),
            MicTick::Idle,
            "the ended press re-armed the over"
        );
        let ends: Vec<_> = s.hold.reports().into_iter().filter_map(|r| r.end).collect();
        assert_eq!(
            ends.last(),
            Some(&HoldEnd::Stopped),
            "the page was not told the over ended"
        );
    }

    /// An armed over that never keyed has put nothing on the air: its end arms no abort, which
    /// would otherwise cut an unrelated over riding the same PTT.
    #[test]
    fn an_armed_over_that_never_keyed_ends_without_an_abort() {
        let mut s = scene();
        s.held_tick(0);
        assert!(s.e.mic_armed());
        s.tick(300); // the hold lapsed at 200 ms
        assert!(
            !s.e.mic_armed(),
            "the hold's gap did not end the armed over"
        );
        assert!(
            !s.e.take_mic_abort(),
            "an over that never keyed armed the abort"
        );
    }

    // ── M7: a lease lapse mid-over ends it ────────────────────────────────────────────────────

    /// ★ M7: presence lapses mid-over (the page's heartbeats stopped). The next radio-loop poll
    /// ends the over and arms the abort. CONTROL: a tick before the deadline it is still keyed.
    #[test]
    fn a_lapse_of_presence_mid_over_unkeys_on_the_next_tick() {
        let mut s = scene();
        s.key();
        // Keep the hold and the audio alive; only presence is allowed to lapse.
        let deadline = PRESENCE.as_millis() as u64;
        for k in 1..(deadline / 20) {
            s.hold.hold(PRESS, s.at(20 * k));
            s.feed.push(frame(k, s.at(20 * k))).unwrap();
            s.tick(20 * k);
        }
        assert!(s.e.mic_keyed(), "control: ended before presence lapsed");
        s.hold.hold(PRESS, s.at(deadline));
        s.feed.push(frame(deadline / 20, s.at(deadline))).unwrap();
        s.tick(deadline);
        assert!(!s.e.mic_armed(), "a lapse of presence left the over keyed");
        assert!(s.e.take_mic_abort(), "no abort armed");
    }

    /// A reconnect is a new session: its presence is not this over's (G5), so the over ends and
    /// must be pressed again. CONTROL: the same session's renewal keeps it.
    #[test]
    fn a_new_sessions_presence_is_not_this_overs() {
        for same in [true, false] {
            let mut s = scene();
            s.key();
            let other = TransmitAuthority::default();
            let renewal = if same { &s.presence } else { &other };
            let at = s.at(40);
            assert!(s
                .e
                .hold_remote_presence(renewal.permit(at + PRESENCE).unwrap(), at));
            s.hold.hold(PRESS, s.at(40));
            s.feed.push(frame(1, s.at(40))).unwrap();
            s.tick(40);
            assert_eq!(s.e.mic_keyed(), same, "same session: {same}");
        }
    }

    // ── M10: the existing stops, unchanged ────────────────────────────────────────────────────

    /// `halt_tx` (Stop TX, a Stop from the stream, HaltTx) and TX Off each end a keyed over, arm
    /// the abort, and close the feed.
    #[test]
    fn halt_tx_and_tx_off_end_a_live_mic_over() {
        for (name, stop) in [("halt_tx", 0), ("TX Off", 1), ("leaving Phone", 2)] {
            let mut s = scene();
            s.key();
            match stop {
                0 => s.e.halt_tx(),
                1 => s.e.set_tx_enabled(false),
                _ => s.e.set_operating_mode("cw", false),
            }
            // halt_tx and TX Off end it at once; leaving Phone on the next tick (G4).
            if stop == 2 {
                s.tick(20);
            }
            assert!(!s.e.mic_armed(), "{name}: the over survived");
            assert!(s.e.take_mic_abort(), "{name}: no abort armed");
            assert_eq!(
                s.feed.push(frame(1, s.at(20))),
                Err(crate::mic::Refused::Closed),
                "{name}: the feed still takes audio"
            );
        }
    }

    // ── What the Phone cockpit's PTT reads (display only) ───────────────────────────────────

    /// The operator's pick "Show Armed until keyed": the snapshot says the over is `Armed` from the
    /// press and `Keyed` from the first frame of the operator's voice, and nothing once it ends.
    /// The cockpit's PTT label reads it; nothing keys, refuses or releases on it.
    #[test]
    fn the_snapshot_says_armed_until_the_voice_keys_the_rig() {
        use crate::dto::StreamMic;
        let mut s = scene();
        assert_eq!(
            s.e.snapshot().radio.stream_mic,
            None,
            "an over reported before any press"
        );
        assert_eq!(s.held_tick(0), MicTick::Armed, "premise: the press arms");
        assert_eq!(
            s.e.snapshot().radio.stream_mic,
            Some(StreamMic::Armed),
            "armed, no voice yet"
        );
        s.feed.push(frame(0, s.at(0))).unwrap();
        assert_eq!(s.tick(0), MicTick::Key, "premise: the first frame keys");
        assert_eq!(
            s.e.snapshot().radio.stream_mic,
            Some(StreamMic::Keyed),
            "keyed by the voice"
        );
        s.e.halt_tx();
        assert!(!s.e.mic_armed(), "premise: Stop TX ends the over");
        assert_eq!(
            s.e.snapshot().radio.stream_mic,
            None,
            "an ended over still reported"
        );
    }

    /// And `Armed` only while the armed over OWNS the transmitter (the lead's Q1 ruling,
    /// 2026-09-29): the header's ON AIR sign waits on it for every over, the page's own Hold PTT
    /// included. Anything the arbiter puts in front of it is on the air (a key at the shack, an FT
    /// over, the tune carrier), and then the over is not reported armed, so the sign lights.
    #[test]
    fn the_snapshot_says_armed_only_while_the_over_owns_the_transmitter() {
        use super::super::TxOwner;
        use crate::dto::StreamMic;
        let mut s = scene();
        assert_eq!(
            s.held_tick(0),
            MicTick::Armed,
            "premise: the page's press arms"
        );
        assert_eq!(
            s.e.snapshot().radio.stream_mic,
            Some(StreamMic::Armed),
            "armed, with nothing else on the air"
        );
        // A key at the shack rides along: the arbiter names it first.
        s.e.set_ptt(true);
        assert_eq!(s.e.tx_owner(), Some(TxOwner::ManualPtt), "premise");
        assert!(s.e.mic_armed(), "premise: the over is still armed");
        assert_eq!(
            s.e.snapshot().radio.stream_mic,
            None,
            "armed reported under a key at the shack"
        );
        s.e.set_ptt(false);
        assert_eq!(
            s.e.snapshot().radio.stream_mic,
            Some(StreamMic::Armed),
            "the shack's key let go, and the over is not armed again"
        );
        // An FT over in flight.
        s.e.app.radio.transmitting = true;
        assert_eq!(s.e.tx_owner(), Some(TxOwner::Slot), "premise");
        assert_eq!(
            s.e.snapshot().radio.stream_mic,
            None,
            "armed reported under an FT over"
        );
        s.e.app.radio.transmitting = false;
        // The tune carrier, before the next tick ends the over (G3).
        s.e.set_tune(true);
        assert!(s.e.mic_armed(), "premise: armed until the next tick");
        assert_eq!(
            s.e.snapshot().radio.stream_mic,
            None,
            "armed reported under the tune carrier"
        );
    }

    // ── One owner ─────────────────────────────────────────────────────────────────────────────

    /// The microphone joins the transmit arbiter: while it is armed the voice keyer is refused, a
    /// tune takes the transmitter from it (G3), and it is refused while another source owns the
    /// transmitter.
    #[test]
    fn the_mic_is_one_owner_among_the_others() {
        let mut s = scene();
        s.held_tick(0);
        assert!(s.e.mic_armed());
        assert_eq!(s.e.tx_owner(), Some(super::super::TxOwner::Mic));
        assert_eq!(
            s.e.send_voice(vec![0.25; 12_000]),
            Err(super::super::TxOwner::Mic.busy_reason()),
            "the keyer's refusal must name the microphone"
        );
        assert!(
            s.e.voice_tx.is_none(),
            "a voice message was queued under the microphone"
        );
        // A tune is the operator reaching for the transmitter: it ends the over (G3).
        s.e.set_tune(true);
        s.tick(20);
        assert!(
            !s.e.mic_armed(),
            "a tune carrier went up under a microphone over"
        );
        // The other way round: a voice message owns the transmitter, so a press is refused.
        let mut s = scene();
        s.e.send_voice(vec![0.25; 12_000]).unwrap();
        assert_eq!(
            s.e.tx_owner(),
            Some(super::super::TxOwner::Voice),
            "premise"
        );
        s.held_tick(0);
        assert!(
            !s.e.mic_armed(),
            "a press armed the mic under a voice message"
        );
        let ends: Vec<_> = s.hold.reports().into_iter().filter_map(|r| r.end).collect();
        assert_eq!(ends.last(), Some(&HoldEnd::Refused));
    }

    // ── R2: the no-power warning (display only) ───────────────────────────────────────────────

    /// A keyed over fed `ms` of audio at `level`, frame by frame.
    fn fed(level: f32, ms: u64) -> Scene {
        let mut s = scene();
        s.key();
        for k in 1..=ms / 20 {
            let n = 20 * k;
            s.hold.hold(PRESS, s.at(n));
            let mut f = frame(k, s.at(n));
            f.samples.iter_mut().for_each(|x| *x = level);
            s.feed.push(f).unwrap();
            s.tick(n);
        }
        s
    }

    /// ★ R2: voice arriving for two seconds and the rig reporting no power out raises the warning,
    /// and the warning changes nothing about the over. CONTROLS: power out above zero, no power
    /// reading, and no voice arriving each leave it down.
    #[test]
    fn the_no_power_warning_needs_voice_a_zero_reading_and_no_power() {
        // What the page is shown after the loop's next tick.
        let shown = |s: &mut Scene| {
            s.tick(2_200);
            s.feed.status().no_power_out
        };
        let mut s = fed(0.5, 2_200);
        s.e.observe_rig_tx_meters(None, None, Some(0.0), None);
        assert!(
            s.e.mic_no_power_out(),
            "voice for 2 s, the rig at 0 W, and no warning"
        );
        assert!(shown(&mut s), "the page was not shown the warning");
        assert!(
            s.e.mic_keyed(),
            "the warning changed the over: it is display only"
        );
        assert!(!s.e.take_mic_abort(), "the warning armed an abort");

        // Power out above zero, once, settles the over.
        let mut s = fed(0.5, 2_200);
        s.e.observe_rig_tx_meters(None, None, Some(25.0), None);
        s.e.observe_rig_tx_meters(None, None, Some(0.0), None);
        assert!(
            !s.e.mic_no_power_out(),
            "warned on a rig that reported power"
        );
        assert!(
            !shown(&mut s),
            "the page was shown a warning for a rig with power out"
        );

        // A rig that reports no power reading never warns.
        let mut s = fed(0.5, 2_200);
        s.e.observe_rig_tx_meters(Some(1.2), None, None, None);
        assert!(!s.e.mic_no_power_out(), "warned with no power reading");
        assert!(
            !shown(&mut s),
            "the page was shown a warning with no power reading"
        );

        // No voice arriving (the operator holding PTT in silence): no warning, whatever the meter.
        let mut s = fed(0.0, 2_200);
        s.e.observe_rig_tx_meters(None, None, Some(0.0), None);
        assert!(!s.e.mic_no_power_out(), "warned with no voice arriving");
        assert!(
            !shown(&mut s),
            "the page was shown a warning with no voice arriving"
        );

        // …and not before two seconds of voice.
        let mut s = fed(0.5, 1_000);
        s.e.observe_rig_tx_meters(None, None, Some(0.0), None);
        assert!(!s.e.mic_no_power_out(), "warned after one second of voice");
    }

    // ── What the page is shown (micState) ─────────────────────────────────────────────────────

    /// The over as the page is shown it: armed, then keyed by the voice, then its end and why.
    /// Each end names its own cause, so the page can say the right thing (design §7).
    #[test]
    fn the_page_is_shown_the_over_and_why_it_ended() {
        use crate::mic::{MicEnded, MicStatus};
        let status = |armed, keyed, ended| MicStatus {
            armed,
            keyed,
            no_power_out: false,
            ended,
        };
        // The voice stops: the audio gap.
        let mut s = scene();
        s.held_tick(0);
        assert_eq!(
            s.feed.status(),
            status(true, false, None),
            "the arm was not shown"
        );
        s.feed.push(frame(0, s.at(0))).unwrap();
        s.tick(0);
        assert_eq!(
            s.feed.status(),
            status(true, true, None),
            "the key was not shown"
        );
        let mut n = 20;
        while s.e.mic_armed() && n < 1_000 {
            s.hold.hold(PRESS, s.at(n));
            s.tick(n);
            n += 20;
        }
        assert_eq!(
            s.feed.status(),
            status(false, false, Some(MicEnded::AudioGap))
        );
        // The station ends the old press on its next tick; the page's next press is a new one,
        // and it clears the last end.
        s.tick(n);
        s.hold
            .hold("10000000-0000-4000-8000-00000000000b", s.at(n + 20));
        s.tick(n + 20);
        assert_eq!(
            s.feed.status(),
            status(true, false, None),
            "a new arm kept the old end"
        );

        for (name, end, why) in [
            ("the cockpit let go", 0, MicEnded::Released),
            ("Stop TX at the shack", 1, MicEnded::Stopped),
            ("presence lapsed", 2, MicEnded::Presence),
            ("TX Off", 3, MicEnded::Stopped),
        ] {
            let mut s = scene();
            s.key();
            match end {
                0 => s.e.release_remote_mic(),
                1 => s.e.halt_tx(),
                2 => {
                    s.presence.revoke();
                    s.e.poll_remote_transmit(s.at(20));
                }
                _ => s.e.set_tx_enabled(false),
            }
            assert_eq!(s.feed.status(), status(false, false, Some(why)), "{name}");
            assert!(
                s.e.take_mic_abort(),
                "{name}: the keyed over ended without its abort"
            );
        }
    }

    /// ★ The operator's ruling "Arms your mic": a press of the Phone cockpit's PTT made through the
    /// stream's picture arms THE SAME over as the page's own PTT, with no page press behind it, and
    /// it is held by the press alone: the page's hold does not drop it for being absent, only the
    /// voice keys it, it never keys the shack's own microphone, and the cockpit's release ends it.
    /// CONTROL: the microphone's own rules still end it, here the audio gap.
    #[test]
    fn a_press_through_the_picture_arms_the_same_over_and_its_release_ends_it() {
        use crate::mic::MicEnded;
        let mut s = scene();
        assert!(
            s.e.arm_remote_mic(s.at(0)),
            "the cockpit's press did not arm"
        );
        for n in 0..10u64 {
            assert_eq!(
                s.tick(20 * n),
                MicTick::Armed,
                "tick {n}: armed, and nothing keys"
            );
        }
        assert!(
            !s.e.manual_ptt(),
            "a streamed press keyed the shack's own microphone"
        );
        let mut n = 200;
        for k in 0..10u64 {
            s.feed.push(frame(k, s.at(n))).unwrap();
            s.tick(n);
            n += 20;
        }
        assert!(s.e.mic_keyed(), "the voice did not key it");
        s.e.release_remote_mic();
        assert!(!s.e.mic_armed(), "the cockpit's release left the over up");
        assert!(
            s.e.take_mic_abort(),
            "the keyed over ended without its abort"
        );
        assert_eq!(s.feed.status().ended, Some(MicEnded::Released));

        // CONTROL: the same press, and the voice stops: the audio gap ends it (G6).
        let mut s = scene();
        assert!(s.e.arm_remote_mic(s.at(0)));
        s.feed.push(frame(0, s.at(0))).unwrap();
        assert_eq!(s.tick(0), MicTick::Key);
        let mut n = 20;
        while s.e.mic_armed() && n < 1_000 {
            s.tick(n);
            n += 20;
        }
        assert!(!s.e.mic_armed(), "the audio gap did not end it");
        assert!(n <= 260, "ended only at {n} ms");
        assert_eq!(s.feed.status().ended, Some(MicEnded::AudioGap));
    }

    /// Pressing PTT is an operator action: the watchdog's clock restarts at the arm.
    #[test]
    fn the_arm_restarts_the_watchdog_clock() {
        let mut s = scene();
        s.e.tx_watchdog_start = Some(1);
        s.held_tick(0);
        assert!(s.e.mic_armed());
        assert_eq!(
            s.e.tx_watchdog_start, None,
            "the arm did not restart the watchdog clock"
        );
    }

    /// Blind means no authority, at the arm itself and not only at the held PTT in front of it: an
    /// arm with no live presence is refused (the path a press made through the picture takes).
    /// CONTROL: with presence it arms.
    #[test]
    fn nothing_arms_without_presence() {
        let mut e = Engine::new("W9XYZ", "EN52", 0);
        e.set_remote_mic_feed(MicFeed::default());
        e.set_operating_mode("phone", false);
        let t0 = Instant::now();
        assert!(!e.arm_remote_mic(t0), "armed with no presence");
        let presence = TransmitAuthority::default();
        e.hold_remote_presence(presence.permit(t0 + PRESENCE).unwrap(), t0);
        assert!(e.arm_remote_mic(t0), "control: with presence it arms");
    }

    /// With no microphone feed installed (a build or a station without the stream) nothing arms.
    #[test]
    fn nothing_arms_without_a_feed() {
        let mut e = Engine::new("W9XYZ", "EN52", 0);
        e.set_operating_mode("phone", false);
        let presence = TransmitAuthority::default();
        let t0 = Instant::now();
        e.hold_remote_presence(presence.permit(t0 + PRESENCE).unwrap(), t0);
        assert!(
            !e.arm_remote_mic(t0),
            "armed with nowhere for audio to come from"
        );
        e.set_remote_mic_feed(MicFeed::default());
        assert!(e.arm_remote_mic(t0), "control: with a feed it arms");
    }
}
