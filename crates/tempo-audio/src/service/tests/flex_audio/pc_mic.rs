//! The computer's microphone in Phone at the shack through Nexus's own Flex client with native DAX
//! audio on (`tempo_app::engine::pc_mic`): the radio has its own mic between overs, and a held PTT
//! rides a DAX HOLD for as long as it is held. Nexus asks the radio for DAX, keys only once the radio
//! reports it, carries the microphone over DAX, unkeys at the release (or at any stop) and gives the
//! mic back only after the radio has proved the unkey. Every transmit guard still cuts the over: TX
//! Off, the watchdog, Stop TX, a handoff, a lost connection, the radio leaving DAX.
//!
//! The radio is the SmartSDR simulator and the loop the real `RadioLoop::step`, as in the parent
//! module; the microphone is the scene's sound card delivering a steady level on the real clock.
//! What the radio saw is read by value, in its order ([`voice_trace`]).

use super::voice_keyer::{
    dax_in, holder_kept_in_usb, phone, run_until_count, run_until_seen, seen_at, send, shack,
    shack_on, slow_dax, slow_mic_back, voice_trace, with_rule, IDLE,
};
use super::*;

/// The microphone, by a card name every host's naming tells apart from the receive inputs.
const DEVICE: &str = "plughw:CARD=Headset,DEV=0";

/// Switch the computer's microphone on for the scene's radio.
fn mic_on(s: &FlexScene) {
    let mut e = s.engine.lock().unwrap();
    let mut settings = e.settings().clone();
    settings.pc_mic = true;
    settings.pc_mic_device = DEVICE.into();
    e.apply_settings(settings);
}

/// Phone at the shack on `session`, settled, with the computer's microphone on.
fn shack_with_mic(session: SimSession) -> FlexScene {
    let s = shack(session);
    mic_on(&s);
    s
}

/// Why the last computer-microphone over ended or never keyed, as the UI receives it, or null.
fn ended(s: &FlexScene) -> serde_json::Value {
    let snap = serde_json::to_value(s.engine.lock().unwrap().snapshot()).unwrap();
    snap["radio"]["pcMicEnded"]["cause"].clone()
}

/// Every open and close of the microphone the loop asked for.
fn mic_calls(s: &FlexScene) -> Vec<Option<String>> {
    s.backend.pc_mic.lock().unwrap().calls.clone()
}

fn ptt(s: &FlexScene, on: bool) {
    s.engine.lock().unwrap().set_ptt(on);
}

/// The whole over as the radio saw it, from the press to the mic coming back.
const WHOLE_OVER: [&str; 8] = [
    "stream create type=dax_tx",
    "transmit set dax=1",
    "radio dax=1",
    "xmit 1",
    "xmit 0",
    "idle",
    "transmit set dax=0",
    "radio dax=0",
];

/// ⭐ A HELD OVER ON THE DAX HOLD. The radio has its mic and takes 600 ms to report DAX after the
/// write. The press keys nothing until the radio reports DAX (the microphone is open and delivering
/// all the while), then the microphone's audio goes out over DAX for as long as PTT is held, the
/// release unkeys, and the mic comes back only after the radio has let go of the transmitter. The
/// engine is never told the radio lost the mic (APRS and SSTV stay refused through it), the
/// microphone is open only for the over, and the lane says nothing.
#[test]
fn a_held_over_waits_for_dax_carries_the_microphone_and_gives_the_mic_back() {
    let mut s = shack_with_mic(slow_dax(600));
    assert!(
        mic_calls(&s).is_empty(),
        "premise: the microphone is closed"
    );
    let since = Instant::now();
    ptt(&s, true);
    assert!(
        s.engine.lock().unwrap().pc_mic_dax_hold().is_some(),
        "the press armed on the DAX hold"
    );
    let told = run_until_seen(&mut s, since, "xmit 1");
    let opened = s.backend.pc_mic.lock().unwrap().calls.len();
    assert_eq!(opened, 1, "the microphone opened once, for the press");
    // Held well past the press: the over stays up and carries the microphone.
    s.run(2_000);
    assert!(
        s.engine.lock().unwrap().pc_mic_keyed(),
        "the held over ended: {} {:?}",
        ended(&s),
        voice_trace(&s, since)
    );
    assert_eq!(keys(&s), 1);
    ptt(&s, false);
    let told_after = run_until_seen(&mut s, since, "radio dax=0");
    s.run(200);
    assert_eq!(voice_trace(&s, since), WHOLE_OVER);
    assert_eq!(ended(&s), serde_json::Value::Null, "the lane spoke");
    assert_eq!(
        mic_calls(&s),
        [Some(DEVICE.to_string()), None],
        "the microphone opened for the press and closed after it"
    );
    assert_eq!(
        told.union(&told_after).copied().collect::<Vec<_>>(),
        [(true, false)],
        "(has the mic, mic off) as the engine was told, every tick"
    );
    let at = |want: &str| seen_at(&s, since, want).expect(want);
    let wrote = at("transmit set dax=1");
    let reported = at("radio dax=1");
    assert!(
        reported.duration_since(wrote) >= Duration::from_millis(500),
        "premise: the radio was slow to switch"
    );
    let audio = dax_in(&s, since);
    assert!(
        audio
            .iter()
            .all(|(t, _)| *t > reported && *t < at("xmit 0")),
        "every DAX datagram after the radio took DAX and before the unkey"
    );
    let frames: usize = audio.iter().map(|(_, n)| n).sum();
    assert!(
        frames >= 24_000 * 3 / 2,
        "the held over carried the microphone: {frames} frames at 24 kHz"
    );
}

