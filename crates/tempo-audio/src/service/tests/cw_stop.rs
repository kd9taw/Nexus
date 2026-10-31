//! Stop TX (Esc) in the middle of a CW message ends the keying on every keyer, and the stops that
//! need no answer from the radio go out first.
//!
//! The field report (IL QSO Party, 1.20.0-test4): "I saw the TX stopped with Esc, but a CW macro
//! already started did not immediately stop." The halt reaches the engine and the TX indicator
//! drops at once; what keys the rest of a word is whatever the keyer still holds. Each test runs
//! the real `RadioLoop::step` against a rigctld that logs every line it is sent.

use super::*;
use std::io::{BufRead, BufReader, Write};

/// A rigctld that logs each line it is sent and answers it, `hook` first: `Some(reply)` answers
/// with that (an empty reply writes nothing, as Hamlib does for a raw `w` set), `None` falls
/// through to `RPRT 0`.
pub(super) fn logging_rigctld(
    hook: impl Fn(&str) -> Option<String> + Send + 'static,
) -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let log2 = Arc::clone(&log);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let mut reader = BufReader::new(match stream.try_clone() {
                Ok(r) => r,
                Err(_) => continue,
            });
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let l = line.trim().to_string();
                log2.lock().unwrap().push(l.clone());
                let reply = hook(&l).unwrap_or_else(|| match l.as_str() {
                    "f" => "7030000\n".to_string(),
                    "m" => "CW\n500\n".to_string(),
                    "t" => "0\n".to_string(),
                    _ => "RPRT 0\n".to_string(),
                });
                if stream.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (addr, log)
}

/// An engine in the CW section on 40 m with `keyer`, on `model`, its macro word queued.
fn cw_engine(
    model: u32,
    keyer: &str,
    set: impl FnOnce(&mut tempo_app::settings::Settings),
) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = model;
        set(&mut s);
        e.apply_settings(s);
        e.set_license_class("extra");
        e.set_cw_keyer(keyer, 600.0);
        e.set_operating_mode("cw", false);
        e.set_frequency(7.03, "40m", "CW");
        e.set_cw_wpm(20);
        e.send_cw("TEST DE KD9TAW");
    }
    engine
}

