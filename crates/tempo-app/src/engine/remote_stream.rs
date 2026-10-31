//! A streamed Remote session's hold on the transmitter: its transmit PRESENCE, and its held PTT.
//! Both are decided here, on the radio loop's own poll (`poll_remote_transmit`, which the loop
//! runs every tick and FT planning and commit run too), from the station's own clock.
//!
//! ## Presence (S8, TX sign-off 2026-09-27: "Session permit")
//!
//! While a streamed controller is attached, the station holds a presence permit for the session.
//! The station mints it from the session's lease (deadline at most five seconds out, never past
//! the lease) and renews it only on a heartbeat whose picture is fresh (S9: blind means no
//! authority). When it lapses, because the heartbeats stopped or the picture went stale, the next
//! poll runs [`Engine::halt_tx`], the universal stop, and EVERY transmission at the station ends:
//! FT, a Phone PTT, RTTY/PSK, Tune, the voice keyer, CW, whoever started it. So a stream that goes
//! away cannot leave the station transmitting for longer than five seconds and one tick, half-open
//! link or not.
//!
//! Presence only ever stops. Holding it arms, keys and grants nothing, and a station with no stream
//! attached never holds one.
//!
//! ⚠️ IT HAS ITS OWN SLOT, NOT THE FT PERMIT'S (`remote_transmit`), and the plan that proposed the
//! FT permit was wrong about the code. `set_tx_enabled(true)` clears `remote_transmit` on EVERY
//! local arm ("an explicit native arm owns the next transmission"), and under a stream every click
//! the operator makes IS a local arm, so a presence kept there vanished the moment they pressed
//! Call CQ, and a link that dropped before the next heartbeat was never halted. `halt_tx` clears
//! that slot too, which left a gap after every Stop. Pinned by
//! `a_local_arm_keeps_the_sessions_presence` and `a_local_stop_ends_the_transmission_not_the_presence`,
//! both watched red against the shared slot first. For the same reason the station mints presence
//! from its own revocation, which `halt_tx`'s stand-down does not move: a Stop ends a
//! transmission, not the session.
//!
//! ## The held PTT (S7), under M1
//!
//! [`PttHold`] is the state; this is where it is decided. Under the audio design's M1 the hold
//! ARMS the streamed operator's microphone over (`engine/remote_mic.rs`) and their audio keys it:
//! a hold with no audio puts nothing on the air. (Before M1 the hold keyed the station through the
//! desktop's own PTT verb, on the hold alone.) Either gap ends the over: the hold's own 200 ms,
//! here, and 200 ms without audio, in the microphone's predicate, which ends the press with it. A
//! press is admitted only while presence is live and in Phone, the one cockpit whose PTT it stands
//! for, and every other gate the arm checks is the microphone's own.
use super::{AprsArm, Engine};
use crate::remote_control::ptt_hold::PttHold;
use crate::remote_control::transmit::TransmitPermit;
use crate::remote_control::Standing;
use crate::settings::OperatingMode;
use std::time::Instant;

impl Engine {
    /// Install or renew the streamed session's presence. Returns whether presence is held after.
    ///
    /// A lapse is carried out before anything can replace it: a fresh permit arriving a moment
    /// after the old one ran out (blind for five seconds, then the picture came back a tick before
    /// the radio loop looked) must not erase the stop the lapse owes.
    pub fn hold_remote_presence(&mut self, permit: TransmitPermit, now: Instant) -> bool {
        self.poll_remote_presence(now);
        if !permit.valid(now) {
            return false;
        }
        let renewed = self
            .remote_presence
            .as_mut()
            .is_some_and(|current| current.renew(permit.clone(), now));
        if !renewed {
            // None, or a different session's: this one is the live authority now. A different
            // session's presence it replaces has ended.
            if self
                .remote_presence
                .as_ref()
                .is_some_and(|current| !current.same_session(&permit))
            {
                self.remote_presence_ended.revoke();
            }
            self.remote_presence = Some(permit);
        }
        true
    }

    /// Whether a streamed session holds live presence.
    pub fn remote_presence_live(&self, now: Instant) -> bool {
        self.remote_presence.as_ref().is_some_and(|p| p.valid(now))
    }

    /// The live session's presence as a [`Standing`](crate::remote_control::Standing), for work
    /// that computer starts at the station and that must end when it is no longer there: held
    /// until that presence lapses or another session's takes its place. `None` with no live
    /// presence. A Stop does not end it: a Stop ends a transmission, not the session.
    pub fn remote_presence_standing(
        &self,
        now: Instant,
    ) -> Option<crate::remote_control::Standing> {
        self.remote_presence_live(now)
            .then(|| self.remote_presence_ended.standing())
    }

    /// Bind `what` to the presence it was turned on under: `Some` from a computer driving the
    /// station, which ends it once that presence lapses or another session's takes its place;
    /// `None` from the shack's own screen, or as it is turned off, which unbinds it. The latest
    /// press owns it.
    pub fn bind_to_presence(&mut self, what: OnItsOwn, standing: Option<Standing>) {
        self.presence_bound.retain(|(bound, _)| *bound != what);
        if let Some(standing) = standing {
            self.presence_bound.push((what, standing));
        }
    }

    /// ⛔ End what a computer that is no longer here turned on: each [`OnItsOwn`] whose presence no
    /// longer stands is turned off, as the shack's own screen turns it off. Only ever stops.
    fn end_what_presence_left(&mut self) {
        if self
            .presence_bound
            .iter()
            .all(|(_, standing)| standing.held())
        {
            return;
        }
        let (ended, kept) = std::mem::take(&mut self.presence_bound)
            .into_iter()
            .partition::<Vec<_>, _>(|(_, standing)| !standing.held());
        self.presence_bound = kept;
        self.end_from_afar(ended.into_iter().map(|(what, _)| what).collect());
    }