/// ⛔ A QUICK RE-PRESS WHILE THE MIC IS COMING BACK. After an over, Nexus writes the radio's mic
/// back; until the radio reports it (400 ms here), the radio still says DAX. A press in that window
/// (a quick "go ahead" release, a double-tap) must not key on that stale DAX: the radio would get
/// `xmit 1` after `transmit set dax=0` and, if it applies them in order, put its own mic (the room)
/// on the air. It keys only once the radio has reported the mic and then DAX again, and carries the
/// computer's microphone as any over does. CONTROL: a press once the mic is reported back keys the
/// same way.
#[test]
fn a_quick_re_press_keys_only_once_the_radio_is_back_on_dax() {
    for quick in [true, false] {
        let mut s = shack_with_mic(slow_mic_back(400));
        let since = Instant::now();
        ptt(&s, true);
        run_until_seen(&mut s, since, "xmit 1");
        s.run(500);
        ptt(&s, false);
        if quick {
            run_until_seen(&mut s, since, "transmit set dax=0");
            assert!(
                seen_at(&s, since, "radio dax=0").is_none(),
                "quick={quick}: premise: the mic's write is in flight"
            );
        } else {
            run_until_seen(&mut s, since, "radio dax=0");
            s.run(200);
        }
        ptt(&s, true);
        run_until_count(&mut s, since, "xmit 1", 2);
        s.run(500);
        let keyed = s.engine.lock().unwrap().pc_mic_keyed();
        let held = format!("keyed={keyed} ended={} keys={}", ended(&s), keys(&s));
        ptt(&s, false);
        run_until_count(&mut s, since, "radio dax=0", 2);
        s.run(200);
        assert_eq!(
            format!("{held} wire={:?}", voice_trace(&s, since)),
            "keyed=true ended=null keys=2 wire=[\"stream create type=dax_tx\", \
             \"transmit set dax=1\", \"radio dax=1\", \"xmit 1\", \"xmit 0\", \"idle\", \
             \"transmit set dax=0\", \"radio dax=0\", \"transmit set dax=1\", \"radio dax=1\", \
             \"xmit 1\", \"xmit 0\", \"idle\", \"transmit set dax=0\", \"radio dax=0\"]",
            "quick={quick}"
        );
        let second = dax_in(&s, since)
            .into_iter()
            .filter(|(t, _)| *t > seen_at(&s, since, "radio dax=0").unwrap())
            .count();
        assert!(
            second > 0,
            "quick={quick}: the re-press carried no microphone"
        );
    }
}

/// What the radio sees for one held over that waits for the mic's write and then DAX, from the mic's
/// write to the mic back after it.
const MIC_THEN_OVER: [&str; 9] = [
    "transmit set dax=0",
    "radio dax=0",
    "transmit set dax=1",
    "radio dax=1",
    "xmit 1",
    "xmit 0",
    "idle",
    "transmit set dax=0",
    "radio dax=0",
];