/// Step the loop once at `t` ms.
pub(super) fn step(
    state: &mut RadioLoop,
    engine: &Arc<Mutex<Engine>>,
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

/// The lines the rigctld was sent from `from` on.
pub(super) fn lines_since(log: &Mutex<Vec<String>>, from: usize) -> Vec<String> {
    log.lock().unwrap()[from..].to_vec()
}

/// Key the first word, then press Stop TX (Esc) and run the tick that acts on it. Returns the
/// rigctld's lines from the stop on.
fn key_then_stop(
    engine: &Arc<Mutex<Engine>>,
    backend: &mut MockBackend,
    rig: &mut Rig,
    log: &Mutex<Vec<String>>,
) -> Vec<String> {
    let mut state = loop_state_for(engine);
    step(&mut state, engine, backend, rig, 100.0);
    let from = log.lock().unwrap().len();
    engine.lock().unwrap().halt_tx();
    step(&mut state, engine, backend, rig, 140.0);
    lines_since(log, from)
}

/// ⭐ THE SOUNDCARD KEYER UNKEYS BEFORE THE RADIO IS ASKED TO STOP ITS OWN CW. The soundcard
/// keyer's tone is Nexus's audio: the flush and the unkey end it. The CAT CW stop (`\stop_morse`)
/// is a question to the radio that a slow or silent radio holds for a whole CAT deadline, and it
/// used to go first, so the carrier stayed up behind it.
#[test]
fn a_soundcard_stop_unkeys_before_the_radio_is_asked_to_stop_cw() {
    let engine = cw_engine(3073, "soundcard", |_| {});
    let (addr, log) = logging_rigctld(|_| None);
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    step(&mut state, &engine, &mut backend, &mut rig, 100.0);
    // POSITIVE CONTROL: the word was keyed and played, or the stop has nothing to stop.
    assert!(
        log.lock().unwrap().iter().any(|l| l == "T 1"),
        "control: the soundcard word must key the radio: {:?}",
        log.lock().unwrap()
    );
    assert!(
        !backend.played.is_empty(),
        "control: the word's tone must be playing"
    );
    let flushes = backend.flush_calls;
    let from = log.lock().unwrap().len();
    engine.lock().unwrap().halt_tx();
    step(&mut state, &engine, &mut backend, &mut rig, 140.0);
    let after = lines_since(&log, from);
    let unkey = after.iter().position(|l| l == "T 0");
    let cw_stop = after.iter().position(|l| l == "\\stop_morse");
    assert!(backend.flush_calls > flushes, "the tone is flushed");
    assert!(
        matches!((unkey, cw_stop), (Some(u), Some(c)) if u < c),
        "the unkey (T 0) must go out before the CAT CW stop: {after:?}"
    );
}

/// ⭐ A YAESU WHOSE CAT REFERENCE HAS A PLAYBACK STOP GETS IT. The bundled Hamlib (4.7.1) sends
/// a Yaesu newcat radio's CAT CW as `KM1<text>;` `KY6;` and has no `stop_morse` for it at all
/// (`\stop_morse` answers `RPRT -11` and nothing reaches the radio), so the word in the radio
/// played to its end. Yaesu's own references define `KY P1 P2;` with P2 `0` as the stop of a CW
/// TEXT (P1 `0`) or MESSAGE (P1 `1`) memory's playback for the FT-710 and the FTX-1, and Nexus
/// now sends both after `\stop_morse`. The FT-991A and the FTDX10 references have no stop form
/// of `KY`, so nothing raw is sent to them, nor to any other make.
#[test]
fn an_ft710_and_an_ftx1_stop_gets_yaesus_own_playback_stop_and_no_other_radio_does() {
    for (model, name, stops) in [
        (1049u32, "FT-710", vec!["W KY00; 0", "W KY10; 0"]),
        (1051, "FTX-1", vec!["W KY00; 0", "W KY10; 0"]),
        (1035, "FT-991A", vec![]),
        (1042, "FTDX10", vec![]),
        (3073, "IC-7300", vec![]),
        (2037, "TS-590SG", vec![]),
    ] {
        let engine = cw_engine(model, "cat", |_| {});
        // A raw set goes as `W <cmd> 0`, which rigctld answers with one NUL (`Rig::send_raw_set`).
        let (addr, log) = logging_rigctld(|l| l.starts_with("W KY").then(|| "\0".to_string()));
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let after = key_then_stop(&engine, &mut backend, &mut rig, &log);
        // POSITIVE CONTROL: the CAT keyer keyed the word, and the stop asked the radio.
        assert!(
            log.lock().unwrap().iter().any(|l| l.starts_with("b TEST")),
            "{name}: control: the word must go to the radio's keyer: {:?}",
            log.lock().unwrap()
        );
        let cw_stop = after
            .iter()
            .position(|l| l == "\\stop_morse")
            .unwrap_or_else(|| panic!("{name}: the CAT CW stop must go out: {after:?}"));
        // Where none is expected, give a stray one time to arrive.
        std::thread::sleep(Duration::from_millis(200));
        let raw: Vec<String> = lines_since(&log, 0)
            .into_iter()
            .filter(|l| l.contains("KY"))
            .collect();
        assert_eq!(raw, stops, "{name}: the raw playback stops sent");
        if let Some(first) = after.iter().position(|l| l.contains("KY")) {
            assert!(cw_stop < first, "{name}: after the CAT CW stop: {after:?}");
        }
    }
}

/// ⭐ STOP TX ON AN FT-710 OR AN FTX-1 UNKEYS WITHIN THE PTT DEADLINE, BEHIND THE PLAYBACK STOPS.
/// Every halt arms the CW abort, in any mode, so every Stop TX on these two radios sends the
/// `KY` playback stops in the same tick, before the over's `T 0`. Hamlib's rigctld answers a
/// `w` (send_cmd) by waiting for the radio's reply for the radio's whole read timeout, and a
/// Yaesu never answers a set: 1000 ms on the FT-710, 2000 ms on the FTX-1 (Hamlib 4.7.1
/// `ft710.c` / `ftx1.c` `.timeout`), per stop, with the port held. The `T 0` waited behind both
/// and missed the 700 ms PTT deadline (measured against a real rigctld: the radio got `TX0;`
/// 2.06 s after Stop on an FT-710, 4.18 s on an FTX-1). This rigctld holds its connection the
/// same way after a `w` set, and answers rigctld's `W <cmd> 0` (send_cmd_rx, read nothing) at
/// once with the NUL Hamlib 4.7.1 sends.
#[test]
fn a_stop_on_an_ft710_or_an_ftx1_unkeys_in_time_behind_its_playback_stops() {
    for (model, name, hold_ms) in [(1049u32, "FT-710", 1000), (1051, "FTX-1", 2000)] {
        let engine = cw_engine(model, "cat", |_| {});
        let (addr, log) = logging_rigctld(move |l| {
            if l.starts_with("w KY") {
                std::thread::sleep(Duration::from_millis(hold_ms));
                Some(String::new())
            } else if l.starts_with("W KY") {
                Some("\0".to_string())
            } else {
                None
            }
        });
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let mut state = loop_state_for(&engine);
        step(&mut state, &engine, &mut backend, &mut rig, 100.0);
        // An over on the air: the radio keyed and the loop holding its deadline.
        rig.ptt(true).expect("premise: the key");
        state.tx_until_ms = Some(60_000.0);
        let from = log.lock().unwrap().len();
        engine.lock().unwrap().halt_tx();
        let started = std::time::Instant::now();
        step(&mut state, &engine, &mut backend, &mut rig, 140.0);
        let took = started.elapsed();
        let after = lines_since(&log, from);
        // POSITIVE CONTROL: the playback stops went out, ahead of the unkey.
        let first_stop = after.iter().position(|l| l.contains("KY00;"));
        let unkey = after.iter().position(|l| l == "T 0");
        assert!(
            matches!((first_stop, unkey), (Some(s), Some(u)) if s < u),
            "{name}: control: the playback stops must precede the unkey: {after:?}"
        );
        assert!(
            !rig.keyed,
            "{name}: the unkey must be answered within the PTT deadline (tick took {took:?}): \
             {after:?}"
        );
        assert!(
            took < Duration::from_millis(700),
            "{name}: the Stop tick, its unkey included, took {took:?}: {after:?}"
        );
    }
}

/// ⛔ A STOP TX WHILE A LATE KEY'S ANSWER IS OWED STILL SENDS EVERY STOP. When rigctld holds a key
/// past its deadline, the connection is kept and its answer owed (`Rig::command_lines`). The CAT
/// CW stop and an FT-710's playback stops then waited out that answer and were not sent at all;
/// they go out at once behind the key, as the unkey does, and all of them reach the radio after it.
#[test]
fn a_stop_while_a_late_keys_answer_is_owed_still_sends_every_stop() {
    let engine = cw_engine(1049, "cat", |_| {});
    let (addr, log) = logging_rigctld(|l| {
        if l == "T 1" {
            std::thread::sleep(Duration::from_millis(3_000)); // rigctld holding the key
            None
        } else if l.starts_with("W KY") {
            Some("\0".to_string())
        } else {
            None
        }
    });
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    step(&mut state, &engine, &mut backend, &mut rig, 100.0);
    let from = log.lock().unwrap().len();
    assert!(rig.ptt(true).is_err(), "premise: the key's answer is late");
    state.tx_until_ms = Some(60_000.0);
    engine.lock().unwrap().halt_tx();
    step(&mut state, &engine, &mut backend, &mut rig, 140.0);
    let until = std::time::Instant::now() + Duration::from_secs(5);
    let mut after = lines_since(&log, from);
    while after.len() < 5 && std::time::Instant::now() < until {
        std::thread::sleep(Duration::from_millis(50));
        after = lines_since(&log, from);
    }
    for stop in ["\\stop_morse", "W KY00; 0", "W KY10; 0", "T 0"] {
        let at = after.iter().position(|l| l == stop);
        assert!(
            at.is_some_and(|at| at > 0) && after[0] == "T 1",
            "{stop} must reach the radio, after the key: {after:?}"
        );
    }
}

/// ⭐ THE WINKEYER IS CLEARED BEFORE THE RADIO IS ASKED TO STOP ITS CW. K1EL's Clear Buffer
/// (`0A`) "will cancel any Morse character in progress immediately ending it in midstream" (WK3
/// datasheet, p. 10). It is one byte on a serial port Nexus owns. It used to wait behind the CAT
/// CW stop, which a slow or silent radio holds for a whole CAT deadline, while the WinKeyer kept
/// keying. The rigctld here looks at the keyer's wire at the moment `\stop_morse` reaches it.
#[cfg(target_os = "linux")]
#[cfg(feature = "serial")]
#[test]
fn a_winkeyer_is_cleared_before_the_radio_is_asked_to_stop_cw() {
    use serialport::SerialPort;
    use std::io::Read;

    let (keyer, slave) = serialport::TTYPort::pair().expect("a pty pair");
    let port = slave.name().expect("the pts side has a path");
    let mut responder = keyer.try_clone().expect("clone the pty master");
    responder
        .set_timeout(Duration::from_millis(3000))
        .expect("bounded read");
    std::thread::spawn(move || {
        let mut host_open = [0u8; 2];
        if responder.read_exact(&mut host_open).is_ok() {
            let _ = responder.write_all(&[0x17]); // WK2, revision 23
            let _ = responder.flush();
        }
    });
    let wire = Arc::new(Mutex::new((keyer, Vec::<u8>::new())));
    let seen = Arc::clone(&wire);
    // What the keyer's wire held each time `\stop_morse` arrived: had it been cleared yet?
    let cleared_at_cw_stop = Arc::new(Mutex::new(Vec::<bool>::new()));
    let record = Arc::clone(&cleared_at_cw_stop);
    let (addr, log) = logging_rigctld(move |l| {
        if l == "\\stop_morse" {
            let mut w = seen.lock().unwrap();
            let (k, bytes) = &mut *w;
            k.set_timeout(Duration::from_millis(50)).unwrap();
            let mut buf = [0u8; 256];
            while let Ok(n) = k.read(&mut buf) {
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&buf[..n]);
            }
            record
                .lock()
                .unwrap()
                .push(bytes.contains(&crate::winkeyer::CLEAR_BUFFER[0]));
        }
        None
    });
    let engine = cw_engine(3073, "winkeyer", |s| s.winkeyer_port = port.clone());
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let after = key_then_stop(&engine, &mut backend, &mut rig, &log);
    assert_eq!(
        engine.lock().unwrap().cw_keyer_error(),
        None,
        "control: the stand-in keyer must open cleanly"
    );
    assert!(
        after.iter().any(|l| l == "\\stop_morse"),
        "control: the CAT CW stop still goes out: {after:?}"
    );
    let bytes = wire.lock().unwrap().1.clone();
    assert!(
        bytes.windows(4).any(|w| w == b"TEST"),
        "control: the word reached the keyer's wire: {bytes:02x?}"
    );
    assert_eq!(
        cleared_at_cw_stop.lock().unwrap().as_slice(),
        &[true],
        "the WinKeyer's Clear Buffer must already be on its wire when the radio is asked: {bytes:02x?}"
    );
}

