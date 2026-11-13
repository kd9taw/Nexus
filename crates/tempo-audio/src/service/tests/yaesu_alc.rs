//! The ALC meter of a Yaesu FT-991/991A (Hamlib model 1035), FT-710 (1049), FTDX10 (1042) and
//! FT-450/450D (1027, 1046), on the radio's own scale.
//!
//! Both radios give their ALC meter as 0 to 255. Yaesu's FT-991A CAT manual, `RM` READ METER:
//! `RM4;` is answered `RM4` and three digits. Its FT-710 CAT manual: the same three digits, then a
//! fixed `000`. Hamlib 4.7.1's `l ALC` sends that same `RM4;`, but neither radio's backend has an
//! ALC table, so it scales with newcat's default, raw 64 = 1.0 and clamped there. The meter showed
//! four times the radio's reading, and full from a quarter of the radio's bar up.
//!
//! The rigctld here answers both ways of asking as Hamlib and the radio do (see [`FakeRigctld`]).
//! The loop is the real `RadioLoop::step`, with the radio keyed at the rig, the mic-PTT scene of
//! `a_rig_that_answers_its_transmit_meters_keeps_being_polled`. What is asserted is what the meter
//! is given: the engine's `tx_alc`, by value.
use super::*;

/// What Hamlib 4.7.1 answers `l ALC` with on a Yaesu whose backend has no ALC table: newcat's
/// `yaesu_default_alc_cal`, raw 0 = 0.0 and raw 64 = 1.0, clamped past its last point.
fn hamlib_alc(raw: u16) -> f32 {
    (f32::from(raw) / 64.0).min(1.0)
}

/// A fake rigctld in front of a radio whose ALC meter reads `raw`, 0 to 255.
///
/// - `t` answers 1 once `mic` is set (the operator holding the radio's mic key), 0 before.
/// - `w RM4;` gets the radio's own reply when `answers_raw`: `RM4`, `raw` in three digits, `p3`
///   (the FT-710's fixed `000`, empty for an FT-991A) and `;`, then a NUL and no newline, the
///   shape rigctld's send_cmd hands back
///   (`a_nul_terminated_raw_cat_reply_is_read_instead_of_timing_out`).
///   Without `answers_raw` it gets NOTHING, as from a rigctld before Hamlib 4.6: measured against
///   the real Hamlib 4.5.5 `rigctld -m 1035`, the radio answering `RM4;` with `RM4100;`, its
///   send_cmd waits for a CR or LF the radio never sends, gives up after the radio's 2 s timeout
///   and prints no reply at all (4.6 added the `;` terminator for Yaesu and Kenwood).
/// - `l ALC` gets [`hamlib_alc`] of the same reading, six decimals as rigctld prints a level.
/// - Every other line gets `RPRT 0`, no reading, so the other meters drop out after their three
///   strikes and the rest of the over is ALC reads.
///
/// Every line is logged with when it arrived. [`Self::with`] adds a radio that stops answering
/// the raw read after its first few, and rigctld 4.5's client lock.
struct FakeRigctld {
    addr: String,
    log: Arc<Mutex<Vec<(Instant, String)>>>,
    mic: Arc<std::sync::atomic::AtomicBool>,
    /// Each line whose reply waited for the client lock, and how long.
    held: Arc<Mutex<Vec<(String, Duration)>>>,
    /// When each connection was accepted: a new one after the first is the client having dropped
    /// the last.
    connects: Arc<Mutex<Vec<Instant>>>,
}

impl FakeRigctld {
    fn start(raw: u16, p3: &'static str, answers_raw: bool) -> Self {
        let raw_answers = if answers_raw { usize::MAX } else { 0 };
        Self::with(raw, p3, raw_answers, None)
    }

    /// [`Self::start`], answering only the first `raw_answers` raw reads, and with `lock` set,
    /// rigctld 4.5's one client lock: an unanswered `w` holds the radio for `lock` after it
    /// arrived (the radio's timeout, waiting for a newline), and every reply on any connection
    /// waits for it.
    fn with(raw: u16, p3: &'static str, raw_answers: usize, lock: Option<Duration>) -> Self {
        Self::serve(raw, p3, raw_answers, lock, Duration::ZERO)
    }

    /// [`Self::start`] answering, behind a network radio link that takes `delay` to answer each
    /// raw read: rigctld's round trip to a radio reached over the network.
    fn slow_raw(raw: u16, p3: &'static str, delay: Duration) -> Self {
        Self::serve(raw, p3, usize::MAX, None, delay)
    }

