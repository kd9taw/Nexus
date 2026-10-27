//! ⛔ A SIGN-IN AUTO-GRANTED DEVICE HOLDS TRANSMIT (the 2026-10-23 sign-in decision). A device the
//! owner's account signed in and was auto-granted control to may connect, operate the receiver and
//! hold station control, but no transmission STARTS while the service holds it — no arm, key,
//! send or over, even under a latch that was already on. The hold itself keys, unkeys and disarms nothing: Stop and the off half of the TX-enable
//! latch are taken at once, held or not. The service decides the hold FAIL-CLOSED (a sign-in
//! controller is held unless positively waived) and sets it here
//! ([`Engine::set_remote_transmit_held`]); these drive the engine's own gates against the flag.
//!
//! This is ADDITIVE to the clock-repair hold ([`super::clock_repair_hold_tests`]) — never a
//! weakening of any transmit guard. Every start is tried twice, held and then not, so a refusal
//! here is the hold's and not some other gate's: the second try is the control.

use super::*;

/// An FT8 station with its identity set, TX off.
fn ft8() -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    e
}

/// A station in `section`, armed as entering a manual section arms it.
fn station_in(section: &str) -> Engine {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_operating_mode(section, false);
    assert!(e.tx_enabled(), "premise: entering {section} arms TX");
    e
}

/// ★ TX On is refused while the device is held, and arms once the hold is lifted.
#[test]
fn tx_on_is_refused_while_held_and_arms_after() {
    let mut e = ft8();
    assert!(!e.tx_enabled(), "premise: TX off");
    e.set_remote_transmit_held(true);
    e.set_tx_enabled(true);
    assert!(!e.tx_enabled(), "TX On while the sign-in hold stands");

    e.set_remote_transmit_held(false);
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "control: TX On once the hold is lifted");
}

/// ★ Entering a manual section does not arm its latch while held, and arms once the hold is off:
/// FT, Phone, CW and the keyboard modes all arm through `set_tx_enabled`, which this refuses.
#[test]
fn a_manual_section_does_not_arm_while_held() {
    for section in ["phone", "cw", "rtty", "keyboard"] {
        let mut e = Engine::new("KD9TAW", "EN52", 0);
        e.set_remote_transmit_held(true);
        e.set_operating_mode(section, false);
        assert!(!e.tx_enabled(), "{section}: the latch stays off while held");

        e.set_remote_transmit_held(false);
        e.set_operating_mode("qso-monitor", false);
        e.set_operating_mode(section, false);
        assert!(
            e.tx_enabled(),
            "control: {section} arms once the hold is off"
        );
    }
}

/// ★ An FT8 run cannot be started while held (Call CQ funnels through `set_tx_enabled`), and starts
/// once the hold is off.
#[test]
fn an_ft8_run_cannot_start_while_held() {
    let mut e = ft8();
    e.set_remote_transmit_held(true);
    e.start_cq(None).unwrap();
    assert!(!e.tx_enabled(), "no FT8 run arms while held");

    e.set_remote_transmit_held(false);
    e.start_cq(None).unwrap();
    assert!(e.tx_enabled(), "control: the run arms once the hold is off");
}

/// ★ Tune is refused while held — the one keying path that bypasses the TX-enable latch.
#[test]
fn tune_is_refused_while_held() {
    let mut e = ft8();
    e.set_remote_transmit_held(true);
    e.set_tune(true);
    assert!(!e.tuning(), "Tune while the sign-in hold stands");

    e.set_remote_transmit_held(false);
    e.set_tune(true);
    assert!(e.tuning(), "control: Tune once the hold is off");
}

/// ★ A CW send self-arms the latch, so it is refused by the hold's own gate in `send_cw`, queues
/// nothing, and leaves the latch off; it goes once the hold is off.
#[test]
fn a_cw_send_is_refused_while_held_and_arms_nothing() {
    let mut e = station_in("cw");
    e.set_tx_enabled(false);
    e.set_remote_transmit_held(true);
    e.send_cw("CQ TEST");
    assert_eq!(e.tx_owner(), None, "nothing queued");
    assert!(!e.tx_enabled(), "nothing armed");
    assert_eq!(e.cw_keyer_error.as_deref(), Some(REMOTE_HOLD_DROPPED));

    e.set_remote_transmit_held(false);
    e.send_cw("CQ TEST");
    assert_eq!(
        e.tx_owner(),
        Some(TxOwner::Cw),
        "control: it goes once the hold is off"
    );
}

