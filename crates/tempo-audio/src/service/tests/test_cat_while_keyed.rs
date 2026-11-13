//! ⛔ TEST CAT NEVER DELAYS AN UNKEY. Test CAT's probe is blocking CAT (round trips the link may
//! never answer), and it ran near the top of the tick, ahead of that tick's deadline unkey: pressed
//! during an over on a link that had stopped answering, it held the over's PTT up for the probe's
//! whole run (1.03 s measured, 2026-11-12 radio-loop map, finding 1). So while the loop holds any
//! key, Test CAT is refused, typed, in one sentence, and the CAT status is left as it was; with
//! nothing keyed it probes exactly as before.
//!
//! The rig is a rigctld on the 20 m FT8 dial, keyed by CAT. It writes each line it is sent into a
//! log (each `T 0` timestamped on arrival), and while `dead` is set it answers nothing at all.
//! Brought in from the characterization pin
//! `radio_loop_pins::test_cat_pressed_during_an_over_probes_ahead_of_the_deadline_unkey`
//! (test/radio-loop-pins-2026-11-12 @ c5399ba25), copied with the part of its scene it uses, and
//! turned from "the probe runs first" into "the unkey is on time".

use super::*;
use crate::rig::remote_tests::{retuning_peer, Peer};
use std::sync::atomic::{AtomicBool, Ordering};

const DIAL: u64 = 14_074_000;

struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    /// Kept alive for the rigctld's thread.
    _peer: Peer,
    /// Every line the rigctld was sent, in order.
    order: Arc<Mutex<Vec<String>>>,
    /// When each `T 0` reached the rigctld.
    unkey_at: Arc<Mutex<Vec<Instant>>>,
    /// While set, the rigctld answers nothing: the link has stopped answering.
    dead: Arc<AtomicBool>,
    t: f64,
}

impl Scene {
    /// An `extra` station with TX armed, PTT by CAT through the rigctld (Hamlib's NET rigctl).
    fn new() -> Scene {
        let order = Arc::new(Mutex::new(Vec::new()));
        let heard = Arc::clone(&order);
        let unkey_at = Arc::new(Mutex::new(Vec::new()));
        let unkeyed = Arc::clone(&unkey_at);
        let dead = Arc::new(AtomicBool::new(false));
        let silent = Arc::clone(&dead);
        let peer = retuning_peer(DIAL, "PKTUSB", move |line, radio| {
            heard.lock().unwrap().push(line.to_string());
            if line == "T 0" {
                unkeyed.lock().unwrap().push(Instant::now());
            }
            if silent.load(Ordering::SeqCst) {
                return Some(String::new());
            }
            if line.starts_with("l ") {
                return Some("0.500000\n".to_string());
            }
            match line {
                "T 1" | "T 3" => {
                    radio.keyed = true;
                    Some("RPRT 0\n".to_string())
                }
                _ => None,
            }
        });
        let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
        {
            let mut e = engine.lock().unwrap();
            e.set_license_class("extra");
            e.set_tier(Tier::Ft8);
            e.set_frequency(14.074, "20m", "USB");
            let mut st = e.settings().clone();
            st.ptt_method = "cat".into();
            st.rig_model = 2; // Hamlib's NET rigctl: the scene's rigctld is the radio
            e.apply_settings(st);
            e.set_tx_enabled(true);
        }
        let mut state = loop_state_for(&engine);
        state.applied = Transport::from_settings(engine.lock().unwrap().settings());
        Scene {
            state,
            engine,
            backend: MockBackend::new(),
            rig: Rig::rigctld(&peer.address),
            _peer: peer,
            order,
            unkey_at,
            dead,
            t: now_unix_ms(),
        }
    }

    /// One loop tick at the scene's time, then the clock moves on 20 ms.
    fn tick(&mut self) {
        let (sinks, mut ra, mut rr) = (no_sinks(), mock_reopen_audio(), mock_reopen_rig());
        let mut station = StationSinks::new();
        self.state
            .step(
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                &sinks,
                self.t,
                &mut ra,
                &mut rr,
                &mut station,
            )
            .unwrap();
        self.t += 20.0;
    }

    fn run_to(&mut self, until: f64) {
        while self.t <= until {
            self.tick();
        }
    }

    fn mark(&self) -> usize {
        self.order.lock().unwrap().len()
    }

    /// The lines the rigctld was sent since `from` that are not keying (`T`).
    fn cat_lines_since(&self, from: usize) -> Vec<String> {
        self.order.lock().unwrap()[from..]
            .iter()
            .filter(|l| !l.starts_with("T "))
            .cloned()
            .collect()
    }

    /// The next tick reads no transmit meter.
    fn meter_not_due(&mut self) {
        self.state.last_tx_meter_poll = self.t;
    }
}