    fn serve(
        raw: u16,
        p3: &'static str,
        raw_answers: usize,
        lock: Option<Duration>,
        raw_delay: Duration,
    ) -> Self {
        use std::io::{BufRead, BufReader, Write};
        use std::sync::atomic::{AtomicBool, Ordering};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
        let log = Arc::new(Mutex::new(Vec::<(Instant, String)>::new()));
        let mic = Arc::new(AtomicBool::new(false));
        let held = Arc::new(Mutex::new(Vec::<(String, Duration)>::new()));
        let connects = Arc::new(Mutex::new(Vec::<Instant>::new()));
        let (log2, mic2, held2) = (Arc::clone(&log), Arc::clone(&mic), Arc::clone(&held));
        let connects2 = Arc::clone(&connects);
        let mut raw_left = raw_answers;
        let mut locked_until: Option<Instant> = None;
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                connects2.lock().unwrap().push(Instant::now());
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
                    let arrived = Instant::now();
                    log2.lock().unwrap().push((arrived, l.clone()));
                    if let Some(until) = locked_until.filter(|u| *u > arrived) {
                        std::thread::sleep(until - arrived);
                        held2.lock().unwrap().push((l.clone(), until - arrived));
                    }
                    let reply = match l.as_str() {
                        "f" => "14074000\n".to_string(),
                        "t" if mic2.load(Ordering::SeqCst) => "1\n".to_string(),
                        "t" => "0\n".to_string(),
                        "w RM4;" if raw_left > 0 => {
                            raw_left -= 1;
                            std::thread::sleep(raw_delay);
                            format!("RM4{raw:03}{p3};\0")
                        }
                        "w RM4;" => {
                            // No reply at all, and the radio held behind it.
                            locked_until = lock.map(|d| Instant::now() + d);
                            continue;
                        }
                        "l ALC" => format!("{:.6}\n", hamlib_alc(raw)),
                        _ => "RPRT 0\n".to_string(),
                    };
                    if stream.write_all(reply.as_bytes()).is_err() {
                        break;
                    }
                }
            }
        });
        Self {
            addr,
            log,
            mic,
            held,
            connects,
        }
    }

    /// Every line sent so far.
    fn lines(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|(_, l)| l.clone())
            .collect()
    }
}

/// Four seconds of an over on a radio of Hamlib `model` whose ALC reads `raw`: what the meter is
/// given, and every line the radio's rigctld was sent.
fn alc_after_an_over(model: u32, raw: u16, p3: &'static str) -> (Option<f32>, Vec<String>) {
    let (radio, sent) = radio_after_an_over(model, raw, p3);
    (radio.tx_alc, sent)
}

/// [`alc_after_an_over`], with the whole radio status the meters are drawn from.
fn radio_after_an_over(
    model: u32,
    raw: u16,
    p3: &'static str,
) -> (tempo_app::dto::RadioStatus, Vec<String>) {
    let rigctld = FakeRigctld::start(raw, p3, true);
    let over = run_an_over(&engine_for(model), &rigctld);
    (over.radio, rigctld.lines())
}

/// An engine set to Hamlib `model`.
fn engine_for(model: u32) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = model;
        e.apply_settings(s);
    }
    engine
}

/// What [`run_an_over`] saw: the radio status the meters are drawn from, how many lines the
/// rigctld had been sent when the mic was keyed, and how long each loop step took that began with
/// the loop knowing the radio keyed.
struct Over {
    radio: tempo_app::dto::RadioStatus,
    lines_before_key: usize,
    keyed_steps: Vec<Duration>,
    /// How many connections the rigctld had accepted when the mic was keyed.
    connects_before_key: usize,
}

/// Two seconds of receiving (the link comes up and is confirmed), then four seconds of an over
/// keyed at the radio's mic, through the fake rigctld. The loop is the real `RadioLoop::step`.
fn run_an_over(engine: &Arc<Mutex<Engine>>, rigctld: &FakeRigctld) -> Over {
    run_an_over_on(engine, rigctld, false)
}