/// A rigctld whose radio keeps its own CW speed: `L KEYSPD n` sets it, `l KEYSPD` reads it back,
/// and the test can turn the radio's speed knob itself (SmartSDR, a Maestro, the front panel).
pub(super) fn keyer_speed_rigctld() -> (
    String,
    Arc<Mutex<Vec<String>>>,
    Arc<std::sync::atomic::AtomicU32>,
) {
    use std::sync::atomic::{AtomicU32, Ordering as AtomicOrdering};
    let speed = Arc::new(AtomicU32::new(0));
    let knob = Arc::clone(&speed);
    let (addr, log) = logging_rigctld(move |l| {
        if let Some(wpm) = l.strip_prefix("L KEYSPD ") {
            knob.store(wpm.trim().parse().unwrap_or(0), AtomicOrdering::Relaxed);
            None
        } else if l == "l KEYSPD" {
            Some(format!("{}\n", knob.load(AtomicOrdering::Relaxed)))
        } else {
            None
        }
    });
    (addr, log, speed)
}

/// An engine in the CW section on 40 m with the CAT keyer, on `model`, at `wpm`, nothing queued.
pub(super) fn cat_engine(model: u32, wpm: u32) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = model;
        e.apply_settings(s);
        e.set_license_class("extra");
        e.set_cw_keyer("cat", 600.0);
        e.set_operating_mode("cw", false);
        e.set_frequency(7.03, "40m", "CW");
        e.set_cw_wpm(wpm);
    }
    engine
}