    /// ⛔ A Stop ends everything a computer driving the station left to key later, whoever is
    /// there: [`Engine::halt_tx`] calls it, so every Stop, a presence lapse and a context change
    /// end each [`OnItsOwn`], and nothing it left keys once TX is armed again. What the shack's
    /// own screen turned on is bound to nothing and is left as a Stop has always left it.
    pub(super) fn end_all_from_afar(&mut self) {
        let ended = std::mem::take(&mut self.presence_bound)
            .into_iter()
            .map(|(what, _)| what)
            .collect();
        self.end_from_afar(ended);
    }

    /// Turn off, or drop, each of `ended`. Only ever stops.
    fn end_from_afar(&mut self, ended: Vec<OnItsOwn>) {
        for what in ended {
            tempo_core::applog::warn(
                "tx",
                &format!("{what:?} left by a computer driving the station is ended"),
            );
            match what {
                // The decoder keeps listening, receive only, as a view's own arm leaves it.
                OnItsOwn::AprsAck => {
                    if self.aprs_arm == AprsArm::Explicit {
                        self.aprs_arm = AprsArm::Auto;
                        self.aprs_health.arm = AprsArm::Auto;
                    }
                }
                OnItsOwn::Beacon => self.set_beacon(false),
                OnItsOwn::ChatCq => {
                    // Turning the run off cannot fail.
                    let _ = self.set_chat_cq(false);
                }
                // Dropped from the store-and-forward queue, its bubble marked abandoned so the
                // operator can send it again; one already delivered is left as it is.
                OnItsOwn::ChatMessage { peer, id } => {
                    if self.app.abandon_queued(&peer, id) {
                        self.persist_pending_msgs();
                    }
                }
            }
        }
    }

    /// A start from afar ran under the transmit stand-down `under`: until a halt, a revocation of
    /// it with no halt ends what was started as a Stop does ([`Self::poll_remote_transmit`]).
    pub fn note_started_from_afar(&mut self, under: u64) {
        self.afar_started = Some(under);
    }

    /// Share the stream's held-PTT state with the engine. Installed once, when Remote is built.
    pub fn set_remote_ptt_hold(&mut self, hold: PttHold) {
        self.remote_ptt_hold = Some(hold);
    }

    /// Halt everything if presence has lapsed, once, and forget it.
    fn poll_remote_presence(&mut self, now: Instant) -> bool {
        if self.remote_presence.as_ref().is_none_or(|p| p.valid(now)) {
            return false;
        }
        self.remote_presence = None;
        self.remote_presence_ended.revoke();
        tempo_core::applog::warn(
            "tx",
            "remote stream presence lapsed: stopping every transmission at the station",
        );
        // The halt ends a microphone over too; the page is told it was presence (M7).
        if self.mic_armed() {
            self.mic_end = Some(crate::mic::MicEnded::Presence);
        }
        self.halt_tx();
        true
    }

    /// Presence first, so a lapse has already stopped the over before the held PTT is decided.
    /// The press's "key" is the microphone over's ARM (M1); it stays up while the over does.
    pub(super) fn poll_remote_stream(&mut self, now: Instant) -> bool {
        let mut halted = self.poll_remote_presence(now);
        // ⛔ The transmit stand-down revoked with no halt (the driving computer's permission to
        // transmit withdrawn, its lease gone) after a start from afar ran under it: what it
        // started, queued or turned on ends as a Stop ends it.
        let stand_down = self.remote_transmit_stand_down.generation();
        if self.afar_started.is_some_and(|under| under != stand_down) {
            tempo_core::applog::warn(
                "tx",
                "remote transmit stand-down revoked: stopping every transmission at the station",
            );
            self.halt_tx();
            halted = true;
        }
        self.end_what_presence_left();
        // Decoder screens left while that computer was attached stop once it has gone (RX only).
        self.carry_out_held_leaves(now);
        if let Some(hold) = self.remote_ptt_hold.clone() {
            let may_key = self.remote_presence_live(now)
                && self.settings.operating_mode == OperatingMode::Phone;
            let armed = self.mic_armed();
            hold.tick(now, may_key, armed, |on| {
                if on {
                    self.arm_remote_mic(now)
                } else {
                    // The press ended (released, or its holds stopped arriving), or it is no
                    // longer admitted here (presence, Phone).
                    self.drop_mic_latch_for(if may_key {
                        crate::mic::MicEnded::Released
                    } else {
                        crate::mic::MicEnded::Stopped
                    });
                    false
                }
            });
        }
        halted
    }
}

