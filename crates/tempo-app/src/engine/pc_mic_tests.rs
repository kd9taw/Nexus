//! The computer microphone's safety rules at the engine (`engine/pc_mic.rs`): audio reaches the
//! transmitter only between the operator's PTT press and release with every gate open, every stop
//! ends the over within one tick with the abort armed, the digital modes never take it, it is off by
//! default, and it is per radio.
use super::*;
use crate::mic::{MicTick, PC_MIC};
use crate::settings::Settings;
use std::time::Duration;

const TICK: f64 = 20.0;
const MIC_DEVICE: &str = "Desk Microphone";

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// A station in Phone with TX armed, the computer's microphone switched on for radio 0 only, and a
/// second radio (1) with it off.
fn station() -> Engine {
    let mut e = Engine::new("W9XYZ", "EN52", 0);
    let mut s: Settings = e.settings.clone();
    s.pc_mic = true;
    s.pc_mic_device = MIC_DEVICE.into();
    s.audio_in = "Radio Codec".into();
    s.sync_active_from_flat();
    let mut other = s.radios[0].clone();
    other.id = 1;
    other.name = "Second radio".into();
    other.pc_mic = false;
    s.radios.push(other);
    e.apply_restored_settings(s); // the roster as given: a form save keeps the live one
    e.set_operating_mode("phone", false); // arms TX, as entering Phone does
    assert!(e.tx_enabled(), "premise: Phone arms TX");
    e
}

/// The microphone's delivery of `n` ms of audio at `level`.
fn audio(n: u64, level: f32) -> Vec<f32> {
    vec![level; PC_MIC.samples(n) as usize]
}

/// Press PTT and run to a keyed over at `t0`, with the microphone delivering as a sound card does.
fn keyed(e: &mut Engine, t0: Instant) {
    e.set_ptt(true);
    assert!(
        e.pc_mic_armed(),
        "premise: the press armed the computer microphone"
    );
    assert_eq!(e.poll_pc_mic(t0, TICK, 1), MicTick::Armed);
    e.push_pc_mic(&audio(40, 0.25), t0);
    assert_eq!(
        e.poll_pc_mic(t0, TICK, 1),
        MicTick::Key,
        "premise: audio keys"
    );
    let mut t = 20;
    while t <= 200 {
        e.push_pc_mic(&audio(20, 0.25), t0 + ms(t));
        let _ = e.poll_pc_mic(t0 + ms(t), TICK, 1);
        t += 20;
    }
    assert!(e.pc_mic_keyed(), "premise: still keyed");
    assert!(!e.take_pc_mic_abort(), "premise: no abort yet");
}

#[test]
fn the_computer_microphone_is_off_by_default_and_ptt_keys_the_radios_own_mic() {
    assert!(!Settings::default().pc_mic, "the switch defaults OFF");
    assert!(
        !crate::settings::RadioProfile::default().pc_mic,
        "a new radio's switch defaults OFF"
    );
    let mut e = Engine::new("W9XYZ", "EN52", 0);
    e.set_operating_mode("phone", false);
    e.set_ptt(true);
    assert!(
        e.manual_ptt(),
        "PTT keys the radio's own microphone, as always"
    );
    assert!(!e.pc_mic_armed(), "no computer-microphone over");
    assert_eq!(e.pc_mic_source(), None, "the microphone is never opened");
}

#[test]
fn audio_reaches_the_transmitter_only_between_press_and_release() {
    let mut e = station();
    let t0 = Instant::now();
    // Before the press: the microphone is not open and nothing it delivers is kept.
    assert_eq!(
        e.pc_mic_source(),
        None,
        "the microphone is open with no press"
    );
    e.push_pc_mic(&audio(100, 0.5), t0);
    assert_eq!(e.poll_pc_mic(t0, TICK, 1), MicTick::Idle);
    // The press arms and keys nothing, however long it is held without audio.
    e.set_ptt(true);
    assert!(
        !e.manual_ptt(),
        "the press keyed the radio's own microphone as well"
    );
    assert_eq!(
        e.pc_mic_source().map(|s| s.device),
        Some(MIC_DEVICE.to_string())
    );
    for k in 0..50 {
        assert_eq!(e.poll_pc_mic(t0 + ms(k * 20), TICK, 1), MicTick::Armed);
    }
    let t1 = t0 + ms(1_000);
    e.push_pc_mic(&audio(20, 0.5), t1);
    assert_eq!(
        e.poll_pc_mic(t1, TICK, 1),
        MicTick::Key,
        "the first audio keys"
    );
    let mut heard = false;
    for k in 1..20 {
        let t = t1 + ms(k * 20);
        e.push_pc_mic(&audio(20, 0.5), t);
        if let MicTick::Samples(s) = e.poll_pc_mic(t, TICK, 1) {
            heard |= s.contains(&0.5);
        }
    }
    assert!(heard, "the operator's voice never reached the transmitter");
    // The release: what the over holds may play out (the tail's own tests below), nothing
    // captured after it reaches the transmitter, and by the bound the over is over.
    let r = t1 + ms(20 * 19);
    e.release_pc_mic_at(r);
    let t2 = r + ms(RELEASE_TAIL_MS);
    e.push_pc_mic(&audio(100, 0.9), t2);
    assert_eq!(e.poll_pc_mic(t2, TICK, 1), MicTick::Idle);
    assert!(!e.pc_mic_armed());
    assert!(
        e.take_pc_mic_abort(),
        "the release's bound did not arm the flush + unkey"
    );
    assert_eq!(
        e.pc_mic_source(),
        None,
        "the microphone stayed open after the release"
    );
    e.push_pc_mic(&audio(100, 0.9), t2 + ms(20));
    assert_eq!(e.poll_pc_mic(t2 + ms(20), TICK, 1), MicTick::Idle);
}

