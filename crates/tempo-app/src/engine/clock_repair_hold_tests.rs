//! ⛔ A CLOCK REPAIR HOLDS TRANSMIT (the operator's ruling, 2026-10-06: "Yes, hold TX until it
//! finishes"). From the press of Repair clock until the elevated helper exits, nothing STARTS
//! transmitting, whichever way it would start; the hold itself keys, unkeys and disarms nothing;
//! and it lets go at its bound even if the repair never ends. `tempo_audio`'s clock repair raises
//! and ends it ([`Engine::hold_tx_for_clock_repair`]); these drive the engine's own gates.
//!
//! Every start is tried twice, held and then not, so a refusal here is the hold's and not some
//! other gate's: the second try is the control.

use super::*;
use std::time::{Duration, Instant};

/// A bound no test outlives.
fn held_until() -> Instant {
    Instant::now() + Duration::from_secs(600)
}

/// An FT8 station with its identity set, TX off.
fn ft8() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    e
}

/// The slot an armed FT8 run keys in.
fn tx_slot(e: &Engine) -> u64 {
    if e.tx_even() {
        0
    } else {
        1
    }
}

/// A station in `section`, armed as entering a manual section arms it.
fn station_in(section: &str) -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_operating_mode(section, false);
    assert!(e.tx_enabled(), "premise: entering {section} arms TX");
    e
}

/// What says whether anything transmits, or could on its own.
fn transmit_state(e: &Engine) -> (bool, Option<TxOwner>, bool, bool, bool) {
    (
        e.tx_enabled(),
        e.tx_owner(),
        e.manual_ptt(),
        e.tuning(),
        e.slot_tx_abort,
    )
}

/// ★ TX On is refused while held, and the snapshot says a repair holds transmit (the status
/// lane's line). Once the repair has ended, TX On arms.
#[test]
fn tx_on_is_refused_while_held_and_arms_after() {
    let mut e = ft8();
    assert!(!e.tx_enabled(), "premise: TX off");
    e.hold_tx_for_clock_repair(held_until());
    e.set_tx_enabled(true);
    assert!(!e.tx_enabled(), "TX On while a clock repair runs");
    assert!(e.snapshot().radio.clock_repair_tx_held);

    e.end_clock_repair_hold();
    assert!(!e.snapshot().radio.clock_repair_tx_held);
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "control: TX On once the repair has ended");
}

/// ★ An FT8 run armed before the press plans no over while held, and is still armed for the
/// overs after it: the hold refuses a start and disarms nothing.
#[test]
fn an_armed_run_plans_no_over_while_held_and_stays_armed() {
    let mut e = ft8();
    e.start_cq(None).unwrap();
    let slot = tx_slot(&e);
    e.hold_tx_for_clock_repair(held_until());
    assert!(
        e.plan_tx(slot).is_none(),
        "an FT8 over while a clock repair runs"
    );
    assert!(e.tx_enabled(), "the run is still armed");
    assert_eq!(e.tx_owner(), None, "and nothing keyed");

    e.end_clock_repair_hold();
    assert!(
        e.plan_tx(slot).is_some(),
        "control: the run's next over plans once the repair has ended"
    );
}

/// An over planned before the hold began does not key: the hold moves the generation, as every
/// change to what may transmit does ([`Engine::commit_tx`]). The same over with no hold keys.
#[test]
fn an_over_planned_before_the_hold_does_not_key() {
    for hold in [true, false] {
        let mut e = ft8();
        e.start_cq(None).unwrap();
        let slot = tx_slot(&e);
        let plan = e.plan_tx(slot).expect("premise: an over planned");
        if hold {
            e.hold_tx_for_clock_repair(held_until());
        }
        assert_eq!(
            e.commit_tx(&plan, vec![0.0; 120], slot).is_empty(),
            hold,
            "held: {hold}"
        );
    }
}

/// ★ Tune is refused while held. A carrier already up when a hold begins stays up, and its
/// release is honoured: the hold never unkeys. (The press is refused while a carrier is up, so
/// this is the gate's own promise, not a state the press can reach.)
#[test]
fn tune_is_refused_while_held_and_a_carrier_already_up_stays_up() {
    let mut e = ft8();
    e.hold_tx_for_clock_repair(held_until());
    e.set_tune(true);
    assert!(!e.tuning(), "Tune while a clock repair runs");

    e.end_clock_repair_hold();
    e.set_tune(true);
    assert!(e.tuning(), "control: Tune once the repair has ended");

    e.hold_tx_for_clock_repair(held_until());
    assert!(e.tuning(), "the hold drops no carrier");
    e.set_tune(true);
    assert!(e.tuning(), "…not even on a second press");
    e.set_tune(false);
    assert!(!e.tuning(), "and Tune's release is honoured");
}

