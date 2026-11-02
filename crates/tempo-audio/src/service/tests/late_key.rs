//! A key the radio loop gave up on, or left owed on a connection that dropped, never stays on the
//! air after its release (#429). Behind a rigctld held up for seconds such a key can be carried out
//! long after Nexus stopped waiting for it, and on a rigctld that does not serve its connections in
//! order (Hamlib's, under one lock) after the release that followed it.
//!
//! The rig is a rigctld with a radio behind it: `T 1`/`T 3` key it and `T 0` unkeys it, `t` reads
//! it, and each key-down is answered as the scene says (late, a refusal, never with the connection
//! closed under it). A scene can also have the radio keyed again later, by a key from elsewhere
//! reaching it after its release. The loop is the real `RadioLoop::step`.
//!
//! What is checked: a late refusal is not said as a key that keyed; a late key read during a
//! second Tune that keyed says nothing after that Tune; a key given up on, or left owed on a
//! dropped connection, that the radio carries out after its release is unkeyed by the watch on the
//! radio's own PTT, and said. And the watch's negative controls: a key of Nexus's own inside it (the
//! operator's PTT, the radio's ATU tune-up Nexus started) and a radio keyed after it stay keyed.

use super::*;
use std::io::{BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// How the radio answers the `n`th key-down (from 0).
#[derive(Clone, Copy)]
struct KeyAnswer {
    /// How long it takes.
    after: Duration,
    /// What it answers, or `None` to close the connection unanswered once `after` has passed.
    reply: Option<&'static str>,
    /// Whether the radio keys on it.
    keys: bool,
    /// Whether it waits for the scene's go ([`Radio::go`]) before `after`.
    held_for_go: bool,
}

const AT_ONCE: KeyAnswer = KeyAnswer {
    after: Duration::ZERO,
    reply: Some("RPRT 0\n"),
    keys: true,
    held_for_go: false,
};

/// The rigctld and the radio behind it.
struct Radio {
    addr: String,
    keyed: Arc<AtomicBool>,
    log: Arc<Mutex<Vec<String>>>,
    /// Lets a key answer held for it go on.
    go: Arc<AtomicBool>,
}

impl Radio {
    /// `answer(n)` for the `n`th key-down; `rekey_at`, the radio keyed again (a key from elsewhere
    /// reaching it after its release) that long after the start.
    fn start(answer: fn(usize) -> KeyAnswer, rekey_at: Option<Duration>) -> Radio {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let keyed = Arc::new(AtomicBool::new(false));
        let log = Arc::new(Mutex::new(Vec::<String>::new()));
        let keys = Arc::new(AtomicUsize::new(0));
        let go = Arc::new(AtomicBool::new(false));
        if let Some(at) = rekey_at {
            let keyed = keyed.clone();
            std::thread::spawn(move || {
                std::thread::sleep(at);
                keyed.store(true, Ordering::SeqCst);
            });
        }
        let (keyed_w, log_w, go_w) = (keyed.clone(), log.clone(), go.clone());
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { break };
                let (keyed, log, keys, go) =
                    (keyed_w.clone(), log_w.clone(), keys.clone(), go_w.clone());
                std::thread::spawn(move || {
                    let Ok(reader) = stream.try_clone() else {
                        return;
                    };
                    for line in BufReader::new(reader).lines() {
                        let Ok(line) = line else { return };
                        log.lock().unwrap().push(line.clone());
                        let reply = match line.as_str() {
                            "T 1" | "T 3" => {
                                let a = answer(keys.fetch_add(1, Ordering::SeqCst));
                                while a.held_for_go && !go.load(Ordering::SeqCst) {
                                    std::thread::sleep(Duration::from_millis(10));
                                }
                                std::thread::sleep(a.after);
                                if a.keys {
                                    keyed.store(true, Ordering::SeqCst);
                                }
                                match a.reply {
                                    Some(r) => r.to_string(),
                                    None => return, // closed, unanswered
                                }
                            }
                            "T 0" => {
                                keyed.store(false, Ordering::SeqCst);
                                "RPRT 0\n".to_string()
                            }
                            "t" => format!("{}\n", u8::from(keyed.load(Ordering::SeqCst))),
                            // A radio with a tuner, whose tune-up keys the radio itself.
                            "u TUNER" => "1\n".to_string(),
                            "U TUNER 2" => {
                                keyed.store(true, Ordering::SeqCst);
                                "RPRT 0\n".to_string()
                            }
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
        Radio {
            addr,
            keyed,
            log,
            go,
        }
    }
}

/// The radio loop on that rigctld, with `setup` applied to its engine (Phone on 20 m first).
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    t: f64,
    started: std::time::Instant,
}

impl Scene {
    fn new(radio: &Radio, setup: impl FnOnce(&mut Engine)) -> Scene {
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            e.set_operating_mode("phone", true);
            e.set_frequency(14.250, "20m", "USB");
            setup(&mut e);
        }
        Scene {
            engine,
            state: loop_state(),
            backend: MockBackend::new(),
            rig: Rig::rigctld(&radio.addr),
            t: 100.0,
            started: std::time::Instant::now(),
        }
    }

    /// Run the loop a tick at a time until `done` or `limit` of real time has passed.
    fn run_until(&mut self, limit: Duration, mut done: impl FnMut(&Scene) -> bool) {
        while !done(self) && self.started.elapsed() < limit {
            let next = self.t + 20.0;
            step_to(
                &self.engine,
                &mut self.state,
                &mut self.backend,
                &mut self.rig,
                &mut self.t,
                next,
            );
        }
    }

    fn banner(&self) -> Option<String> {
        self.engine.lock().unwrap().snapshot().radio.audio_error
    }

    fn says_keyed_late(&self) -> bool {
        self.banner().is_some_and(|b| {
            b.starts_with("The radio keyed ") || b.starts_with("The radio was keyed")
        })
    }
}

/// ⛔ A LATE REFUSAL IS NOT A KEY THAT KEYED. A Tune key given up on that the radio answers, seconds
/// later, with a refusal keyed nothing: the loop still sends one more release (fail-closed), and the
/// line never says the radio keyed late.
#[test]
fn a_late_refusal_of_a_tune_key_is_never_said_as_a_key_that_keyed() {
    let radio = Radio::start(
        |n| match n {
            0 => KeyAnswer {
                after: Duration::from_millis(4_000),
                reply: Some("RPRT -1\n"),
                keys: false,
                held_for_go: false,
            },
            _ => AT_ONCE,
        },
        None,
    );
    let mut s = Scene::new(&radio, |e| e.set_tune(true));
    let mut said = false;
    s.run_until(Duration::from_secs(7), |s| {
        said |= s.says_keyed_late();
        false
    });
    assert!(!said, "a late refusal was said as a key that keyed");
    let log = radio.log.lock().unwrap().clone();
    let key = log.iter().position(|l| l == "T 1").unwrap();
    assert!(
        log[key..].iter().filter(|l| *l == "T 0").count() >= 2,
        "no release behind the late refusal: {log:?}"
    );
    assert!(!radio.keyed.load(Ordering::SeqCst));
}

/// ⛔ A LATE KEY READ DURING A SECOND TUNE THAT KEYED SAYS NOTHING AFTER IT. The first Tune's key is
/// given up on; a second Tune keys behind it and is confirmed, reading the first one's late answer
/// on the way. When the second Tune ends, the radio is unkeyed (its release, and one more) and the
/// line does not say the radio keyed late.
#[test]
fn a_late_key_read_during_a_second_tune_that_keyed_says_nothing_after_it() {
    let radio = Radio::start(
        |n| match n {
            // Answered only once the second Tune's key is behind it.
            0 => KeyAnswer {
                after: Duration::ZERO,
                reply: Some("RPRT 0\n"),
                keys: true,
                held_for_go: true,
            },
            _ => AT_ONCE,
        },
        None,
    );
    let mut s = Scene::new(&radio, |e| e.set_tune(true));
    s.rig.set_network_chain(true); // the second key waits 2.5 s for its answer
                                   // The first Tune gives up on its key and ends.
    s.run_until(Duration::from_secs(8), |s| {
        s.banner().is_some() && !s.engine.lock().unwrap().tuning()
    });
    assert!(
        !s.engine.lock().unwrap().tuning(),
        "premise: the first Tune ended unconfirmed: {:?}",
        s.banner()
    );
    // The second Tune keys behind it, and the first key is answered while the second waits.
    s.engine.lock().unwrap().set_tune(true);
    let go = radio.go.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(800));
        go.store(true, Ordering::SeqCst);
    });
    s.run_until(Duration::from_secs(14), |s| !s.backend.played.is_empty());
    assert!(
        !s.backend.played.is_empty(),
        "premise: the second Tune keyed"
    );
    s.engine.lock().unwrap().set_tune(false);
    let mut said = false;
    let until = s.started.elapsed() + Duration::from_secs(1);
    s.run_until(until, |s| {
        said |= s.says_keyed_late();
        false
    });
    assert!(!said, "a Tune that worked was followed by a late-key line");
    assert!(
        !radio.keyed.load(Ordering::SeqCst),
        "the radio was left keyed"
    );
}