#[test]
fn every_stop_ends_a_keyed_over_within_one_tick() {
    type Stop = fn(&mut Engine);
    // `true`: the stop itself ends the over, before any tick (the operator's own stops and a lost
    // microphone); `false`: the next tick's gates end it. The PTT release is not here: it lets the
    // over's audio play out for up to `RELEASE_TAIL_MS` (its own tests, below).
    let stops: [(&str, bool, Stop); 7] = [
        ("Stop TX (halt_tx)", true, |e| e.halt_tx()),
        ("TX Off (the latch)", true, |e| e.set_tx_enabled(false)),
        ("the microphone went away", true, |e| {
            e.pc_mic_device_failed(true)
        }),
        ("leaving Phone for FT8", false, |e| {
            e.set_operating_mode("digital", false);
        }),
        ("a tune", false, |e| e.set_tune(true)),
        ("the switch turned off", false, |e| {
            e.settings.pc_mic = false
        }),
        ("another microphone chosen", false, |e| {
            e.settings.pc_mic_device = "Webcam".into()
        }),
    ];
    for (name, at_once, stop) in stops {
        let mut e = station();
        let t0 = Instant::now();
        keyed(&mut e, t0);
        stop(&mut e);
        if at_once {
            assert!(
                !e.pc_mic_armed(),
                "{name}: the over outlived the stop itself"
            );
        }
        // One tick, with the microphone still delivering: no audio, and the abort is armed.
        let t = t0 + ms(220);
        e.push_pc_mic(&audio(20, 0.5), t);
        let tick = e.poll_pc_mic(t, TICK, 1);
        assert!(
            !matches!(tick, MicTick::Samples(_) | MicTick::Ahead | MicTick::Key),
            "{name}: the over carried on ({tick:?})"
        );
        assert!(!e.pc_mic_armed(), "{name}: the over is still armed");
        assert!(e.take_pc_mic_abort(), "{name}: no flush + unkey was armed");
    }
}

#[test]
fn the_watchdog_ends_the_over_and_disarms_tx() {
    let mut e = station();
    e.settings.tx_watchdog_min = 1;
    let t0 = Instant::now();
    keyed(&mut e, t0);
    // The unattended-transmit clock started a minute ago.
    e.tx_watchdog_start = Some(now_unix_secs() - 61);
    let t = t0 + ms(220);
    e.push_pc_mic(&audio(20, 0.5), t);
    assert_eq!(e.poll_pc_mic(t, TICK, 1), MicTick::Idle);
    assert!(!e.pc_mic_armed());
    assert!(e.take_pc_mic_abort(), "the watchdog did not arm the unkey");
    assert!(!e.tx_enabled(), "the watchdog trip must leave TX off");
    assert_eq!(
        e.pc_mic.ended.as_ref().map(|x| x.cause),
        Some(PcMicCause::Watchdog)
    );
}

#[test]
fn a_microphone_that_stops_delivering_ends_the_over_at_the_gap() {
    let mut e = station();
    let t0 = Instant::now();
    keyed(&mut e, t0);
    let last = 200; // the last delivery, in `keyed`
    let mut ended = None;
    let mut t = last + 20;
    while t <= last + 600 {
        let _ = e.poll_pc_mic(t0 + ms(t), TICK, 1);
        if !e.pc_mic_armed() && ended.is_none() {
            ended = Some(t - last);
        }
        t += 20;
    }
    let after = ended.expect("the over outlived its microphone");
    assert!(
        (PC_MIC.gap_ms..=PC_MIC.gap_ms + 20).contains(&after),
        "ended {after} ms after the last audio"
    );
    assert!(e.take_pc_mic_abort());
    assert_eq!(
        e.pc_mic.ended.as_ref().map(|x| x.cause),
        Some(PcMicCause::NoAudio)
    );
}

#[test]
fn a_lost_microphone_is_named_and_the_next_over_that_keys_clears_it() {
    let mut e = station();
    let t0 = Instant::now();
    keyed(&mut e, t0);
    e.pc_mic_device_failed(true);
    assert!(e.take_pc_mic_abort());
    assert_eq!(
        e.snapshot().radio.pc_mic_ended.map(|x| x.cause),
        Some(PcMicCause::DeviceLost)
    );
    let t1 = t0 + ms(2_000);
    keyed(&mut e, t1);
    assert_eq!(e.snapshot().radio.pc_mic_ended, None);
}

#[test]
fn ft_and_the_other_digital_modes_never_take_the_microphone() {
    let mut e = station();
    for mode in ["digital", "cw", "rtty"] {
        e.set_operating_mode(mode, false);
        e.set_ptt(true);
        assert!(
            !e.pc_mic_armed(),
            "{mode}: PTT armed the computer microphone"
        );
        assert_eq!(e.pc_mic_source(), None, "{mode}: the microphone was opened");
        let t = Instant::now();
        e.push_pc_mic(&audio(100, 0.5), t);
        assert_eq!(e.poll_pc_mic(t, TICK, 1), MicTick::Idle, "{mode}");
        e.set_ptt(false);
    }
}

#[test]
fn it_never_arms_without_every_gate_or_while_another_owner_transmits() {
    // TX disarmed.
    let mut e = station();
    e.set_tx_enabled(false);
    e.set_ptt(true);
    assert!(!e.pc_mic_armed(), "armed with TX off");
    // A tune is up.
    let mut e = station();
    e.set_tune(true);
    e.set_ptt(true);
    assert!(!e.pc_mic_armed(), "armed under a tune");
    // Outside the licence: a Technician on 20 m phone.
    let mut e = station();
    assert!(e.set_license_class("technician"));
    e.set_ptt(true);
    assert!(
        !e.tx_allowed() && !e.pc_mic_armed(),
        "armed outside the licence"
    );
}

#[test]
fn a_missing_or_receive_microphone_is_refused_and_said() {
    let mut e = station();
    e.settings.pc_mic_device = "  ".into();
    e.set_ptt(true);
    assert!(!e.pc_mic_armed());
    assert!(
        !e.manual_ptt(),
        "the press fell back to the radio's microphone"
    );
    assert_eq!(
        e.snapshot().radio.pc_mic_ended.map(|x| x.cause),
        Some(PcMicCause::NoDevice)
    );
}

#[test]
fn it_is_per_radio() {
    let mut e = station();
    // Radio 1 has it off: PTT keys that radio's own microphone.
    e.set_active_radio(1);
    assert_eq!(e.settings.active_radio, 1, "premise: radio 1 is active");
    e.set_operating_mode("phone", false);
    e.set_ptt(true);
    assert!(!e.pc_mic_armed(), "radio 1 took the computer microphone");
    assert!(e.manual_ptt(), "radio 1's PTT keys its own microphone");
    e.set_ptt(false);
    // Back on radio 0 it is on.
    e.set_active_radio(0);
    e.set_operating_mode("phone", false);
    e.set_ptt(true);
    assert!(e.pc_mic_armed(), "radio 0 lost its computer microphone");
    e.set_ptt(false);
    // A switch to the other radio mid-over ends it.
    let t0 = Instant::now();
    keyed(&mut e, t0);
    e.set_active_radio(1);
    let t = t0 + ms(220);
    e.push_pc_mic(&audio(20, 0.5), t);
    assert_eq!(e.poll_pc_mic(t, TICK, 1), MicTick::Idle);
    assert!(!e.pc_mic_armed(), "the over moved to another radio");
    assert!(e.take_pc_mic_abort());
}

