//! THE FILTER WIDTH THROUGH THE RADIO LOOP — what reaches the radio when the scope's filter edge
//! (or the BW stepper) asks for a width. By value on both backends: the Hamlib path's `M <mode>
//! <hz>` and the native CI-V daemon's `1A 03 <code>`; one width per mode read, the last one
//! asked; nothing at all while the radio is keyed; the read that carries it owed at once; and a
//! width the radio does not answer sent three times, then given up, as a dial is.

use super::*;

/// An engine in Phone on 20 m (USB, DATA off) — the mode a filter edge is dragged in — on the
/// IC-9700 (Hamlib model 3081) for the native scenes, so the loop's transport matches the
/// engine's and the first step does not tear the rig down (see `loop_state_for`).
fn phone_engine(rig_model: u32) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = rig_model;
        e.apply_settings(s);
        e.set_operating_mode("phone", false);
    }
    engine
}

/// `n` loop steps 800 ms apart, so every one is past `RIG_POLL_MS` and runs the receive-time poll.
fn steps(engine: &Arc<Mutex<Engine>>, state: &mut RadioLoop, rig: &mut Rig, n: usize, t: &mut f64) {
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let (mut backend, mut station) = (MockBackend::new(), StationSinks::new());
    // The connection's capability probes are done (a `\dump_state` read runs to its deadline),
    // so each poll stays inside its read budget and reaches the mode read.
    state.rx_ranges_probed = true;
    state.tuner_probed = true;
    for _ in 0..n {
        *t += 800.0;
        state
            .step(
                engine,
                &mut backend,
                rig,
                &sinks,
                *t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
    }
}

/// Steps until `done` holds, at most `max` of them; whether it came to hold. The native daemon's
/// reads cross a real serial engine, so a heavy poll can spend its read budget before the mode
/// read and leave it for a later poll (`ReadTurns`' resume) — what the scenes below assert is
/// what reached the wire over the whole window, never which poll it rode.
fn steps_until(
    engine: &Arc<Mutex<Engine>>,
    state: &mut RadioLoop,
    rig: &mut Rig,
    max: usize,
    t: &mut f64,
    done: impl Fn(&Engine) -> bool,
) -> bool {
    for _ in 0..max {
        steps(engine, state, rig, 1, t);
        if done(&engine.lock().unwrap()) {
            return true;
        }
    }
    false
}

/// The `1A 03` WRITES on the fake radio's wire since `from`, each as its one payload byte.
fn width_writes(
    regs: &Arc<Mutex<crate::civ::engine::tests_support::Regs>>,
    from: usize,
) -> Vec<u8> {
    regs.lock().unwrap().log[from..]
        .iter()
        .filter(|(cmd, data)| *cmd == 0x1A && data.len() == 2 && data[0] == 0x03)
        .map(|(_, data)| data[1])
        .collect()
}

/// The rigctld lines since `from` that carry a width: `M` with a positive passband.
fn width_lines(log: &Arc<Mutex<Vec<String>>>, from: usize) -> Vec<String> {
    log.lock().unwrap()[from..]
        .iter()
        .filter(|l| {
            let mut p = l.split_whitespace();
            p.next() == Some("M")
                && p.nth(1)
                    .and_then(|w| w.parse::<i64>().ok())
                    .is_some_and(|w| w > 0)
        })
        .cloned()
        .collect()
}

fn width_shown(engine: &Arc<Mutex<Engine>>) -> Option<u32> {
    engine.lock().unwrap().snapshot().radio.filter_width_hz
}

/// ⭐ THE IC-9700 ON THE NATIVE DAEMON, BY VALUE. Its width is READ through `1A 03` (the `m`
/// passband stays 0 there, on purpose), and a width the edge asks for goes out as exactly ONE
/// `1A 03` frame carrying the code the vendor table gives the LAST width asked — never as an
/// `M` (which would re-send the mode: `06`, `1A 06`).
#[test]
fn a_width_reaches_the_native_ic9700_once_as_its_code() {
    let (d, mut rig, regs) = civ_daemon_rig(false);
    let engine = phone_engine(3081);
    let mut state = loop_state_for(&engine);
    state.rigctld_proc = Some(CatDaemon::Native(d));
    let mut t = 0.0;
    steps(&engine, &mut state, &mut rig, 2, &mut t); // the retune settles: USB, DATA off
    assert!(
        steps_until(&engine, &mut state, &mut rig, 16, &mut t, |e| {
            e.snapshot().radio.filter_width_hz == Some(2400)
        }),
        "the radio's own width, code 28, read back through `1A 03`"
    );

    let from = regs.lock().unwrap().log.len();
    {
        let mut e = engine.lock().unwrap();
        e.request_filter_width(1800);
        e.request_filter_width(1700); // the drag's next report: the slot keeps the last
    }
    assert!(steps_until(
        &engine,
        &mut state,
        &mut rig,
        16,
        &mut t,
        |e| !e.passband_request_pending()
    ));
    assert_eq!(
        width_writes(&regs, from),
        vec![0x21],
        "ONE write: 1.7 kHz is code 21, BCD 0x21"
    );
    assert_eq!(regs.lock().unwrap().filter_raw, 0x21);
    assert!(
        !regs.lock().unwrap().log[from..]
            .iter()
            .any(|(cmd, _)| *cmd == 0x06),
        "the width never re-sent the mode"
    );
    assert_eq!(width_shown(&engine), Some(1700), "the width the radio took");
    assert!(
        !engine.lock().unwrap().passband_request_pending(),
        "drained, not re-queued"
    );

    // CLAMPED BY THE RADIO'S TABLE, not by anything upstream: 5 kHz in SSB is its 3.6 kHz.
    let from = regs.lock().unwrap().log.len();
    engine.lock().unwrap().request_filter_width(5000);
    assert!(steps_until(
        &engine,
        &mut state,
        &mut rig,
        16,
        &mut t,
        |e| !e.passband_request_pending()
    ));
    assert_eq!(width_writes(&regs, from), vec![0x40]);
    assert_eq!(
        width_shown(&engine),
        Some(3600),
        "the snapshot says what the radio has"
    );
}

/// ⛔ A REFUSAL IS FINAL ON THE NATIVE PATH. With a DATA mode commanded (the soundcard CW
/// keyer's PKTUSB — the filter FT8 decodes through) and in FM (no width table), nothing is
/// written, the request is not re-queued to be re-sent every cycle, and the optimistic width the
/// request put on screen gives way to the radio's.
#[test]
fn a_native_width_the_radio_cannot_take_is_dropped_not_retried() {
    let (d, mut rig, regs) = civ_daemon_rig(false);
    let engine = phone_engine(3081);
    {
        let mut e = engine.lock().unwrap();
        e.set_cw_keyer("soundcard", 600.0);
        e.set_operating_mode("cw", false);
    }
    let mut state = loop_state_for(&engine);
    state.rigctld_proc = Some(CatDaemon::Native(d));
    let mut t = 0.0;
    steps(&engine, &mut state, &mut rig, 2, &mut t);
    assert!(
        mode_is_data(&state.last_mode),
        "premise: the soundcard keyer commanded a DATA mode, not {:?}",
        state.last_mode
    );
    let from = regs.lock().unwrap().log.len();
    engine.lock().unwrap().request_filter_width(500);
    assert_eq!(
        width_shown(&engine),
        Some(500),
        "premise: the request shows optimistically"
    );
    assert!(
        steps_until(&engine, &mut state, &mut rig, 16, &mut t, |e| !e
            .passband_request_pending()),
        "the request is drained, not re-queued"
    );
    assert!(
        width_writes(&regs, from).is_empty(),
        "no `1A 03` write with DATA commanded"
    );
    assert_eq!(
        width_shown(&engine),
        Some(2400),
        "the radio's real width is back on screen"
    );
    steps(&engine, &mut state, &mut rig, 8, &mut t);
    assert!(
        width_writes(&regs, from).is_empty(),
        "…and it is not retried later"
    );

    // POSITIVE CONTROL: the CAT keyer (true CW, no DATA) — the same request is written.
    engine.lock().unwrap().set_cw_keyer("cat", 600.0);
    steps(&engine, &mut state, &mut rig, 2, &mut t);
    assert!(
        !mode_is_data(&state.last_mode),
        "premise: {:?}",
        state.last_mode
    );
    engine.lock().unwrap().request_filter_width(500);
    assert!(steps_until(
        &engine,
        &mut state,
        &mut rig,
        16,
        &mut t,
        |e| !e.passband_request_pending()
    ));
    assert_eq!(width_writes(&regs, from), vec![0x09], "500 Hz is code 09");

    // FM on 2 m: no table, so nothing to write — and no width to show, not the last mode's.
    {
        let mut e = engine.lock().unwrap();
        e.set_operating_mode("phone", false);
        let mut s = e.settings().clone();
        s.phone_mode = "fm".into();
        e.apply_settings(s);
        e.set_frequency(146.52, "2m", "FM");
    }
    for _ in 0..16 {
        if regs.lock().unwrap().main_mode == 0x05 {
            break;
        }
        steps(&engine, &mut state, &mut rig, 1, &mut t);
    }
    assert_eq!(
        regs.lock().unwrap().main_mode,
        0x05,
        "premise: the radio is in FM ({:?})",
        state.last_mode
    );
    let from = regs.lock().unwrap().log.len();
    engine.lock().unwrap().request_filter_width(1800);
    assert!(steps_until(
        &engine,
        &mut state,
        &mut rig,
        16,
        &mut t,
        |e| !e.passband_request_pending()
    ));
    assert!(
        width_writes(&regs, from).is_empty(),
        "no `1A 03` write in FM: {:02X?}",
        &regs.lock().unwrap().log[from..]
    );
    assert_eq!(width_shown(&engine), None, "FM has no width to show");
}

/// ⭐ THE HAMLIB PATH, BY VALUE: one `M USB <hz>` with the last width asked, and nothing else
/// carrying a width.
#[test]
fn a_width_reaches_a_hamlib_rig_once_as_set_mode() {
    let (addr, _port, log) = mock_polled_rigctld();
    let mut rig = Rig::rigctld(&addr);
    let engine = phone_engine(0);
    let mut state = loop_state();
    let mut t = 0.0;
    steps(&engine, &mut state, &mut rig, 2, &mut t);
    let from = log.lock().unwrap().len();
    {
        let mut e = engine.lock().unwrap();
        e.request_filter_width(1800);
        e.request_filter_width(2100);
    }
    steps(&engine, &mut state, &mut rig, 2, &mut t);
    assert_eq!(width_lines(&log, from), vec!["M USB 2100".to_string()]);
    assert!(!engine.lock().unwrap().passband_request_pending());
}

/// A rigctld that answers any width over 3.6 kHz with `past_3600` and accepts everything else,
/// logging every line — `mock_polled_rigctld` otherwise. `RPRT -9` is the REFUSAL an Icom on Hamlib
/// gives past its table; `RPRT -5` is Hamlib's own "the rig did not answer".
fn width_rigctld(past_3600: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let log = Arc::new(Mutex::new(Vec::<String>::new()));
    let log2 = Arc::clone(&log);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let Ok(r) = stream.try_clone() else { continue };
            let mut reader = BufReader::new(r);
            let mut line = String::new();
            loop {
                line.clear();
                match reader.read_line(&mut line) {
                    Ok(0) | Err(_) => break,
                    Ok(_) => {}
                }
                let l = line.trim().to_string();
                log2.lock().unwrap().push(l.clone());
                let mut p = l.split_whitespace();
                let too_wide = p.next() == Some("M")
                    && p.nth(1)
                        .and_then(|w| w.parse::<i64>().ok())
                        .is_some_and(|w| w > 3600);
                let reply = match l.as_str() {
                    "f" => "14250000\n",
                    "m" => "USB\n2400\n",
                    _ if too_wide => past_3600,
                    _ => "RPRT 0\n",
                };
                if stream.write_all(reply.as_bytes()).is_err() {
                    break;
                }
            }
        }
    });
    (addr, log)
}

