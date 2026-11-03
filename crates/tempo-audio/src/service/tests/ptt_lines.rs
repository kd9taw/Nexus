//! The diagnostic log's PTT lines, from the real `RadioLoop::step` on a rigctld with a radio behind
//! it: one line per key and unkey naming who keyed, one when the radio is keyed by something that
//! is not Nexus, and the PTT method once. And the control: the radio keys and unkeys exactly when
//! the operator's PTT says, with every `T` command the loop sends unchanged by the lines.

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};

/// A rigctld whose radio keys on `T 1`, unkeys on `T 0` and answers `t` with its PTT; `keyed` can
/// also be set from the test, as a key from somewhere else.
struct Radio {
    addr: String,
    keyed: Arc<AtomicBool>,
    log: Arc<Mutex<Vec<String>>>,
}

impl Radio {
    fn start() -> Radio {
        Radio::start_with_hang(Duration::ZERO)
    }

    /// [`Radio::start`] for a radio that still reports transmit for `hang` after its `T 0` (VOX or
    /// break-in hang, a slow changeover).
    fn start_with_hang(hang: Duration) -> Radio {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let keyed = Arc::new(AtomicBool::new(false));
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let (keyed_w, log_w) = (keyed.clone(), log.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let (keyed, log) = (keyed_w.clone(), log_w.clone());
                std::thread::spawn(move || {
                    let Ok(reader) = stream.try_clone() else {
                        return;
                    };
                    for line in BufReader::new(reader).lines() {
                        let Ok(line) = line else { return };
                        log.lock().unwrap().push(line.clone());
                        let reply = match line.as_str() {
                            "T 1" | "T 3" => {
                                keyed.store(true, Ordering::SeqCst);
                                "RPRT 0\n".to_string()
                            }
                            "T 0" if hang.is_zero() => {
                                keyed.store(false, Ordering::SeqCst);
                                "RPRT 0\n".to_string()
                            }
                            "T 0" => {
                                let keyed = keyed.clone();
                                std::thread::spawn(move || {
                                    std::thread::sleep(hang);
                                    keyed.store(false, Ordering::SeqCst);
                                });
                                "RPRT 0\n".to_string()
                            }
                            "t" => format!("{}\n", u8::from(keyed.load(Ordering::SeqCst))),
                            "f" => "14250000\n".to_string(),
                            _ => "RPRT 0\n".to_string(),
                        };
                        if stream.write_all(reply.as_bytes()).is_err() {
                            return;
                        }
                    }
                });
            }
        });
        Radio { addr, keyed, log }
    }

    fn keys(&self) -> Vec<String> {
        let log = self.log.lock().unwrap();
        log.iter()
            .filter(|l| l.starts_with("T "))
            .cloned()
            .collect()
    }
}

struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    t: f64,
}

impl Scene {
    /// Phone on 20 m, TX armed, nothing keyed.
    fn new(radio: &Radio) -> Scene {
        Scene::on(Rig::rigctld(&radio.addr))
    }

    /// [`Scene::new`] on `rig`.
    fn on(rig: Rig) -> Scene {
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            e.set_operating_mode("phone", true);
            e.set_frequency(14.250, "20m", "USB");
            e.set_tx_enabled(true);
        }
        Scene {
            engine,
            state: loop_state(),
            backend: MockBackend::new(),
            rig,
            t: 100.0,
        }
    }

    /// `ms` of loop time, a tick every 20 ms.
    fn run(&mut self, ms: f64) {
        let until = self.t + ms;
        step_to(
            &self.engine,
            &mut self.state,
            &mut self.backend,
            &mut self.rig,
            &mut self.t,
            until,
        );
    }

    /// Tick until `done` or `limit` of real time.
    fn run_until(&mut self, limit: Duration, done: impl Fn() -> bool) {
        let start = std::time::Instant::now();
        while !done() && start.elapsed() < limit {
            self.run(20.0);
        }
    }

    /// The PTT lines written since the last call.
    fn lines(&mut self) -> Vec<String> {
        std::mem::take(&mut self.state.ptt_diag.lines)
    }

    fn key_lines(&mut self) -> Vec<String> {
        self.lines()
            .into_iter()
            .filter(|l| !l.starts_with("PTT method"))
            .collect()
    }
}

/// ⭐ WHO KEYED, AND HOW. The operator's PTT: one key line naming it and the method, nothing while
/// it is held, one unkey line. THE CONTROL: the radio keys on the press and unkeys on the release.
#[test]
fn the_operators_ptt_is_one_key_line_and_one_unkey_line() {
    let radio = Radio::start();
    let mut s = Scene::new(&radio);
    s.run(200.0);
    let first = s.lines();
    assert_eq!(
        first
            .iter()
            .filter(|l| l.starts_with("PTT method:"))
            .count(),
        1,
        "the method once, at the first tick: {first:?}"
    );
    assert!(first[0].starts_with("PTT method: CAT (T 1)"), "{first:?}");
    assert!(!radio.keyed.load(Ordering::SeqCst), "premise: idle");

    s.engine.lock().unwrap().set_ptt(true);
    s.run(1_000.0);
    assert!(
        radio.keyed.load(Ordering::SeqCst),
        "control: the press keys"
    );
    let lines = s.lines();
    assert_eq!(
        lines,
        ["PTT KEY: manual PTT, via CAT".to_string()],
        "one key line, and nothing per tick"
    );

    s.engine.lock().unwrap().set_ptt(false);
    s.run(500.0);
    assert!(
        !radio.keyed.load(Ordering::SeqCst),
        "control: the release unkeys"
    );
    let lines = s.key_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("PTT unkey: manual PTT, after "),
        "{lines:?}"
    );
    assert_eq!(
        radio.keys().first().map(String::as_str),
        Some("T 1"),
        "control: the key the loop sent"
    );
    assert!(
        radio.keys().contains(&"T 0".to_string()),
        "control: the release it sent"
    );
    s.run(500.0);
    assert!(s.lines().is_empty(), "nothing while idle");
}