#[test]
fn the_gain_is_applied_and_clipped() {
    let mut e = station();
    e.settings.pc_mic_gain_db = 6.0206; // ×2
    let t0 = Instant::now();
    e.set_ptt(true);
    e.push_pc_mic(&audio(20, 0.2), t0);
    assert_eq!(e.poll_pc_mic(t0, TICK, 1), MicTick::Key);
    e.push_pc_mic(&audio(20, 0.7), t0); // ×2 = 1.4, clipped to full scale
    let mut out = Vec::new();
    for k in 1..20 {
        if let MicTick::Samples(s) = e.poll_pc_mic(t0 + ms(k * 20), TICK, 1) {
            out.extend(s);
        }
        e.push_pc_mic(&audio(20, 0.0), t0 + ms(k * 20));
    }
    assert!(
        out.iter().any(|&x| (x - 0.4).abs() < 1e-3),
        "6 dB did not double"
    );
    assert!(out.iter().all(|&x| x <= 1.0), "a sample past full scale");
    assert!(
        out.contains(&1.0),
        "the loud frame was not clipped to full scale"
    );
}

/// The cause the status lane shows, if any.
fn lane(e: &Engine) -> Option<PcMicCause> {
    e.snapshot().radio.pc_mic_ended.map(|x| x.cause)
}

/// ⛔ The voice keyer and the computer microphone never share the transmitter: a message sent
/// while a PC-mic over is armed or keyed is refused (on every rig, not only a Flex), a message
/// already taken is dropped, and a press while a message is pending or playing arms nothing and
/// says why. CONTROL: with no press the same message is queued.
#[test]
fn the_voice_keyer_and_the_computer_microphone_exclude_each_other() {
    let msg = || vec![0.1f32; 12_000];
    // Armed (no audio yet): refused.
    let mut e = station();
    e.set_ptt(true);
    assert!(e.pc_mic_armed(), "premise: armed");
    assert!(
        e.send_voice(msg()).is_err(),
        "a message queued under an armed over"
    );
    assert!(e.voice_tx.is_none(), "the message was kept");
    // Keyed: refused.
    let mut e = station();
    keyed(&mut e, Instant::now());
    assert!(
        e.send_voice(msg()).is_err(),
        "a message queued under a keyed over"
    );
    // A message already queued when an over arms (the press is refused then, so set by hand):
    // the loop's take drops it.
    let mut e = station();
    e.set_ptt(true);
    e.voice_tx = Some(msg());
    assert_eq!(
        e.poll_voice(),
        None,
        "a message played under the computer microphone"
    );
    assert!(e.voice_tx.is_none(), "the message was held for later");
    // The other way: a pending message, then the press. Nothing arms, and the lane says why.
    let mut e = station();
    e.send_voice(msg())
        .expect("CONTROL: with no press the message is queued");
    e.set_ptt(true);
    assert!(!e.pc_mic_armed(), "armed under a pending voice message");
    assert_eq!(lane(&e), Some(PcMicCause::Busy));
    // …and one on the air.
    let mut e = station();
    e.send_voice(msg()).unwrap();
    assert!(
        e.poll_voice().is_some(),
        "premise: the loop took the message"
    );
    e.set_ptt(true);
    assert!(!e.pc_mic_armed(), "armed under a playing voice message");
    e.set_voice_on_air(false);
    e.set_ptt(false);
    e.set_ptt(true);
    assert!(
        e.pc_mic_armed(),
        "CONTROL: the message over, the press arms"
    );
}

/// ⛔ No feedback path, by name at the press: a Flex DAX receive channel and a loopback or virtual
/// cable are refused with the reason, and nothing arms. CONTROL: an ordinary microphone arms.
#[test]
fn a_microphone_named_as_receive_or_loopback_audio_is_refused_at_the_press() {
    let cases: [(&str, PcMicCause); 4] = [
        (
            "DAX Audio RX 1 (FlexRadio Systems DAX Audio)",
            PcMicCause::RadioReceive,
        ),
        (
            "CABLE Output (VB-Audio Virtual Cable)",
            PcMicCause::Loopback,
        ),
        ("Stereo Mix (Realtek(R) Audio)", PcMicCause::Loopback),
        (
            "Monitor of Built-in Audio Analog Stereo",
            PcMicCause::Loopback,
        ),
    ];
    for (mic, cause) in cases {
        let mut e = station();
        e.settings.pc_mic_device = mic.into();
        e.set_ptt(true);
        assert!(!e.pc_mic_armed(), "{mic:?} armed");
        assert_eq!(lane(&e), Some(cause), "{mic:?}");
    }
    let mut e = station();
    e.set_ptt(true);
    assert!(e.pc_mic_armed(), "CONTROL: an ordinary microphone");
}