/// ★ PTT is refused while held. A key already held stays held, and its release is honoured.
#[test]
fn ptt_is_refused_while_held_and_a_held_key_stays() {
    let mut e = station_in("phone");
    e.hold_tx_for_clock_repair(held_until());
    e.set_ptt(true);
    assert!(!e.manual_ptt(), "PTT while a clock repair runs");

    e.end_clock_repair_hold();
    e.set_ptt(true);
    assert!(e.manual_ptt(), "control: PTT once the repair has ended");

    e.hold_tx_for_clock_repair(held_until());
    assert!(e.manual_ptt(), "the hold drops no key");
    e.set_ptt(false);
    assert!(!e.manual_ptt(), "and the release is honoured");
}

/// A CAT client's key (the broker's `T 1`) and its voice-memory playback, which keys the rig,
/// are refused while held, and nothing is queued for the radio. A stop is always relayed.
#[test]
fn a_cat_clients_key_and_voice_memory_are_refused_while_held() {
    let mut e = station_in("phone");
    e.settings.cat_broker_ptt = true;
    e.hold_tx_for_clock_repair(held_until());
    assert!(!e.broker_ptt(true), "a CAT client's key");
    assert!(!e.request_voice_mem(1), "a CAT client's voice memory");
    assert_eq!(e.take_voice_mem(), None, "nothing queued for the radio");
    e.request_voice_mem_stop();
    assert_eq!(e.take_voice_mem(), Some(VoiceMemCmd::Stop), "a stop goes");

    e.end_clock_repair_hold();
    assert!(e.broker_ptt(true), "control: the key");
    assert!(e.broker_ptt(false));
    assert!(e.request_voice_mem(1), "control: the voice memory");
    assert_eq!(e.take_voice_mem(), Some(VoiceMemCmd::Play(1)));
}

/// The Remote microphone (a streamed operator's PTT) arms no over while held.
#[test]
fn the_remote_microphone_arms_no_over_while_held() {
    let mut e = station_in("phone");
    e.set_remote_mic_feed(crate::mic::MicFeed::default());
    let presence = crate::remote_control::transmit::TransmitAuthority::default();
    let t0 = Instant::now();
    assert!(e.hold_remote_presence(presence.permit(t0 + Duration::from_secs(5)).unwrap(), t0));
    e.hold_tx_for_clock_repair(held_until());
    assert!(
        !e.arm_remote_mic(t0),
        "a streamed PTT while a clock repair runs"
    );
    assert_eq!(e.tx_owner(), None);

    e.end_clock_repair_hold();
    assert!(
        e.arm_remote_mic(t0),
        "control: it arms once the repair has ended"
    );
}

/// ★ Every send with an up-front answer is refused while held, with the hold's own words, and
/// nothing is queued or keyed. Each works once the repair has ended (the control).
#[test]
fn every_send_is_refused_up_front_while_held() {
    type Send = fn(&mut Engine) -> Result<(), String>;
    let sends: [(&str, &str, Send); 8] = [
        ("voice message", "phone", |e| {
            e.send_voice(vec![0.0; 12_000])
        }),
        ("SSTV picture", "phone", |e| {
            e.sstv_send(vec![0.0; 12_000], "Robot 36".into())
        }),
        ("APRS beacon", "phone", |e| {
            e.aprs_beacon(43.0, -89.4, '/', '>', "", &[])
        }),
        ("ATU tune-up", "phone", |e| {
            e.observe_rig_tuner(Some(false), true);
            e.atu_tune()
        }),
        ("RTTY send", "rtty", |e| e.rtty_send_text("CQ TEST")),
        ("RTTY continuous TX", "rtty", |e| e.set_rtty_latched(true)),
        ("PSK send", "keyboard", |e| e.psk_send_text("CQ TEST")),
        ("PSK continuous TX", "keyboard", |e| e.set_psk_latched(true)),
    ];
    for (what, section, send) in sends {
        let mut e = station_in(section);
        e.hold_tx_for_clock_repair(held_until());
        assert_eq!(
            send(&mut e),
            Err(CLOCK_REPAIR_HOLDS_TX.to_string()),
            "{what} while a clock repair runs"
        );
        assert_eq!(e.tx_owner(), None, "{what}: nothing queued or keyed");

        e.end_clock_repair_hold();
        assert_eq!(
            send(&mut e),
            Ok(()),
            "control: {what} once the repair has ended"
        );
    }
}

/// A CW send has no up-front answer: refused while held, it says so on the CW cockpit's warning
/// line, queues nothing, and leaves the latch as it was (a CW send arms TX by itself; the hold
/// arms nothing).
#[test]
fn a_cw_send_is_refused_while_held_and_arms_nothing() {
    let mut e = station_in("cw");
    e.set_tx_enabled(false);
    e.hold_tx_for_clock_repair(held_until());
    e.send_cw("CQ TEST");
    assert_eq!(e.tx_owner(), None, "nothing queued");
    assert!(!e.tx_enabled(), "nothing armed");
    assert_eq!(e.cw_keyer_error.as_deref(), Some(CLOCK_REPAIR_DROPPED));

    e.end_clock_repair_hold();
    e.send_cw("CQ TEST");
    assert_eq!(
        e.tx_owner(),
        Some(TxOwner::Cw),
        "control: it goes once the repair has ended"
    );
}