/// Tune is named as Tune.
#[test]
fn a_tune_carrier_is_named_tune() {
    let radio = Radio::start();
    let mut s = Scene::new(&radio);
    s.run(200.0);
    s.lines();
    s.engine.lock().unwrap().set_tune(true);
    s.run(600.0);
    assert!(radio.keyed.load(Ordering::SeqCst), "control: Tune keys");
    let lines = s.key_lines();
    assert_eq!(lines, ["PTT KEY: Tune, via CAT".to_string()], "{lines:?}");
    s.engine.lock().unwrap().set_tune(false);
    s.run(600.0);
    assert!(
        !radio.keyed.load(Ordering::SeqCst),
        "control: Tune off unkeys"
    );
    let lines = s.key_lines();
    assert!(
        lines.len() == 1 && lines[0].starts_with("PTT unkey: Tune"),
        "{lines:?}"
    );
}

/// ⭐ THE QUESTION THE LOG COULD NOT ANSWER: the radio keyed while Nexus holds nothing. One line
/// when its own PTT reads keyed, one when it reads off, and no Nexus key line between them; Nexus
/// sends no `T` of its own for it.
#[test]
fn the_radio_keyed_by_something_else_is_said_and_so_is_its_end() {
    let radio = Radio::start();
    let mut s = Scene::new(&radio);
    s.run(200.0);
    s.lines();
    let sent_before = radio.keys();
    radio.keyed.store(true, Ordering::SeqCst);
    s.run(2_500.0);
    let lines = s.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("radio keyed by something else"),
        "{lines:?}"
    );
    radio.keyed.store(false, Ordering::SeqCst);
    s.run(2_500.0);
    let lines = s.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("radio no longer keyed by something else"),
        "{lines:?}"
    );
    assert!(
        lines[0].contains("the radio reports its PTT off"),
        "{lines:?}"
    );
    assert_eq!(radio.keys(), sent_before, "control: Nexus keyed nothing");
}

/// ⭐ A RADIO STILL REPORTING TRANSMIT JUST AFTER NEXUS LET GO IS THAT OVER'S TAIL. A radio whose
/// `t` reads 1 for 600 ms after its `T 0` (VOX or break-in hang, a slow changeover) was logged as
/// "keyed by something else" at the end of every over. It is said as the tail, with how long after
/// the release. CONTROL: a key reaching the radio well after the over is still "something else".
#[test]
fn a_radio_still_transmitting_just_after_the_release_is_the_overs_tail() {
    let radio = Radio::start_with_hang(Duration::from_millis(600));
    let mut s = Scene::new(&radio);
    s.run(200.0);
    s.lines();
    s.engine.lock().unwrap().set_ptt(true);
    s.run(1_000.0);
    assert!(
        radio.keyed.load(Ordering::SeqCst),
        "premise: the press keys"
    );
    s.lines();
    s.engine.lock().unwrap().set_ptt(false);
    s.run_until(Duration::from_secs(5), || {
        !radio.keyed.load(Ordering::SeqCst)
    });
    assert!(
        !radio.keyed.load(Ordering::SeqCst),
        "premise: the hang ended"
    );
    s.run(2_500.0);
    let lines = s.key_lines();
    assert!(lines[0].starts_with("PTT unkey: manual PTT"), "{lines:?}");
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("radio still reports transmit ")
                && l.contains("s after Nexus let go")),
        "the tail is said as the tail: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.contains("something else")),
        "the tail is never another keyer: {lines:?}"
    );
    assert!(
        lines
            .iter()
            .any(|l| l.starts_with("radio no longer reports transmit")),
        "{lines:?}"
    );

    // THE CONTROL: well after the over, a key from elsewhere is something else.
    s.run(3_000.0);
    s.lines();
    radio.keyed.store(true, Ordering::SeqCst);
    s.run(2_500.0);
    let lines = s.lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("radio keyed by something else"),
        "{lines:?}"
    );
}

/// On a VOX radio the operator's PTT plays no Nexus audio, so its key line never says it does.
#[test]
fn a_vox_key_nexus_plays_no_audio_for_is_not_said_as_playing_it() {
    let mut s = Scene::on(Rig::vox());
    s.run(200.0);
    s.lines();
    s.engine.lock().unwrap().set_ptt(true);
    s.run(600.0);
    assert!(s.rig.keyed, "control: the loop keys its PTT as before");
    let lines = s.key_lines();
    assert_eq!(lines.len(), 1, "{lines:?}");
    assert!(
        lines[0].starts_with("PTT KEY: manual PTT, via VOX"),
        "{lines:?}"
    );
    assert!(lines[0].contains("plays no audio"), "{lines:?}");
    assert!(!lines[0].contains("Nexus plays the audio"), "{lines:?}");
    s.engine.lock().unwrap().set_ptt(false);
    s.run(600.0);
    assert!(!s.rig.keyed, "control: and releases it");
}