/// The device guard's answer named for the operator ([`Engine::pc_mic_receive_refusal`]), with a
/// guard that knows exactly which names are one device: the system default input, the station's
/// receive input, this radio's, another radio's and the one the sound card opened are each asked;
/// one device names whose receive it is, and outranks an unsure answer from another input; an
/// unsure answer alone is `Unsure`. CONTROL: an input the guard calls different refuses nothing.
#[test]
fn the_device_guards_answer_is_named_for_whose_receive_it_is() {
    // "same:<x>" is one device with the microphone "<x>"; "unsure" cannot be told apart from it.
    let guard = |mic: &str, input: &str| -> Option<SameInput> {
        if input == "unsure" || (input.is_empty() && mic == "server") {
            Some(SameInput::Unsure)
        } else if input.strip_prefix("same:") == Some(mic) {
            Some(SameInput::Same)
        } else {
            None
        }
    };
    let mut e = station();
    e.settings.audio_in = "Radio Codec".into();
    e.settings.radios[0].audio_in = "Radio Codec".into();
    e.settings.radios[1].audio_in = "Second Codec".into();
    let ask =
        |e: &Engine, mic: &str, opened: Option<&str>| e.pc_mic_receive_refusal(mic, opened, &guard);
    assert_eq!(
        ask(&e, "mic", None),
        None,
        "CONTROL: different from every input"
    );
    assert_eq!(
        ask(&e, "server", None),
        Some(PcMicCause::Unsure),
        "the system default input"
    );
    assert_eq!(
        ask(&e, "mic", Some("same:mic")),
        Some(PcMicCause::SameAsReceive),
        "the input the sound card opened"
    );
    assert_eq!(ask(&e, "mic", Some("unsure")), Some(PcMicCause::Unsure));
    e.settings.audio_in = "same:mic".into();
    assert_eq!(
        ask(&e, "mic", None),
        Some(PcMicCause::SameAsReceive),
        "the station's"
    );
    e.settings.audio_in = "Radio Codec".into();
    e.settings.radios[0].audio_in = "same:mic".into();
    assert_eq!(
        ask(&e, "mic", None),
        Some(PcMicCause::SameAsReceive),
        "this radio's"
    );
    e.settings.radios[0].audio_in = "Radio Codec".into();
    e.settings.radios[1].audio_in = "same:mic".into();
    assert_eq!(
        ask(&e, "mic", None),
        Some(PcMicCause::RadioReceive),
        "another radio's"
    );
    // One device outranks unsure, whichever is asked first.
    assert_eq!(
        ask(&e, "mic", Some("unsure")),
        Some(PcMicCause::RadioReceive),
        "the same device outranks an unsure one"
    );
    e.settings.radios[1].audio_in = "Second Codec".into();
    e.settings.audio_in = "unsure".into();
    assert_eq!(
        ask(&e, "mic", Some("same:mic")),
        Some(PcMicCause::SameAsReceive),
        "the same device outranks an unsure one asked before it"
    );
}

/// ⛔ The loop's refusal ends the armed over before the microphone opens, keys nothing, arms no
/// abort, and says why.
#[test]
fn a_refusal_at_the_loop_ends_the_over_before_the_microphone_opens() {
    let mut e = station();
    e.set_ptt(true);
    assert!(e.pc_mic_armed());
    e.pc_mic_refused(PcMicCause::Unsure);
    assert!(!e.pc_mic_armed());
    assert_eq!(e.pc_mic_source(), None, "the loop would still open it");
    assert!(
        !e.take_pc_mic_abort(),
        "nothing was keyed, nothing to unkey"
    );
    assert_eq!(lane(&e), Some(PcMicCause::Unsure));
}

/// The radio loop's view of Nexus's Flex client in Phone at the shack: the radio has its own mic.
fn flex_station() -> Engine {
    let mut e = station();
    e.observe_flex_radio_has_mic(true);
    e
}

/// Press PTT on a Flex whose radio has its own mic and run to a keyed over on the DAX hold at `t0`:
/// the radio reports DAX (the route put in place for it moved from 1 to 2 meanwhile), then audio.
fn keyed_on_hold(e: &mut Engine, t0: Instant) {
    e.set_ptt(true);
    assert!(e.pc_mic_dax_hold().is_some(), "premise: armed on the hold");
    e.push_pc_mic(&audio(40, 0.25), t0);
    assert_eq!(e.poll_pc_mic(t0, TICK, 1), MicTick::Armed);
    e.observe_pc_mic_dax(true);
    e.push_pc_mic(&audio(40, 0.25), t0 + ms(20));
    assert_eq!(
        e.poll_pc_mic(t0 + ms(20), TICK, 2),
        MicTick::Key,
        "premise: audio keys once the radio takes DAX"
    );
    let mut t = 40;
    while t <= 200 {
        e.push_pc_mic(&audio(20, 0.25), t0 + ms(t));
        let _ = e.poll_pc_mic(t0 + ms(t), TICK, 2);
        t += 20;
    }
    assert!(e.pc_mic_keyed(), "premise: still keyed");
    assert!(!e.take_pc_mic_abort(), "premise: no abort yet");
}

/// ⛔ ON A FLEX WHOSE RADIO HAS ITS OWN MIC, THE PRESS ARMS ON THE DAX HOLD AND KEYS ONLY ON DAX.
/// Nexus's own Flex client gives the radio its mic in Phone, and the computer's audio reaches the
/// radio only as DAX: the press arms (never the radio's own mic, never a key), the microphone's
/// audio goes nowhere and keys nothing while the radio still takes its mic, the client's transmit
/// route coming up for the switch does not end the press, and the first audio after the radio
/// reports DAX keys. CONTROL: on any other radio there is no hold, and audio keys at once.
#[test]
fn on_a_flex_with_its_own_mic_the_press_waits_for_dax_and_keys_only_on_it() {
    let mut e = flex_station();
    let pressed = Instant::now();
    e.set_ptt(true);
    assert!(e.pc_mic_armed(), "the press arms the computer microphone");
    assert!(!e.manual_ptt(), "the press keyed the radio's own mic");
    let since = e.pc_mic_dax_hold().expect("the over rides the DAX hold");
    assert!(since >= pressed, "the hold is timed from the press");
    assert_eq!(lane(&e), None, "nothing to say yet");
    // A second of speech while the radio still takes its mic, the route moving under it.
    let t0 = Instant::now();
    for k in 0..50u64 {
        let t = t0 + ms(k * 20);
        e.push_pc_mic(&audio(20, 0.5), t);
        assert_eq!(
            e.poll_pc_mic(t, TICK, 1 + k / 25),
            MicTick::Armed,
            "tick {k}: keyed on the radio's own mic"
        );
    }
    assert!(e.pc_mic_armed(), "the route coming up ended the press");
    assert!(!e.take_pc_mic_abort(), "nothing keyed, nothing to unkey");
    // The radio reports DAX: the next audio keys, and only audio captured since.
    e.observe_pc_mic_dax(true);
    let t = t0 + ms(1_000);
    assert_eq!(
        e.poll_pc_mic(t, TICK, 2),
        MicTick::Armed,
        "keyed on nothing"
    );
    e.push_pc_mic(&audio(20, 0.25), t + ms(20));
    assert_eq!(e.poll_pc_mic(t + ms(20), TICK, 2), MicTick::Key);
    let mut played = Vec::new();
    for k in 2..20u64 {
        let t = t + ms(k * 20);
        e.push_pc_mic(&audio(20, 0.25), t);
        if let MicTick::Samples(b) = e.poll_pc_mic(t, TICK, 2) {
            played.extend(b);
        }
    }
    assert!(played.contains(&0.25), "the over played no speech");
    assert!(
        !played.contains(&0.5),
        "audio from before the radio took DAX reached the air"
    );

    // CONTROL: the radio not on its own mic (any other radio): no hold, and audio keys at once.
    let mut e = station();
    e.set_ptt(true);
    assert_eq!(e.pc_mic_dax_hold(), None, "a hold where none is needed");
    let t0 = Instant::now();
    e.push_pc_mic(&audio(40, 0.25), t0);
    assert_eq!(e.poll_pc_mic(t0, TICK, 1), MicTick::Key);
}

