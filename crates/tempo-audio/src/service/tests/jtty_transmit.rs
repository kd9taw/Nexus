//! JTTY's transmitter at the radio loop. With TX off nothing of JTTY's reaches PTT, the audio
//! output or CAT: asserted on the keying seam's own records (a logging rigctld's command log, the
//! mock card's samples and flushes) against a control run that never asked. With TX on, what the
//! loop plays reads back as sent, chained segments ride one key, and every stop, and a licence gate
//! closing, cuts it within a tick. Each test holds the remote-selection tests' lock: its sends
//! render under the process-wide modem mutex, which those tests need free.

use super::*;

/// A scene: the engine, the loop, the mock card, the rig, and the rigctld's log of every command.
type Scene = (
    Arc<Mutex<Engine>>,
    RadioLoop,
    MockBackend,
    Rig,
    Arc<Mutex<Vec<String>>>,
);

/// The JTTY screen on 20 m, W9XYZ, TX as `tx_on` says; the rig a logging rigctld.
fn jtty_scene(tx_on: bool) -> Scene {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        e.set_operating_mode("jtty", false);
        e.set_frequency(14.090, "20m", "USB");
        e.set_tx_enabled(tx_on);
    }
    let (addr, log) = mock_rigctld_on(14_090_000, false);
    let rig = Rig::rigctld(&addr);
    let mut state = loop_state();
    state.applied = Transport::from_settings(engine.lock().unwrap().settings());
    (engine, state, MockBackend::new(), rig, log)
}

/// One loop tick at `t`.
fn tick(
    engine: &Arc<Mutex<Engine>>,
    state: &mut RadioLoop,
    backend: &mut MockBackend,
    rig: &mut Rig,
    t: f64,
) {
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    state
        .step(
            engine,
            backend,
            rig,
            &sinks,
            t,
            &mut ra,
            &mut rr,
            &mut station,
        )
        .unwrap();
}

fn keys(log: &Arc<Mutex<Vec<String>>>) -> usize {
    log.lock().unwrap().iter().filter(|c| *c == "T 1").count()
}

fn unkeys(log: &Arc<Mutex<Vec<String>>>) -> usize {
    log.lock().unwrap().iter().filter(|c| *c == "T 0").count()
}

/// What OUR receiver makes of the audio the loop played: the app's own tap and decode driver.
/// The complete messages, as the receive window shows them.
fn read_back(played: &[f32]) -> Vec<String> {
    let rx = Mutex::new(Engine::new("K1ABC", "FN42", 0));
    rx.lock().unwrap().set_jtty_armed(true);
    let mut driver = tempo_app::engine::jtty_rx::JttyDriver::default();
    let mut audio = vec![0.0f32; 24_000];
    audio.extend(played.iter().map(|s| s * 0.3));
    audio.extend(vec![0.0f32; 36_000]);
    for chunk in audio.chunks(1200) {
        rx.lock().unwrap().feed_rx_audio(chunk);
        driver.tick(&rx);
    }
    rx.lock().unwrap().set_jtty_armed(false);
    driver.tick(&rx);
    let lines = rx.lock().unwrap().jtty_state().lines;
    lines
        .into_iter()
        .filter(|l| l.state == "complete")
        .map(|l| l.text)
        .collect()
}

/// ⛔ THE TX-ENABLE LATCH, AT THE KEYING SEAM. Every way to send JTTY is tried with TX off, each
/// refused with the latch's words, and the loop runs on for six seconds: no PTT command reaches
/// the rig, no sample reaches the card, no flush, and the CAT conversation is the one a loop that
/// was never asked has, command for command. The control: the same scene with TX on keys (`T 1`)
/// and plays.
#[test]
fn with_tx_off_nothing_reaches_ptt_audio_or_cat() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    let run = |attempt: bool, on: bool| {
        let (engine, mut state, mut backend, mut rig, log) = jtty_scene(on);
        let mut t = 100.0;
        step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, 500.0);
        if attempt {
            let tries = [
                jtty_tx::send_text(&engine, "CQ CQ DE W9XYZ", "").map(|_| ()),
                jtty_tx::send_fkey(&engine, 1, "").map(|_| ()),
                jtty_tx::send_fkey(&engine, 2, "W1AW").map(|_| ()),
                jtty_tx::live_start(&engine, "HELLO ", "").map(|_| ()),
                jtty_tx::live_commit(&engine, "HELLO THERE ", "", jtty_tx::JttyCommit::Flush)
                    .map(|_| ()),
                jtty_tx::sign_off(&engine, "", "").map(|_| ()),
            ];
            for r in tries {
                if on {
                    let _ = r;
                } else {
                    assert!(r.unwrap_err().starts_with("TX is off"));
                }
            }
        }
        step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, 6_000.0);
        let commands = log.lock().unwrap().clone();
        (
            commands,
            backend.played.len(),
            backend.flush_calls,
            rig.keyed,
        )
    };
    let (never, played0, flushed0, _) = run(false, false);
    let (asked, played, flushed, keyed) = run(true, false);
    assert!(
        !asked.iter().any(|c| c == "T 1"),
        "PTT reached the rig: {asked:?}"
    );
    assert_eq!(played, 0, "audio reached the card");
    assert_eq!(
        (played0, flushed0),
        (0, 0),
        "premise: the idle loop plays nothing"
    );
    assert_eq!(flushed, 0, "the output was touched");
    assert!(!keyed);
    assert_eq!(
        asked, never,
        "the refused sends changed what the loop said to the rig"
    );
    // The control: with TX on the same tries key and play.
    let (open, played, _, _) = run(true, true);
    assert!(open.iter().any(|c| c == "T 1"), "control: keyed, {open:?}");
    assert!(played > 0, "control: played");
}