/// ⛔ A PRESS RIGHT AFTER A DIGITAL MODE. From FT8 to Phone at the shack Nexus writes the radio's
/// mic; until the radio reports it (600 ms here) the radio still says DAX. A press in that window
/// must not take the stale DAX for the computer's route: it would key without the hold, and the
/// radio would get `xmit 1` after `transmit set dax=0` and put its own mic (the room) on the air.
/// The press counts the mic's write as the radio having its mic, arms on the hold, and keys only
/// once the radio has reported the mic and then DAX again. CONTROL: a press once the mic is
/// reported keys the same way.
#[test]
fn a_press_right_after_a_digital_mode_keys_only_once_the_radio_is_on_dax() {
    for quick in [true, false] {
        let boot = Instant::now();
        let mut s = FlexScene::with_session(true, slow_mic_back(600));
        mic_on(&s);
        run_until(&mut s, "the radio never took DAX for FT8", |s| {
            seen_at(s, boot, "radio dax=1").is_some()
        });
        s.run(300);
        let since = Instant::now();
        {
            let mut e = s.engine.lock().unwrap();
            e.set_operating_mode("phone", false); // arms TX
            e.set_frequency(14.250, "20m", "USB");
        }
        run_until_seen(&mut s, since, "transmit set dax=0");
        if quick {
            assert!(
                seen_at(&s, since, "radio dax=0").is_none(),
                "quick={quick}: premise: the mic's write is in flight"
            );
        } else {
            run_until_seen(&mut s, since, "radio dax=0");
            s.run(200);
        }
        ptt(&s, true);
        let hold = s.engine.lock().unwrap().pc_mic_dax_hold().is_some();
        run_until_seen(&mut s, since, "xmit 1");
        s.run(500);
        let keyed = s.engine.lock().unwrap().pc_mic_keyed();
        let held = format!(
            "hold={hold} keyed={keyed} ended={} keys={}",
            ended(&s),
            keys(&s)
        );
        ptt(&s, false);
        run_until_count(&mut s, since, "radio dax=0", 2);
        s.run(200);
        assert_eq!(
            format!("{held} wire={:?}", voice_trace(&s, since)),
            format!("hold=true keyed=true ended=null keys=1 wire={MIC_THEN_OVER:?}"),
            "quick={quick}"
        );
        let reported = seen_at(&s, since, "radio dax=1").unwrap();
        assert!(
            dax_in(&s, since).iter().any(|(t, _)| *t > reported),
            "quick={quick}: the press carried no microphone"
        );
    }
}

/// ⛔ A RE-PRESS WHILE A SLOW MIC WRITE OUTLIVES THE ROUTING'S CONFIRM WINDOW. The radio reports the
/// mic 2.6 s after Nexus writes it, later than the routing's own 2 s window, after which the
/// routing decides again. A press just before that window ends must still not key on the radio's
/// stale DAX: for keying, the write stays in flight until the radio reports it (or refuses it), so
/// the press waits for the mic's report, then DAX again, and only then keys.
#[test]
fn a_re_press_waits_for_a_mic_write_the_radio_reports_late() {
    let mut s = shack_with_mic(slow_mic_back(2_600));
    let since = Instant::now();
    ptt(&s, true);
    run_until_seen(&mut s, since, "xmit 1");
    s.run(500);
    ptt(&s, false);
    run_until_seen(&mut s, since, "transmit set dax=0");
    s.run(1_800);
    assert!(
        seen_at(&s, since, "radio dax=0").is_none(),
        "premise: the mic's write is still in flight"
    );
    ptt(&s, true);
    run_until_count(&mut s, since, "xmit 1", 2);
    s.run(500);
    let keyed = s.engine.lock().unwrap().pc_mic_keyed();
    let held = format!("keyed={keyed} ended={} keys={}", ended(&s), keys(&s));
    ptt(&s, false);
    run_until_count(&mut s, since, "radio dax=0", 2);
    s.run(200);
    assert_eq!(
        format!("{held} wire={:?}", voice_trace(&s, since)),
        "keyed=true ended=null keys=2 wire=[\"stream create type=dax_tx\", \
         \"transmit set dax=1\", \"radio dax=1\", \"xmit 1\", \"xmit 0\", \"idle\", \
         \"transmit set dax=0\", \"radio dax=0\", \"transmit set dax=1\", \"radio dax=1\", \
         \"xmit 1\", \"xmit 0\", \"idle\", \"transmit set dax=0\", \"transmit set dax=0\", \
         \"radio dax=0\"]",
        "the last write of the mic, reported late again, is the routing's to try again at 2 s"
    );
}