/// ⭐ THE HOLD HAS NO LENGTH OF ITS OWN: an over on the DAX hold stays on the air past the 20
/// seconds a voice-keyer message may carry on the same client, for as long as PTT is held and the
/// radio keeps taking DAX.
#[test]
fn an_over_on_the_dax_hold_outlasts_the_voice_keyers_longest_message() {
    let mut e = flex_station();
    let t0 = Instant::now();
    keyed_on_hold(&mut e, t0);
    let mut fed = 0usize;
    let mut t = 220;
    while t <= 45_000 {
        e.push_pc_mic(&audio(20, 0.25), t0 + ms(t));
        match e.poll_pc_mic(t0 + ms(t), TICK, 2) {
            MicTick::Samples(b) => fed += b.len(),
            MicTick::Ahead => {}
            other => panic!("at {t} ms the over ended: {other:?} {:?}", lane(&e)),
        }
        t += 20;
    }
    assert!(e.pc_mic_keyed());
    assert!(
        fed > crate::engine::FLEX_VOICE_MAX_SAMPLES * PC_MIC.rate_hz as usize / 12_000 * 2,
        "{fed} samples: not past twice the keyer's longest message"
    );
}

/// The radio never switches to DAX: the radio loop gives up after the client's confirm window, the
/// press ends with nothing keyed and the lane says why. CONTROL: the same call on a keyed over
/// (the radio had switched) leaves it alone.
#[test]
fn a_press_the_radio_never_switches_to_dax_for_ends_with_nothing_keyed() {
    let mut e = flex_station();
    e.set_ptt(true);
    e.pc_mic_no_dax();
    assert!(!e.pc_mic_armed());
    assert_eq!(e.pc_mic_dax_hold(), None, "the hold outlived the press");
    assert!(
        !e.take_pc_mic_abort(),
        "nothing was keyed, nothing to unkey"
    );
    assert_eq!(lane(&e), Some(PcMicCause::FlexMic));
    let mut e = flex_station();
    keyed_on_hold(&mut e, Instant::now());
    e.pc_mic_no_dax();
    assert!(e.pc_mic_keyed(), "CONTROL: a keyed over is not the press");
}

/// ⛔ The radio leaving DAX under a keyed over on the hold (SmartSDR's own switch, the stream gone)
/// ends it on that tick, its audio not played: the radio's mic would carry the over. CONTROL: while
/// the radio keeps DAX the over carries on.
#[test]
fn a_keyed_over_on_the_dax_hold_ends_when_the_radio_leaves_dax() {
    let mut e = flex_station();
    let t0 = Instant::now();
    keyed_on_hold(&mut e, t0);
    let t = t0 + ms(220);
    e.push_pc_mic(&audio(20, 0.5), t);
    assert!(
        matches!(
            e.poll_pc_mic(t, TICK, 2),
            MicTick::Samples(_) | MicTick::Ahead
        ),
        "CONTROL: the over ended on DAX"
    );
    e.observe_pc_mic_dax(false);
    let t = t0 + ms(240);
    e.push_pc_mic(&audio(20, 0.5), t);
    assert_eq!(e.poll_pc_mic(t, TICK, 2), MicTick::Idle);
    assert!(!e.pc_mic_armed());
    assert!(e.take_pc_mic_abort(), "no flush + unkey");
    assert_eq!(lane(&e), Some(PcMicCause::GateDown));
}

/// ⛔ EVERY TX GUARD STILL CUTS AN OVER ON THE DAX HOLD, and the hold ends with it (the radio loop
/// then gives the radio its mic back): Stop TX, TX Off and a lost Flex connection or handoff at
/// once; the watchdog on its tick, with TX left off; the release once its tail is spent; the
/// transmit route (the client's DAX) going on its tick.
#[test]
fn every_stop_ends_an_over_on_the_dax_hold_and_the_hold_with_it() {
    type Stop = fn(&mut Engine, Instant);
    let stops: [(&str, Stop); 6] = [
        ("Stop TX", |e, _| e.halt_tx()),
        ("TX Off (the latch)", |e, _| e.set_tx_enabled(false)),
        ("a handoff or a lost connection", |e, _| {
            e.pc_mic_interrupted()
        }),
        ("the watchdog", |e, _| {
            e.settings.tx_watchdog_min = 1;
            e.tx_watchdog_start = Some(now_unix_secs() - 61);
        }),
        ("the release, its tail spent", |e, t| {
            e.release_pc_mic_at(t - ms(RELEASE_TAIL_MS));
            e.set_ptt(false);
        }),
        ("the client's DAX route gone", |_, _| {}),
    ];
    for (name, stop) in stops {
        let mut e = flex_station();
        let t0 = Instant::now();
        keyed_on_hold(&mut e, t0);
        let t = t0 + ms(220);
        stop(&mut e, t);
        e.push_pc_mic(&audio(20, 0.5), t);
        // The route the over keyed on is 2: a new one is the client's DAX tee replaced or gone.
        let route = if name.contains("route") { 3 } else { 2 };
        let tick = e.poll_pc_mic(t, TICK, route);
        assert_eq!(tick, MicTick::Idle, "{name}: the over carried on");
        assert!(!e.pc_mic_armed(), "{name}: still armed");
        assert_eq!(
            e.pc_mic_dax_hold(),
            None,
            "{name}: the hold outlived the over"
        );
        assert!(e.take_pc_mic_abort(), "{name}: no flush + unkey");
        if name == "the watchdog" {
            assert!(!e.tx_enabled(), "the watchdog left TX on");
            assert_eq!(lane(&e), Some(PcMicCause::Watchdog));
        }
    }
}

/// While an over on the hold is armed or keyed, the voice keyer's own switch is refused (one owner),
/// so the two never share the client's DAX.
#[test]
fn the_voice_keyer_is_refused_while_an_over_rides_the_dax_hold() {
    let mut e = flex_station();
    e.set_ptt(true);
    assert!(e.send_voice(audio(1_000, 0.1)).is_err(), "armed");
    let mut e = flex_station();
    keyed_on_hold(&mut e, Instant::now());
    assert!(e.send_voice(audio(1_000, 0.1)).is_err(), "keyed");
}

