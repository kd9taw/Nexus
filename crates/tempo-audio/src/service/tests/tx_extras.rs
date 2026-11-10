//! The radio's PA temperature and SSB transmit bandwidth, and the speech-processor level write,
//! through the real `RadioLoop::step` (`crate::txextras`), against a rigctld that behaves as
//! Hamlib's does with a Yaesu behind it: a raw READ is answered, a raw SET through `w` is answered
//! with nothing AND holds the radio's port for the model's read timeout (2 s), a raw SET through
//! `W <cmd> 0` (what `Rig::send_raw_set` sends) is answered with one terminator at once and holds
//! nothing, and a level set is
//! answered `RPRT 0` once Hamlib's own `ID;` verify is back, or, from a radio that has gone
//! silent, holds the port through Hamlib's verify retries. What each radio is asked, what reaches
//! the snapshot, and that no write this feature can make stalls the next command, a release
//! above all.
use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// How long the fake rigctld holds the radio's port after a raw set: the FTDX101's Hamlib
/// `.timeout` (measured 2.0 s on rigctld 4.5.5 with a fake FTDX101).
const SET_TAIL: Duration = Duration::from_secs(2);

/// How long a level set to a SILENT radio holds rigctld's port: Hamlib 4.7.1's `newcat_set_cmd`
/// writes the set, then `ID;`, waits the port timeout for an answer, and goes round again while
/// `retry_count++ <= rp->retry`. An FTDX101 has `.retry = 3`, `.timeout = 2000`: four tries of 2 s.
/// Read from the source, not measured.
const SILENT_LEVEL_SET_TAIL: Duration = Duration::from_secs(8);

/// What the fake radio says, changeable mid-test.
#[derive(Default)]
struct Radio {
    /// Its answer to `ID;` (`ID0681;`); `?;` when `None`.
    id: Option<&'static str>,
    /// Its TEMP meter, 0-255; `?;` when `None`.
    temp_raw: Option<u16>,
    /// The radio has stopped answering: an `L …` gets no reply at all, and rigctld's port is held
    /// for [`SILENT_LEVEL_SET_TAIL`] behind it, as Hamlib's verify retries hold it.
    silent_level_sets: AtomicBool,
}

/// Every line the fake got, with when the RADIO got it (after any set's tail).
type Log = Arc<Mutex<Vec<(Instant, String)>>>;

fn fake_rigctld(radio: Arc<Mutex<Radio>>, keyed: Arc<AtomicBool>) -> (String, Log) {
    use std::io::{BufRead, BufReader, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = format!("127.0.0.1:{}", listener.local_addr().unwrap().port());
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    // rigctld serves each client on its own thread, but there is ONE radio port behind them.
    let port_free_at = Arc::new(Mutex::new(Instant::now()));
    let log2 = Arc::clone(&log);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { break };
            let (radio, keyed, log, port) = (
                Arc::clone(&radio),
                Arc::clone(&keyed),
                Arc::clone(&log2),
                Arc::clone(&port_free_at),
            );
            std::thread::spawn(move || {
                let mut reader = BufReader::new(match stream.try_clone() {
                    Ok(r) => r,
                    Err(_) => return,
                });
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break,
                        Ok(_) => {}
                    }
                    let l = line.trim().to_string();
                    // Wait for the radio's port, as rigctld's rig lock makes every client do.
                    let free = *port.lock().unwrap();
                    if let Some(wait) = free.checked_duration_since(Instant::now()) {
                        std::thread::sleep(wait);
                    }
                    log.lock().unwrap().push((Instant::now(), l.clone()));
                    let r = radio.lock().unwrap();
                    let reply = if l == "f" {
                        "14200000\n".to_string()
                    } else if l == "t" {
                        format!("{}\n", u8::from(keyed.load(Ordering::SeqCst)))
                    } else if l == "w ID;" {
                        format!(
                            "{};\0",
                            r.id.map_or("?".to_string(), |id| format!("ID{id}"))
                        )
                    } else if l == "w MD0;" {
                        // The loop's own raw mode read on a Yaesu (not this feature's).
                        "MD02;\0".to_string()
                    } else if l == "w RM9;" {
                        r.temp_raw
                            .map_or("?;\0".to_string(), |t| format!("RM9{t:03}000;\0"))
                    } else if l.starts_with("W ") && l.ends_with(" 0") {
                        // `W <cmd> 0`: Hamlib reads no reply for it and answers with one
                        // terminator (a NUL on 4.7.1), holding nothing.
                        "\0".to_string()
                    } else if l.starts_with("w ") {
                        // A raw SET: Hamlib returns nothing and holds the port for its timeout.
                        *port.lock().unwrap() = Instant::now() + SET_TAIL;
                        String::new()
                    } else if l.starts_with("L ") && r.silent_level_sets.load(Ordering::SeqCst) {
                        *port.lock().unwrap() = Instant::now() + SILENT_LEVEL_SET_TAIL;
                        String::new()
                    } else {
                        "RPRT 0\n".to_string()
                    };
                    drop(r);
                    if !reply.is_empty() && stream.write_all(reply.as_bytes()).is_err() {
                        break;
                    }
                }
            });
        }
    });
    (addr, log)
}

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