/// ⛔ …and a licence gate that closes with JTTY on the air stops it on the next tick (here the class
/// changes to one with no 20 m data): the engine reads every gate again before every frame, drops
/// what is queued, and the loop flushes and unkeys.
#[test]
fn a_licence_gate_closing_stops_what_the_loop_is_feeding() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    let (engine, mut state, mut backend, mut rig, log) = jtty_scene(true);
    jtty_tx::send_text(&engine, "CQ CQ CQ DE W9XYZ W9XYZ W9XYZ K", "").unwrap();
    let mut t = 100.0;
    step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, 1_000.0);
    assert_eq!(keys(&log), 1, "premise: keyed");
    engine.lock().unwrap().set_license_class("technician");
    let (played, flushed) = (backend.played.len(), backend.flush_calls);
    tick(&engine, &mut state, &mut backend, &mut rig, t);
    assert!(
        backend.flush_calls > flushed,
        "the queued audio was not flushed"
    );
    assert!(!rig.keyed && unkeys(&log) >= 1, "not unkeyed");
    t += 20.0;
    step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, 9_000.0);
    assert_eq!(
        backend.played.len(),
        played,
        "fed on after the licence gate closed"
    );
    assert_eq!(keys(&log), 1, "keyed again");
}

/// ENCODE → THE AIR → OUR OWN RECEIVE, through the loop: three sends queued at once go out on ONE
/// key (`T 1` once), chained with no gap, each frame played as the loop's look-ahead asks, the
/// look-ahead never more than two frames; PTT drops once, after the last, and what was played
/// reads back as the three messages, in order.
#[test]
fn what_the_loop_plays_reads_back_as_sent_on_one_key() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    let (engine, mut state, mut backend, mut rig, log) = jtty_scene(true);
    jtty_tx::send_text(&engine, "CQ CQ CQ DE W9XYZ W9XYZ K", "").unwrap();
    jtty_tx::send_fkey(&engine, 2, "W1AW").unwrap();
    jtty_tx::send_text(&engine, "TU 73", "").unwrap();
    let frame_ms = keyboard::JTTY.char_ms(jtty_tx::JTTY_BAUD);
    let mut t = 100.0;
    let mut keyed_since: Option<f64> = None;
    let mut dropped_at: Option<f64> = None;
    while t < 120_000.0 && dropped_at.is_none() {
        tick(&engine, &mut state, &mut backend, &mut rig, t);
        let ahead = state.jtty_busy_until - t;
        assert!(
            ahead <= 2.0 * frame_ms + 1.0,
            "{ahead} ms queued ahead at {t}"
        );
        match (rig.keyed, keyed_since) {
            (true, None) => keyed_since = Some(t),
            (false, Some(_)) => dropped_at = Some(t),
            _ => {}
        }
        t += 20.0;
    }
    let (since, dropped) = (keyed_since.expect("keyed"), dropped_at.expect("unkeyed"));
    let commands = log.lock().unwrap().clone();
    let first_key = commands.iter().position(|c| c == "T 1").expect("a key");
    assert_eq!(keys(&log), 1, "one key for all three: {commands:?}");
    assert_eq!(
        commands[first_key..].iter().filter(|c| *c == "T 0").count(),
        1,
        "one unkey, after the last: {commands:?}"
    );
    let air_ms = backend.played.len() as f64 / 12.0;
    assert!(
        dropped - since >= air_ms && dropped - since <= air_ms + crate::slot::TX_TAIL_MS + 60.0,
        "PTT held {} ms for {air_ms} ms of audio",
        dropped - since
    );
    assert_eq!(
        backend.played.len() % (frame_ms * 12.0) as usize,
        0,
        "whole frames"
    );
    assert_eq!(
        read_back(&backend.played),
        vec!["CQ CQ CQ DE W9XYZ W9XYZ K", "W1AW 599 001", "TU 73"]
    );
}