/// An over keyed WITHOUT the hold (the radio was not on its own mic at the press) still ends on the
/// next tick when the radio takes its mic: its audio would go to a DAX the radio is not listening
/// to.
#[test]
fn an_over_keyed_off_the_hold_ends_when_the_radio_takes_its_mic() {
    let mut e = station();
    let t0 = Instant::now();
    keyed(&mut e, t0);
    e.observe_flex_radio_has_mic(true);
    let t = t0 + ms(220);
    e.push_pc_mic(&audio(20, 0.5), t);
    assert_eq!(e.poll_pc_mic(t, TICK, 1), MicTick::Idle);
    assert!(
        e.take_pc_mic_abort(),
        "no unkey when the radio took its mic"
    );
    assert_eq!(lane(&e), Some(PcMicCause::GateDown));
}

/// A gate that goes down under a held over is named in the lane (the PTT is still held, so the
/// operator needs to know why nothing is going out). CONTROL: the operator's own release says
/// nothing.
#[test]
fn an_over_ended_by_a_gate_says_so() {
    let mut e = station();
    let t0 = Instant::now();
    keyed(&mut e, t0);
    e.set_tune(true);
    let t = t0 + ms(220);
    e.push_pc_mic(&audio(20, 0.5), t);
    let _ = e.poll_pc_mic(t, TICK, 1);
    assert_eq!(lane(&e), Some(PcMicCause::GateDown));
    let mut e = station();
    keyed(&mut e, t0);
    e.set_ptt(false);
    assert_eq!(lane(&e), None, "CONTROL: the release is the operator's own");
    for (name, call) in [
        ("interrupted", Engine::pc_mic_interrupted as fn(&mut Engine)),
        ("key refused", Engine::pc_mic_key_refused),
    ] {
        let mut e = station();
        keyed(&mut e, t0);
        call(&mut e);
        assert!(!e.pc_mic_armed(), "{name}: still armed");
        assert!(e.take_pc_mic_abort(), "{name}: no unkey");
        assert!(lane(&e).is_some(), "{name}: the lane says nothing");
    }
}

/// ⛔ Digital silence (every sample exactly zero) never STARTS an over: a microphone that keeps
/// calling back with nothing but zeros (stalled, muted in hardware, asleep) keys nothing.
#[test]
fn digital_silence_never_keys() {
    let mut e = station();
    let t0 = Instant::now();
    e.set_ptt(true);
    for k in 0..300 {
        let t = t0 + ms(k * 20);
        e.push_pc_mic(&audio(20, 0.0), t);
        let tick = e.poll_pc_mic(t, TICK, 1);
        assert_ne!(tick, MicTick::Key, "digital silence keyed the radio");
    }
    // CONTROL: the first real audio keys it.
    let t = t0 + ms(6_000);
    e.push_pc_mic(&audio(20, 0.25), t);
    assert_eq!(
        e.poll_pc_mic(t, TICK, 1),
        MicTick::Key,
        "CONTROL: audio keys"
    );
}

/// Once keyed, digital silence is the pause between words of a microphone with a noise gate: a
/// second of it holds the over (it used to end at the 300 ms gap, mid-sentence), and the voice
/// after it goes out.
#[test]
fn a_gated_microphones_pause_does_not_end_the_over() {
    let mut e = station();
    let t0 = Instant::now();
    keyed(&mut e, t0);
    for k in 11..=60u64 {
        let t = t0 + ms(k * 20);
        e.push_pc_mic(&audio(20, 0.0), t);
        let _ = e.poll_pc_mic(t, TICK, 1);
        assert!(
            e.pc_mic_keyed(),
            "a {} ms pause ended the over",
            k * 20 - 200
        );
    }
    let mut voice = false;
    for k in 61..80u64 {
        let t = t0 + ms(k * 20);
        e.push_pc_mic(&audio(20, 0.25), t);
        if let MicTick::Samples(buf) = e.poll_pc_mic(t, TICK, 1) {
            voice |= buf.contains(&0.25);
        }
    }
    assert!(e.pc_mic_keyed(), "the voice after the pause did not hold");
    assert!(voice, "the voice after the pause never went out");
    assert!(!e.take_pc_mic_abort());
}

/// ⛔ An unbroken run of digital silence under a keyed over ends it at the silence limit, as no
/// audio: a stuck all-zero stream cannot hold the radio keyed. CONTROL: a quiet microphone's noise
/// floor holds the over past the same point.
#[test]
fn a_stuck_all_zero_microphone_ends_the_over_at_the_silence_limit() {
    let mut e = station();
    let t0 = Instant::now();
    keyed(&mut e, t0);
    let first_zero = 220; // the first delivery after `keyed`'s last audio
    let mut ended = None;
    for k in 11..400u64 {
        let t = t0 + ms(k * 20);
        e.push_pc_mic(&audio(20, 0.0), t);
        let tick = e.poll_pc_mic(t, TICK, 1);
        if !e.pc_mic_armed() {
            assert_eq!(tick, MicTick::Idle, "audio was due on the ending tick");
            ended = Some(k * 20 - first_zero);
            break;
        }
    }
    let after = ended.expect("a keyed over stayed up on digital silence");
    assert!(
        (DIGITAL_SILENCE_LIMIT_MS..=DIGITAL_SILENCE_LIMIT_MS + 20).contains(&after),
        "ended {after} ms into the silence"
    );
    assert!(e.take_pc_mic_abort(), "no unkey");
    assert_eq!(lane(&e), Some(PcMicCause::NoAudio));
    // CONTROL: a quiet microphone (its noise floor) holds the over.
    let mut e = station();
    keyed(&mut e, t0);
    for k in 11..400u64 {
        let t = t0 + ms(k * 20);
        e.push_pc_mic(&audio(20, 1e-4), t);
        let _ = e.poll_pc_mic(t, TICK, 1);
    }
    assert!(e.pc_mic_keyed(), "CONTROL: quiet audio ended the over");
}

/// A release and a new press before the loop's next tick: the new over is a new capture (its
/// generation moves), so the loop stops the old over and opens the microphone afresh for it.
#[test]
fn a_new_press_is_a_new_capture() {
    let mut e = station();
    let t0 = Instant::now();
    keyed(&mut e, t0);
    let first = e.pc_mic_source().unwrap();
    e.set_ptt(false);
    e.set_ptt(true);
    let second = e.pc_mic_source().expect("the re-press armed");
    assert_eq!(first.device, second.device);
    assert_ne!(first.gen, second.gen, "the re-press reuses the old capture");
    assert!(e.take_pc_mic_abort(), "the old over's unkey was lost");
}