/// The loop, its rig, the fake radio and a clock, so a test can step on and act in between.
struct Bench {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    rig: Rig,
    radio: Arc<Mutex<Radio>>,
    keyed: Arc<AtomicBool>,
    log: Log,
    t: f64,
}

impl Bench {
    fn new(model: u32, radio: Radio) -> Self {
        let radio = Arc::new(Mutex::new(radio));
        let keyed = Arc::new(AtomicBool::new(false));
        let (addr, log) = fake_rigctld(Arc::clone(&radio), Arc::clone(&keyed));
        let engine = engine_for(model);
        let state = loop_state_for(&engine);
        Self {
            engine,
            state,
            rig: Rig::rigctld(&addr),
            radio,
            keyed,
            log,
            t: 0.0,
        }
    }
    /// `secs` of the loop at its 50 ms tick.
    fn run(&mut self, secs: f64) {
        let mut backend = MockBackend::new();
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        for _ in 0..(secs * 20.0).round() as usize {
            self.t += 50.0;
            self.state
                .step(
                    &self.engine,
                    &mut backend,
                    &mut self.rig,
                    &sinks,
                    self.t,
                    &mut ra,
                    &mut rr,
                    &mut station,
                )
                .unwrap();
        }
    }
    fn radio(&self) -> tempo_app::dto::RadioStatus {
        self.engine.lock().unwrap().snapshot().radio
    }
    fn sent(&self) -> Vec<String> {
        self.log
            .lock()
            .unwrap()
            .iter()
            .map(|(_, l)| l.clone())
            .collect()
    }
    /// The raw strings sent, less the loop's own `MD0;` mode read, which predates this feature.
    fn raw_sent(&self) -> Vec<String> {
        self.sent()
            .into_iter()
            .filter(|l| l.starts_with("w ") && l != "w MD0;")
            .collect()
    }
    fn comp_sets(&self) -> Vec<String> {
        self.sent()
            .into_iter()
            .filter(|l| l.starts_with("L COMP"))
            .collect()
    }
}

fn yaesu(id: &'static str, temp_raw: u16) -> Radio {
    Radio {
        id: Some(id),
        temp_raw: Some(temp_raw),
        ..Radio::default()
    }
}

/// ⭐ THE FTDX101, KNOWN BY ITS OWN ID, SHOWS ITS TEMP METER, and nothing else of this feature:
/// the meter as the radio's own position (102 of 255 = 0.4, never degrees). Its TX BPF menu is
/// parked, so it is never read or written, and the meter offers no PROC on a Yaesu.
#[test]
fn an_ftdx101_by_its_id_reports_its_temp_meter_and_nothing_of_the_parked_menu() {
    let mut b = Bench::new(1040, yaesu("0681", 102));
    b.run(15.0);
    let r = b.radio();
    assert_eq!(r.pa_temp_meter, Some(0.4));
    assert_eq!(r.pa_temp_c, None, "the FTDX101 publishes no degree scale");
    assert!(
        r.tx_bw_steps.is_empty() && r.tx_bw.is_none(),
        "parked: no chip"
    );
    assert!(!r.tx_proc_switch, "no PROC on the meter for a Yaesu");
    let raw = b.raw_sent();
    assert_eq!(raw.first().map(String::as_str), Some("w ID;"), "{raw:?}");
    assert!(raw.iter().any(|l| l == "w RM9;"), "{raw:?}");
    assert!(
        raw.iter().all(|l| l == "w ID;" || l == "w RM9;"),
        "anything else raw: {raw:?}"
    );
    assert!(
        b.engine.lock().unwrap().set_tx_bw("50-3050").is_err(),
        "a pick has nowhere to go"
    );
}