/// ★ Voice-memory playback keys the rig without the latch, so the hold refuses the request and
/// queues nothing; it is accepted once the hold is off.
#[test]
fn voice_memory_is_refused_while_held() {
    let mut e = station_in("phone");
    e.set_remote_transmit_held(true);
    assert!(!e.request_voice_mem(1), "voice memory while held");
    assert_eq!(e.take_voice_mem(), None, "nothing queued for the radio");

    e.set_remote_transmit_held(false);
    assert!(
        e.request_voice_mem(1),
        "control: accepted once the hold is off"
    );
    assert_eq!(e.take_voice_mem(), Some(VoiceMemCmd::Play(1)));
}

/// ★ STOP IS NEVER HELD. Inside the window, an armed station's Stop (`halt_tx`) disarms at once,
/// exactly as it does with no hold. The hold refuses a start; it never refuses a stop.
#[test]
fn stop_is_taken_at_once_while_held() {
    let mut e = station_in("phone");
    assert!(e.tx_enabled(), "premise: armed");
    e.set_remote_transmit_held(true);
    e.halt_tx();
    assert!(!e.tx_enabled(), "Stop disarms the latch while held");
    assert_eq!(e.tx_owner(), None, "and nothing is keyed");
    assert!(e.remote_transmit_held(), "the hold itself still stands");
}

/// ★ THE OFF HALF OF THE SWITCH IS NEVER HELD. `set_tx_enabled(false)` disarms at once while held;
/// only an arm (`true`) is refused.
#[test]
fn disarming_is_honoured_while_held() {
    let mut e = station_in("phone");
    assert!(e.tx_enabled(), "premise: armed");
    e.set_remote_transmit_held(true);
    e.set_tx_enabled(false);
    assert!(!e.tx_enabled(), "TX Off is honoured while held");
}

// ── The hold under an ALREADY-ARMED latch ───────────────────────────────────────────────────────
//
// The gates above all catch the ARM (`set_tx_enabled`, which every section's entry and Call CQ
// funnel through). These catch a start that keys UNDER a latch that was already up when a held
// device took control — the paths the arm gate never sees. Each starts with the latch armed and
// the hold NOT yet set, so the refusal is the hold's own and not the latch's; the second try, with
// the hold lifted, is the control.

/// ★ An FT over cannot START under the already-armed latch while held: `plan_tx` refuses the over
/// and leaves the arm up, exactly as under a clock repair. The over goes with no hold — the control.
#[test]
fn an_ft_over_is_refused_under_an_armed_latch() {
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_tx_enabled(true);
    e.broadcast("CQ K2DEF FN31");
    e.set_remote_transmit_held(true);
    assert!(
        e.plan_tx(0).is_none(),
        "held: the FT over is refused under the armed latch"
    );
    assert!(
        e.tx_enabled(),
        "the latch is left armed — the hold refuses a start, never a disarm"
    );

    let mut c = Engine::new("K2DEF", "FN31", 0);
    c.set_tx_enabled(true);
    c.broadcast("CQ K2DEF FN31");
    assert!(
        c.plan_tx(0).is_some(),
        "control: the over plans with no hold"
    );
}

/// ★ A keyboard-mode (RTTY) send under the already-armed latch is refused while held — the send
/// gate's own answer — and goes once the hold is off.
#[test]
fn a_keyboard_send_is_refused_under_an_armed_latch() {
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    e.set_operating_mode("rtty", false);
    assert!(e.tx_enabled(), "premise: entering RTTY armed the latch");
    e.set_remote_transmit_held(true);
    assert_eq!(
        e.rtty_send_text("\r\nCQ TEST W9XYZ "),
        Err(REMOTE_HOLD_HOLDS_TX.to_string()),
        "held: the RTTY send is refused under the armed latch"
    );

    e.set_remote_transmit_held(false);
    assert!(
        e.rtty_send_text("\r\nCQ TEST W9XYZ ").is_ok(),
        "control: the send goes once the hold is off"
    );
}