/// Step the loop every `tick` ms over `[from, to)`, and return when (ms) each word reached the
/// radio's keyer (`b <word>`), in order.
#[allow(clippy::too_many_arguments)]
fn handovers(
    state: &mut RadioLoop,
    engine: &Arc<Mutex<Engine>>,
    backend: &mut MockBackend,
    rig: &mut Rig,
    log: &Mutex<Vec<String>>,
    from: f64,
    to: f64,
    tick: f64,
) -> Vec<(String, f64)> {
    let mut out = Vec::new();
    let mut seen = log.lock().unwrap().len();
    let mut t = from;
    while t < to {
        step(state, engine, backend, rig, t);
        let lines = lines_since(log, seen);
        seen += lines.len();
        for l in lines {
            if let Some(word) = l.strip_prefix("b ") {
                out.push((word.trim().to_string(), t));
            }
        }
        t += tick;
    }
    out
}

/// What the radio takes to key `word` at `wpm` and then leave a word space: the earliest the next
/// word may reach it without queueing behind this one.
pub(super) fn word_window_ms(word: &str, wpm: u32) -> f64 {
    tempo_core::cw::morse_duration_ms(word, wpm) + 7.0 * 1200.0 / wpm as f64
}

/// ⭐ ON A FLEX OVER SMARTSDR CAT, NEXUS'S SPEED WINS, AND EACH WORD IS PACED AT IT. FlexRadio
/// documents no CW stop over SmartSDR CAT, and its `KY;` buffer answer is always "room" (SmartSDR
/// CAT User Guide §3.4.11), so whatever text Nexus has handed the radio plays out after Esc. Nexus
/// hands it one word at a time, timed at Nexus's speed, which holds only if the radio keys at that
/// speed: turn the radio's speed down in SmartSDR and the words went over ahead of the radio, and
/// the backlog kept keying after a stop (IL QSO Party). So the radio is told Nexus's speed at the
/// start of every message, even unchanged (operator ruling, 2026-10-19: Nexus's speed always wins),
/// and the radio's own speed is neither read nor taken into the CW control.
#[test]
fn on_smartsdr_cat_nexus_speed_wins_over_a_speed_set_on_the_radio() {
    let engine = cat_engine(2036, 30);
    let (addr, log, knob) = keyer_speed_rigctld();
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    engine.lock().unwrap().send_cw("E");
    let first = handovers(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        &log,
        100.0,
        1_000.0,
        5.0,
    );
    assert_eq!(
        first.len(),
        1,
        "control: the first message keyed: {first:?}"
    );
    // Between messages the operator turns the radio's speed down in SmartSDR.
    knob.store(20, std::sync::atomic::Ordering::Relaxed);
    let from = log.lock().unwrap().len();
    engine.lock().unwrap().send_cw("CQ TEST KD9TAW");
    let words = handovers(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        &log,
        2_000.0,
        12_000.0,
        5.0,
    );
    let wire: Vec<String> = lines_since(&log, from)
        .into_iter()
        .filter(|l| l.starts_with("L KEYSPD") || l.starts_with("b ") || l == "l KEYSPD")
        .collect();
    assert_eq!(
        wire,
        ["L KEYSPD 30", "b CQ", "b TEST", "b KD9TAW"],
        "Nexus's 30 goes to the radio before the message, once; the radio's speed is not read"
    );
    let gaps: Vec<f64> = words.windows(2).map(|w| w[1].1 - w[0].1).collect();
    let want = [word_window_ms("CQ", 30), word_window_ms("TEST", 30)];
    for (gap, want) in gaps.iter().zip(want) {
        assert!(
            (gap - want).abs() <= 5.0,
            "each word is paced at the 30 WPM the radio was just told: gaps {gaps:?}, want {want}"
        );
    }
    assert_eq!(
        engine.lock().unwrap().cw_wpm(),
        30,
        "the CW control is not moved"
    );
    assert_eq!(
        knob.load(std::sync::atomic::Ordering::Relaxed),
        30,
        "the radio keys at Nexus's speed"
    );
}