/// ⛔ C1 AT THE LOOP. A radio set up as an FTDX101 that answers `ID;` as an FT-991 (or an FTDX10)
/// is asked nothing beyond its ID: no TEMP meter it does not have, no menu item at all.
#[test]
fn a_radio_whose_id_is_not_the_models_is_asked_nothing_more() {
    for id in ["0570", "0761", "0670"] {
        let mut b = Bench::new(1040, yaesu(id, 102));
        b.run(15.0);
        let raw = b.raw_sent();
        assert!(raw.iter().all(|l| l == "w ID;"), "ID {id}: {raw:?}");
        assert!(!raw.is_empty(), "control: ID {id} was asked its ID");
        assert_eq!(b.radio().pa_temp_meter, None, "ID {id}");
        assert!(b.radio().tx_bw_steps.is_empty(), "ID {id}");
    }
}

/// ⛔ C1: the FT-991/FT-991A model (1035) gets no chip and no `EX110` read or write, whichever
/// radio answers. (Which of the two would be offered the chip once the menu is live is the ID
/// table's, tested in `txextras`: the FT-991A, never the plain FT-991.)
#[test]
fn the_ft991_model_never_reads_or_writes_ex110() {
    for id in ["0570", "0670"] {
        let mut b = Bench::new(1035, yaesu(id, 0));
        b.run(15.0);
        assert!(b.engine.lock().unwrap().set_tx_bw("100-2900").is_err());
        b.run(3.0);
        let raw = b.raw_sent();
        assert!(!raw.iter().any(|l| l.contains("EX110")), "ID {id}: {raw:?}");
        assert!(b.radio().tx_bw_steps.is_empty(), "ID {id}");
        assert!(
            b.sent().iter().any(|l| l == "f"),
            "control: the loop polled"
        );
    }
}

/// A radio that never answers `ID;` is asked three times on the slow cadence and then left
/// alone, and is never read for a temperature it was never shown to have.
#[test]
fn a_radio_that_never_says_its_id_is_given_up() {
    let mut b = Bench::new(1040, Radio::default());
    b.run(40.0);
    let raw = b.raw_sent();
    assert_eq!(raw, vec!["w ID;"; 3], "{raw:?}");
}

/// A temperature the loop gives up on goes from the screen, rather than stand there as if live.
#[test]
fn a_given_up_temperature_is_cleared() {
    let mut b = Bench::new(1044, yaesu("0682", 51));
    b.run(15.0);
    assert_eq!(b.radio().pa_temp_meter, Some(0.2), "control: a reading");
    b.radio.lock().unwrap().temp_raw = None;
    b.run(40.0);
    let misses = b.raw_sent().iter().filter(|l| *l == "w RM9;").count();
    assert!(
        misses >= 4,
        "control: it was read and then missed: {misses}"
    );
    assert_eq!(b.radio().pa_temp_meter, None);
}

/// A radio this table does not name is asked for nothing new. The one raw line a receiving
/// FTDX10 or FT-710 is sent is not this table's: it is the ALC meter's single question, whether
/// the link answers the raw ALC read (`RadioLoop::tx_alc_raw`), asked once.
#[test]
fn radios_without_them_are_asked_nothing() {
    for model in [1042, 1049, 1036, 2014] {
        // FTDX10, FT-710, FT-891 (parked: nothing to ask), and a Kenwood through Hamlib.
        let mut b = Bench::new(model, yaesu("0761", 0));
        b.run(15.0);
        let alc_probe = crate::rigmodels::raw_alc_query(model).map(|q| format!("w {q}"));
        let (probes, raw): (Vec<String>, Vec<String>) = b
            .raw_sent()
            .into_iter()
            .partition(|l| Some(l) == alc_probe.as_ref());
        assert!(
            probes.len() <= 1,
            "model {model}: the ALC question once: {probes:?}"
        );
        assert!(raw.is_empty(), "model {model}: {raw:?}");
        assert!(
            b.sent().iter().any(|l| l == "f"),
            "control: model {model} polled"
        );
        assert!(b.radio().tx_bw_steps.is_empty());
    }
}