/// ⛔ A TUNE KEY GIVEN UP ON THAT REACHES THE RADIO AFTER ITS RELEASE IS UNKEYED BY THE PTT WATCH.
/// The key is answered late with a refusal, so nothing on the link says it keyed, but the radio
/// keys afterwards (a key from another connection carried out after the release, as a rigctld that
/// does not serve its connections in order does). The loop watches the radio's own PTT after a
/// give-up, unkeys it, and says so.
#[test]
fn a_tune_key_given_up_on_that_lands_after_its_release_is_unkeyed_by_the_ptt_watch() {
    let radio = Radio::start(
        |n| match n {
            0 => KeyAnswer {
                after: Duration::from_millis(1_500),
                reply: Some("RPRT -1\n"),
                keys: false,
                held_for_go: false,
            },
            _ => AT_ONCE,
        },
        Some(Duration::from_millis(3_500)),
    );
    let mut s = Scene::new(&radio, |e| e.set_tune(true));
    s.run_until(Duration::from_secs(8), |s| {
        s.started.elapsed() > Duration::from_millis(3_600) && !radio.keyed.load(Ordering::SeqCst)
    });
    assert!(
        !radio.keyed.load(Ordering::SeqCst),
        "the radio was left keyed after the release"
    );
    s.run_until(Duration::from_secs(9), |s| s.says_keyed_late());
    assert!(s.says_keyed_late(), "not said: {:?}", s.banner());
}