/// ⭐ PACED, BUT NEVER GAPPED. At a speed the radio and Nexus agree on, each word reaches the radio
/// exactly as the last one's word space ends (within one loop tick): the macro sounds as one
/// message, and the radio never holds more than the word it is keying.
#[test]
fn normal_sending_on_smartsdr_cat_hands_each_word_over_as_the_last_one_ends() {
    let engine = cat_engine(2036, 25);
    let (addr, log, knob) = keyer_speed_rigctld();
    let mut rig = Rig::rigctld(&addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    let macro_words = ["CQ", "TEST", "KD9TAW", "KD9TAW", "TEST", "CQ"];
    engine.lock().unwrap().send_cw(&macro_words.join(" "));
    let tick = 5.0;
    let words = handovers(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        &log,
        100.0,
        15_000.0,
        tick,
    );
    let names: Vec<&str> = words.iter().map(|(w, _)| w.as_str()).collect();
    assert_eq!(
        names, macro_words,
        "control: every word keyed once, in order"
    );
    assert_eq!(
        knob.load(std::sync::atomic::Ordering::Relaxed),
        25,
        "control: radio at 25"
    );
    for (pair, word) in words.windows(2).zip(macro_words) {
        let gap = pair[1].1 - pair[0].1;
        let want = word_window_ms(word, 25);
        assert!(
            gap >= want && gap < want + tick,
            "{word}: the next word goes when this one and its word space end, not before (a \
             second word queued in the radio) and not a tick later (a gap on the air): {gap} vs {want}"
        );
    }
    let sets = lines_since(&log, 0)
        .into_iter()
        .filter(|l| l.starts_with("L KEYSPD"))
        .count();
    assert_eq!(sets, 1, "the radio is told the speed once, not per word");
}

/// Only a radio that no CAT CW stop is sure to reach is told Nexus's speed before EVERY message,
/// changed or not: a Flex over SmartSDR CAT, and the Yaesu newcat radios Hamlib keys with `KM1…;`
/// `KY6;` and stops with nothing. A radio with a stop of its own (Icom `17 FF`, Kenwood and
/// Elecraft `KY`, the FT-710 and FTX-1 playback stop Nexus sends) is told it only when it changes,
/// as before. No radio is asked its speed.
#[test]
fn only_a_radio_without_a_sure_cat_cw_stop_is_told_the_speed_before_every_message() {
    for (model, name, every) in [
        (2036u32, "Flex over SmartSDR CAT", true),
        (1035, "FT-991A", true),
        (1042, "FTDX10", true),
        (1049, "FT-710", false),
        (1051, "FTX-1", false),
        (3073, "IC-7300", false),
        (2037, "TS-590SG", false),
        (2047, "K4", false),
    ] {
        let engine = cat_engine(model, 25);
        let (addr, log) = logging_rigctld(|_| None);
        let mut rig = Rig::rigctld(&addr);
        let mut backend = MockBackend::new();
        let mut state = loop_state_for(&engine);
        engine.lock().unwrap().send_cw("TEST");
        step(&mut state, &engine, &mut backend, &mut rig, 100.0);
        engine.lock().unwrap().send_cw("TEST");
        step(&mut state, &engine, &mut backend, &mut rig, 5_000.0);
        let lines = lines_since(&log, 0);
        let wire: Vec<&str> = lines
            .iter()
            .map(String::as_str)
            .filter(|l| l.starts_with("L KEYSPD") || l.starts_with("b ") || *l == "l KEYSPD")
            .collect();
        let want: &[&str] = if every {
            &["L KEYSPD 25", "b TEST", "L KEYSPD 25", "b TEST"]
        } else {
            &["L KEYSPD 25", "b TEST", "b TEST"]
        };
        assert_eq!(wire, want, "{name}");
    }
}

/// Nexus's own Flex client keys CW through CWX, which `cwx clear` stops, and it already follows
/// the radio's speed itself; the SmartSDR CAT per-message speed is not laid over it.
#[test]
fn the_native_flex_client_is_not_paced_as_smartsdr_cat() {
    assert!(
        cat_cw_speed_set_per_message(2036, false),
        "control: SmartSDR CAT is"
    );
    assert!(
        !cat_cw_speed_set_per_message(2036, true),
        "Nexus's Flex client is not"
    );
}