/// When the line after `L COMP` reached the radio, ms after the set did.
fn gaps_after_comp_sets(log: &Log) -> Vec<u128> {
    let log = log.lock().unwrap();
    log.windows(2)
        .filter(|w| w[0].1.starts_with("L COMP"))
        .map(|w| w[1].0.duration_since(w[0].0).as_millis())
        .collect()
}

/// ⛔ H1, THE COMP LEVEL'S HALF: on a Yaesu the level goes out as Hamlib's own `L COMP` (Hamlib
/// writes `PLnnn;` and verifies with `ID;` before it answers), never as a raw `w` set. A raw set
/// holds rigctld for the radio's whole timeout and every command behind it waits; this one is
/// answered by an ANSWERING radio, and the loop's next command reaches the radio at once. A silent
/// radio holds it through Hamlib's verify retries instead, which is why none goes out while keyed
/// (`a_ptt_release_is_never_queued_behind_a_comp_write_over_rigctld`).
///
/// The rig's own raw set (`Rig::send_raw_set`, `W <cmd> 0`) does not hold the next command either.
///
/// THE CONTROL, in the same run: a raw set through `w` does hold the next command for the tail, so
/// a level, or a raw set, written that way would have shown here.
#[test]
fn a_comp_level_write_does_not_stall_the_next_command() {
    let mut b = Bench::new(1040, yaesu("0681", 0));
    b.run(15.0);
    for v in [0.30, 0.45, 0.60] {
        b.engine.lock().unwrap().set_comp_level(v);
        b.run(1.0);
    }
    let sets = b.comp_sets();
    assert_eq!(
        sets,
        ["L COMP 0.300", "L COMP 0.450", "L COMP 0.600"],
        "{:?}",
        b.sent()
    );
    let gaps = gaps_after_comp_sets(&b.log);
    assert_eq!(gaps.len(), 3, "a command followed each set");
    assert!(
        gaps.iter().all(|&g| g < 300),
        "the next command waited: {gaps:?} ms"
    );
    assert!(
        !b.raw_sent().iter().any(|l| l.starts_with("w PL")),
        "never the raw way: {:?}",
        b.raw_sent()
    );

    // The rig's raw set goes as `W <cmd> 0`, which rigctld does not wait on: the next command
    // reaches the radio at once.
    assert!(b.rig.send_raw_set("EX0101101;"), "the raw set was taken");
    let _ = b.rig.send_raw("ID;");
    let asked = Instant::now();
    while b.log.lock().unwrap().last().map(|(_, l)| l.as_str()) != Some("w ID;")
        && asked.elapsed() < Duration::from_secs(5)
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    {
        let log = b.log.lock().unwrap();
        let at = log
            .iter()
            .rposition(|(_, l)| l == "W EX0101101; 0")
            .expect("the raw set went as `W <cmd> 0`");
        let next = log.get(at + 1).expect("a command followed the raw set");
        assert_eq!(next.1, "w ID;");
        let gap = next.0.duration_since(log[at].0);
        assert!(
            gap < Duration::from_millis(300),
            "the next command waited behind the raw set: {gap:?}"
        );
    }

    // The control: a raw set through `w` stalls the next command by the set's tail.
    let _ = b.rig.send_raw("EX0101101;"); // a set gets no answer: gives up at its deadline
    let _ = b.rig.send_raw("ID;"); // gives up at the transport's deadline, before the tail ends
    let asked = Instant::now();
    while b.log.lock().unwrap().last().map(|(_, l)| l.as_str()) != Some("w ID;")
        && asked.elapsed() < Duration::from_secs(5)
    {
        std::thread::sleep(Duration::from_millis(20));
    }
    let log = b.log.lock().unwrap();
    let at = log
        .iter()
        .rposition(|(_, l)| l == "w EX0101101;")
        .expect("the raw set reached the fake");
    let tail = log[at + 1].0.duration_since(log[at].0);
    assert!(
        tail >= SET_TAIL - Duration::from_millis(100),
        "the fake must hold the port after a raw set: {tail:?}"
    );
}