// ---- The release tail (operator ruling: "Let ≤200 ms finish") ----
//
// On the operator's PTT release only, the microphone audio already taken before the release plays
// out, at most `RELEASE_TAIL_MS` of it, bounded by the wall clock from the release, and then the
// radio unkeys. Every other stop stays a hard cut. The scenes below put a known amount of audio in
// the over before the release: it all arrives at the key (`t0`, as one capture block), and the
// release falls at `t0 + 100 ms`, the moment the over's first sample is on the air (the 60 ms
// lead-in plus the 40 ms look-ahead), so everything taken is still to be heard at the release.

/// The level of the operator's voice before the release, and of anything captured after it.
const VOICE: f32 = 0.5;
const AFTER: f32 = 0.9;

/// A keyed over holding `queued_ms` of `VOICE` at the release; returns the engine, `t0`, the
/// release moment, and every sample fed before the release.
fn tail_scene(queued_ms: u64) -> (Engine, Instant, Instant, Vec<f32>) {
    let mut e = station();
    let t0 = Instant::now();
    e.set_ptt(true);
    assert_eq!(e.poll_pc_mic(t0, TICK, 1), MicTick::Armed);
    e.push_pc_mic(&audio(queued_ms, VOICE), t0);
    assert_eq!(
        e.poll_pc_mic(t0, TICK, 1),
        MicTick::Key,
        "premise: audio keys"
    );
    let mut fed = Vec::new();
    let mut t = 10;
    while t < 100 {
        if let MicTick::Samples(s) = e.poll_pc_mic(t0 + ms(t), 40.0, 1) {
            fed.extend(s);
        }
        t += 10;
    }
    (e, t0, t0 + ms(100), fed)
}

/// After the release at `r`: poll every 10 ms up to `r + until_ms`, the microphone still
/// delivering `AFTER` audio at each poll. Returns the samples fed after the release and the time
/// (ms after `r`) of the poll that ended the over, with its abort armed, if it ended.
fn run_tail(e: &mut Engine, r: Instant, until_ms: u64) -> (Vec<f32>, Option<u64>) {
    run_tail_every(e, r, until_ms, 10)
}

/// [`run_tail`], polling every `step` ms (a loop whose ticks do not fall on the bound).
fn run_tail_every(e: &mut Engine, r: Instant, until_ms: u64, step: u64) -> (Vec<f32>, Option<u64>) {
    let mut fed = Vec::new();
    let mut k = 0;
    while k <= until_ms {
        let t = r + ms(k);
        e.push_pc_mic(&audio(10, AFTER), t);
        match e.poll_pc_mic(t, 40.0, 1) {
            MicTick::Samples(s) => fed.extend(s),
            MicTick::Idle => {
                assert!(
                    !e.pc_mic_armed(),
                    "Idle at {k} ms with the over still armed"
                );
                assert!(
                    e.take_pc_mic_abort(),
                    "ended at {k} ms with no flush + unkey"
                );
                return (fed, Some(k));
            }
            _ => {}
        }
        k += step;
    }
    (fed, None)
}

fn count(samples: &[f32], level: f32) -> usize {
    samples.iter().filter(|&&x| x == level).count()
}

/// 150 ms queued at the release: all of it is sent, then the radio unkeys, when it has played
/// (150 ms after the release, and never past 200 ms). Nothing captured after the release is sent.
#[test]
fn a_release_with_150_ms_queued_sends_those_150_ms_then_unkeys() {
    let (mut e, _t0, r, before) = tail_scene(150);
    e.release_pc_mic_at(r);
    assert!(e.pc_mic_armed(), "the release cut the queued audio off");
    assert!(!e.take_pc_mic_abort(), "the release armed a flush");
    let (after, ended) = run_tail(&mut e, r, 400);
    let sent = count(&before, VOICE) + count(&after, VOICE);
    assert_eq!(
        sent,
        PC_MIC.samples(150) as usize,
        "not exactly the 150 ms queued were sent"
    );
    assert_eq!(
        count(&after, AFTER),
        0,
        "audio captured after the release was sent"
    );
    let ended = ended.expect("the over never ended");
    assert!(
        (150..=200).contains(&ended),
        "unkeyed {ended} ms after the release (want when the 150 ms have played, by 200)"
    );
    assert_eq!(
        e.snapshot().radio.pc_mic_ended,
        None,
        "the release is the operator's own"
    );
}

/// 400 ms queued at the release: at most 200 ms of it is sent, and the radio unkeys 200 ms after
/// the release by the wall clock, never "when the queue empties".
#[test]
fn a_release_with_400_ms_queued_sends_at_most_200_ms_then_unkeys() {
    let (mut e, _t0, r, before) = tail_scene(400);
    e.release_pc_mic_at(r);
    let (after, ended) = run_tail(&mut e, r, 600);
    let sent = count(&before, VOICE) + count(&after, VOICE);
    assert_eq!(
        sent,
        PC_MIC.samples(RELEASE_TAIL_MS) as usize,
        "not exactly 200 ms of the 400 ms queued were sent"
    );
    assert_eq!(
        count(&after, AFTER),
        0,
        "audio captured after the release was sent"
    );
    assert_eq!(
        ended,
        Some(RELEASE_TAIL_MS),
        "not unkeyed at 200 ms by the clock"
    );
    // A loop ticking every 30 ms, off the bound: still nothing past 200 ms is fed, and it unkeys
    // on its first tick at or after the bound.
    let (mut e, _t0, r, before) = tail_scene(400);
    e.release_pc_mic_at(r);
    let (after, ended) = run_tail_every(&mut e, r, 600, 30);
    assert_eq!(
        count(&before, VOICE) + count(&after, VOICE),
        PC_MIC.samples(RELEASE_TAIL_MS) as usize,
        "30 ms ticks: more than 200 ms was sent"
    );
    assert_eq!(
        ended,
        Some(210),
        "30 ms ticks: not unkeyed on the first tick past the bound"
    );
}

