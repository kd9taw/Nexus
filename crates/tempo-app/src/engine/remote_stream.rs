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
//! ## The held PTT (S7)
//!
//! [`PttHold`] is the state; this is where it is keyed and released, through the engine's own PTT
//! verb, so every guard the desktop's PTT has still applies (TX enabled, inside the licence). A
//! remote key is admitted only while presence is live and in Phone, the one cockpit whose PTT it
//! stands for.
use super::Engine;
use crate::remote_control::ptt_hold::PttHold;
use crate::remote_control::transmit::TransmitPermit;
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
            // None, or a different session's: this one is the live authority now.
            self.remote_presence = Some(permit);
        }
        true
    }

    /// Whether a streamed session holds live presence.
    pub fn remote_presence_live(&self, now: Instant) -> bool {
        self.remote_presence.as_ref().is_some_and(|p| p.valid(now))
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
        tempo_core::applog::warn(
            "tx",
            "remote stream presence lapsed: stopping every transmission at the station",
        );
        self.halt_tx();
        true
    }

    /// Presence first, so a lapse has already stopped the key before the held PTT is decided.
    pub(super) fn poll_remote_stream(&mut self, now: Instant) -> bool {
        let halted = self.poll_remote_presence(now);
        if let Some(hold) = self.remote_ptt_hold.clone() {
            let may_key = self.remote_presence_live(now)
                && self.settings.operating_mode == OperatingMode::Phone;
            let key_up = self.manual_ptt;
            hold.tick(now, may_key, key_up, |on| {
                self.set_ptt(on);
                self.manual_ptt
            });
        }
        halted
    }
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
                e.send_voice(vec![0.25; 12_000]);
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
        assert!(e.remote_presence_live(t0), "a stop ended presence");
        let _ = e.take_slot_tx_abort();
        // The operator starts again, and the heartbeats stop.
        start(&mut e, Kind::PhonePtt);
        assert!(e.poll_remote_transmit(t0 + PRESENCE));
        stopped(&mut e, Kind::PhonePtt);
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
        let (e, _transmit, _presence, t0) = attached();
        assert!(!e.tx_enabled(), "holding presence armed TX");
        assert!(!e.manual_ptt());
        let mut native = Engine::new("W9XYZ", "EN52", 0);
        start(&mut native, Kind::PhonePtt);
        assert!(!native.poll_remote_transmit(t0 + Duration::from_secs(3600)));
        assert!(native.manual_ptt(), "a station with no stream was halted");
    }

    // ----- S7: the held PTT, keyed and released on the radio loop's poll -----

    const PRESS: &str = "10000000-0000-4000-8000-00000000000a";

    fn phone(e: &mut Engine) -> PttHold {
        e.set_operating_mode("phone", false);
        e.remote_ptt_hold.clone().unwrap()
    }

    /// ★ A6: no hold for 200 ms and the over ends; re-asserted every 100 ms, it does not.
    #[test]
    fn a_held_ptt_keys_while_reasserted_and_ends_200_ms_after_the_last_hold() {
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
                assert!(e.manual_ptt(), "dropped at {} ms", i * 100 + tick * 20);
            }
        }
        let last = t0 + Duration::from_millis(1900);
        e.poll_remote_transmit(last + Duration::from_millis(199));
        assert!(e.manual_ptt(), "released inside the gap");
        e.poll_remote_transmit(last + Duration::from_millis(200));
        assert!(!e.manual_ptt(), "the gap did not end the over");
        // The key came down through the PTT verb, not a halt: Phone stays armed for the next press.
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
        assert!(!e.manual_ptt(), "keyed with no presence");
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
        assert!(!e.manual_ptt(), "a held PTT keyed outside Phone");
    }

    /// Every existing guard on the PTT verb still applies: TX off refuses the key.
    #[test]
    fn a_hold_with_tx_off_is_refused_by_the_ptt_verb() {
        let (mut e, _transmit, _presence, t0) = attached();
        let hold = phone(&mut e);
        e.set_tx_enabled(false);
        hold.hold(PRESS, t0);
        e.poll_remote_transmit(t0 + Duration::from_millis(20));
        assert!(!e.manual_ptt(), "keyed with TX off");
    }

    /// A Stop at the shack while the held PTT is keyed: the key comes down and the same press
    /// does not re-key it.
    #[test]
    fn a_stop_ends_a_held_ptt_for_good() {
        let (mut e, _transmit, _presence, t0) = attached();
        let hold = phone(&mut e);
        hold.hold(PRESS, t0);
        e.poll_remote_transmit(t0);
        assert!(e.manual_ptt());
        e.halt_tx();
        e.set_tx_enabled(true);
        hold.hold(PRESS, t0 + Duration::from_millis(100));
        e.poll_remote_transmit(t0 + Duration::from_millis(110));
        assert!(!e.manual_ptt(), "the press re-keyed after a stop");
    }
}