/// …and a mic write the radio never answers at all (no report, no reply): the press is not locked
/// out silently. It keys nothing and ends in the routing's confirm window with its reason, as a
/// press the radio never switches for does.
#[test]
fn a_re_press_after_a_mic_write_the_radio_never_answers_keys_nothing_and_says_so() {
    let boot = Instant::now();
    let mut s = FlexScene::with_faults(
        true,
        reports(&["USB"]),
        vec![tempo_flexsim::Fault::Unanswered {
            starting: "transmit set dax=0".into(),
            then_silent: false,
        }],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    mic_on(&s);
    run_until(&mut s, "the radio never took DAX for FT8", |s| {
        seen_at(s, boot, "radio dax=1").is_some()
    });
    let since = Instant::now();
    {
        let mut e = s.engine.lock().unwrap();
        e.set_operating_mode("phone", false); // arms TX
        e.set_frequency(14.250, "20m", "USB");
    }
    run_until_seen(&mut s, since, "transmit set dax=0");
    s.run(2_500);
    ptt(&s, true);
    s.run(3_000);
    let trace = voice_trace(&s, since);
    assert_eq!(
        format!(
            "ended={} keys={} reported={}",
            ended(&s),
            keys(&s),
            trace.iter().any(|t| t.starts_with("radio "))
        ),
        "ended=\"flexMic\" keys=0 reported=false",
        "{trace:?}"
    );
    ptt(&s, false);
}

/// What ends the over in [`every_transmit_guard_cuts_an_over_on_the_dax_hold`].
type Stop = fn(&mut FlexScene);

/// ⛔ EVERY TRANSMIT GUARD STILL CUTS AN OVER ON THE DAX HOLD, with PTT still held: TX Off (the
/// latch), the TX watchdog, Stop TX and a radio handoff each unkey within a tick or two of the stop,
/// no DAX leaves after the unkey, the microphone closes, nothing re-keys while PTT stays down, and
/// the radio's DAX is released (the mic back) once the radio has let go of the transmitter.
/// CONTROL: the release, the operator's own stop, the same way once its tail has played.
#[test]
fn every_transmit_guard_cuts_an_over_on_the_dax_hold() {
    let stops: [(&str, Stop); 5] = [
        ("TX Off (the latch)", |s| {
            s.engine.lock().unwrap().set_tx_enabled(false)
        }),
        ("a watchdog trip", |s| {
            s.engine.lock().unwrap().trip_tx_watchdog_for_test()
        }),
        ("Stop TX", |s| s.engine.lock().unwrap().halt_tx()),
        ("a radio handoff", |s| s.state.handoff_deferred = true),
        ("CONTROL: the release", |s| ptt(s, false)),
    ];
    for (name, stop) in stops {
        let mut s = shack_with_mic(reports(&["USB"]));
        let since = Instant::now();
        ptt(&s, true);
        run_until_seen(&mut s, since, "xmit 1");
        s.run(600);
        assert!(
            s.engine.lock().unwrap().pc_mic_keyed(),
            "{name}: premise: keyed"
        );
        stop(&mut s);
        let stopped = Instant::now();
        s.run(300);
        s.state.handoff_deferred = false;
        assert!(
            s.sim.wait_for(Duration::from_secs(2), |log| log.iter().any(
                |l| matches!(&l.event, SimEvent::Command { text, .. } if text == "xmit 0")
            )),
            "{name}: no unkey"
        );
        let unkey = seen_at(&s, since, "xmit 0").unwrap();
        assert!(
            unkey.duration_since(stopped) < Duration::from_millis(500),
            "{name}: the unkey waited {:?}",
            unkey.duration_since(stopped)
        );
        // PTT stays held (except the release's own row): nothing comes back on the air.
        run_until_seen(&mut s, since, "radio dax=0");
        s.run(500);
        // Stop TX's own hard stop sends an unkey as well as the over's: the same stop twice.
        let mut trace = voice_trace(&s, since);
        trace.dedup();
        assert_eq!(trace, WHOLE_OVER, "{name}");
        assert_eq!(keys(&s), 1, "{name}: re-keyed");
        let e = s.engine.lock().unwrap();
        assert!(!e.pc_mic_armed(), "{name}: the over outlived the stop");
        assert_eq!(e.pc_mic_dax_hold(), None, "{name}: the hold outlived it");
        drop(e);
        assert_eq!(
            mic_calls(&s),
            [Some(DEVICE.to_string()), None],
            "{name}: the microphone"
        );
        assert!(
            dax_in(&s, since).iter().all(|(t, _)| *t < unkey),
            "{name}: DAX after the unkey"
        );
        ptt(&s, false);
    }
}

/// ⛔ A LOST FLEX CONNECTION MID-OVER. The radio drops the session 300 ms into the over and stays
/// keyed. No DAX leaves after Nexus reads the loss; the operator is told the radio may still be
/// transmitting; the over ends (the PTT still held keys nothing more); the loop starts a fresh
/// session, whose first stop unkeys the lost session's transmitter, and the radio's DAX source goes
/// back to the mic only after that.
#[test]
fn a_session_lost_mid_over_ends_it_stops_dax_and_unkeys_before_the_mic_comes_back() {
    let mut s = FlexScene::with_faults(
        true,
        holder_kept_in_usb(),
        vec![tempo_flexsim::Fault::DisconnectMidOver {
            after: Duration::from_millis(300),
            radio_stays_keyed: true,
        }],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    s.replace_on_rebuild = true;
    shack_on(&mut s);
    mic_on(&s);
    let since = Instant::now();
    ptt(&s, true);
    let detail = status_after_the_rebuild(&mut s);
    assert!(
        detail.starts_with(
            "the connection to the radio was lost during a transmission — it may still be \
             transmitting. Check the radio now."
        ),
        "{detail}"
    );
    s.run(200);
    assert!(
        !s.engine.lock().unwrap().pc_mic_armed(),
        "the over outlived its session"
    );
    let log = s.log();
    let lost = log
        .iter()
        .find(|(at, e)| *at >= since && matches!(e, SimEvent::Closed { conn: 0, .. }))
        .map(|(at, _)| *at)
        .expect("the first session closed");
    let dax = dax_in(&s, since);
    let route = s.backend.installed.lock().unwrap()[0].clone();
    assert!(
        dax.iter().any(|(t, _)| *t < lost),
        "premise: the over was on the air when the session was lost"
    );
    assert_eq!(
        Some(dax.len()),
        route.sent_before_its_session_closed(),
        "DAX after the session was lost"
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    while !sent(&s.sim.log(), 1)
        .iter()
        .any(|c| c == "transmit set dax=0")
    {
        assert!(
            Instant::now() < deadline,
            "the new session never put the mic back: {:?}",
            sent(&s.sim.log(), 1)
        );
        s.run(100);
    }
    let on_new = sent(&s.sim.log(), 1);
    let unkey = on_new.iter().position(|c| c == "xmit 0");
    let mic = on_new.iter().position(|c| c == "transmit set dax=0");
    assert!(
        unkey.is_some() && unkey < mic,
        "the new session's unkey goes first: {on_new:?}"
    );
    assert!(
        !on_new.iter().any(|c| c == "xmit 1"),
        "the held PTT keyed the new session: {on_new:?}"
    );
    assert_eq!(
        mic_calls(&s).last(),
        Some(&None),
        "the microphone was left open"
    );
    ptt(&s, false);
}

/// The bundled radio, reporting its DAX source back to the mic `ms` after each `xmit 1`: the
/// operator pressing SmartSDR's own DAX button mid-over.
fn leaves_dax_mid_over(ms: u64) -> SimSession {
    let mut session = reports(&["USB"]);
    let (_, rules) = session
        .rules
        .iter_mut()
        .find(|(p, _)| *p == Pattern::Exact("xmit 1".to_string()))
        .expect("the bundled session answers xmit 1");
    for rule in rules.iter_mut() {
        rule.items.push(Item::Wait(ms));
        rule.items.push(send("S0|transmit dax=0"));
    }
    session
}

/// ⛔ The radio leaving DAX under the over (SmartSDR's own switch): the radio's mic would carry the
/// rest of it, so it ends at once with the reason on the lane, and nothing re-keys while PTT stays
/// down.
#[test]
fn the_radio_leaving_dax_mid_over_ends_it() {
    let mut s = shack_with_mic(leaves_dax_mid_over(500));
    let since = Instant::now();
    ptt(&s, true);
    run_until_seen(&mut s, since, "xmit 1");
    run_until_seen(&mut s, since, "xmit 0");
    s.run(500);
    let left = seen_at(&s, since, "radio dax=0").expect("the radio left DAX");
    let unkey = seen_at(&s, since, "xmit 0").unwrap();
    assert!(left < unkey, "premise: the unkey followed the radio");
    assert!(
        unkey.duration_since(left) < Duration::from_millis(500),
        "the unkey waited {:?}",
        unkey.duration_since(left)
    );
    assert_eq!(keys(&s), 1, "re-keyed");
    assert!(!s.engine.lock().unwrap().pc_mic_armed());
    assert_eq!(ended(&s), "gateDown");
    ptt(&s, false);
}

/// ⛔ THE RADIO NEVER SWITCHES: it never answers the DAX source write, so it goes on taking its mic.
/// The press keys nothing and ends 2 seconds after it (the routing's confirm window) with its
/// reason, the microphone closed; a PTT still held never keys later.
#[test]
fn a_press_the_radio_never_switches_for_keys_nothing_and_says_so() {
    let mut s = FlexScene::with_faults(
        true,
        reports(&["USB"]),
        vec![tempo_flexsim::Fault::Unanswered {
            starting: "transmit set dax".into(),
            then_silent: false,
        }],
        tempo_net::flex::session::Config::new(Station::new("Nexus").unwrap()),
    );
    shack_on(&mut s);
    mic_on(&s);
    let since = Instant::now();
    ptt(&s, true);
    s.run(1_500);
    let waiting = s.engine.lock().unwrap().pc_mic_armed();
    s.run(1_000);
    let after = s.engine.lock().unwrap().pc_mic_armed();
    s.run(1_000);
    assert_eq!(
        format!(
            "armed at 1.5 s {waiting}, at 2.5 s {after}, ended={} keys={} wire={:?}",
            ended(&s),
            keys(&s),
            voice_trace(&s, since)
        ),
        "armed at 1.5 s true, at 2.5 s false, ended=\"flexMic\" keys=0 \
         wire=[\"stream create type=dax_tx\", \"transmit set dax=1\"]"
    );
    assert_eq!(mic_calls(&s), [Some(DEVICE.to_string()), None]);
    ptt(&s, false);
}

/// …and the radio keyed by something else while the press waits for DAX (the operator's own mic
/// PTT on the radio): the press ends with nothing keyed and says so, though the radio then reports
/// DAX with its transmitter free.
#[test]
fn a_press_waiting_for_dax_ends_when_the_radio_is_keyed_first() {
    let mic = with_rule(
        reports(&["USB"]),
        "transmit set dax=1",
        vec![
            send(
                "S0|interlock tx_client_handle=0x00000000 state=PTT_REQUESTED reason= source=MIC \
                 tx_allowed=1 amplifier=",
            ),
            Item::Wait(30),
            send(
                "S0|interlock tx_client_handle=0x00000000 state=TRANSMITTING reason= source=MIC \
                 tx_allowed=1 amplifier=",
            ),
            Item::Wait(400),
            send(IDLE),
            Item::Wait(200),
            send("S0|transmit dax=1"),
        ],
    );
    let mut s = shack_with_mic(mic);
    let since = Instant::now();
    ptt(&s, true);
    run_until_seen(&mut s, since, "radio dax=0");
    s.run(500);
    assert_eq!(
        format!(
            "ended={} keys={} wire={:?}",
            ended(&s),
            keys(&s),
            voice_trace(&s, since)
        ),
        "ended=\"interrupted\" keys=0 wire=[\"stream create type=dax_tx\", \
         \"transmit set dax=1\", \"idle\", \"radio dax=1\", \"transmit set dax=0\", \
         \"radio dax=0\"]"
    );
    ptt(&s, false);
}

/// CONTROL: the computer's microphone off (the default), the press keys the radio's own mic exactly
/// as before: no DAX source written, the microphone never opened.
#[test]
fn with_the_computer_microphone_off_the_press_keys_the_radios_mic_as_before() {
    let mut s = phone();
    let since = Instant::now();
    ptt(&s, true);
    run_until_seen(&mut s, since, "xmit 1");
    s.run(300);
    ptt(&s, false);
    run_until_seen(&mut s, since, "xmit 0");
    s.run(300);
    assert_eq!(voice_trace(&s, since), ["xmit 1", "xmit 0", "idle"]);
    assert!(mic_calls(&s).is_empty(), "the microphone opened");
}