/// What a computer driving the station can turn on or leave queued there that then transmits by
/// itself, with no further press: so each ends when that computer is no longer there
/// ([`Engine::bind_to_presence`]), as its transmissions are cut, and with any Stop.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OnItsOwn {
    /// APRS Monitor armed by hand, which lets the automatic ack key. Ended, the decoder listens on,
    /// receive only.
    AprsAck,
    /// Chat's presence beacon.
    Beacon,
    /// Chat's CQ run.
    ChatCq,
    /// A chat message in the store-and-forward queue, which keys when its peer is heard and
    /// again until acknowledged: its peer and its id there.
    ChatMessage { peer: String, id: char },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dto::Tier;
    use crate::remote_control::transmit::TransmitAuthority;
    use std::time::Duration;

    /// The deadline a heartbeat gives presence: five seconds, as the lease.
    const PRESENCE: Duration = Duration::from_secs(5);

    /// A station with a stream attached: the engine wired as production wires it (its stand-down
    /// is the FT transmit authority's revocation), and a presence authority for the session.
    fn attached() -> (Engine, TransmitAuthority, TransmitAuthority, Instant) {
        let mut e = Engine::new("W9XYZ", "EN52", 0);
        let transmit = TransmitAuthority::default();
        e.set_remote_transmit_revocation(transmit.revocation());
        e.set_remote_ptt_hold(PttHold::default());
        e.set_remote_mic_feed(crate::mic::MicFeed::default());
        let presence = TransmitAuthority::default();
        let t0 = Instant::now();
        assert!(e.hold_remote_presence(presence.permit(t0 + PRESENCE).unwrap(), t0));
        (e, transmit, presence, t0)
    }

    /// Every kind of transmission a streamed operator can start, each from the same verb the
    /// desktop's own control calls, and the proof it is up.
    #[derive(Clone, Copy, Debug)]
    enum Kind {
        Ft,
        PhonePtt,
        Rtty,
        Psk,
        Tune,
        VoiceKeyer,
        Cw,
    }
    const KINDS: [Kind; 7] = [
        Kind::Ft,
        Kind::PhonePtt,
        Kind::Rtty,
        Kind::Psk,
        Kind::Tune,
        Kind::VoiceKeyer,
        Kind::Cw,
    ];

    fn start(e: &mut Engine, kind: Kind) {
        match kind {
            Kind::Ft => {
                e.set_tier(Tier::Ft8);
                e.start_cq(None).unwrap();
                assert!(e.tx_enabled(), "{kind:?}: baseline armed");
            }
            Kind::PhonePtt => {
                e.set_operating_mode("phone", false);
                e.set_ptt(true);
                assert!(e.manual_ptt(), "{kind:?}: baseline keyed");
            }
            Kind::Rtty => {
                e.set_operating_mode("rtty", false);
                e.set_rtty_latched(true).unwrap();
                e.rtty_type("CQ CQ DE W9XYZ").unwrap();
                assert!(e.rtty_latched(), "{kind:?}: baseline latched");
            }
            Kind::Psk => {
                e.set_operating_mode("keyboard", false);
                e.set_psk_latched(true).unwrap();
                assert!(e.psk_latched(), "{kind:?}: baseline latched");
            }
            Kind::Tune => {
                e.set_operating_mode("phone", false);
                e.set_tune(true);
                assert!(e.tuning(), "{kind:?}: baseline tuning");
            }
            Kind::VoiceKeyer => {
                e.set_operating_mode("phone", false);
                e.send_voice(vec![0.25; 12_000]).unwrap();
                assert!(e.voice_tx.is_some(), "{kind:?}: baseline queued");
            }
            Kind::Cw => {
                e.set_operating_mode("cw", false);
                e.send_cw("CQ CQ DE W9XYZ");
                assert!(!e.cw_queue.is_empty(), "{kind:?}: baseline queued");
            }
        }
    }

    /// The halt's fingerprint for each kind: what the radio loop reads on its next pass.
    fn stopped(e: &mut Engine, kind: Kind) {
        assert!(!e.tx_enabled(), "{kind:?}: TX still armed");
        assert!(e.take_slot_tx_abort(), "{kind:?}: no immediate kill armed");
        match kind {
            Kind::Ft => {
                let slot = if e.tx_even() { 0 } else { 1 };
                assert!(e.plan_tx(slot).is_none(), "{kind:?}: an over still planned");
            }
            Kind::PhonePtt => assert!(!e.manual_ptt(), "{kind:?}: the mic key survived"),
            Kind::Rtty => {
                assert!(!e.rtty_latched(), "{kind:?}: the latch survived");
                assert!(e.take_rtty_abort(), "{kind:?}: no abort armed");
            }
            Kind::Psk => {
                assert!(!e.psk_latched(), "{kind:?}: the latch survived");
                assert!(e.take_psk_abort(), "{kind:?}: no abort armed");
            }
            Kind::Tune => assert!(!e.tuning(), "{kind:?}: the carrier survived"),
            Kind::VoiceKeyer => assert!(e.voice_tx.is_none(), "{kind:?}: the message survived"),
            Kind::Cw => {
                assert!(e.cw_queue.is_empty(), "{kind:?}: the macro survived");
                assert!(e.take_cw_abort(), "{kind:?}: no abort armed");
            }
        }
    }

    /// ★ S8: while a stream is attached, EVERY transmission stops within the presence deadline
    /// of the heartbeats stopping, on the radio loop's own poll, from the station's own clock.
    /// The transmission starts AFTER presence is held — a streamed operator's click is a local
    /// arm — and the timing is shown: up at 4.999 s, down at 5.000 s.
    #[test]
    fn a_lapsed_presence_stops_every_kind_of_transmission() {
        for kind in KINDS {
            let (mut e, _transmit, _presence, t0) = attached();
            start(&mut e, kind);
            assert!(
                !e.poll_remote_transmit(t0 + PRESENCE - Duration::from_millis(1)),
                "{kind:?}: halted before the deadline"
            );
            assert!(e.tx_enabled(), "{kind:?}: disarmed before the deadline");
            assert!(
                e.poll_remote_transmit(t0 + PRESENCE),
                "{kind:?}: not halted at the deadline"
            );
            stopped(&mut e, kind);
        }
    }

    /// The spec's gap, pinned: a local arm (`set_tx_enabled(true)`) clears the FT permit, and a
    /// streamed operator's every click is a local arm. Presence must survive it.
    #[test]
    fn a_local_arm_keeps_the_sessions_presence() {
        let (mut e, _transmit, _presence, t0) = attached();
        e.set_tx_enabled(true);
        e.start_cq(None).unwrap();
        assert!(e.remote_presence_live(t0), "a local arm dropped presence");
        assert!(
            e.poll_remote_transmit(t0 + PRESENCE),
            "the lapse did not halt"
        );
        assert!(!e.tx_enabled());
    }

    /// A Stop at the station (the desktop's Stop TX, WSJT-X's HaltTx, the stream's own Stop) ends
    /// the transmission, not the session: presence stays, so the NEXT transmission is still
    /// guarded, with no window between the stop and the next heartbeat.
    #[test]
    fn a_local_stop_ends_the_transmission_not_the_presence() {
        let (mut e, _transmit, _presence, t0) = attached();
        start(&mut e, Kind::Ft);
        e.halt_tx();
        // The Stop stops at once, not at the presence deadline: nothing waits on presence.
        stopped(&mut e, Kind::Ft);
        assert!(e.remote_presence_live(t0), "a stop ended presence");
        // The operator starts again, and the heartbeats stop. A stop holds TX off until the
        // operator's own act: in Phone the first PTT press turns it back on.
        e.set_tx_enabled_by_operator(true);
        start(&mut e, Kind::PhonePtt);
        assert!(e.poll_remote_transmit(t0 + PRESENCE));
        stopped(&mut e, Kind::PhonePtt);
    }

    /// ★ A local arm in the middle of the session (Call CQ's own arm, the
    /// one every streamed click is) leaves EVERY kind of transmission covered. For each kind:
    /// presence is attached, the operator arms locally a second in, starts the transmission, and
    /// the heartbeats stop; the station halts at the deadline, up at 4.999 s and down at 5.000 s.
    /// The plan's shared slot failed exactly here: `set_tx_enabled(true)` clears the FT permit.
    #[test]
    fn a_local_arm_mid_session_leaves_every_kind_covered() {
        for kind in KINDS {
            let (mut e, _transmit, _presence, t0) = attached();
            // A second in, the local arm (what Call CQ, and every streamed click, performs).
            e.set_tx_enabled(true);
            assert!(e.remote_presence_live(t0 + Duration::from_secs(1)));
            start(&mut e, kind);
            assert!(
                !e.poll_remote_transmit(t0 + PRESENCE - Duration::from_millis(1)),
                "{kind:?}: halted before the deadline"
            );
            assert!(
                e.poll_remote_transmit(t0 + PRESENCE),
                "{kind:?}: a local arm left the transmission uncovered"
            );
            stopped(&mut e, kind);
        }
    }

    /// A fresh heartbeat renews presence; one that does not come lets it lapse.
    #[test]
    fn a_renewal_extends_presence_and_its_absence_lapses_it() {
        let (mut e, _transmit, presence, t0) = attached();
        start(&mut e, Kind::PhonePtt);
        let t1 = t0 + Duration::from_secs(4);
        assert!(e.hold_remote_presence(presence.permit(t1 + PRESENCE).unwrap(), t1));
        assert!(
            !e.poll_remote_transmit(t0 + PRESENCE),
            "halted inside the renewal"
        );
        assert!(e.manual_ptt());
        assert!(
            e.poll_remote_transmit(t1 + PRESENCE),
            "the renewal never lapsed"
        );
        stopped(&mut e, Kind::PhonePtt);
    }

    /// The station ends the session (lease released or lost, device revoked, Remote off): its
    /// presence authority is revoked, and the next poll halts — no waiting for the deadline.
    #[test]
    fn revoking_presence_halts_on_the_next_poll() {
        let (mut e, _transmit, presence, t0) = attached();
        start(&mut e, Kind::Tune);
        presence.revoke();
        assert!(e.poll_remote_transmit(t0 + Duration::from_millis(20)));
        stopped(&mut e, Kind::Tune);
    }

    /// An FT over planned while presence was live is not committed once presence is revoked:
    /// commit runs the same poll, so the radio loop never keys what the session's end stopped.
    #[test]
    fn a_revoked_presence_cannot_commit_a_previously_planned_ft_over() {
        let (mut e, _transmit, presence, t0) = attached();
        start(&mut e, Kind::Ft);
        let slot = if e.tx_even() { 0 } else { 1 };
        let plan = e
            .plan_tx(slot)
            .expect("positive control: native CQ plans an over");
        presence.revoke();
        assert!(e.commit_tx(&plan, vec![0.0; 120], slot).is_empty());
        stopped(&mut e, Kind::Ft);
        assert!(!e.remote_presence_live(t0));
    }

    /// ⚠️ A lapse is never erased by a renewal that arrives before the poll: the lapse happened,
    /// and the stop it owes is carried out first. (Blind for five seconds, then the picture comes
    /// back a tick before the radio loop looks.)
    #[test]
    fn a_renewal_after_a_lapse_does_not_erase_the_stop() {
        let (mut e, _transmit, presence, t0) = attached();
        start(&mut e, Kind::PhonePtt);
        let late = t0 + PRESENCE + Duration::from_millis(5);
        assert!(e.hold_remote_presence(presence.permit(late + PRESENCE).unwrap(), late));
        stopped(&mut e, Kind::PhonePtt);
        assert!(
            e.remote_presence_live(late),
            "the fresh presence was not installed"
        );
    }

    /// Presence only ever stops. Holding it arms nothing, keys nothing, and a station with no
    /// stream attached is untouched by any of this.
    #[test]
    fn presence_grants_nothing() {
        let (mut e, _transmit, presence, t0) = attached();
        assert!(!e.tx_enabled(), "holding presence armed TX");
        assert!(!e.manual_ptt());
        // S8 (b): renewing it, again and again, arms nothing, keys nothing and starts nothing.
        for s in 1..=4u64 {
            let at = t0 + Duration::from_secs(s);
            assert!(e.hold_remote_presence(presence.permit(at + PRESENCE).unwrap(), at));
            assert!(!e.poll_remote_transmit(at));
        }
        assert!(!e.tx_enabled(), "a renewal armed TX");
        assert!(!e.manual_ptt(), "a renewal keyed");
        assert!(!e.tuning() && !e.rtty_latched() && !e.psk_latched());
        assert!(e.voice_tx.is_none() && e.cw_queue.is_empty());
        // Nor does a live presence let a refused key through: with TX off, the PTT verb refuses.
        // (Switching to Phone arms TX, as a local click does; the operator then turns it off.)
        e.set_operating_mode("phone", false);
        e.set_tx_enabled(false);
        e.set_ptt(true);
        assert!(
            !e.manual_ptt(),
            "presence keyed a transmitter that TX-off refuses"
        );
        let mut native = Engine::new("W9XYZ", "EN52", 0);
        start(&mut native, Kind::PhonePtt);
        assert!(!native.poll_remote_transmit(t0 + Duration::from_secs(3600)));
        assert!(native.manual_ptt(), "a station with no stream was halted");
    }

    // ----- S7: the held PTT, decided on the radio loop's poll. Under M1 a press ARMS the
    // microphone over and the page's audio keys it (`engine/remote_mic.rs` tests the keying);
    // these pin what the press itself does. -----

    const PRESS: &str = "10000000-0000-4000-8000-00000000000a";

    fn phone(e: &mut Engine) -> PttHold {
        e.set_operating_mode("phone", false);
        e.remote_ptt_hold.clone().unwrap()
    }

    /// ★ A6: no hold for 200 ms and the over ends; re-asserted every 100 ms, it does not.
    #[test]
    fn a_held_ptt_arms_while_reasserted_and_ends_200_ms_after_the_last_hold() {
        let (mut e, _transmit, presence, t0) = attached();
        let hold = phone(&mut e);
        for i in 0..20u64 {
            let at = t0 + Duration::from_millis(i * 100);
            // Presence renewed by the page's heartbeats, once a second.
            if i % 10 == 0 {
                e.hold_remote_presence(presence.permit(at + PRESENCE).unwrap(), at);
            }
            hold.hold(PRESS, at);
            for tick in 0..5u64 {
                e.poll_remote_transmit(at + Duration::from_millis(tick * 20));
                assert!(e.mic_armed(), "dropped at {} ms", i * 100 + tick * 20);
            }
        }
        // M1: the press arms the microphone and never the desktop's own PTT.
        assert!(!e.manual_ptt(), "the hold keyed the desktop's PTT");
        let last = t0 + Duration::from_millis(1900);
        e.poll_remote_transmit(last + Duration::from_millis(199));
        assert!(e.mic_armed(), "released inside the gap");
        e.poll_remote_transmit(last + Duration::from_millis(200));
        assert!(!e.mic_armed(), "the gap did not end the over");
        // The over came down through its own release, not a halt: Phone stays armed for the next
        // press.
        assert!(e.tx_enabled());
    }

    /// Blind means no authority: with no live presence a hold is refused, and it stays refused.
    #[test]
    fn a_hold_without_presence_is_refused() {
        let (mut e, _transmit, presence, t0) = attached();
        let hold = phone(&mut e);
        presence.revoke();
        e.poll_remote_transmit(t0);
        // The lapse halted, and halting disarms. Arm again, as the operator would, so that the
        // ONLY thing left to refuse the key is the missing presence (without this line the PTT
        // verb refused on TX-off and a mutation removing the presence check stayed green).
        e.set_tx_enabled(true);
        hold.hold(PRESS, t0);
        e.poll_remote_transmit(t0 + Duration::from_millis(20));
        assert!(!e.mic_armed(), "armed with no presence");
        assert_eq!(
            hold.reports().last().unwrap().end,
            Some(crate::remote_control::ptt_hold::HoldEnd::Refused)
        );
    }

    /// The held PTT is the Phone cockpit's PTT: in any other mode it is refused.
    #[test]
    fn a_hold_outside_phone_is_refused() {
        let (mut e, _transmit, _presence, t0) = attached();
        let hold = e.remote_ptt_hold.clone().unwrap();
        e.set_tier(Tier::Ft8);
        e.set_tx_enabled(true);
        hold.hold(PRESS, t0);
        e.poll_remote_transmit(t0 + Duration::from_millis(20));
        assert!(!e.mic_armed(), "a held PTT armed outside Phone");
    }

    /// The existing transmit gates still apply to the arm: TX off refuses the press.
    #[test]
    fn a_hold_with_tx_off_is_refused() {
        let (mut e, _transmit, _presence, t0) = attached();
        let hold = phone(&mut e);
        e.set_tx_enabled(false);
        hold.hold(PRESS, t0);
        e.poll_remote_transmit(t0 + Duration::from_millis(20));
        assert!(!e.mic_armed(), "armed with TX off");
    }

    /// A Stop at the shack while the held PTT is armed: the over ends and the same press does not
    /// arm it again.
    #[test]
    fn a_stop_ends_a_held_ptt_for_good() {
        let (mut e, _transmit, _presence, t0) = attached();
        let hold = phone(&mut e);
        hold.hold(PRESS, t0);
        e.poll_remote_transmit(t0);
        assert!(e.mic_armed());
        e.halt_tx();
        assert!(!e.mic_armed(), "the stop left the over armed");
        e.set_tx_enabled(true);
        hold.hold(PRESS, t0 + Duration::from_millis(100));
        e.poll_remote_transmit(t0 + Duration::from_millis(110));
        assert!(!e.mic_armed(), "the press re-armed after a stop");
    }

    /// ⛔ A KEYING PRESS STILL QUEUED FOR THE RADIO LOOP DOES NOT OUTLIVE A STAND-DOWN. The rig's
    /// ATU tune-up and a voice-memory PLAY wait for the loop's next poll; a Stop TX, a presence
    /// lapse, or the stand-down moving on its own (Remote's Stop on the link, a lease lost) taken
    /// before that poll drops them, so re-arming TX at once keys nothing. The halt drops them by
    /// value, and the drain refuses any pressed under an earlier stand-down. A voice-memory STOP
    /// queued is never dropped: it stops a transmission. CONTROL: with TX armed again, every gate
    /// the press asks passes, and a press made after the stand-down fires.
    #[test]
    fn a_stand_down_drops_a_queued_atu_press_and_a_voice_memory_play() {
        use crate::engine::VoiceMemCmd;
        for end in ["stop", "presence", "stand-down"] {
            let (mut e, transmit, presence, t0) = attached();
            e.set_license_class("extra");
            e.set_frequency(14.290, "20m", "USB");
            e.set_operating_mode("phone", false);
            e.observe_rig_tuner(Some(true), true);
            let mut at = t0;
            let mut end_it = |e: &mut Engine| match end {
                "stop" => e.halt_tx(),
                "presence" => {
                    assert!(e.hold_remote_presence(presence.permit(at + PRESENCE).unwrap(), at));
                    at += PRESENCE + Duration::from_secs(1);
                    assert!(e.poll_remote_transmit(at), "{end}: the lapse halts");
                }
                _ => transmit.revoke(),
            };
            e.atu_tune()
                .expect("scene guard: the rig may run its tuner");
            assert!(e.request_voice_mem(1));
            end_it(&mut e);
            if end != "stand-down" {
                assert!(
                    e.pending_atu_tune.is_none(),
                    "{end}: the halt drops the ATU press"
                );
                assert_eq!(e.pending_voice_mem, None, "{end}: the halt drops the PLAY");
            }
            e.set_tx_enabled(true);
            assert!(
                e.atu_tune_gate().is_ok(),
                "{end}: scene guard, every gate passes"
            );
            assert!(
                !e.take_atu_tune(),
                "{end}: the ATU press keyed after the stand-down"
            );
            assert_eq!(
                e.take_voice_mem(),
                None,
                "{end}: the PLAY keyed after the stand-down"
            );
            e.request_voice_mem_stop();
            end_it(&mut e);
            assert_eq!(
                e.take_voice_mem(),
                Some(VoiceMemCmd::Stop),
                "{end}: a STOP still goes out"
            );
            e.set_tx_enabled(true);
            e.atu_tune().expect("CONTROL: a press after the stand-down");
            assert!(e.take_atu_tune(), "{end}: CONTROL, it fires");
            assert!(e.request_voice_mem(2));
            assert_eq!(
                e.take_voice_mem(),
                Some(VoiceMemCmd::Play(2)),
                "{end}: CONTROL"
            );
        }
    }

    /// What each [`OnItsOwn`] is, read off the station: APRS's automatic ack allowed, the presence
    /// beacon on, the chat CQ run on.
    fn on_its_own(e: &Engine) -> [bool; 3] {
        [
            e.aprs_arm_source() == AprsArm::Explicit,
            e.beacon_enabled(),
            e.chat_cq_state() != "off",
        ]
    }

    /// Turn each [`OnItsOwn`] on as its handler does, bound to `standing`.
    fn turn_on(e: &mut Engine, standing: Option<Standing>) {
        e.set_aprs_arm(AprsArm::Explicit);
        e.bind_to_presence(OnItsOwn::AprsAck, standing.clone());
        e.set_beacon(true);
        e.bind_to_presence(OnItsOwn::Beacon, standing.clone());
        e.set_chat_cq(true).expect("the CQ run starts");
        e.bind_to_presence(OnItsOwn::ChatCq, standing);
        assert_eq!(on_its_own(e), [true; 3], "CONTROL: each is on");
    }

    /// ⛔ What a computer driving the station turned on that transmits by itself (APRS's automatic
    /// ack, the presence beacon, the chat CQ run) ends when that computer's presence lapses, on the
    /// radio loop's tick, and when another session's presence takes its place. The decoder listens
    /// on, receive only. The session's own renewed heartbeat keeps each. CONTROL: the same three
    /// turned on at the shack outlive the lapse, and once ended none keys with TX armed again.
    #[test]
    fn what_transmits_by_itself_ends_with_the_presence_it_was_turned_on_under() {
        let (mut e, _transmit, presence, t0) = attached();
        let standing = e.remote_presence_standing(t0);
        assert!(standing.is_some(), "premise: presence is live");
        turn_on(&mut e, standing);
        assert!(!e.poll_remote_transmit(t0 + Duration::from_secs(1)));
        assert_eq!(on_its_own(&e), [true; 3], "a tick leaves each on");
        let renewed = t0 + Duration::from_secs(4);
        assert!(e.hold_remote_presence(presence.permit(renewed + PRESENCE).unwrap(), renewed));
        assert!(!e.poll_remote_transmit(renewed + Duration::from_secs(1)));
        assert_eq!(on_its_own(&e), [true; 3], "its own heartbeat keeps each");
        assert!(e.poll_remote_transmit(renewed + PRESENCE));
        assert_eq!(on_its_own(&e), [false; 3], "the lapse ended each");
        assert_eq!(e.aprs_arm_source(), AprsArm::Auto, "the decoder listens on");
        // Nothing keys with TX armed again: no CQ, no beacon, no ack.
        e.set_tx_enabled(true);
        for slot in 0..16 {
            assert!(e.poll_tx(slot).is_empty(), "keyed at {slot}");
        }
        e.aprs_auto_ack("N0CALL-7", "W9XYZ", "042");
        assert!(e.poll_aprs_tx().is_none(), "an ack keyed");
        e.set_tx_enabled(false);

        // Another session's presence takes the place of the one they were turned on under.
        let t1 = renewed + Duration::from_secs(10);
        assert!(e.hold_remote_presence(presence.permit(t1 + PRESENCE).unwrap(), t1));
        let standing = e.remote_presence_standing(t1);
        turn_on(&mut e, standing);
        let other = TransmitAuthority::default();
        assert!(e.hold_remote_presence(other.permit(t1 + PRESENCE).unwrap(), t1));
        e.poll_remote_transmit(t1);
        assert_eq!(on_its_own(&e), [false; 3], "another's presence ended each");

        // CONTROL: turned on at the shack, each outlives a lapse.
        turn_on(&mut e, None);
        assert!(e.poll_remote_transmit(t1 + PRESENCE));
        assert_eq!(on_its_own(&e), [true; 3], "the shack's own stay on");
    }

    /// The ways what a computer driving the station queued or turned on ends.
    const ENDS: [&str; 3] = ["stop", "lapse", "revoked"];

    /// What a computer driving the station can leave waiting to key, or turned on to key by
    /// itself: an APRS beacon and message queued, an SSTV picture waiting for the loop, a chat
    /// message and a broadcast queued, the chat CQ run, the presence beacon, and APRS Monitor armed
    /// by hand (the automatic ack).
    const WAITING: [&str; 8] = [
        "aprs-beacon",
        "aprs-message",
        "sstv",
        "chat-message",
        "broadcast",
        "chat-cq",
        "beacon",
        "aprs-ack",
    ];

    /// Queue or turn on `what` as its handler does from afar, under `standing`, with TX on and
    /// nothing keyed yet.
    fn leave_waiting(e: &mut Engine, what: &str, standing: Option<Standing>) {
        if what == "sstv" {
            e.set_operating_mode("phone", false);
        } else if what != "aprs-beacon" && what != "aprs-message" && what != "aprs-ack" {
            e.set_tier(Tier::TempoFast);
            e.set_mode("chat").unwrap();
            e.hear_for_test("CQ W1AW FN31", -5, 99);
        }
        e.set_tx_enabled(true);
        let under = e.remote_transmit_stand_down_generation();
        e.note_started_from_afar(under);
        match what {
            "aprs-beacon" => e
                .aprs_beacon(41.9, -87.6, '/', '>', "FROM AFAR", &[])
                .unwrap(),
            "aprs-message" => e.aprs_send_message("W1AW", "HELLO FROM AFAR").unwrap(),
            "sstv" => e.sstv_send(vec![0.1; 1200], "Robot 36".into()).unwrap(),
            "chat-message" => {
                let id = e.send_message("W1AW", "HELLO FROM AFAR").unwrap();
                let peer = "W1AW".to_string();
                e.bind_to_presence(OnItsOwn::ChatMessage { peer, id }, standing);
            }
            "broadcast" => e.broadcast("HELLO FROM AFAR"),
            "chat-cq" => {
                e.set_chat_cq(true).unwrap();
                e.bind_to_presence(OnItsOwn::ChatCq, standing);
            }
            "beacon" => {
                e.set_beacon(true);
                e.bind_to_presence(OnItsOwn::Beacon, standing);
            }
            "aprs-ack" => {
                e.set_aprs_arm(AprsArm::Explicit);
                e.bind_to_presence(OnItsOwn::AprsAck, standing);
            }
            _ => unreachable!(),
        }
    }

    /// What keys once TX is armed again, the radio loop asked as it asks: an APRS frame (an ack
    /// asked for first), an SSTV picture holding the transmitter, a chat frame in any of 40 slots.
    fn keys_again(e: &mut Engine) -> Vec<&'static str> {
        let mut keyed = Vec::new();
        e.set_tx_enabled(true);
        e.aprs_auto_ack("N0CALL-7", "W9XYZ", "042");
        if e.poll_aprs_tx().is_some() {
            keyed.push("aprs");
        }
        if e.tx_owner() == Some(crate::engine::TxOwner::Sstv) {
            keyed.push("sstv");
        }
        e.set_operating_mode("digital", false);
        e.set_tx_enabled(true);
        if (100..140).any(|slot| !e.poll_tx(slot).is_empty()) {
            keyed.push("chat");
        }
        keyed
    }

    /// ⛔ NOTHING A COMPUTER DRIVING THE STATION LEFT WAITING KEYS AFTER IT IS STOPPED. For each
    /// thing queued or turned on from afar (an APRS beacon or message, an SSTV picture, a chat
    /// message or broadcast, the chat CQ run, the presence beacon, the APRS automatic ack), before
    /// it keys: a Stop (`halt_tx`, every Stop's verb), that computer's presence lapsing, or the
    /// transmit stand-down revoked with no halt (its permission withdrawn) each drop it, and once
    /// TX is armed again nothing keys. CONTROL: with no end, each keys (or is still on).
    #[test]
    fn nothing_left_waiting_from_afar_keys_after_a_stop_a_lapse_or_a_revoked_stand_down() {
        let mut keyed_after = Vec::new();
        for what in WAITING {
            // CONTROL: left alone, it keys.
            let (mut e, _transmit, _presence, t0) = attached();
            let standing = e.remote_presence_standing(t0);
            leave_waiting(&mut e, what, standing);
            assert!(!e.poll_remote_transmit(t0 + Duration::from_millis(10)));
            assert!(
                !keys_again(&mut e).is_empty(),
                "CONTROL {what}: left alone, it keys"
            );
            for end in ENDS {
                let (mut e, transmit, _presence, t0) = attached();
                let standing = e.remote_presence_standing(t0);
                leave_waiting(&mut e, what, standing);
                let now = match end {
                    "stop" => {
                        e.halt_tx();
                        t0 + Duration::from_millis(10)
                    }
                    "lapse" => t0 + PRESENCE,
                    _ => {
                        transmit.revoke();
                        t0 + Duration::from_millis(10)
                    }
                };
                e.poll_remote_transmit(now);
                let keyed = keys_again(&mut e);
                if !keyed.is_empty() || on_its_own(&e).contains(&true) {
                    keyed_after.push(format!(
                        "{what}, ended by {end}: keyed {keyed:?}, on {:?}",
                        on_its_own(&e)
                    ));
                }
            }
        }
        assert!(keyed_after.is_empty(), "{keyed_after:#?}");
    }

    /// ⛔ NO SETTINGS SAVE MOVES A SWITCH THAT TRANSMITS BY ITSELF. The presence beacon and JS8's
    /// automatic replies, relay and heartbeat acks each have ONE writer, the switch the operator
    /// presses (`set_beacon`, `js8_arm`), which from afar runs only under the start's rules. A whole
    /// save (the settings form at the shack, or the same save from a computer driving the station)
    /// carries each as a stale snapshot, and moves none in either direction. CONTROL: a field
    /// beside them in the same save is taken.
    #[test]
    fn a_settings_save_turns_on_nothing_that_transmits_by_itself() {
        use crate::engine::js8::Js8Switch;
        let switches = |e: &Engine| {
            let s = e.settings();
            [s.beacon, s.js8_autoreply, s.js8_relay, s.js8_hb_ack]
        };
        let mut e = Engine::new("W9XYZ", "EN52", 0);
        for on in [false, true] {
            e.set_beacon(on);
            for which in [Js8Switch::Autoreply, Js8Switch::Relay, Js8Switch::HbAck] {
                e.js8_arm(which, on).unwrap();
            }
            assert_eq!(switches(&e), [on; 4], "premise");
            let mut save = e.settings().clone();
            save.beacon = !on;
            save.js8_autoreply = !on;
            save.js8_relay = !on;
            save.js8_hb_ack = !on;
            save.cq_max_calls = Some(if on { 7 } else { 9 });
            e.apply_settings(save);
            assert_eq!(switches(&e), [on; 4], "a save moved one, from {on}");
            assert_eq!(
                e.settings().cq_max_calls,
                Some(if on { 7 } else { 9 }),
                "CONTROL: the save was taken"
            );
        }
    }

    /// ⛔ An unkey from afar halts nothing a start from afar began; only the stand-down does. TX On
    /// and the tune carrier from afar on Phone, then the PTT let go and Tune off from afar, each
    /// moving the starts-voided generation as the command lane moves it for an unkey: tick after
    /// tick TX stays on. CONTROL: the stand-down revoked with no halt then stops everything on the
    /// next tick, and with neither moved the same presses leave TX on, as at the shack.
    #[test]
    fn an_unkey_from_afar_leaves_tx_armed_and_only_the_stand_down_halts() {
        let mut e = Engine::new("W9XYZ", "EN52", 0);
        let transmit = TransmitAuthority::default();
        e.set_remote_transmit_revocation(transmit.revocation());
        e.set_operating_mode_with_arming("phone", false, false);
        assert!(
            !e.tx_enabled(),
            "premise: a Phone entry from afar arms nothing"
        );
        let under = e.remote_transmit_stand_down_generation();
        e.set_tx_enabled_by_operator(true);
        e.note_started_from_afar(under);
        e.set_tune(true);
        e.note_started_from_afar(under);
        assert!(
            e.tx_enabled() && e.tuning(),
            "premise: TX on, the carrier up"
        );
        let now = Instant::now();
        e.remote_starts_voided().revoke();
        e.set_ptt(false);
        assert!(
            !e.poll_remote_transmit(now),
            "a PTT let go from afar halted"
        );
        assert!(e.tx_enabled(), "a PTT let go from afar turned TX off");
        assert!(e.tuning(), "a PTT let go from afar dropped the carrier");
        e.remote_starts_voided().revoke();
        e.set_tune(false);
        for tick in 1..=3 {
            let later = now + Duration::from_millis(50 * tick);
            assert!(!e.poll_remote_transmit(later), "Tune off from afar halted");
        }
        assert!(e.tx_enabled(), "Tune off from afar turned TX off");
        transmit.revocation().revoke();
        assert!(
            e.poll_remote_transmit(now),
            "a revoked stand-down halted nothing"
        );
        assert!(!e.tx_enabled(), "a revoked stand-down left TX on");
    }
}