/// ⭐ TEST CAT PRESSED IN AN OVER'S LAST TICK, ON A LINK THAT HAS STOPPED ANSWERING, DOES NOT HOLD
/// THE OVER'S UNKEY: the `T 0` reaches the rig within one tick and a loaded machine's slack of the
/// tick starting, and nothing is probed ahead of it. The press is refused, typed, in the sentence
/// the operator reads, and the CAT status the loop last published is left as it was.
#[test]
fn test_cat_pressed_during_an_over_on_a_dead_link_never_delays_its_unkey() {
    const BOUND_MS: f64 = 20.0 + 100.0;
    let mut s = Scene::new();
    s.run_to(s.t + 200.0);
    s.rig.ptt(true).expect("premise: the rigctld takes the key");
    s.state.tx_until_ms = Some(s.t); // the over's deadline is this tick
    s.meter_not_due();
    let status = s.engine.lock().unwrap().snapshot().radio.cat_detail;
    s.dead.store(true, Ordering::SeqCst);
    let from = s.mark();
    let asked = s.engine.lock().unwrap().request_cat_reprobe();
    let started = Instant::now();
    s.tick();
    let unkeyed = *s
        .unkey_at
        .lock()
        .unwrap()
        .first()
        .expect("no unkey in the deadline tick");
    let late = unkeyed.saturating_duration_since(started).as_secs_f64() * 1_000.0;
    eprintln!("Test CAT pressed in the deadline tick on a dead link: unkeyed {late:.0} ms in");
    assert!(
        late <= BOUND_MS,
        "the deadline unkey went out {late:.0} ms into its tick (bound {BOUND_MS} ms): Test CAT \
         held the transmitter keyed; sent {:?}",
        s.cat_lines_since(from)
    );
    let e = s.engine.lock().unwrap();
    assert_eq!(
        (e.cat_reprobe_answer(asked), e.cat_reprobe_refused()),
        (Some((None, TEST_CAT_WHILE_KEYED.to_string())), true),
        "Test CAT while keyed is answered at once, refused, typed (so no baud ladder sweeps the \
         port mid-over)"
    );
    assert_eq!(
        e.snapshot().radio.cat_detail,
        status,
        "a refused Test CAT says nothing about the link, so the CAT status stays as it was"
    );
}

/// CONTROL: with nothing keyed, Test CAT probes the rig and answers from what it found, as it
/// always has.
#[test]
fn test_cat_with_nothing_keyed_still_probes() {
    let mut s = Scene::new();
    s.run_to(s.t + 200.0);
    let from = s.mark();
    let asked = s.engine.lock().unwrap().request_cat_reprobe();
    s.tick();
    assert!(
        !s.cat_lines_since(from).is_empty(),
        "the probe asked the rig nothing"
    );
    let e = s.engine.lock().unwrap();
    let (ok, detail) = e
        .cat_reprobe_answer(asked)
        .expect("Test CAT was not answered");
    assert_eq!(ok, Some(true), "the answering rig probed bad: {detail:?}");
    assert!(!e.cat_reprobe_refused());
}

/// Test CAT pressed while the loop holds a key with no over's deadline (`tx_until_ms` empty) on a
/// link that has stopped answering: refused with the sentence, nothing sent to the rig, the CAT
/// status as it was. `key` puts the loop in that key and asserts it holds it.
fn refused_while(key: impl FnOnce(&mut Scene)) {
    let mut s = Scene::new();
    s.run_to(s.t + 200.0);
    key(&mut s);
    assert!(s.state.tx_until_ms.is_none(), "premise: no over's deadline");
    s.meter_not_due();
    let status = s.engine.lock().unwrap().snapshot().radio.cat_detail;
    s.dead.store(true, Ordering::SeqCst);
    let from = s.mark();
    let asked = s.engine.lock().unwrap().request_cat_reprobe();
    s.tick();
    assert_eq!(
        s.cat_lines_since(from),
        Vec::<String>::new(),
        "Test CAT probed the rig while it was keyed"
    );
    let e = s.engine.lock().unwrap();
    assert_eq!(
        (e.cat_reprobe_answer(asked), e.cat_reprobe_refused()),
        (Some((None, TEST_CAT_WHILE_KEYED.to_string())), true),
        "Test CAT while keyed is refused, typed"
    );
    assert_eq!(
        e.snapshot().radio.cat_detail,
        status,
        "the CAT status stays as it was"
    );
}

/// ⭐ THE OPERATOR'S PTT HELD: Test CAT is refused.
#[test]
fn test_cat_while_the_ptt_is_held_is_refused() {
    refused_while(|s| {
        s.engine.lock().unwrap().set_ptt(true);
        let until = s.t + 200.0;
        s.run_to(until);
        assert!(s.state.manual_ptt_applied, "premise: the PTT is held");
    });
}

/// ⭐ A TUNE CARRIER KEYED: Test CAT is refused.
#[test]
fn test_cat_while_tuning_is_refused() {
    refused_while(|s| {
        s.engine.lock().unwrap().set_tune(true);
        let until = s.t + 300.0;
        s.run_to(until);
        assert!(s.state.tuning_keyed, "premise: the tune is keyed");
    });
}

/// ⭐ A CW WORD ON THE AIR THROUGH A KEYER (no PTT hold of Nexus's own): Test CAT is refused.
#[test]
fn test_cat_while_a_cw_word_is_keying_is_refused() {
    refused_while(|s| {
        s.state.cw_keying_until = s.t + 5_000.0;
        assert!(
            !s.state.holds_tx() && !s.state.tuning_keyed,
            "premise: only the CW word"
        );
    });
}