/// ★ THE BACKSTOP AT THE KEYING POINT. What reaches a mode's poll while held is dropped with its
/// warning line: never keyed, and never held for later. The press is refused while any of these
/// is going out ([`Engine::on_air`]), so no message in progress meets it; what can is queued by
/// itself (a CW ID after a 73, an auto-sequencer's reply, an APRS auto-ack). Queued here before
/// the hold through the ordinary sends, and each control keys the same queue with no hold.
#[test]
fn a_queued_send_reaching_its_poll_while_held_is_dropped_with_its_warning() {
    for hold in [true, false] {
        let raise = |e: &mut Engine| {
            if hold {
                e.hold_tx_for_clock_repair(held_until());
            }
        };

        let mut e = station_in("cw");
        e.send_cw("CQ");
        raise(&mut e);
        assert_eq!(e.poll_cw_one().is_some(), !hold, "CW, held: {hold}");
        if hold {
            assert_eq!(e.cw_keyer_error.as_deref(), Some(CLOCK_REPAIR_DROPPED));
            assert_eq!(e.tx_owner(), None, "CW dropped, not held");
        }

        let mut e = station_in("rtty");
        e.rtty_send_text("CQ").unwrap();
        raise(&mut e);
        assert_eq!(e.poll_rtty_one().is_some(), !hold, "RTTY, held: {hold}");
        if hold {
            assert_eq!(e.rtty_keyer_error.as_deref(), Some(CLOCK_REPAIR_DROPPED));
            assert_eq!(e.tx_owner(), None, "RTTY dropped, not held");
        }

        let mut e = station_in("keyboard");
        e.psk_send_text("CQ").unwrap();
        raise(&mut e);
        assert_eq!(e.poll_psk_one().is_some(), !hold, "PSK, held: {hold}");
        if hold {
            assert_eq!(e.psk_keyer_error.as_deref(), Some(CLOCK_REPAIR_DROPPED));
            assert_eq!(e.tx_owner(), None, "PSK dropped, not held");
        }

        let mut e = station_in("phone");
        e.sstv_send(vec![0.0; 12_000], "Robot 36".into()).unwrap();
        raise(&mut e);
        assert_eq!(e.poll_sstv_tx().is_some(), !hold, "SSTV, held: {hold}");
        if hold {
            assert_eq!(e.sstv_tx_notice(), Some(CLOCK_REPAIR_DROPPED));
            assert_eq!(e.tx_owner(), None, "SSTV dropped, not held");
        }

        let mut e = station_in("phone");
        e.aprs_beacon(43.0, -89.4, '/', '>', "", &[]).unwrap();
        raise(&mut e);
        assert_eq!(e.poll_aprs_tx().is_some(), !hold, "APRS, held: {hold}");
        if hold {
            assert_eq!(e.aprs_tx_notice(), Some(CLOCK_REPAIR_DROPPED));
            assert!(e.aprs_tx_queue.is_empty(), "APRS dropped, not held");
        }

        let mut e = station_in("phone");
        e.send_voice(vec![0.0; 12_000]).unwrap();
        raise(&mut e);
        assert_eq!(e.poll_voice().is_some(), !hold, "voice, held: {hold}");
        if hold {
            assert_eq!(e.tx_owner(), None, "the voice message dropped, not held");
        }
    }
}

/// ★ The hold lets go at its bound, even if the repair never ends: past it, TX On arms. A bound
/// still ahead holds (the control).
#[test]
fn the_hold_lets_go_at_its_bound() {
    let mut e = ft8();
    e.hold_tx_for_clock_repair(Instant::now());
    assert!(!e.clock_repair_holds_tx());
    assert!(!e.snapshot().radio.clock_repair_tx_held);
    e.set_tx_enabled(true);
    assert!(
        e.tx_enabled(),
        "TX On past the bound, the repair never ended"
    );

    let mut c = ft8();
    c.hold_tx_for_clock_repair(held_until());
    c.set_tx_enabled(true);
    assert!(!c.tx_enabled(), "control: a bound still ahead holds");
}

/// ★ The hold itself keys nothing, arms nothing and disarms nothing: raised and ended on a
/// station, armed or not, it leaves every transmit state as it found it.
#[test]
fn raising_and_ending_the_hold_changes_no_transmit_state() {
    for armed in [false, true] {
        let mut e = station_in("phone");
        e.set_tx_enabled(armed);
        let before = transmit_state(&e);
        e.hold_tx_for_clock_repair(held_until());
        assert_eq!(transmit_state(&e), before, "raised, armed: {armed}");
        e.end_clock_repair_hold();
        assert_eq!(transmit_state(&e), before, "ended, armed: {armed}");
    }
}