/// ⛔ A KEY LEFT OWED ON A CONNECTION THAT DROPPED IS WATCHED FOR TOO. A voice message keys, its key
/// gets no answer and its connection closes under it (the over still goes out, as a late key's
/// does). The release then goes out on a new connection, and the old key reaches the radio after
/// it. The loop, once it holds nothing, watches the radio's own PTT, unkeys it and says so.
#[test]
fn a_key_left_owed_on_a_dropped_connection_that_lands_later_is_unkeyed_by_the_ptt_watch() {
    let radio = Radio::start(
        |n| match n {
            0 => KeyAnswer {
                after: Duration::from_millis(1_200),
                reply: None,
                keys: false,
                held_for_go: false,
            },
            _ => AT_ONCE,
        },
        Some(Duration::from_millis(4_000)),
    );
    let mut s = Scene::new(&radio, |e| {
        e.send_voice(vec![0.05; 12_000]).unwrap();
    });
    s.run_until(Duration::from_secs(9), |s| {
        s.started.elapsed() > Duration::from_millis(4_100) && !radio.keyed.load(Ordering::SeqCst)
    });
    assert!(
        !radio.keyed.load(Ordering::SeqCst),
        "the radio was left keyed after the release"
    );
    s.run_until(Duration::from_secs(10), |s| s.says_keyed_late());
    assert!(s.says_keyed_late(), "not said: {:?}", s.banner());
}

/// A Tune key given up on, refused late: the watch on the radio's PTT is armed.
fn armed(n: usize) -> KeyAnswer {
    match n {
        0 => KeyAnswer {
            after: Duration::from_millis(1_500),
            reply: Some("RPRT -1\n"),
            keys: false,
            held_for_go: false,
        },
        _ => AT_ONCE,
    }
}

/// Run until the first Tune has ended unconfirmed and its late refusal is in.
fn past_the_give_up(s: &mut Scene) {
    s.run_until(Duration::from_secs(5), |s| {
        s.started.elapsed() > Duration::from_millis(2_000) && !s.engine.lock().unwrap().tuning()
    });
    assert!(
        !s.engine.lock().unwrap().tuning(),
        "premise: the Tune ended"
    );
}

/// Negative control: the operator's own PTT, pressed inside the watch, keys the radio and stays
/// keyed. The watch only acts while the loop holds nothing.
#[test]
fn the_operators_ptt_inside_the_watch_stays_keyed() {
    let radio = Radio::start(armed, None);
    let mut s = Scene::new(&radio, |e| e.set_tune(true));
    past_the_give_up(&mut s);
    s.engine.lock().unwrap().set_ptt(true);
    let until = s.started.elapsed() + Duration::from_secs(3);
    s.run_until(until, |_| false);
    assert!(
        radio.keyed.load(Ordering::SeqCst),
        "the operator's PTT was unkeyed by the watch"
    );
    assert!(!s.says_keyed_late(), "{:?}", s.banner());
}

