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
    // The release: dropped now, abort armed, and nothing after it reaches the transmitter.
    e.set_ptt(false);
    assert!(!e.pc_mic_armed());
    assert!(
        e.take_pc_mic_abort(),
        "the release did not arm the flush + unkey"
    );
    assert_eq!(
        e.pc_mic_source(),
        None,
        "the microphone stayed open after the release"
    );
    let t2 = t1 + ms(500);
    e.push_pc_mic(&audio(100, 0.5), t2);
    assert_eq!(e.poll_pc_mic(t2, TICK, 1), MicTick::Idle);
}

#[test]
fn every_stop_ends_a_keyed_over_within_one_tick() {
    type Stop = fn(&mut Engine);
    // `true`: the stop itself ends the over, before any tick (the operator's own stops and a lost
    // microphone); `false`: the next tick's gates end it.
    let stops: [(&str, bool, Stop); 8] = [
        ("Stop TX (halt_tx)", true, |e| e.halt_tx()),
        ("TX Off (the latch)", true, |e| e.set_tx_enabled(false)),
        ("PTT release", true, |e| e.set_ptt(false)),
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
    e.set_ptt(false);
    // The radio's own receive input as the "microphone" would put the band on the air.
    e.settings.pc_mic_device = "Radio Codec".into();
    e.set_ptt(true);
    assert!(!e.pc_mic_armed());
    assert_eq!(
        e.snapshot().radio.pc_mic_ended.map(|x| x.cause),
        Some(PcMicCause::SameAsReceive)
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

/// ⛔ No feedback path: the microphone is refused, with the reason, whenever it may carry receive
/// audio or Nexus's own: the radio's receive input under another ALSA name or ordinal, another
/// radio's receive input, a Flex DAX receive channel, and a loopback or virtual cable.
/// CONTROL: an ordinary microphone arms, and the exact-name case still refuses.
#[test]
fn a_microphone_that_may_carry_receive_or_transmit_audio_is_refused() {
    let cases: [(&str, &str, PcMicCause); 9] = [
        ("Radio Codec", "Radio Codec", PcMicCause::SameAsReceive),
        ("radio codec #2", "Radio Codec", PcMicCause::SameAsReceive),
        (
            "plughw:CARD=Codec,DEV=0",
            "hw:CARD=Codec,DEV=0",
            PcMicCause::SameAsReceive,
        ),
        ("dsnoop:1,0", "hw:1,0", PcMicCause::SameAsReceive),
        ("Second Codec", "Radio Codec", PcMicCause::RadioReceive),
        (
            "DAX Audio RX 1 (FlexRadio Systems DAX Audio)",
            "Radio Codec",
            PcMicCause::RadioReceive,
        ),
        (
            "CABLE Output (VB-Audio Virtual Cable)",
            "Radio Codec",
            PcMicCause::Loopback,
        ),
        (
            "Stereo Mix (Realtek(R) Audio)",
            "Radio Codec",
            PcMicCause::Loopback,
        ),
        (
            "Monitor of Built-in Audio Analog Stereo",
            "Radio Codec",
            PcMicCause::Loopback,
        ),
    ];
    for (mic, rx, cause) in cases {
        let mut e = station();
        e.settings.audio_in = rx.into();
        e.settings.radios[0].audio_in = rx.into();
        e.settings.radios[1].audio_in = "Second Codec".into();
        e.settings.pc_mic_device = mic.into();
        e.set_ptt(true);
        assert!(!e.pc_mic_armed(), "{mic:?} armed beside {rx:?}");
        assert_eq!(lane(&e), Some(cause), "{mic:?} beside {rx:?}");
    }
    let mut e = station();
    e.settings.radios[1].audio_in = "Second Codec".into();
    e.set_ptt(true);
    assert!(e.pc_mic_armed(), "CONTROL: an ordinary microphone");
}

#[test]
fn device_names_compare_as_the_same_input_when_in_doubt() {
    use super::same_audio_device as same;
    assert!(same("USB Audio CODEC ", "usb audio codec"));
    assert!(same("USB Audio CODEC #2", "USB Audio CODEC"));
    assert!(same("hw:CARD=Device,DEV=0", "default:CARD=Device"));
    assert!(same("plughw:2,0", "hw:2"));
    assert!(!same("hw:CARD=Device,DEV=0", "hw:CARD=Other,DEV=0"));
    assert!(!same("hw:1,0", "hw:2,0"));
    assert!(!same("Desk Microphone", "USB Audio CODEC"));
    assert!(!same("", ""), "no device is no device");
    assert!(
        !same("Desk Microphone", ""),
        "an unset receive input is the loop's to check"
    );
}

/// ⛔ The loop's check of the input the sound card actually opened (the system default when none
/// is set) ends the over before the microphone opens, and says why.
#[test]
fn the_input_receive_actually_opened_is_refused_at_the_loop() {
    let mut e = station();
    e.set_ptt(true);
    assert!(e.pc_mic_armed());
    e.pc_mic_is_receive();
    assert!(!e.pc_mic_armed());
    assert!(
        !e.take_pc_mic_abort(),
        "nothing was keyed, nothing to unkey"
    );
    assert_eq!(lane(&e), Some(PcMicCause::SameAsReceive));
}

/// ⛔ Nexus's own Flex client giving the radio its own mic: the press arms nothing and says why
/// (the PC audio would leave over DAX while the radio transmits its own mic), and an over already
/// keyed when the radio takes its mic ends on the next tick.
#[test]
fn a_flex_with_its_own_mic_refuses_the_computer_microphone() {
    let mut e = station();
    e.observe_flex_radio_has_mic(true);
    e.set_ptt(true);
    assert!(!e.pc_mic_armed(), "armed while the radio has its own mic");
    assert!(
        !e.manual_ptt(),
        "the press fell back to keying the radio's mic"
    );
    assert_eq!(lane(&e), Some(PcMicCause::FlexMic));
    e.set_ptt(false);
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