/// [`run_an_over`] on a link marked a slow transport (`slow`), as a network radio link is.
fn run_an_over_on(engine: &Arc<Mutex<Engine>>, rigctld: &FakeRigctld, slow: bool) -> Over {
    let model = engine.lock().unwrap().settings().rig_model;
    let mut rig = Rig::rigctld(&rigctld.addr);
    rig.set_slow_transport(slow);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(engine);
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    let mut t = 0.0;
    let mut lines_before_key = 0;
    let mut connects_before_key = 0;
    let mut keyed_steps = Vec::new();
    for k in 0..120 {
        if k == 40 {
            lines_before_key = rigctld.log.lock().unwrap().len();
            connects_before_key = rigctld.connects.lock().unwrap().len();
            rigctld.mic.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        t += 50.0;
        // Keyed as the loop knows it: the radio's PTT read back (once a second) says so.
        let keyed = state.rig_keyed;
        let started = Instant::now();
        state
            .step(
                engine,
                &mut backend,
                &mut rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        if keyed {
            keyed_steps.push(started.elapsed());
        }
    }
    assert!(
        state.rig_keyed,
        "model {model}: control: the radio must be keyed, or no meter is read at all"
    );
    Over {
        radio: engine.lock().unwrap().snapshot().radio,
        lines_before_key,
        keyed_steps,
        connects_before_key,
    }
}

/// ⭐ THE FT-991A, THE FT-710 AND THE FTDX10 SHOW THE RADIO'S OWN BAR. Raw 64 is a quarter of the radio's bar
/// and 128 half of it; Nexus showed both as 100 %, red. Asserted by value first, every reading
/// against the bar position, raw / 255, for each radio's own reply shape; then how it was asked.
/// The FTDX10's CAT manual gives the FT-710's reply shape and range, and Hamlib 4.7.1's `ftdx10.c`
/// has no ALC table either, so it showed the same four times.
///
/// The FT-450D (#489, "ALC stuck at 100 %") and the FT-450 are the same Hamlib radio: 4.7.1's
/// `yaesu.c` builds `ft450d_caps` as a copy of `ft450_caps`, which has no `alc_cal`, and newcat asks
/// both with `RM4;` and reads three digits (`newcat.c:5285-5299`, 5572-5586). Hamlib's own tables
/// for the same `RM` command put that radio's readings on 0 to 255, not 0 to 64:
/// `FT450_RFPOWER_METER_CAL` (`ft450.h`) runs to raw 250 and `FT450_STR_CAL` to raw 240.
#[test]
fn an_ft991a_an_ft710_an_ftdx10_and_the_ft450s_show_alc_where_the_radios_own_bar_is() {
    for (model, name, p3) in [
        (1035u32, "FT-991A", ""),
        (1049, "FT-710", "000"),
        (1042, "FTDX10", "000"),
        (1046, "FT-450D", ""),
        (1027, "FT-450", ""),
    ] {
        let overs: Vec<(u16, Option<f32>, Vec<String>)> = [0u16, 64, 128, 255]
            .into_iter()
            .map(|raw| {
                let (alc, sent) = alc_after_an_over(model, raw, p3);
                (raw, alc, sent)
            })
            .collect();
        let table: Vec<String> = overs
            .iter()
            .map(|(raw, alc, _)| {
                let shown = alc.map_or("nothing".to_string(), |a| format!("{:.1} %", a * 100.0));
                format!(
                    "raw {raw}: bar {:.1} %, meter {shown}",
                    f32::from(*raw) / 2.55
                )
            })
            .collect();
        for (raw, alc, _) in &overs {
            let bar = f32::from(*raw) / 255.0;
            assert!(
                alc.is_some_and(|a| (a - bar).abs() < 1e-6),
                "{name}, raw {raw}: the meter must show where the radio's bar is — {table:?}"
            );
        }
        for (_, _, sent) in &overs {
            assert!(
                sent.iter().any(|l| l == "w RM4;"),
                "{name}: the radio's own ALC value is read — {sent:?}"
            );
            assert!(
                !sent.iter().any(|l| l == "l ALC"),
                "{name}: Hamlib's scaled ALC is not asked for — {sent:?}"
            );
            for meter in ["SWR", "RFPOWER_METER_WATTS", "COMP_METER"] {
                assert!(
                    sent.iter().any(|l| *l == format!("l {meter}")),
                    "{name}: {meter} is still read through Hamlib — {sent:?}"
                );
            }
        }
    }
}

/// Every model on main that reads its ALC raw ([`crate::rigmodels::raw_alc_query`]).
const RAW_ALC_MODELS: [(u32, &str, &str); 6] = [
    (1035, "FT-991A", ""),
    (1049, "FT-710", "000"),
    (1042, "FTDX10", "000"),
    (1046, "FT-450D", ""),
    (1027, "FT-450", ""),
    (1051, "FTX-1", "000"),
];

/// ⭐ A RAW-ALC RADIO BEHIND A RIGCTLD THAT NEVER ANSWERS RAW CAT KEEPS ITS ALC METER (the 1.19.0
/// regression), AND ITS OVER IS NEVER HELD UP ASKING. Network connection, where the operator's
/// own rigctld runs: Nexus shares it, and a Hamlib before 4.6 never answers the raw `w RM4;`.
/// The question is asked ONCE, while receiving; the over then reads `l ALC`, on Hamlib's scale
/// and so with no zone of the radio's own (raw 32 is 0.5 on Hamlib's scale, 0.125 on the
/// radio's). Each unanswered raw read used to block the loop, the only thing that unkeys the
/// radio, for about a second, three times in the first over after every connect: no loop step
/// of the over may take 200 ms.
#[test]
fn a_raw_alc_radio_behind_a_rigctld_that_never_answers_raw_cat_reads_alc_through_hamlib() {
    for (model, name, p3) in RAW_ALC_MODELS {
        let rigctld = FakeRigctld::start(32, p3, false);
        let over = run_an_over(&engine_for(model), &rigctld);
        let sent = rigctld.lines();
        assert_eq!(
            (over.radio.tx_alc, over.radio.tx_alc_zone),
            (Some(0.5), None),
            "{name}: the meter shows Hamlib's ALC, with Hamlib's zone — {sent:?}"
        );
        let raw_asks: Vec<usize> = sent
            .iter()
            .enumerate()
            .filter(|(_, l)| *l == "w RM4;")
            .map(|(at, _)| at)
            .collect();
        assert!(
            raw_asks.len() == 1 && raw_asks[0] < over.lines_before_key,
            "{name}: the raw read is asked once, while receiving, never in the over — {sent:?}"
        );
        assert!(
            over.keyed_steps.len() > 40,
            "{name}: premise: two seconds and more of the over were stepped keyed"
        );
        let slowest = over.keyed_steps.iter().max().copied().unwrap_or_default();
        assert!(
            slowest < Duration::from_millis(200),
            "{name}: no step of the over is held up, the slowest took {slowest:?} — {sent:?}"
        );
    }
}

/// ⭐ STOP TX IN THE FIRST OVER AFTER CONNECTING LANDS AT ONCE, BEHIND A RIGCTLD BEFORE 4.6. A
/// Phone PTT over keyed by Nexus; the Stop is pressed from another thread the moment the over's
/// first ALC read reaches the rigctld, whichever way it is asked. The unkey (`T 0`) must reach
/// the rigctld within 200 ms of the press. It used to wait out the unanswered raw read first.
#[test]
fn a_stop_tx_in_the_first_over_is_not_held_up_by_the_alc_read() {
    for (model, name, p3) in RAW_ALC_MODELS {
        let rigctld = FakeRigctld::start(32, p3, false);
        let engine = engine_for(model);
        engine.lock().unwrap().set_operating_mode("phone", false);
        let mut rig = Rig::rigctld(&rigctld.addr);
        let mut backend = MockBackend::new();
        let mut state = loop_state_for(&engine);
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        // The Stop, pressed when the first ALC read after the key arrives.
        let pressed = Arc::new(Mutex::new(None::<Instant>));
        let done = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let watcher = {
            let (log, engine, pressed, done) = (
                Arc::clone(&rigctld.log),
                Arc::clone(&engine),
                Arc::clone(&pressed),
                Arc::clone(&done),
            );
            std::thread::spawn(move || {
                while !done.load(std::sync::atomic::Ordering::SeqCst) {
                    let alc_after_key = {
                        let log = log.lock().unwrap();
                        log.iter()
                            .skip_while(|(_, l)| l != "T 1")
                            .any(|(_, l)| l == "l ALC" || l == "w RM4;")
                    };
                    if alc_after_key {
                        *pressed.lock().unwrap() = Some(Instant::now());
                        engine.lock().unwrap().halt_tx();
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };
        let mut t = 0.0;
        for k in 0..120 {
            if k == 40 {
                let mut e = engine.lock().unwrap();
                e.set_tx_enabled(true);
                e.set_ptt(true);
            }
            t += 50.0;
            state
                .step(
                    &engine,
                    &mut backend,
                    &mut rig,
                    &sinks,
                    t,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }
        done.store(true, std::sync::atomic::Ordering::SeqCst);
        watcher.join().unwrap();
        let sent = rigctld.lines();
        let pressed = pressed
            .lock()
            .unwrap()
            .unwrap_or_else(|| panic!("{name}: premise: the over read its ALC — {sent:?}"));
        let unkeyed = rigctld
            .log
            .lock()
            .unwrap()
            .iter()
            .find(|(at, l)| l == "T 0" && *at >= pressed)
            .map(|(at, _)| *at)
            .unwrap_or_else(|| panic!("{name}: the Stop unkeyed the radio — {sent:?}"));
        let late = unkeyed - pressed;
        assert!(
            late < Duration::from_millis(200),
            "{name}: the unkey reached the radio {late:?} after Stop TX — {sent:?}"
        );
    }
}

/// …and a rigctld from 4.6 on, which answers the raw read, still has the radio's own ALC read
/// raw during the over, now that it was found out while receiving: the probe is the one raw read
/// before the key, and the over goes on asking `w RM4;`, never `l ALC`.
#[test]
fn a_rigctld_that_answers_raw_cat_still_has_the_over_read_alc_raw() {
    for (model, name, p3) in RAW_ALC_MODELS {
        let rigctld = FakeRigctld::start(128, p3, true);
        let over = run_an_over(&engine_for(model), &rigctld);
        let sent = rigctld.lines();
        let (before, during) = sent.split_at(over.lines_before_key);
        assert_eq!(
            before.iter().filter(|l| *l == "w RM4;").count(),
            1,
            "{name}: asked once while receiving — {sent:?}"
        );
        assert!(
            during.iter().any(|l| l == "w RM4;") && !during.iter().any(|l| l == "l ALC"),
            "{name}: the over reads the radio's own ALC — {sent:?}"
        );
        assert_eq!(
            over.radio.tx_alc.map(|a| (a * 255.0).round()),
            Some(128.0),
            "{name}: on the radio's own bar"
        );
    }
}

/// ⛔ THE QUESTION IS NEVER ASKED UNDER A CARRIER. A Phone PTT over keyed by Nexus before the
/// link comes up, so the link is confirmed under it: while it is keyed, no `w RM4;`; once it
/// unkeys, the question goes, while receiving.
#[test]
fn the_raw_alc_question_waits_for_a_keyed_over_to_end() {
    let rigctld = FakeRigctld::start(64, "", true);
    let engine = engine_for(1035);
    engine.lock().unwrap().set_operating_mode("phone", false);
    let mut rig = Rig::rigctld(&rigctld.addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    {
        let mut e = engine.lock().unwrap();
        e.set_tx_enabled(true);
        e.set_ptt(true);
    }
    let mut confirmed_keyed = false;
    let mut t = 0.0;
    for _ in 0..120 {
        if t < 3_000.0 && t > 0.0 && state.cat_ok == Some(true) && rig.keyed {
            confirmed_keyed = true;
        }
        if t == 3_000.0 {
            engine.lock().unwrap().set_ptt(false);
        }
        t += 50.0;
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
    }
    let sent = rigctld.lines();
    assert!(
        confirmed_keyed,
        "premise: the link was confirmed while keyed — {sent:?}"
    );
    let key = sent.iter().position(|l| l == "T 1");
    let unkey = sent.iter().position(|l| l == "T 0");
    let (Some(key), Some(unkey)) = (key, unkey) else {
        panic!("premise: keyed and unkeyed — {sent:?}");
    };
    assert!(
        !sent[key..unkey].iter().any(|l| l == "w RM4;"),
        "no question while keyed — {sent:?}"
    );
    assert_eq!(
        sent[unkey..].iter().filter(|l| *l == "w RM4;").count(),
        1,
        "the question once, after the unkey — {sent:?}"
    );
}

/// ⭐ A LINK THAT ANSWERED THE QUESTION AND THEN STOPS ANSWERING THE RAW READ, MID-OVER, NEVER
/// HOLDS THE LOOP FOR THE TRANSPORT DEADLINE. Each raw read in the over gives up after 300 ms, and
/// after three misses the over reads `l ALC`. No loop step of the over takes 400 ms.
#[test]
fn a_raw_read_that_stops_answering_mid_over_gives_up_quickly() {
    let rigctld = FakeRigctld::with(64, "", 1, None);
    let over = run_an_over(&engine_for(1035), &rigctld);
    let sent = rigctld.lines();
    let during = &sent[over.lines_before_key..];
    assert!(
        during.iter().filter(|l| *l == "w RM4;").count() >= 1,
        "premise: the over asked raw, the question having been answered — {sent:?}"
    );
    assert!(
        during.iter().any(|l| l == "l ALC"),
        "the backstop: the over falls back to Hamlib's ALC — {sent:?}"
    );
    let slowest = over.keyed_steps.iter().max().copied().unwrap_or_default();
    assert!(
        slowest < Duration::from_millis(400),
        "no step held for the transport deadline, the slowest took {slowest:?} — {sent:?}"
    );
}

/// ⭐ THE SAME IN A TIMED OVER, WHOSE METER READS CARRY THE OVER'S OWN DEADLINE (the time left to
/// its end, as every FT8/FT4 over's do). That deadline must not lift the raw read's own strict
/// bound, and must not stop the backstop: a link that answered the question and then stops
/// answering the raw read gives up after 300 ms a try and falls back to `l ALC`. The hold is set
/// directly, far ahead, rather than keyed by an FT8 over.
#[test]
fn a_timed_over_raw_read_that_stops_answering_gives_up_quickly() {
    let rigctld = FakeRigctld::with(64, "", 1, None);
    let engine = engine_for(1035);
    let mut rig = Rig::rigctld(&rigctld.addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    let (mut t, mut lines_before_key, mut steps) = (0.0, 0, Vec::new());
    for k in 0..120 {
        t += 50.0;
        if k == 40 {
            lines_before_key = rigctld.log.lock().unwrap().len();
        }
        if k >= 40 {
            state.tx_until_ms = Some(t + 60_000.0); // an over's own deadline, far ahead
        }
        let started = Instant::now();
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        if k >= 40 {
            steps.push(started.elapsed());
        }
    }
    let sent = rigctld.lines();
    let during = &sent[lines_before_key..];
    assert!(
        during.iter().any(|l| l == "w RM4;"),
        "premise: the over asked raw — {sent:?}"
    );
    let slowest = steps.iter().max().copied().unwrap_or_default();
    assert!(
        slowest < Duration::from_millis(400),
        "a step held for the transport deadline: {slowest:?} — {sent:?}"
    );
    assert!(
        during.iter().any(|l| l == "l ALC"),
        "the backstop never fell back to Hamlib's ALC — {sent:?}"
    );
}

/// ⭐ A NETWORK RADIO LINK THAT ANSWERS THE RAW READ IN HALF A SECOND KEEPS ITS CONNECTION AND
/// ITS ALC THROUGH THE OVER. The link is a slow transport, as every network link is, and answers
/// the probe and every raw read in 500 ms. The over's raw read waits as long as `l ALC` would
/// there, so it is answered: the meter shows the radio's own bar, `l ALC` is never asked, and no
/// raw read drops the connection. With a 300 ms deadline every raw read of the over missed and
/// dropped the connection mid-over, and the over fell back to Hamlib's scale. No keyed loop step
/// outlasts the slow transport's 2.5 s and one tick.
#[test]
fn a_network_link_slow_to_answer_the_raw_read_keeps_its_connection_and_its_alc() {
    let rigctld = FakeRigctld::slow_raw(128, "", Duration::from_millis(500));
    let over = run_an_over_on(&engine_for(1035), &rigctld, true);
    let sent = rigctld.lines();
    let during = &sent[over.lines_before_key..];
    assert!(
        during.iter().any(|l| l == "w RM4;"),
        "premise: the over asked raw, the question having been answered — {sent:?}"
    );
    // A connection opened in the over right after a raw read is that read's miss dropping the
    // last one. (A one-off `\dump_caps` also opens a new one on a slow transport, by design.)
    let log = rigctld.log.lock().unwrap().clone();
    let dropped_by_raw: Vec<Instant> = rigctld.connects.lock().unwrap()[over.connects_before_key..]
        .iter()
        .copied()
        .filter(|at| {
            log.iter()
                .rev()
                .find(|(line_at, _)| line_at < at)
                .is_some_and(|(_, l)| l == "w RM4;")
        })
        .collect();
    assert!(
        dropped_by_raw.is_empty(),
        "no raw read of the over dropped the connection: {} opened right after one — {sent:?}",
        dropped_by_raw.len()
    );
    assert!(
        !during.iter().any(|l| l == "l ALC"),
        "the over keeps reading the radio's own ALC — {sent:?}"
    );
    assert_eq!(
        over.radio.tx_alc.map(|a| (a * 255.0).round()),
        Some(128.0),
        "on the radio's own bar — {sent:?}"
    );
    let slowest = over.keyed_steps.iter().max().copied().unwrap_or_default();
    assert!(
        slowest < Duration::from_millis(2_500 + 50),
        "no keyed step past the slow transport's deadline and a tick, the slowest took \
         {slowest:?} — {sent:?}"
    );
}

/// ⛔ AN FT8 OVER DUE AT A BOUNDARY NEVER WAITS BEHIND THE QUESTION, on a rigctld before 4.6
/// whose client lock holds the radio for 2 s behind the unanswered raw read. The link comes up
/// 1.5 s before this station's own boundary, inside the probe's 3.5 s lead: the question waits,
/// and the over's key (`T 1`) reaches the radio on time (within 200 ms of its boundary) and is
/// answered without waiting on the lock. The loop steps on the real clock, so the lock's 2 s are
/// the loop's 2 s. Asked on that tick, the question cost the key over a second: the loop's next
/// read waited out the lock.
#[test]
fn an_ft8_key_at_the_boundary_never_waits_behind_the_question() {
    let engine = engine_for(1035);
    {
        let mut e = engine.lock().unwrap();
        e.set_license_class("extra");
        e.set_tier(tempo_app::dto::Tier::Ft8);
        e.set_frequency(14.074, "20m", "USB");
        e.set_tx_offset(1500.0);
        e.start_cq(None).expect("Call CQ");
        let _ = e.take_immediate_tx();
    }
    let rigctld = FakeRigctld::with(64, "", 0, Some(Duration::from_secs(2)));
    let mut rig = Rig::rigctld(&rigctld.addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    // An even FT8 boundary ahead of the real clock, which the key's lateness is measured on.
    let t0 = ((now_unix_ms() + 30_000.0) / 30_000.0).ceil() * 30_000.0;
    let start = t0 - 1_500.0;
    let mut t = start;
    let w0 = Instant::now();
    while t <= t0 + 1_000.0 {
        let due = w0 + Duration::from_secs_f64((t - start) / 1000.0);
        if let Some(wait) = due.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        t += 20.0;
    }
    let sent = rigctld.lines();
    let key_at = rigctld
        .log
        .lock()
        .unwrap()
        .iter()
        .find(|(_, l)| l == "T 1")
        .map(|(at, _)| *at)
        .unwrap_or_else(|| panic!("premise: the over keyed at the boundary — {sent:?}"));
    let due = w0 + Duration::from_secs_f64((t0 - start) / 1000.0);
    let late = key_at.saturating_duration_since(due);
    assert!(
        late < Duration::from_millis(200),
        "the key reached the radio {late:?} after its boundary — {sent:?}"
    );
    let held = rigctld.held.lock().unwrap().clone();
    assert!(
        !held.iter().any(|(l, _)| l == "T 1"),
        "the key waited behind the question's lock: {held:?} — {sent:?}"
    );
}

/// ⛔ ARMED ON FT8, THE QUESTION WAITS THREE SECONDS PAST A BOUNDARY, for the automatic reply that
/// late-starts off that boundary's decode (which can land seconds after it). A CQ armed, the link
/// up 1.2 s into the other station's period, the question owed: it goes no sooner than 3 s into
/// the slot and no later than 3.5 s before the next boundary.
#[test]
fn armed_on_ft8_the_question_waits_three_seconds_past_a_boundary() {
    let engine = engine_for(1035);
    {
        let mut e = engine.lock().unwrap();
        e.set_license_class("extra");
        e.set_tier(tempo_app::dto::Tier::Ft8);
        e.set_frequency(14.074, "20m", "USB");
        e.set_tx_offset(1500.0);
        e.start_cq(None).expect("Call CQ");
        let _ = e.take_immediate_tx();
        assert!(e.tx_enabled(), "premise: TX armed");
    }
    let rigctld = FakeRigctld::start(64, "", true);
    let mut rig = Rig::rigctld(&rigctld.addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    // The even boundaries are this station's; start 1.2 s into an odd (receive) slot.
    let t0 = ((now_unix_ms() + 30_000.0) / 30_000.0).ceil() * 30_000.0;
    let mut t = t0 + 15_000.0 + 1_200.0;
    let mut asked_at = Vec::new();
    while t < t0 + 30_000.0 {
        let before = rigctld.log.lock().unwrap().len();
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        if rigctld.log.lock().unwrap()[before..]
            .iter()
            .any(|(_, l)| l == "w RM4;")
        {
            asked_at.push(t.rem_euclid(15_000.0));
        }
        t += 20.0;
    }
    assert_eq!(
        asked_at.len(),
        1,
        "asked once in the receive slot, at these points (ms into it) — {asked_at:?}"
    );
    assert!(
        (3_000.0..=11_500.0).contains(&asked_at[0]),
        "asked {} ms into the slot: inside the margin",
        asked_at[0]
    );
}

/// A station on FT2 with ALC read raw: an engine on Hamlib `model`, FT2, licensed, on 20 m.
fn ft2_engine(model: u32) -> Arc<Mutex<Engine>> {
    let engine = engine_for(model);
    {
        let mut e = engine.lock().unwrap();
        e.set_license_class("extra");
        e.set_tier(tempo_app::dto::Tier::Ft2);
        e.set_frequency(14.074, "20m", "USB");
        e.set_tx_offset(1500.0);
    }
    engine
}

/// ⭐ FT2 WITH TX OFF ASKS THE QUESTION AT ONCE AND READS THE RADIO'S OWN ALC. FT2's 3.75 s slot
/// has no room for the slot margin, but with TX off nothing keys on a boundary and the quiet
/// point is all: the question goes while receiving, once, and the over that follows reads
/// `w RM4;`, on the radio's own bar, never Hamlib's `l ALC`.
#[test]
fn ft2_with_tx_off_asks_at_once_and_the_next_over_reads_raw() {
    let engine = ft2_engine(1035);
    assert!(!engine.lock().unwrap().tx_enabled(), "premise: TX off");
    let rigctld = FakeRigctld::start(128, "", true);
    let over = run_an_over(&engine, &rigctld);
    let sent = rigctld.lines();
    let (before, during) = sent.split_at(over.lines_before_key);
    assert_eq!(
        before.iter().filter(|l| *l == "w RM4;").count(),
        1,
        "asked once while receiving — {sent:?}"
    );
    assert!(
        during.iter().any(|l| l == "w RM4;") && !during.iter().any(|l| l == "l ALC"),
        "the over reads the radio's own ALC — {sent:?}"
    );
    assert_eq!(
        over.radio.tx_alc.map(|a| (a * 255.0).round()),
        Some(128.0),
        "on the radio's own bar"
    );
}

/// ⛔ FT2 WITH TX ARMED NEVER ASKS INSIDE THE MARGIN OF ANY BOUNDARY, EITHER PERIOD, AND SO NOT AT
/// ALL. A CQ running on FT2, the question owed, for six slots; at the third boundary the period
/// flips (as the sequencer flips it when it answers a station heard in our own period), so a
/// boundary that was the other station's becomes ours. Every tick that sends `w RM4;` would have
/// to be 3.5 s before the next boundary and 1 s after the last, and a 3.75 s slot has no such
/// tick. Measured to this station's own period only, the question would go in the gap after our
/// over, and the flipped boundary would key behind it.
#[test]
fn ft2_with_tx_armed_never_asks_inside_the_margin_of_any_boundary() {
    let engine = ft2_engine(1035);
    {
        let mut e = engine.lock().unwrap();
        e.start_cq(None).expect("Call CQ");
        let _ = e.take_immediate_tx();
        assert!(e.tx_enabled(), "premise: TX armed");
    }
    let period = engine.lock().unwrap().active_slot_secs() * 1000.0;
    assert_eq!(period, 3_750.0, "premise: FT2's slot");
    let rigctld = FakeRigctld::start(64, "", true);
    let mut rig = Rig::rigctld(&rigctld.addr);
    let mut backend = MockBackend::new();
    let mut state = loop_state_for(&engine);
    let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
    let mut station = StationSinks::new();
    // A boundary pair ahead of the real clock, which a key's lateness is measured on.
    let t0 = ((now_unix_ms() + 30_000.0) / (2.0 * period)).ceil() * (2.0 * period);
    let mut t = t0 - 500.0;
    let mut asked_at = Vec::new();
    let mut flipped = false;
    while t <= t0 + 6.0 * period {
        if !flipped && t >= t0 + 3.0 * period {
            let mut e = engine.lock().unwrap();
            let even = e.tx_even();
            e.set_tx_even(!even);
            flipped = true;
        }
        let before = rigctld.log.lock().unwrap().len();
        state
            .step(
                &engine,
                &mut backend,
                &mut rig,
                &sinks,
                t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        if rigctld.log.lock().unwrap()[before..]
            .iter()
            .any(|(_, l)| l == "w RM4;")
        {
            asked_at.push(t.rem_euclid(period));
        }
        t += 20.0;
    }
    assert!(
        engine.lock().unwrap().tx_enabled(),
        "premise: TX stayed armed throughout"
    );
    assert_eq!(
        asked_at,
        Vec::<f64>::new(),
        "asked at these points of the slot (ms after its boundary) — {:?}",
        rigctld.lines()
    );
}

/// …and every other radio reads its ALC as it did: another Yaesu (the FTDX101D, whose Hamlib
/// backend has an ALC table of its own) and an Icom (the IC-7300 through Hamlib). Whatever `l ALC` answers is what the meter
/// is given, and the radio is sent no `RM4;`, which on an Icom's CI-V bus is junk. By value
/// first, every reading, as above: raw 0 reads 0 on either scale, so it proves nothing alone.
#[test]
fn every_other_radios_alc_still_reads_through_hamlib() {
    for (model, name) in [(1040u32, "FTDX101D"), (3073, "IC-7300")] {
        let overs: Vec<(u16, Option<f32>, Vec<String>)> = [0u16, 64, 128, 255]
            .into_iter()
            .map(|raw| {
                let (alc, sent) = alc_after_an_over(model, raw, "000");
                (raw, alc, sent)
            })
            .collect();
        let got: Vec<(u16, Option<f32>)> = overs.iter().map(|(raw, alc, _)| (*raw, *alc)).collect();
        let want: Vec<(u16, Option<f32>)> = overs
            .iter()
            .map(|(raw, _, _)| (*raw, Some(hamlib_alc(*raw))))
            .collect();
        assert_eq!(
            got, want,
            "{name}: unchanged, the meter shows Hamlib's own answer (raw, meter)"
        );
        for (_, _, sent) in &overs {
            assert!(
                sent.iter().any(|l| l == "l ALC"),
                "{name}: ALC is still read through Hamlib — {sent:?}"
            );
            assert!(
                !sent.iter().any(|l| l == "w RM4;"),
                "{name}: no raw RM4 is sent to it — {sent:?}"
            );
        }
    }
}

/// A rigctld whose radio answers every line with `RM4064;` (and the NUL), but only after `late`.
fn late_rm4_rigctld(late: std::time::Duration) -> String {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
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
                std::thread::sleep(late);
                if stream.write_all(b"RM4064;\0").is_err() {
                    break;
                }
            }
        }
    });
    addr
}

/// A tune-up's ALC read is bounded on these radios too. A meter read blocks the loop while the
/// tune carrier plays out of a short lead, so the poll passes the tune's deadline
/// (`TUNE_METER_DEADLINE_MS`) and an answer later than that is given up on. The same late answer
/// IS read with the normal deadline, the control that the fake radio answers at all. A slow
/// transport, so the normal deadline (2.5 s) is far from the late answer (1 s).
#[test]
fn a_tune_ups_alc_read_gives_up_at_the_tune_deadline() {
    let mut rig = Rig::rigctld(&late_rm4_rigctld(std::time::Duration::from_millis(1_000)));
    rig.set_slow_transport(true);
    assert_eq!(
        read_tx_meter(&mut rig, "ALC", 1035, true, None),
        Some(64.0 / 255.0),
        "control: with the normal deadline the late answer is read"
    );
    assert_eq!(
        read_tx_meter(&mut rig, "ALC", 1035, true, Some(TUNE_METER_DEADLINE_MS)),
        None,
        "a tune-up does not wait a second for its ALC"
    );
}

/// The ALC reading carries where the radio's own ALC zone ends, so the bar's colours sit where
/// the radio's meter marks it. The FT-991A's manual puts the zone at "up to half scale deflection"
/// (Operating Manual p. 61), and the FT-710's meter marks the same zone (p. 40); the FTDX10 is read
/// the same way. The FT-450D's and the FTX-1's bar is the radio's own, but no zone is claimed for
/// it. Every other
/// radio's 1.0 is the top of its zone, as before: no zone is carried.
#[test]
fn the_alc_reading_carries_the_radios_own_zone_edge() {
    let zones: Vec<(&str, Option<f32>, Option<f32>)> = [
        (1035u32, "FT-991A", ""),
        (1049, "FT-710", "000"),
        (1042, "FTDX10", "000"),
        (1046, "FT-450D", ""),
        (1051, "FTX-1", "000"),
        (3073, "IC-7300", ""),
    ]
    .into_iter()
    .map(|(model, name, p3)| {
        let (radio, _) = radio_after_an_over(model, 128, p3);
        (
            name,
            radio.tx_alc.map(|a| (a * 255.0).round()),
            radio.tx_alc_zone,
        )
    })
    .collect();
    assert_eq!(
        zones,
        vec![
            ("FT-991A", Some(128.0), Some(0.5)),
            ("FT-710", Some(128.0), Some(0.5)),
            ("FTDX10", Some(128.0), Some(0.5)),
            // The radio's own bar, and no zone claimed: its manual's zone is unsourced here.
            ("FT-450D", Some(128.0), None),
            // #502: the FTX-1 on its own bar too (it read Hamlib's 0-to-64 scale, so raw 128
            // pegged 1.0); no zone claimed, as for the FT-450D.
            ("FTX-1", Some(128.0), None),
            // Hamlib's scale for the IC-7300: raw 128 is past 64, so 1.0, and no zone of its own.
            ("IC-7300", Some(255.0), None),
        ]
    );
}