/// ⛔ Negative control: the radio's ATU tune-up that Nexus started inside the watch is never
/// aborted by it. The radio keys itself for the tune-up and reports keyed; the watch is put past
/// the tune-up's cycle when it is sent, so nothing unkeys it through the cycle and nothing says a
/// late key.
#[test]
fn an_atu_tune_up_nexus_started_inside_the_watch_is_never_aborted() {
    let radio = Radio::start(armed, None);
    let mut s = Scene::new(&radio, |e| e.set_tune(true));
    past_the_give_up(&mut s);
    let sent = |radio: &Radio| radio.log.lock().unwrap().iter().any(|l| l == "U TUNER 2");
    let limit = s.started.elapsed() + Duration::from_secs(4);
    s.run_until(limit, |s| {
        let _ = s.engine.lock().unwrap().atu_tune();
        sent(&radio)
    });
    assert!(sent(&radio), "premise: the tune-up was sent");
    // Through the tune-up's cycle (the watch takes a radio still keyed past it, as a late key).
    let until = s.t + ATU_REPROBE_AFTER_TUNE_MS - 1_000.0;
    step_to(
        &s.engine,
        &mut s.state,
        &mut s.backend,
        &mut s.rig,
        &mut s.t,
        until,
    );
    assert!(
        radio.keyed.load(Ordering::SeqCst),
        "the watch aborted the tune-up Nexus started"
    );
    assert!(!s.says_keyed_late(), "{:?}", s.banner());
}

/// Negative control: once the watch's 10 s are over, a radio that keys (its own microphone, another
/// program) is left keyed.
#[test]
fn a_radio_keyed_after_the_watch_stays_keyed() {
    let radio = Radio::start(armed, Some(Duration::from_millis(13_500)));
    let mut s = Scene::new(&radio, |e| e.set_tune(true));
    s.run_until(Duration::from_millis(15_500), |_| false);
    assert!(
        radio.keyed.load(Ordering::SeqCst),
        "a key after the watch was unkeyed"
    );
    assert!(!s.says_keyed_late(), "{:?}", s.banner());
}

/// ⛔ A LATE KEY READ IN THE TICK THAT STARTS AN ATU TUNE-UP NEVER ABORTS IT. A given-up key's late
/// answer can be read by any command of the tick that sends the radio's tune-up (`U TUNER 2`), up to
/// and including that command's own wait for owed answers; the late-key block, later in the same
/// tick, used to send its `T 0` behind the tune-up, which aborts it on most radios. The unkey now
/// waits: the tune-up runs out its cycle keyed, and the watch on the radio's PTT then takes the key.
#[test]
fn a_late_key_read_in_the_tick_that_starts_an_atu_tune_up_never_aborts_it() {
    let radio = Radio::start(|_| AT_ONCE, None);
    let mut s = Scene::new(&radio, |_| {});
    // The heavy poll asks for the tuner, so the ATU can be pressed.
    s.run_until(Duration::from_secs(5), |s| {
        s.engine.lock().unwrap().atu_tune_gate().is_ok()
    });
    s.engine
        .lock()
        .unwrap()
        .atu_tune()
        .expect("premise: the ATU press is taken");
    // One tick: the heavy poll is due, so it sends the tune-up, and the late key is read in it.
    s.state.last_rig_poll = s.t - RIG_POLL_MS - 1.0;
    s.rig.land_late_key_for_test(Duration::from_secs(4));
    let started = s.t;
    let next = s.t + 20.0;
    step_to(
        &s.engine,
        &mut s.state,
        &mut s.backend,
        &mut s.rig,
        &mut s.t,
        next,
    );
    let log = radio.log.lock().unwrap().clone();
    let tune = log
        .iter()
        .position(|l| l == "U TUNER 2")
        .expect("premise: the tune-up was sent in that tick");
    assert!(
        !log[tune..].iter().any(|l| l == "T 0"),
        "a T 0 went out behind the tune-up: {log:?}"
    );
    assert!(
        radio.keyed.load(Ordering::SeqCst),
        "the tune-up was aborted"
    );
    // The cycle runs out keyed…
    let mid = started + ATU_REPROBE_AFTER_TUNE_MS - 500.0;
    step_to(
        &s.engine,
        &mut s.state,
        &mut s.backend,
        &mut s.rig,
        &mut s.t,
        mid,
    );
    assert!(
        radio.keyed.load(Ordering::SeqCst),
        "the tune-up was cut inside its cycle"
    );
    // …and past it the watch takes the key, and says so.
    let after = started + ATU_REPROBE_AFTER_TUNE_MS + 3_000.0;
    step_to(
        &s.engine,
        &mut s.state,
        &mut s.backend,
        &mut s.rig,
        &mut s.t,
        after,
    );
    assert!(
        !radio.keyed.load(Ordering::SeqCst),
        "the key was left on the air after the tune-up"
    );
    assert!(s.says_keyed_late(), "not said: {:?}", s.banner());
}