/// ★ A manual phone PTT key under the already-armed latch is refused while held (a new key only;
/// a release is never refused), and keys once the hold is off.
#[test]
fn a_manual_ptt_key_is_refused_under_an_armed_latch() {
    let mut e = station_in("phone");
    assert!(e.tx_enabled(), "premise: the phone latch is already armed");
    e.set_remote_transmit_held(true);
    e.set_ptt(true);
    assert!(
        !e.manual_ptt(),
        "held: the manual key is refused under the armed latch"
    );

    e.set_remote_transmit_held(false);
    e.set_ptt(true);
    assert!(
        e.manual_ptt(),
        "control: the key takes once the hold is off"
    );
}

/// ★ A TX On the hold refused is said in the snapshot (`tx_on_held`, the status lane's line) for
/// as long as the hold stands, and only then: no refusal, no line; the hold lifted, no line; held
/// again, no line until a TX On is refused again.
#[test]
fn a_refused_tx_on_is_said_while_the_hold_stands() {
    let mut e = ft8();
    e.set_remote_transmit_held(true);
    assert!(!e.snapshot().radio.tx_on_held, "held, nothing refused yet");

    e.set_tx_enabled(true);
    assert!(!e.tx_enabled(), "premise: refused");
    assert!(e.snapshot().radio.tx_on_held, "the refusal is said");

    e.set_remote_transmit_held(false);
    assert!(
        !e.snapshot().radio.tx_on_held,
        "the hold lifted: nothing to say"
    );

    e.set_remote_transmit_held(true);
    assert!(
        !e.snapshot().radio.tx_on_held,
        "held again: no refusal of this hold yet"
    );
}

/// ★ The control: unheld, TX On arms and nothing is said; TX Off is never a refusal either.
#[test]
fn an_unheld_tx_on_says_nothing() {
    let mut e = ft8();
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "premise: armed");
    assert!(!e.snapshot().radio.tx_on_held);

    e.set_remote_transmit_held(true);
    e.set_tx_enabled_by_operator(false);
    assert!(!e.tx_enabled(), "TX Off is honoured while held");
    assert!(
        !e.snapshot().radio.tx_on_held,
        "a TX Off is not a refused TX On"
    );
}

/// A hold source answering `hold` on every tick, as the Remote service installs its own.
fn source(hold: remote_transmit::RemoteTxHold) -> remote_transmit::RemoteTxHoldSource {
    std::sync::Arc::new(move |_| Some(hold))
}

/// ★ The hold is shown (`radio.txHold`) whenever the source holds the controlling device, in any
/// section, with the seconds left only when its owner was notified; unheld, nothing is shown.
#[test]
fn the_hold_is_shown_with_the_time_left_only_after_a_notice() {
    let left = Some(std::time::Duration::from_millis(90_500));
    for section in ["phone", "cw"] {
        let mut e = station_in(section);
        for (owner_notified, want) in [(true, Some(91)), (false, None)] {
            e.set_remote_tx_hold_source(source(remote_transmit::RemoteTxHold {
                held: true,
                owner_notified,
                left,
            }));
            e.poll_remote_transmit(std::time::Instant::now());
            assert_eq!(
                e.snapshot().radio.tx_hold,
                Some(crate::dto::TxHold {
                    owner_notified,
                    left_secs: want
                }),
                "{section}, notified {owner_notified}"
            );
        }
        e.set_remote_tx_hold_source(source(remote_transmit::RemoteTxHold {
            held: false,
            owner_notified: true,
            left,
        }));
        e.poll_remote_transmit(std::time::Instant::now());
        assert_eq!(e.snapshot().radio.tx_hold, None, "{section}: not held");
    }
}

/// ★ THE GATE NEVER READS THE DISPLAY COPY. With the copy saying the opposite of the gate, a TX On
/// follows the gate both ways, and the snapshot shows no hold the gate does not stand behind.
#[test]
fn the_transmit_gate_ignores_the_shown_hold() {
    let mut e = ft8();
    e.set_remote_transmit_held(true);
    e.remote_tx_hold_shown = None;
    e.set_tx_enabled(true);
    assert!(!e.tx_enabled(), "held with nothing shown: still refused");

    e.set_remote_transmit_held(false);
    e.remote_tx_hold_shown = Some(crate::dto::TxHold {
        owner_notified: true,
        left_secs: Some(600),
    });
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "unheld with a hold shown: arms");
    assert_eq!(
        e.snapshot().radio.tx_hold,
        None,
        "a hold the gate lifted is not shown"
    );
}