/// ⚠️ OVER RIGCTLD, NO COMP WRITE WHILE ON THE AIR; THE LATEST GOES ONCE AFTER THE UNKEY.
/// Dragging the COMP bar through an over keyed from the radio's own mic sends a new level every
/// tick. None reaches the radio while it is keyed, and the operator's last value goes, once,
/// after the unkey.
#[test]
fn over_rigctld_no_comp_write_goes_out_while_keyed_and_the_latest_follows_the_unkey() {
    let mut b = Bench::new(1040, yaesu("0681", 0));
    b.run(15.0);
    b.keyed.store(true, Ordering::SeqCst);
    b.run(3.0);
    assert!(
        b.radio().rig_keyed,
        "control: the radio is keyed from its own mic"
    );
    let before = b.comp_sets().len();
    for i in 0..20 {
        b.engine
            .lock()
            .unwrap()
            .set_comp_level(0.30 + 0.01 * i as f32);
        b.run(0.05);
    }
    b.run(0.5);
    assert_eq!(
        b.comp_sets()[before..].to_vec(),
        Vec::<String>::new(),
        "nothing while keyed"
    );
    b.keyed.store(false, Ordering::SeqCst);
    b.run(3.0);
    assert_eq!(
        b.comp_sets()[before..].to_vec(),
        ["L COMP 0.490"],
        "the latest, once, after the unkey"
    );
}

/// ⛔ A RELEASE IS NEVER QUEUED BEHIND A COMP WRITE OVER RIGCTLD. The operator keys with
/// Nexus's PTT, drags COMP while talking, the radio stops answering (RF in the USB lead), and they
/// let go. Before: the first drag's `L COMP` went out, Hamlib's verify retries held rigctld's port
/// for [`SILENT_LEVEL_SET_TAIL`], and the `T 0` reached the radio only when they ended. Now no
/// `L COMP` goes out between `T 1` and `T 0`, the release reaches the radio at once, and the
/// operator's last value goes once after it.
#[test]
fn a_ptt_release_is_never_queued_behind_a_comp_write_over_rigctld() {
    let mut b = Bench::new(1040, yaesu("0681", 0));
    {
        let mut e = b.engine.lock().unwrap();
        e.set_operating_mode("phone", true);
        e.set_frequency(14.250, "20m", "USB");
    }
    b.run(15.0);
    b.engine.lock().unwrap().set_ptt(true);
    b.run(0.5);
    assert!(
        b.sent().iter().any(|l| l == "T 1"),
        "control: Nexus keyed the radio: {:?}",
        b.sent()
    );
    b.radio
        .lock()
        .unwrap()
        .silent_level_sets
        .store(true, Ordering::SeqCst);

    // The release comes from the UI's thread, while the loop is mid-drag.
    let (engine, log) = (Arc::clone(&b.engine), Arc::clone(&b.log));
    let releaser = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(300));
        let unkeys = |log: &Log| {
            log.lock()
                .unwrap()
                .iter()
                .filter(|(_, l)| l == "T 0")
                .count()
        };
        let seen = unkeys(&log);
        engine.lock().unwrap().set_ptt(false);
        let asked = Instant::now();
        while unkeys(&log) == seen && asked.elapsed() < Duration::from_secs(15) {
            std::thread::sleep(Duration::from_millis(5));
        }
        asked.elapsed()
    });
    let mut i = 0;
    while !releaser.is_finished() {
        if i < 10 {
            b.engine
                .lock()
                .unwrap()
                .set_comp_level(0.20 + 0.05 * i as f32);
            i += 1;
        }
        b.run(0.05);
        std::thread::sleep(Duration::from_millis(20));
    }
    let took = releaser.join().unwrap();
    assert!(
        took < Duration::from_secs(1),
        "the release reached the radio {took:?} after the operator let go: {:?}",
        b.sent()
    );
    let sent = b.sent();
    let key = sent.iter().position(|l| l == "T 1").unwrap();
    let unkey = key + sent[key..].iter().position(|l| l == "T 0").unwrap();
    assert!(
        !sent[key..unkey].iter().any(|l| l.starts_with("L COMP")),
        "a COMP write went out while keyed: {:?}",
        &sent[key..unkey]
    );
    assert!(
        i >= 3,
        "control: the operator dragged while keyed ({i} values)"
    );

    b.radio
        .lock()
        .unwrap()
        .silent_level_sets
        .store(false, Ordering::SeqCst);
    b.run(3.0);
    let sent = b.sent();
    let after: Vec<&String> = sent[unkey..]
        .iter()
        .filter(|l| l.starts_with("L COMP"))
        .collect();
    let last = format!("L COMP {:.3}", 0.20 + 0.05 * (i - 1) as f32);
    assert_eq!(after, [&last], "the latest, once, after the unkey");
}