/// STOP DRAIN, at the loop: Stop TX (the universal halt, Esc's too), TX Off, leaving the JTTY
/// screen (for PSK, the same section, and for FT8), and the loop no longer owning the radio (a
/// radio switch waiting for its handoff: `may_key`, the one gate the engine cannot see) each cut a
/// JTTY transmission on the next tick — the output flushed, PTT down — and nothing more is played
/// after.
#[test]
fn every_stop_cuts_jtty_on_the_next_tick() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    for stop in ["halt", "tx off", "psk screen", "ft8 screen", "handoff"] {
        let (engine, mut state, mut backend, mut rig, log) = jtty_scene(true);
        jtty_tx::send_text(&engine, "CQ CQ CQ DE W9XYZ W9XYZ W9XYZ W9XYZ PSE K", "").unwrap();
        let mut t = 100.0;
        step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, 2_500.0);
        assert!(rig.keyed, "{stop}: premise, keyed");
        {
            let mut e = engine.lock().unwrap();
            match stop {
                "halt" => e.halt_tx(),
                "tx off" => e.set_tx_enabled(false),
                "psk screen" => e.set_operating_mode("keyboard", false),
                "ft8 screen" => e.set_operating_mode("digital", false),
                _ => state.handoff_deferred = true,
            }
        }
        let (played, flushed) = (backend.played.len(), backend.flush_calls);
        tick(&engine, &mut state, &mut backend, &mut rig, t);
        assert!(backend.flush_calls > flushed, "{stop}: not flushed");
        assert!(!rig.keyed, "{stop}: not unkeyed");
        assert!(unkeys(&log) >= 1, "{stop}: no T 0");
        t += 20.0;
        step_to(
            &engine,
            &mut state,
            &mut backend,
            &mut rig,
            &mut t,
            12_000.0,
        );
        assert_eq!(
            backend.played.len(),
            played,
            "{stop}: played on after the stop"
        );
        assert_eq!(keys(&log), 1, "{stop}: keyed again");
        assert!(!engine.lock().unwrap().jtty_sending(), "{stop}");
    }
}

/// A LIVE SESSION ON THE LOOP: the filler keeps one key up while nothing typed is waiting, and a
/// loop that stops ticking lets the transmitter go: the deadline it holds PTT to is never more
/// than two frames and the tail away (`keyboard::JTTY`), and past it the transmitter is unkeyed.
#[test]
fn a_wedged_loop_unkeys_a_live_jtty_session() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    let (engine, mut state, mut backend, mut rig, log) = jtty_scene(true);
    jtty_tx::live_start(&engine, "HELLO ", "").unwrap();
    let mut t = 100.0;
    step_to(
        &engine,
        &mut state,
        &mut backend,
        &mut rig,
        &mut t,
        15_000.0,
    );
    assert!(rig.keyed, "the filler holds the carrier");
    assert_eq!(keys(&log), 1, "on one key");
    let frame_ms = keyboard::JTTY.char_ms(jtty_tx::JTTY_BAUD);
    let deadline = state
        .tx_until_ms
        .expect("a live session holds PTT to a deadline");
    assert!(
        deadline - t <= 2.0 * frame_ms + crate::slot::TX_TAIL_MS + 1.0,
        "the deadline is {} ms out",
        deadline - t
    );
    // The loop wedges until past the deadline; the operator has meanwhile left the screen, so
    // nothing re-keys it on the way back.
    engine.lock().unwrap().set_operating_mode("phone", false);
    tick(&engine, &mut state, &mut backend, &mut rig, deadline + 1.0);
    assert!(!rig.keyed, "a wedged loop left the transmitter keyed");
    assert!(state.tx_until_ms.is_none());
}

/// PSK IS DROPPED WHEN JTTY BECOMES THE MODEM, at the loop: a PSK over typed ahead on the PSK
/// screen never keys once the operator has opened JTTY (the same Keyboard section), and the PSK
/// screen says so on the return. The control: staying on the PSK screen, it keys.
#[test]
fn a_psk_over_typed_ahead_never_keys_from_the_jtty_screen() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    for open_jtty in [true, false] {
        let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_operating_mode("keyboard", false);
            e.psk_send_text("CQ").unwrap();
            e.psk_send_text("DE W9XYZ").unwrap();
        }
        let (mut backend, mut rig, mut state) = (MockBackend::new(), Rig::vox(), loop_state());
        let mut t = 100.0;
        step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, 100.0);
        let first = backend.played.len();
        assert!(first > 0, "premise: the first over keys");
        if open_jtty {
            engine.lock().unwrap().set_operating_mode("jtty", false);
        }
        step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, 8_000.0);
        if open_jtty {
            assert_eq!(
                backend.played.len(),
                first,
                "the over typed ahead keyed from JTTY"
            );
            engine.lock().unwrap().set_operating_mode("keyboard", false);
            assert!(engine.lock().unwrap().psk_state().keyer_error.is_some());
        } else {
            assert!(
                backend.played.len() > first,
                "control: it keys on the PSK screen"
            );
        }
    }
}