/// ⛔ Stop TX, TX Off, a radio handoff and the watchdog during the tail cut it at once: the over
/// ends at the stop itself (or on the watchdog's tick), with the flush + unkey armed, and nothing
/// more is fed.
#[test]
fn every_other_stop_during_the_tail_is_immediate() {
    type Stop = fn(&mut Engine);
    let stops: [(&str, Stop); 3] = [
        ("Stop TX", |e| e.halt_tx()),
        ("TX Off", |e| e.set_tx_enabled(false)),
        ("a radio handoff", |e| e.pc_mic_interrupted()),
    ];
    for (name, stop) in stops {
        let (mut e, _t0, r, _) = tail_scene(400);
        e.release_pc_mic_at(r);
        let (_, ended) = run_tail(&mut e, r, 50);
        assert_eq!(ended, None, "{name}: premise: the tail is playing");
        stop(&mut e);
        assert!(!e.pc_mic_armed(), "{name}: the tail outlived the stop");
        assert!(e.take_pc_mic_abort(), "{name}: no flush + unkey");
        assert_eq!(e.poll_pc_mic(r + ms(60), 40.0, 1), MicTick::Idle, "{name}");
        // The over was ending by the operator's own release: no fault is named for it.
        assert_eq!(
            lane(&e),
            None,
            "{name}: named as a fault in the release's tail"
        );
    }
    // The watchdog: its tick ends the tail, turns TX off and says so.
    let (mut e, _t0, r, _) = tail_scene(400);
    e.release_pc_mic_at(r);
    let (_, ended) = run_tail(&mut e, r, 50);
    assert_eq!(ended, None, "premise: the tail is playing");
    e.settings.tx_watchdog_min = 1;
    e.tx_watchdog_start = Some(now_unix_secs() - 120);
    assert_eq!(e.poll_pc_mic(r + ms(60), 40.0, 1), MicTick::Idle);
    assert!(
        !e.pc_mic_armed() && e.take_pc_mic_abort(),
        "the watchdog did not cut the tail"
    );
    assert!(!e.tx_enabled(), "the watchdog left TX on");
    assert_eq!(lane(&e), Some(PcMicCause::Watchdog));
}

/// ⛔ A loop that stops ticking after the release (wedged, or late) feeds nothing more when it
/// comes back past the bound: that tick ends the over with the flush + unkey, at once.
#[test]
fn a_wedged_loop_still_unkeys_within_the_bound() {
    for (queued, back_at) in [(400, 500), (150, 210), (400, RELEASE_TAIL_MS)] {
        let (mut e, _t0, r, _) = tail_scene(queued);
        e.release_pc_mic_at(r);
        e.push_pc_mic(&audio(20, AFTER), r + ms(back_at));
        assert_eq!(
            e.poll_pc_mic(r + ms(back_at), 40.0, 1),
            MicTick::Idle,
            "{queued} ms queued, back at {back_at} ms: the tail carried on"
        );
        assert!(
            !e.pc_mic_armed() && e.take_pc_mic_abort(),
            "{queued}/{back_at}: no unkey"
        );
    }
}

/// The over's gap (no audio for 300 ms) is no fault in the tail: the operator stopped speaking on
/// purpose, and the wall clock bounds it. A tail whose last audio arrived 250 ms before the release
/// still plays its 200 ms out, and the lane stays quiet.
#[test]
fn the_tail_is_not_cut_by_the_gap() {
    let mut e = station();
    let t0 = Instant::now();
    e.set_ptt(true);
    assert_eq!(e.poll_pc_mic(t0, TICK, 1), MicTick::Armed);
    e.push_pc_mic(&audio(440, VOICE), t0);
    assert_eq!(e.poll_pc_mic(t0, TICK, 1), MicTick::Key);
    let mut fed = Vec::new();
    let mut t = 10;
    while t < 250 {
        if let MicTick::Samples(s) = e.poll_pc_mic(t0 + ms(t), 40.0, 1) {
            fed.extend(s);
        }
        t += 10;
    }
    let r = t0 + ms(250);
    e.release_pc_mic_at(r);
    let (after, ended) = run_tail(&mut e, r, 400);
    assert_eq!(
        ended,
        Some(RELEASE_TAIL_MS),
        "the tail did not play to its bound"
    );
    // Media 0 played at t0 + 100 ms: by r + 200 ms, 350 ms of the voice has been sent.
    assert_eq!(
        count(&fed, VOICE) + count(&after, VOICE),
        PC_MIC.samples(350) as usize
    );
    assert_eq!(lane(&e), None, "the release's tail was named as a fault");
}

/// A press during the tail cuts the tail (flush + unkey) and starts a fresh over, which keys on
/// its own audio, never on the old over's.
#[test]
fn a_press_during_the_tail_starts_a_fresh_over() {
    let (mut e, _t0, r, _) = tail_scene(400);
    let gen = e.pc_mic_source().expect("premise: an over").gen;
    e.release_pc_mic_at(r);
    let (_, ended) = run_tail(&mut e, r, 20);
    assert_eq!(ended, None);
    e.set_ptt(true);
    assert!(e.take_pc_mic_abort(), "the old over was not unkeyed");
    assert!(e.pc_mic_armed() && !e.pc_mic_keyed(), "no fresh over armed");
    assert_eq!(e.pc_mic_source().map(|s| s.gen), Some(gen + 1));
    assert_eq!(e.poll_pc_mic(r + ms(30), TICK, 1), MicTick::Armed);
    e.push_pc_mic(&audio(20, AFTER), r + ms(40));
    assert_eq!(e.poll_pc_mic(r + ms(40), TICK, 1), MicTick::Key);
}

/// The operator's PTT release (`set_ptt(false)`) starts the tail on the wall clock at the release,
/// and a second release never moves it; an over that never keyed ends at once.
#[test]
fn the_release_is_timed_from_the_moment_ptt_is_let_go() {
    let mut e = station();
    let t0 = Instant::now();
    keyed(&mut e, t0);
    let before = Instant::now();
    e.set_ptt(false);
    let after = Instant::now();
    let until = e.pc_mic.tail.expect("the release played no tail");
    assert!(
        until >= before + ms(RELEASE_TAIL_MS) && until <= after + ms(RELEASE_TAIL_MS),
        "the tail is not timed from the release"
    );
    e.release_pc_mic_at(after + ms(50));
    assert_eq!(
        e.pc_mic.tail,
        Some(until),
        "a second release moved the bound"
    );
    // Armed, never keyed: nothing is on the air, so the release ends it at once.
    let mut e = station();
    e.set_ptt(true);
    assert!(e.pc_mic_armed() && !e.pc_mic_keyed());
    e.set_ptt(false);
    assert!(!e.pc_mic_armed() && !e.take_pc_mic_abort());
}