/// ⛔ A WIDTH THE HAMLIB RIG REFUSES IS SENT ONCE, NOT FOREVER. It used to be re-queued and re-sent
/// on every mode read for good; with a waiting width owing every poll's read, that would be a write
/// every poll. The radio's own width stays on screen; a hiccup (no answer) still re-queues.
#[test]
fn a_width_a_hamlib_rig_refuses_is_sent_once() {
    let (addr, log) = width_rigctld("RPRT -9\n");
    let mut rig = Rig::rigctld(&addr);
    let engine = phone_engine(0);
    let mut state = loop_state();
    let mut t = 0.0;
    steps(&engine, &mut state, &mut rig, 2, &mut t);
    let from = log.lock().unwrap().len();
    engine.lock().unwrap().request_filter_width(4000);
    steps(&engine, &mut state, &mut rig, 6, &mut t);
    assert_eq!(
        width_lines(&log, from),
        vec!["M USB 4000".to_string()],
        "once"
    );
    assert!(
        !engine.lock().unwrap().passband_request_pending(),
        "dropped, not re-queued"
    );
    assert_eq!(
        width_shown(&engine),
        Some(2400),
        "the radio's own width, from its `m`"
    );
    // POSITIVE CONTROL: a width it takes goes out, through the same scene.
    let from = log.lock().unwrap().len();
    engine.lock().unwrap().request_filter_width(3000);
    steps(&engine, &mut state, &mut rig, 2, &mut t);
    assert_eq!(width_lines(&log, from), vec!["M USB 3000".to_string()]);
}