/// LAB SUPPORT, run by hand: `cargo test -p tempo-audio --features device,serial --lib --
/// --ignored dump_what_the_loop_plays_for_the_lab` with `JTTY_LAB_DIR` set. What the loop plays
/// for each scene below, written as a 12 kHz 16-bit WAV (a second of
/// silence before, three after) for the parity lab to decode with stock WSJT-X `rjtty` and with
/// Nexus's own decoder: the transmitter's real output, not a re-encoding of it. Without the
/// variable it writes nothing.
#[test]
#[ignore]
fn dump_what_the_loop_plays_for_the_lab() {
    let _modem = super::remote_radio::selection::selection_test_lock();
    let Ok(dir) = std::env::var("JTTY_LAB_DIR") else {
        return;
    };
    type Step = Box<dyn Fn(&Arc<Mutex<Engine>>)>;
    let send = |text: &'static str| -> Step {
        Box::new(move |e| {
            jtty_tx::send_text(e, text, "W1AW").unwrap();
        })
    };
    let fkey = |n: u8| -> Step {
        Box::new(move |e| {
            jtty_tx::send_fkey(e, n, "W1AW").unwrap();
        })
    };
    let live = |typed: &'static str, flush: bool| -> Step {
        Box::new(move |e| {
            let how = if flush {
                jtty_tx::JttyCommit::Flush
            } else {
                jtty_tx::JttyCommit::Advance
            };
            jtty_tx::live_commit(e, typed, "W1AW", how).unwrap();
        })
    };
    // (name, steps, each run at the time given, ms from the start)
    let scenes: Vec<(&str, Vec<(f64, Step)>)> = vec![
        ("send", vec![(0.0, send("CQ CQ DE W9XYZ W9XYZ K"))]),
        (
            "queue",
            vec![
                (0.0, send("CQ W9XYZ")),
                (0.0, fkey(2)),
                (0.0, send("QRZ? %H DE %M")),
            ],
        ),
        ("fkeys", (1..=8).map(|n| (0.0, fkey(n))).collect()),
        (
            "long_line",
            vec![(
                0.0,
                send(
                    "CQ CQ CQ DE W9XYZ W9XYZ W9XYZ PSE K THIS IS A LONGER LINE THAN EIGHTY CHARS \
                     SO IT SPLITS\nTU 73",
                ),
            )],
        ),
        (
            "live",
            vec![
                (
                    0.0,
                    Box::new(|e: &Arc<Mutex<Engine>>| {
                        jtty_tx::live_start(e, "HELLO THERE ", "W1AW").unwrap();
                    }) as Step,
                ),
                (9_000.0, live("HOW ARE YOU ", true)),
                (
                    14_000.0,
                    Box::new(|e: &Arc<Mutex<Engine>>| {
                        jtty_tx::sign_off(e, "", "W1AW").unwrap();
                    }) as Step,
                ),
            ],
        ),
        // Past 16 frames: WSJT-X 3.3.0-beta4's receiver keeps 16 frames of one message.
        (
            "live_long",
            vec![
                (
                    0.0,
                    Box::new(|e: &Arc<Mutex<Engine>>| {
                        jtty_tx::live_start(e, "HELLO THERE ", "W1AW").unwrap();
                    }) as Step,
                ),
                (24_000.0, live("HOW ARE YOU ", true)),
                (
                    30_000.0,
                    Box::new(|e: &Arc<Mutex<Engine>>| {
                        jtty_tx::sign_off(e, "", "W1AW").unwrap();
                    }) as Step,
                ),
            ],
        ),
    ];
    for (name, steps) in scenes {
        let (engine, mut state, mut backend, mut rig, _log) = jtty_scene(true);
        let mut t = 100.0;
        for (at, step) in &steps {
            step_to(
                &engine,
                &mut state,
                &mut backend,
                &mut rig,
                &mut t,
                100.0 + at,
            );
            step(&engine);
        }
        let until = t + 120_000.0;
        step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, until);
        assert!(!rig.keyed, "{name}: still keyed");
        let mut pcm = vec![0i16; 12_000];
        pcm.extend(backend.played.iter().map(|s| (s * 20_000.0) as i16));
        pcm.extend(vec![0i16; 36_000]);
        let path = std::path::Path::new(&dir).join(format!("{name}.wav"));
        tempo_core::wavfile::write_wav_i16(&path, &pcm, 12_000).unwrap();
    }
}