/// The pacing rule itself. An over with its own end gets no level write at all, so none sits
/// between its unkey decision and its unkey; live PTT gets none over rigctld (or any link but the
/// native Icom one), and on the native Icom link is paced and held after a fault; receiving is
/// free on every link.
#[test]
fn the_comp_write_rule() {
    // (timed_over, live_keyed, paced_link, now, last_sent, fault_hold) -> allowed
    for paced in [false, true] {
        assert!(!comp_write_allowed(
            true, false, paced, 10_000.0, None, false
        ));
        assert!(!comp_write_allowed(
            true, true, paced, 10_000.0, None, false
        ));
        assert!(comp_write_allowed(
            false,
            false,
            paced,
            10_000.0,
            Some(9_990.0),
            true
        ));
    }
    // Over rigctld while live-keyed: never, however long since the last.
    assert!(!comp_write_allowed(
        false, true, false, 10_000.0, None, false
    ));
    assert!(!comp_write_allowed(
        false,
        true,
        false,
        10_000.0,
        Some(0.0),
        false
    ));
    // The native Icom link while live-keyed: paced, and held after a fault.
    assert!(comp_write_allowed(false, true, true, 10_000.0, None, false));
    assert!(!comp_write_allowed(
        false,
        true,
        true,
        10_000.0,
        Some(9_800.0),
        false
    ));
    assert!(comp_write_allowed(
        false,
        true,
        true,
        10_000.0,
        Some(9_750.0),
        false
    ));
    assert!(!comp_write_allowed(false, true, true, 10_000.0, None, true));
}

/// The native Icom link: the bandwidth chip (`16 58`) and the meter's PROC switch ship there, on
/// a radio the daemon knows the model of. A generic CI-V radio (no model) is asked nothing.
mod icom_native {
    use super::*;
    use crate::civ::broker::CivDaemon;
    use crate::civ::commands::{self, IcomModel};
    use crate::civ::engine::tests_support::{FakeRadio, Regs};

    const ADDR: u8 = 0x94;

    struct Scene {
        engine: Arc<Mutex<Engine>>,
        state: RadioLoop,
        rig: Rig,
        backend: MockBackend,
        regs: Arc<Mutex<Regs>>,
    }

    impl Scene {
        fn new(model: Option<IcomModel>) -> Self {
            let engine = engine_for(3073); // IC-7300
            let settings = engine.lock().unwrap().settings().clone();
            let (radio, _push) = FakeRadio::new(ADDR);
            let regs = radio.regs();
            let daemon = CivDaemon::start_with_io(Box::new(radio), ADDR, 0, 1, model).unwrap();
            let rig = Rig::rigctld(&format!("127.0.0.1:{}", daemon.local_addr().port()));
            let cfg = RadioConfig {
                rig_model: settings.rig_model,
                ..RadioConfig::default()
            };
            let state = RadioLoop::new(
                Transport::from_settings(&settings),
                Some(CatDaemon::Native(daemon)),
                &cfg,
            );
            Scene {
                engine,
                state,
                rig,
                backend: MockBackend::new(),
                regs,
            }
        }

        /// The loop as `run_radio` drives it (a step, then 20 ms), on the wall clock, until
        /// `done` or `secs`.
        fn run_until(&mut self, secs: u64, done: impl Fn(&Self) -> bool) {
            let deadline = Instant::now() + Duration::from_secs(secs);
            let sinks = no_sinks();
            let mut station = StationSinks::new();
            let (mut ra, mut rr) = (mock_reopen_audio(), mock_reopen_rig());
            while !done(self) && Instant::now() < deadline {
                self.state
                    .step(
                        &self.engine,
                        &mut self.backend,
                        &mut self.rig,
                        &sinks,
                        now_unix_ms(),
                        &mut ra,
                        &mut rr,
                        &mut station,
                    )
                    .unwrap();
                std::thread::sleep(Duration::from_millis(20));
            }
        }

        fn radio(&self) -> tempo_app::dto::RadioStatus {
            self.engine.lock().unwrap().snapshot().radio
        }

        /// The speech-processor level SETS (`14 0E` with a value) the radio was sent.
        fn comp_set_frames(&self) -> Vec<Vec<u8>> {
            let r = self.regs.lock().unwrap();
            r.wire
                .iter()
                .filter(|f| f.get(4) == Some(&0x14) && f.get(5) == Some(&0x0E) && f.len() > 7)
                .cloned()
                .collect()
        }