/// ⛔ NO WIDTH IS WRITTEN TO A KEYED RADIO, on either backend: the apply rides the receive-time
/// poll, which a keyed radio skips. The width waits and goes out at receive — the positive
/// control, which also proves the scene can write at all.
#[test]
fn no_width_is_written_while_keyed_on_either_backend() {
    // Hamlib.
    let (addr, _port, log) = mock_polled_rigctld();
    let mut rig = Rig::rigctld(&addr);
    let engine = phone_engine(0);
    let mut state = loop_state();
    let mut t = 0.0;
    steps(&engine, &mut state, &mut rig, 2, &mut t);
    let from = log.lock().unwrap().len();
    state.rig_keyed = true; // the operator's mic is down
    engine.lock().unwrap().request_filter_width(1500);
    steps(&engine, &mut state, &mut rig, 3, &mut t);
    assert!(
        width_lines(&log, from).is_empty(),
        "nothing while keyed: {:?}",
        &log.lock().unwrap()[from..]
    );
    assert!(
        engine.lock().unwrap().passband_request_pending(),
        "the width waits"
    );
    state.rig_keyed = false;
    steps(&engine, &mut state, &mut rig, 1, &mut t);
    assert_eq!(
        width_lines(&log, from),
        vec!["M USB 1500".to_string()],
        "and goes out at receive"
    );

    // The native daemon.
    let (d, mut rig, regs) = civ_daemon_rig(false);
    let engine = phone_engine(3081);
    let mut state = loop_state_for(&engine);
    state.rigctld_proc = Some(CatDaemon::Native(d));
    let mut t = 0.0;
    steps(&engine, &mut state, &mut rig, 2, &mut t);
    let from = regs.lock().unwrap().log.len();
    state.rig_keyed = true;
    engine.lock().unwrap().request_filter_width(1500);
    steps(&engine, &mut state, &mut rig, 3, &mut t);
    assert!(
        width_writes(&regs, from).is_empty(),
        "no `1A 03` to a keyed radio"
    );
    state.rig_keyed = false;
    assert!(steps_until(
        &engine,
        &mut state,
        &mut rig,
        16,
        &mut t,
        |e| !e.passband_request_pending()
    ));
    assert_eq!(
        width_writes(&regs, from),
        vec![0x19],
        "1.5 kHz (code 19) at receive"
    );
}

/// A WAITING WIDTH OWES THE MODE READ IT RIDES ON, so the radio follows a dragged edge on the
/// next poll instead of up to three polls later. The control is the same poll with nothing
/// waiting: it issues no mode read at all, so the read above is the width's doing.
#[test]
fn a_waiting_width_is_applied_on_the_next_poll() {
    let (addr, _port, log) = mock_polled_rigctld();
    let mut rig = Rig::rigctld(&addr);
    let engine = phone_engine(0);
    let mut state = loop_state();
    let mut t = 0.0;
    steps(&engine, &mut state, &mut rig, 2, &mut t);

    state.rig_poll_ticks = 0; // → 1 on the next poll: not a mode-read poll
    let from = log.lock().unwrap().len();
    steps(&engine, &mut state, &mut rig, 1, &mut t);
    assert!(
        !log.lock().unwrap()[from..].iter().any(|l| l == "m"),
        "CONTROL: an off-cadence poll with nothing waiting reads no mode"
    );

    state.rig_poll_ticks = 0;
    let from = log.lock().unwrap().len();
    engine.lock().unwrap().request_filter_width(2700);
    steps(&engine, &mut state, &mut rig, 1, &mut t);
    let sent = log.lock().unwrap()[from..].to_vec();
    assert!(
        sent.iter().any(|l| l == "m"),
        "the mode read was owed: {sent:?}"
    );
    assert_eq!(width_lines(&log, from), vec!["M USB 2700".to_string()]);
}

/// The CAT status line the operator reads.
fn cat_line(engine: &Arc<Mutex<Engine>>) -> String {
    engine.lock().unwrap().snapshot().radio.cat_detail
}