        /// Where on the wire the PTT frame `1C 00 <on>` was sent, in order.
        fn ptt_at(&self, on: bool) -> Vec<usize> {
            let r = self.regs.lock().unwrap();
            r.wire
                .iter()
                .enumerate()
                .filter(|(_, f)| f.get(4..7) == Some(&[0x1C, 0x00, u8::from(on)][..]))
                .map(|(i, _)| i)
                .collect()
        }

        /// One step of the loop, on the wall clock.
        fn step(&mut self) {
            self.state
                .step(
                    &self.engine,
                    &mut self.backend,
                    &mut self.rig,
                    &no_sinks(),
                    now_unix_ms(),
                    &mut mock_reopen_audio(),
                    &mut mock_reopen_rig(),
                    &mut StationSinks::new(),
                )
                .unwrap();
        }

        /// Key with Nexus's own PTT in phone, once the radio is up.
        fn key(&mut self) {
            {
                let mut e = self.engine.lock().unwrap();
                e.set_operating_mode("phone", true);
                e.set_frequency(14.250, "20m", "USB");
            }
            self.run_until(20, |s| s.radio().tx_proc_switch);
            self.engine.lock().unwrap().set_ptt(true);
            self.run_until(5, |s| !s.ptt_at(true).is_empty());
            assert!(
                !self.ptt_at(true).is_empty(),
                "control: Nexus keyed the radio"
            );
        }

        /// The `16 58` frames the radio was sent.
        fn tbw_frames(&self) -> Vec<Vec<u8>> {
            let r = self.regs.lock().unwrap();
            r.wire
                .iter()
                .filter(|f| f.get(4) == Some(&0x16) && f.get(5) == Some(&0x58))
                .cloned()
                .collect()
        }
    }

    #[test]
    fn an_ic7300_offers_the_chip_and_proc_and_a_pick_is_written_and_read_back() {
        let mut s = Scene::new(Some(IcomModel::Ic7300));
        s.run_until(20, |s| s.radio().tx_bw.is_some());
        let r = s.radio();
        assert_eq!(
            r.tx_bw.as_deref(),
            Some("WIDE"),
            "the fake's 16 58 starts at 00"
        );
        assert_eq!(r.tx_bw_steps, ["WIDE", "MID", "NAR"]);
        assert!(r.tx_proc_switch, "PROC on the native Icom link");
        s.engine.lock().unwrap().set_tx_bw("NAR").unwrap();
        s.run_until(20, |s| s.radio().tx_bw.as_deref() == Some("NAR"));
        assert_eq!(s.radio().tx_bw.as_deref(), Some("NAR"));
        assert!(
            s.tbw_frames().iter().any(|f| f.get(6) == Some(&0x02)),
            "16 58 02 went out"
        );
    }

    #[test]
    fn a_generic_ci_v_radio_is_asked_nothing() {
        let mut s = Scene::new(None);
        // As long as the model-known radio took to show its chip, and then some.
        s.run_until(12, |_| false);
        assert!(
            !s.regs.lock().unwrap().wire.is_empty(),
            "control: the loop talked to the radio"
        );
        assert_eq!(s.tbw_frames(), Vec::<Vec<u8>>::new());
        let r = s.radio();
        assert!(r.tx_bw_steps.is_empty() && !r.tx_proc_switch);
    }

    /// ⭐ A REMOTE RADIO SELECTION DOES NOT LEAVE PROC HIDDEN. The selection clears every
    /// extra in the engine; when that lands after the loop configured this radio, the loop's
    /// next poll says PROC again for the same radio and link.
    #[test]
    fn proc_comes_back_after_the_engine_forgets_it_on_the_same_radio() {
        let mut s = Scene::new(Some(IcomModel::Ic7300));
        s.run_until(20, |s| s.radio().tx_proc_switch);
        assert!(s.radio().tx_proc_switch, "control: PROC offered");
        s.engine.lock().unwrap().clear_rig_tx_extras();
        assert!(!s.radio().tx_proc_switch, "control: the engine forgot it");
        s.run_until(10, |s| s.radio().tx_proc_switch);
        assert!(s.radio().tx_proc_switch, "PROC stayed hidden");
    }