/// ⭐ A WIDTH THE RADIO DOES NOT ANSWER IS SENT THREE TIMES, THEN GIVEN UP, AS A DIAL IS. The
/// IC-9700 on the native daemon answers everything but the `1A 03` write. The width goes out three
/// times, the request is dropped, the radio's own width is back on screen, and the operator is
/// told the radio did not answer. It went out on every poll for as long as the radio stayed quiet,
/// the request never drained, the screen kept the width asked for, and nothing was said.
#[test]
fn a_width_the_native_radio_does_not_answer_is_sent_three_times_then_given_up() {
    let (d, mut rig, regs) = civ_daemon_rig(false);
    let engine = phone_engine(3081);
    let mut state = loop_state_for(&engine);
    state.rigctld_proc = Some(CatDaemon::Native(d));
    let mut t = 0.0;
    steps(&engine, &mut state, &mut rig, 2, &mut t);
    assert!(
        steps_until(&engine, &mut state, &mut rig, 16, &mut t, |e| {
            e.snapshot().radio.filter_width_hz == Some(2400)
        }),
        "premise: the radio's own width, code 28, read back"
    );
    regs.lock().unwrap().drop_filter_width_writes = u32::MAX;
    let from = regs.lock().unwrap().log.len();
    engine.lock().unwrap().request_filter_width(1800);
    steps_until(&engine, &mut state, &mut rig, 16, &mut t, |e| {
        !e.passband_request_pending()
    });
    steps(&engine, &mut state, &mut rig, 4, &mut t); // nothing more goes out after the give-up
    let pending = engine.lock().unwrap().passband_request_pending();
    assert_eq!(
        (
            width_writes(&regs, from),
            pending,
            width_shown(&engine),
            cat_line(&engine),
        ),
        (
            vec![0x22; 3],
            false,
            Some(2400),
            "1800 Hz filter width not sent — no reply from the rig after 3 tries; still 2400 Hz"
                .to_string(),
        )
    );
}

/// The control for the test above: a width the radio answers after one silence is set, and the
/// operator is told nothing. A silence is a try, not a give-up, and a width that lands starts the
/// count again: asked once more, two silences later it lands again.
#[test]
fn a_width_the_native_radio_answers_after_one_silence_is_set() {
    let (d, mut rig, regs) = civ_daemon_rig(false);
    let engine = phone_engine(3081);
    let mut state = loop_state_for(&engine);
    state.rigctld_proc = Some(CatDaemon::Native(d));
    let mut t = 0.0;
    steps(&engine, &mut state, &mut rig, 2, &mut t);
    assert!(
        steps_until(&engine, &mut state, &mut rig, 16, &mut t, |e| {
            e.snapshot().radio.filter_width_hz == Some(2400)
        }),
        "premise: the radio's own width, code 28, read back"
    );
    regs.lock().unwrap().drop_filter_width_writes = 1;
    let from = regs.lock().unwrap().log.len();
    engine.lock().unwrap().request_filter_width(1800);
    steps_until(&engine, &mut state, &mut rig, 16, &mut t, |e| {
        !e.passband_request_pending()
    });
    let first = (
        width_writes(&regs, from),
        regs.lock().unwrap().filter_raw,
        width_shown(&engine),
        cat_line(&engine).contains("1800"),
    );
    regs.lock().unwrap().drop_filter_width_writes = 2;
    let from = regs.lock().unwrap().log.len();
    engine.lock().unwrap().request_filter_width(1800);
    steps_until(&engine, &mut state, &mut rig, 16, &mut t, |e| {
        !e.passband_request_pending()
    });
    let again = (
        width_writes(&regs, from),
        width_shown(&engine),
        cat_line(&engine).contains("1800"),
    );
    assert_eq!(
        (first, again),
        (
            (vec![0x22; 2], 0x22, Some(1800), false),
            (vec![0x22; 3], Some(1800), false)
        )
    );
}

/// ⭐ …AND THE SAME ON THE HAMLIB PATH. A rigctld that answers every width over 3.6 kHz `RPRT -5`,
/// Hamlib's own "the rig did not answer", and everything else as asked. The width goes out three
/// times and is dropped, the radio's own width from its `m` is on screen, and the operator is told.
/// It went out on every poll for good.
#[test]
fn a_width_a_hamlib_rig_does_not_answer_is_sent_three_times_then_given_up() {
    let (addr, log) = width_rigctld("RPRT -5\n");
    let mut rig = Rig::rigctld(&addr);
    let engine = phone_engine(0);
    let mut state = loop_state();
    let mut t = 0.0;
    steps(&engine, &mut state, &mut rig, 2, &mut t);
    let from = log.lock().unwrap().len();
    engine.lock().unwrap().request_filter_width(4000);
    steps(&engine, &mut state, &mut rig, 8, &mut t);
    let pending = engine.lock().unwrap().passband_request_pending();
    assert_eq!(
        (
            width_lines(&log, from),
            pending,
            width_shown(&engine),
            cat_line(&engine),
        ),
        (
            vec!["M USB 4000".to_string(); 3],
            false,
            Some(2400),
            "4000 Hz filter width not sent — no reply from the rig after 3 tries; still 2400 Hz"
                .to_string(),
        )
    );
}