    /// On Nexus's own Icom link, COMP writes while keyed are paced (one CI-V request at a time,
    /// at most one per 250 ms) and the operator's last value is the one the radio holds.
    #[test]
    fn on_the_native_icom_link_comp_writes_while_keyed_are_paced_and_the_latest_wins() {
        let mut s = Scene::new(Some(IcomModel::Ic7300));
        s.key();
        let before = s.comp_set_frames().len();
        for i in 0..20 {
            s.engine
                .lock()
                .unwrap()
                .set_comp_level(0.30 + 0.01 * i as f32);
            s.step();
            std::thread::sleep(Duration::from_millis(50));
        }
        s.run_until(1, |_| false);
        assert!(s.ptt_at(false).is_empty(), "control: still keyed");
        let sets = s.comp_set_frames()[before..].to_vec();
        assert!(
            (2..=6).contains(&sets.len()),
            "20 values over 1 s keyed went out as {} writes",
            sets.len()
        );
        assert_eq!(
            sets.last(),
            Some(&commands::set_dsp_level(ADDR, commands::LVL_COMP, 49).to_bytes()),
            "the latest value"
        );
    }

    /// ⛔ THE SAME ON THE NATIVE ICOM LINK: a release waits behind ONE unanswered COMP write at most, for
    /// the daemon's own deadline (300 ms), never through a retry chain. The radio goes silent
    /// mid-over, the operator drags COMP and lets go while that write is unanswered: the key-up
    /// reaches the radio once that one request's deadline is out, and no further COMP write
    /// went out between the key and the key-up.
    #[test]
    fn on_the_native_icom_link_a_release_waits_behind_one_unanswered_comp_write_at_most() {
        let mut s = Scene::new(Some(IcomModel::Ic7300));
        s.key();
        let before = s.comp_set_frames().len();
        s.regs.lock().unwrap().off = true;
        let (engine, regs) = (Arc::clone(&s.engine), Arc::clone(&s.regs));
        let releaser = std::thread::spawn(move || {
            let comp_sets = |regs: &Arc<Mutex<Regs>>| {
                regs.lock()
                    .unwrap()
                    .wire
                    .iter()
                    .filter(|f| f.get(4) == Some(&0x14) && f.get(5) == Some(&0x0E) && f.len() > 7)
                    .count()
            };
            let unkeys = |regs: &Arc<Mutex<Regs>>| {
                regs.lock()
                    .unwrap()
                    .wire
                    .iter()
                    .filter(|f| f.get(4..7) == Some(&[0x1C, 0x00, 0x00][..]))
                    .count()
            };
            let waited = Instant::now();
            while comp_sets(&regs) == before && waited.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(2));
            }
            // The write is on the bus, unanswered: the radio comes back, the operator lets go.
            regs.lock().unwrap().off = false;
            let seen = unkeys(&regs);
            engine.lock().unwrap().set_ptt(false);
            let asked = Instant::now();
            while unkeys(&regs) == seen && asked.elapsed() < Duration::from_secs(10) {
                std::thread::sleep(Duration::from_millis(2));
            }
            asked.elapsed()
        });
        let mut i = 0;
        while !releaser.is_finished() {
            if i < 10 {
                s.engine
                    .lock()
                    .unwrap()
                    .set_comp_level(0.20 + 0.05 * i as f32);
                i += 1;
            }
            s.step();
            std::thread::sleep(Duration::from_millis(20));
        }
        let took = releaser.join().unwrap();
        assert!(
            took >= Duration::from_millis(150),
            "control: the release did wait behind the unanswered write ({took:?})"
        );
        assert!(
            took < Duration::from_millis(700),
            "the release waited {took:?}, past one request's deadline"
        );
        let key = s.ptt_at(true)[0];
        let unkey = *s.ptt_at(false).iter().find(|&&u| u > key).unwrap();
        let r = s.regs.lock().unwrap();
        let keyed_sets = r.wire[key..unkey]
            .iter()
            .filter(|f| f.get(4) == Some(&0x14) && f.get(5) == Some(&0x0E) && f.len() > 7)
            .count();
        let dump: Vec<String> = r.wire[key..=unkey]
            .iter()
            .map(|f| format!("{:02X?}", &f[4..]))
            .collect();
        assert_eq!(
            keyed_sets, 1,
            "one write, then none to the unkey: {dump:#?}"
        );
    }
}
