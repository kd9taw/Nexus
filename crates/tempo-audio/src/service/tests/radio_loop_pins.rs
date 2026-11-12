//! ⛔ CHARACTERIZATION PINS, NOT A SPEC: the radio loop's keying paths and their per-tick order as
//! they stand on 2026-11-12, pinned on the real `RadioLoop::step` so the refactor that moves the
//! transmit-safety decisions out of the loop into their own module can be proved to change nothing.
//! Every value below is what the loop does today. A pin that reads like a weakness is labelled so:
//! it is a FINDING for the maintainer, and it flips only in its own behaviour change, with its own
//! test, never in a refactor.
//!
//! These fill the gaps the existing pins leave (the index of every keying path and the pin that
//! holds it is kept beside the design, not here): the Hamlib tune's release, Stop TX and timeout;
//! the TX-enable latch at the slot boundary and the PTT press; the licence gate on the slot over,
//! the tune and the PTT, and on a PTT already held; the PTT release; the TX watchdog reaching the
//! loop; the order inside a Stop TX tick; a stalled meter or raw ALC read against Stop TX and the
//! over's end; and one order fact about what runs ahead of a non-slot over's deadline unkey (Test
//! CAT's, fixed 2026-11-12, is held by `test_cat_while_keyed`).
//!
//! The rig is a rigctld on the 20 m FT8 dial that keys (`T 1`/`T 3`), unkeys (`T 0`) and answers
//! every meter read, or none while a scene stalls them. The mock sound card writes `flush` into the
//! same log the rigctld writes each line it is sent into, so the two land in the order they
//! happened, with no clock involved.

use super::*;
use crate::rig::remote_tests::{retuning_peer, Peer};
use std::sync::atomic::{AtomicBool, Ordering};

/// The 20 m FT8 dial, the rigctld's and the engine's.
const DIAL: u64 = 14_074_000;

/// A station on the logging rigctld: `class` licence, FT8 on 20 m, then whatever `setup` adds.
struct Scene {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    backend: MockBackend,
    rig: Rig,
    /// Kept alive for the rigctld's thread.
    _peer: Peer,
    /// Every line the rigctld was sent, and every `flush` of the card, in order.
    order: Arc<Mutex<Vec<String>>>,
    /// When each `T 0` reached the rigctld.
    unkey_at: Arc<Mutex<Vec<Instant>>>,
    /// While set, no meter read (`l NAME`, the raw `w RM4;`) is ever answered.
    stall_meters: Arc<AtomicBool>,
    /// An even FT8 boundary 10 to 40 s ahead of the real clock, so the loop's clock stays ahead of
    /// it and an over's hold is measured on the loop's.
    slot: f64,
    t: f64,
}

impl Scene {
    fn new(class: &str, setup: impl FnOnce(&mut Engine)) -> Scene {
        let order = Arc::new(Mutex::new(Vec::new()));
        let heard = Arc::clone(&order);
        let unkey_at = Arc::new(Mutex::new(Vec::new()));
        let unkeyed = Arc::clone(&unkey_at);
        let stall_meters = Arc::new(AtomicBool::new(false));
        let stall = Arc::clone(&stall_meters);
        let peer = retuning_peer(DIAL, "PKTUSB", move |line, radio| {
            heard.lock().unwrap().push(line.to_string());
            if line == "T 0" {
                unkeyed.lock().unwrap().push(Instant::now());
            }
            let meter = line.starts_with("l ") || line.starts_with("w ");
            if meter && stall.load(Ordering::SeqCst) {
                return Some(String::new());
            }
            if line.starts_with("w RM4;") {
                return Some("RM4064;\0".to_string());
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
            e.set_license_class(class);
            e.set_tier(Tier::Ft8);
            e.set_frequency(14.074, "20m", "USB");
            setup(&mut e);
        }
        let slot = ((now_unix_ms() + 10_000.0) / 30_000.0).ceil() * 30_000.0;
        let mut backend = MockBackend::new();
        backend.events = Some(Arc::clone(&order));
        Scene {
            state: loop_state_for(&engine),
            engine,
            backend,
            rig: Rig::rigctld(&peer.address),
            _peer: peer,
            order,
            unkey_at,
            stall_meters,
            slot,
            t: slot + 100.0,
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

    fn eng(&self) -> std::sync::MutexGuard<'_, Engine> {
        self.engine.lock().unwrap()
    }

    fn mark(&self) -> usize {
        self.order.lock().unwrap().len()
    }

    /// The card's flushes and the `T` lines since `from`.
    fn cut(&self, from: usize) -> Vec<String> {
        self.order.lock().unwrap()[from..]
            .iter()
            .filter(|l| *l == "flush" || l.starts_with("T "))
            .cloned()
            .collect()
    }

    /// [`Self::cut`] with the meter lines too.
    fn cut_and_meters(&self, from: usize) -> Vec<String> {
        self.order.lock().unwrap()[from..]
            .iter()
            .filter(|l| *l == "flush" || l.starts_with("T ") || l.starts_with("l "))
            .cloned()
            .collect()
    }

    fn keys(&self) -> usize {
        let order = self.order.lock().unwrap();
        order.iter().filter(|l| *l == "T 1" || *l == "T 3").count()
    }

    fn unkeys(&self) -> usize {
        let order = self.order.lock().unwrap();
        order.iter().filter(|l| *l == "T 0").count()
    }

    fn meter_lines_since(&self, from: usize) -> Vec<String> {
        self.order.lock().unwrap()[from..]
            .iter()
            .filter(|l| l.starts_with("l ") || l.starts_with("w "))
            .cloned()
            .collect()
    }

    /// The next tick reads one transmit meter, from scratch: due, none parked, SWR first.
    fn meter_due(&mut self) {
        self.state.tx_meter_supported = [None; N_TX_METERS];
        self.state.tx_meter_misses = [0; N_TX_METERS];
        self.state.last_tx_meter_poll = self.t - 1_000.0;
        self.state.tx_meter_idx = 0;
    }

    /// The next tick reads no transmit meter.
    fn meter_not_due(&mut self) {
        self.state.last_tx_meter_poll = self.t;
    }

    /// The FT8 CQ keyed at its slot boundary: the over's PTT hold.
    fn key_the_cq(&mut self) -> f64 {
        self.run_to(self.slot + 300.0);
        assert_eq!(self.keys(), 1, "premise: the slot boundary keyed the over");
        assert!(!self.backend.played.is_empty(), "premise: the over plays");
        self.state.tx_until_ms.expect("premise: the over holds PTT")
    }

    /// Live PTT pressed and keyed, TX armed.
    fn hold_ptt(&mut self) {
        {
            let mut e = self.eng();
            e.set_tx_enabled(true);
            e.set_ptt(true);
        }
        let until = self.t + 200.0;
        self.run_to(until);
        assert!(
            self.state.manual_ptt_applied,
            "premise: the PTT is held by the loop"
        );
        assert_eq!(self.keys(), 1, "premise: the PTT keyed");
    }

    /// The Tune button, keyed through the rigctld.
    fn hold_tune(&mut self) {
        self.eng().set_tune(true);
        let until = self.t + 300.0;
        self.run_to(until);
        assert!(self.state.tuning_keyed, "premise: the tune is keyed");
        assert_eq!(self.keys(), 1, "premise: the tune keyed once");
    }
}

/// The FT8 CQ, Call CQ pressed and the snappy first over drained, so the boundary keys it.
fn cq(e: &mut Engine) {
    let _ = e.start_cq(None);
    let _ = e.take_immediate_tx();
}

// ---- the Hamlib tune ----

/// ⭐ A TUNE ENDED BY ITS RELEASE OR BY STOP TX FLUSHES ITS CARRIER, THEN UNKEYS, IN THE TICK THAT
/// SEES IT, and once. On a VOX rig the audio is the key, so the flush is what ends it.
#[test]
fn a_tune_released_or_stopped_flushes_then_unkeys_in_the_next_tick() {
    for stop in [false, true] {
        let what = if stop { "Stop TX" } else { "Tune released" };
        let mut s = Scene::new("extra", |_| {});
        s.hold_tune();
        let from = s.mark();
        if stop {
            s.eng().halt_tx();
        } else {
            s.eng().set_tune(false);
        }
        s.tick();
        assert_eq!(
            s.cut(from),
            ["flush", "T 0"],
            "{what}: the tick that ends it"
        );
        assert!(
            !s.state.tuning_keyed,
            "{what}: the loop still holds the tune"
        );
        assert!(!s.rig.keyed, "{what}: the loop believes the radio keyed");
        let after = s.mark();
        let until = s.t + 200.0;
        s.run_to(until);
        assert_eq!(s.cut(after), Vec::<String>::new(), "{what}: nothing after");
    }
}

/// ⭐ THE TUNE ENDS ITSELF AT ITS TIMEOUT (`tune_timeout_secs`, capped at 60 s): held on the last
/// tick inside it, flushed then unkeyed on the first tick past it, and the engine's Tune is off.
#[test]
fn a_tune_ends_itself_at_its_timeout_flush_then_unkey() {
    let mut s = Scene::new("extra", |e| {
        let mut st = e.settings().clone();
        st.tune_timeout_secs = 1;
        e.apply_settings(st);
    });
    s.hold_tune();
    let started = s.state.tune_started_ms.expect("premise: the tune's start");
    s.run_to(started + 980.0);
    assert!(s.state.tuning_keyed, "the tune ended before its timeout");
    assert_eq!(s.unkeys(), 0, "an unkey before the timeout");
    let from = s.mark();
    s.run_to(started + 1_040.0);
    assert_eq!(s.cut(from), ["flush", "T 0"], "the timeout's tick");
    assert!(
        !s.state.tuning_keyed && !s.eng().tuning(),
        "the tune is still on"
    );
}

/// ⭐ THE TUNE IS THE ONE KEY THE TX-ENABLE LATCH DOES NOT GATE (by design, `Engine::set_tune_for`:
/// "tune is the one keying path that bypasses the TX-enable latch"; the licence gates it instead).
/// With TX off, Tune keys.
#[test]
fn tune_is_the_one_key_the_tx_enable_latch_does_not_gate() {
    let mut s = Scene::new("extra", |e| e.set_tx_enabled(false));
    assert!(!s.eng().tx_enabled(), "premise: TX is off");
    s.hold_tune();
}

// ---- the TX-enable latch ----

/// ⭐ WITH TX OFF NOTHING THE LATCH GATES KEYS: the slot boundary keys no over, and a PTT press keys
/// nothing. Each with its control: TX on, the same boundary and the same press key.
#[test]
fn with_tx_off_the_slot_boundary_and_a_ptt_press_key_nothing() {
    for armed in [false, true] {
        let mut s = Scene::new("extra", cq);
        if !armed {
            s.eng().set_tx_enabled_by_operator(false);
        }
        s.run_to(s.slot + 600.0);
        assert_eq!(
            s.keys(),
            usize::from(armed),
            "armed {armed}: the slot boundary's keys"
        );
        assert_eq!(
            s.backend.played.is_empty(),
            !armed,
            "armed {armed}: the over's audio"
        );
    }
    for armed in [false, true] {
        let mut s = Scene::new("extra", |e| e.set_tx_enabled(armed));
        s.eng().set_ptt(true);
        let until = s.t + 200.0;
        s.run_to(until);
        assert_eq!(
            s.keys(),
            usize::from(armed),
            "armed {armed}: the PTT's keys"
        );
        assert_eq!(s.state.manual_ptt_applied, armed, "armed {armed}: PTT held");
    }
}

// ---- the licence gate ----

/// ⭐ OUTSIDE THE LICENCE NO KEYING PATH KEYS: a Technician on the 20 m FT8 dial (no 20 m
/// privileges) gets no slot over, no tune and no PTT. The control: an Extra, the same three, keys.
#[test]
fn outside_the_licence_the_slot_over_the_tune_and_the_ptt_key_nothing() {
    for class in ["technician", "extra"] {
        let keys = usize::from(class == "extra");
        let mut s = Scene::new(class, cq);
        s.run_to(s.slot + 600.0);
        assert_eq!(s.keys(), keys, "{class}: the slot over's keys");

        let mut s = Scene::new(class, |_| {});
        s.eng().set_tune(true);
        let until = s.t + 300.0;
        s.run_to(until);
        assert_eq!(s.keys(), keys, "{class}: the tune's keys");

        let mut s = Scene::new(class, |e| e.set_tx_enabled(true));
        s.eng().set_ptt(true);
        let until = s.t + 200.0;
        s.run_to(until);
        assert_eq!(s.keys(), keys, "{class}: the PTT's keys");
    }
}

/// ⭐ A HELD PTT THE LICENCE NO LONGER COVERS IS UNKEYED IN THE NEXT TICK: `Engine::manual_ptt`
/// masks the key on every read, so the loop's PTT applier releases it.
#[test]
fn a_held_ptt_the_licence_no_longer_covers_is_unkeyed_in_the_next_tick() {
    let mut s = Scene::new("extra", |_| {});
    s.hold_ptt();
    let from = s.mark();
    assert!(s.eng().set_license_class("technician"));
    s.tick();
    assert!(
        s.cut(from).iter().any(|l| l == "T 0"),
        "the held key went on: {:?}",
        s.cut(from)
    );
    assert!(!s.state.manual_ptt_applied, "the loop still holds the PTT");
}

// ---- the PTT release ----

/// ⭐ RELEASING THE HELD PTT UNKEYS IN THE TICK THAT SEES IT, AND FLUSHES NOTHING (characterization:
/// live PTT plays no audio of its own, so its release is a bare `T 0`).
#[test]
fn releasing_the_held_ptt_unkeys_in_the_tick_that_sees_it_and_flushes_nothing() {
    let mut s = Scene::new("extra", |_| {});
    s.hold_ptt();
    let from = s.mark();
    s.eng().set_ptt(false);
    s.tick();
    assert_eq!(s.cut(from), ["T 0"], "the release's tick");
    assert!(!s.state.manual_ptt_applied && !s.rig.keyed, "still held");
}

// ---- the TX watchdog, as the loop sees it ----

/// ⭐ PAST ITS LIMIT THE WATCHDOG REFUSES THE NEXT FT8 OVER AND DISARMS TX; HALF WAY TO IT THE OVER
/// KEYS. The watchdog is judged when an over is built (`Engine::plan_tx`), never during one, so for
/// a slot over its limit is the next over refused, through the real check, on the real loop.
#[test]
fn past_its_limit_the_watchdog_refuses_the_next_ft8_over_and_disarms_tx() {
    for (aged, trips) in [(30, false), (61, true)] {
        let mut s = Scene::new("extra", |e| {
            let mut st = e.settings().clone();
            st.tx_watchdog_min = 1;
            e.apply_settings(st);
            cq(e);
        });
        let end = s.key_the_cq();
        s.run_to(end + 40.0);
        s.eng().age_tx_watchdog_for_test(aged);
        s.run_to(s.slot + 30_000.0 + 300.0); // this station's next slot
        assert_eq!(s.keys(), if trips { 1 } else { 2 }, "aged {aged} s: keys");
        let e = s.eng();
        assert_eq!(e.tx_enabled(), !trips, "aged {aged} s: TX armed");
        assert_eq!(
            e.snapshot().radio.tx_watchdog,
            trips,
            "aged {aged} s: the trip shown"
        );
    }
}

/// ⭐ PAST ITS LIMIT THE WATCHDOG CUTS A LATCHED RTTY OVER ON THE AIR: the queued audio is flushed,
/// then the radio unkeyed, and TX is off. The control, half way to the limit, goes on keyed.
#[test]
fn past_its_limit_the_watchdog_cuts_a_latched_rtty_over_flush_then_unkey() {
    for (aged, trips) in [(30, false), (61, true)] {
        let mut s = Scene::new("extra", |e| {
            let mut st = e.settings().clone();
            st.tx_watchdog_min = 1;
            e.apply_settings(st);
            e.set_operating_mode("rtty", false);
            e.set_rtty_latched(true).unwrap();
            e.rtty_type("CQ CQ CQ CQ DE KD9TAW KD9TAW K").unwrap();
        });
        let until = s.t + 1_500.0;
        s.run_to(until);
        assert!(s.keys() >= 1, "aged {aged} s: premise: the stream keyed");
        assert!(
            s.state.tx_until_ms.is_some(),
            "aged {aged} s: premise: on the air"
        );
        let from = s.mark();
        s.eng().age_tx_watchdog_for_test(aged);
        let until = s.t + 400.0;
        s.run_to(until);
        let cut = s.cut(from);
        if trips {
            assert_eq!(cut, ["flush", "T 0"], "aged {aged} s: the cut");
            assert!(!s.eng().tx_enabled(), "aged {aged} s: TX still armed");
            assert!(s.state.tx_until_ms.is_none(), "aged {aged} s: still held");
        } else {
            assert_eq!(
                cut,
                Vec::<String>::new(),
                "aged {aged} s: cut under the limit"
            );
            assert!(s.state.tx_until_ms.is_some(), "aged {aged} s: let go");
        }
    }
}

// ---- the order inside a tick ----

/// ⭐ IN THE TICK THAT TAKES A STOP TX, THE ONE TRANSMIT METER READ DUE GOES BEFORE THE HARD STOP:
/// the meter block runs ahead of the engine-locked section where `take_slot_tx_abort` is read, so
/// the order on the wire is the read, the flush, the unkey. (Characterization: a Stop TX waits
/// behind at most one meter read, bounded by the read's deadline; see the next pin.)
#[test]
fn in_the_stop_tx_tick_the_one_meter_read_goes_before_the_hard_stop() {
    let mut s = Scene::new("extra", cq);
    s.key_the_cq();
    s.run_to(s.slot + 3_000.0);
    s.meter_due();
    let from = s.mark();
    s.eng().halt_tx();
    s.tick();
    assert_eq!(
        s.cut_and_meters(from),
        ["l SWR", "flush", "T 0"],
        "the Stop TX tick"
    );
    assert!(s.state.tx_until_ms.is_none(), "the over survived the stop");
}

/// ⭐ A STALLED METER READ ON AN FT8 OVER CANNOT HOLD A STOP TX PAST THE LINK'S DEADLINE: the stop is
/// pressed (from another thread, as the UI does) 20 ms into the stalled read, and the unkey is on the
/// wire within the 700 ms link deadline, one tick, and a loaded machine's slack.
#[test]
fn a_stalled_meter_read_on_an_ft8_over_cannot_hold_stop_tx_past_the_link_deadline() {
    const DEADLINE_MS: f64 = 700.0;
    let mut s = Scene::new("extra", cq);
    s.key_the_cq();
    s.run_to(s.slot + 3_000.0);
    s.meter_due();
    let asked = s.mark();
    s.stall_meters.store(true, Ordering::SeqCst);
    let engine = Arc::clone(&s.engine);
    let operator = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(20));
        let pressed = Instant::now();
        engine.lock().unwrap().halt_tx();
        pressed
    });
    for _ in 0..10 {
        s.tick();
        if !s.unkey_at.lock().unwrap().is_empty() {
            break;
        }
    }
    let pressed = operator.join().unwrap();
    assert_eq!(
        s.meter_lines_since(asked)
            .first()
            .expect("premise: the stalled tick asked a meter"),
        "l SWR",
        "premise: the stalled tick asked the SWR meter"
    );
    let unkeyed = *s.unkey_at.lock().unwrap().first().expect("never unkeyed");
    let late = unkeyed.saturating_duration_since(pressed).as_secs_f64() * 1_000.0;
    eprintln!("Stop TX on a stalled meter read: unkeyed {late:.0} ms after the press");
    assert!(
        late <= DEADLINE_MS + 20.0 + 100.0,
        "unkeyed {late:.0} ms after the press, past the link's {DEADLINE_MS} ms deadline + a tick"
    );
}

/// ⭐ A STALLED RAW ALC READ (THE YAESU READ OF THE 1.23 STALL) NEVER HOLDS AN FT8 OVER PAST ITS END:
/// on an FT-991A whose link answered the raw probe, every read near the end of the over stalls, and
/// each tick that asks one ends by the over's next end (its audio stop, then its PTT off) plus a
/// loaded machine's slack. Under 150 ms left nothing is asked.
#[test]
fn a_stalled_raw_alc_read_never_holds_an_ft8_over_past_its_end() {
    const SLACK_MS: f64 = 100.0;
    let mut s = Scene::new("extra", |e| {
        let mut st = e.settings().clone();
        st.rig_model = 1035;
        e.apply_settings(st);
        cq(e);
    });
    s.state = loop_state_for(&s.engine);
    s.key_the_cq();
    let mut raw_reads = 0;
    let mut checked = 0;
    for left in [1_000.0, 500.0, 300.0, 200.0, 160.0, 120.0] {
        // Tick to the first tick with no more than `left` before the over's next end.
        loop {
            let Some(next_end) = s.state.over_left_ms(s.t) else {
                break;
            };
            if next_end <= left {
                break;
            }
            s.meter_not_due();
            s.tick();
        }
        let Some(to_end) = s.state.over_left_ms(s.t) else {
            break;
        };
        s.meter_due();
        s.state.tx_meter_idx = 1; // ALC
        s.state.tx_alc_raw = Some(true);
        s.stall_meters.store(true, Ordering::SeqCst);
        let from = s.mark();
        let started = Instant::now();
        s.tick();
        let held = started.elapsed().as_secs_f64() * 1_000.0;
        s.stall_meters.store(false, Ordering::SeqCst);
        let asked = s.meter_lines_since(from);
        raw_reads += asked.iter().filter(|l| l.starts_with("w RM4;")).count();
        checked += 1;
        eprintln!("{to_end:.0} ms left: tick held {held:.0} ms, asked {asked:?}");
        if to_end < TX_METER_OVER_MIN_LEFT_MS {
            assert!(asked.is_empty(), "{to_end:.0} ms left: a read was asked");
        }
        assert!(
            held <= to_end.max(0.0) + SLACK_MS,
            "{to_end:.0} ms left: the stalled read held the tick {held:.0} ms, past the over's end"
        );
    }
    assert!(checked >= 5, "premise: the over's end was reached in steps");
    assert!(
        raw_reads >= 3,
        "premise: the raw ALC read was asked ({raw_reads})"
    );
}

/// ⚠️ KNOWN DEFECT, PINNED AS IT STANDS, NOT A CONTRACT: A NON-SLOT OVER'S DEADLINE UNKEY WAITS BEHIND
/// THE ENGINE LOCK. Its fix is deferred to a later change, which inverts this pin. Only an
/// FT8/FT4 over's end is re-tested at the top of the tick, before anything can wait
/// (`retest_ft_window`; its own pin is `ft_window::an_ft8_overs_end_does_not_wait_for_the_engine_lock`).
/// Any other over (voice, APRS, SSTV, RTTY/PSK tails, an FT2 or JS8 over) ends at the deadline unkey
/// further down, after the tick has taken the engine lock (first at `poll_remote_transmit`), so a lock
/// held elsewhere (a long UI command) holds that over's PTT for as long. The engine's own long jobs
/// already yield to an over's deadline (the loop publishes its hold, `observe_loop_tx_hold`); an
/// arbitrary command holding the lock, which the 300 ms holder here stands for, is not covered. When
/// the deferred fix moves every deadline ahead of the lock, this pin is inverted with the operator's
/// sign-off; a refactor must leave it as it is.
#[test]
fn a_non_slot_overs_deadline_unkey_waits_behind_the_engine_lock() {
    const HELD_MS: u64 = 300;
    let mut s = Scene::new("extra", |e| e.set_tx_enabled(true));
    s.run_to(s.t + 200.0);
    s.rig.ptt(true).expect("premise: the rigctld takes the key");
    s.state.tx_until_ms = Some(s.t); // a non-slot over whose deadline is this tick
    assert!(!s.state.holds_slot_over(), "premise: not a slot over");
    let (locked_tx, locked_rx) = std::sync::mpsc::channel();
    let engine = Arc::clone(&s.engine);
    let holder = std::thread::spawn(move || {
        let guard = engine.lock().unwrap();
        locked_tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(HELD_MS));
        let released = Instant::now();
        drop(guard);
        released
    });
    locked_rx.recv().unwrap();
    s.tick();
    let released = holder.join().unwrap();
    let unkeyed = *s.unkey_at.lock().unwrap().last().expect("never unkeyed");
    assert!(
        unkeyed >= released,
        "KNOWN-DEFECT PIN: the non-slot deadline unkey no longer waits for the engine lock ({:?} \
         early). If that is the deferred fix, invert this pin with the operator's sign-off; if it \
         is a refactor, the per-tick order changed: find out why",
        released.saturating_duration_since(unkeyed)
    );
    assert!(s.state.tx_until_ms.is_none() && !s.rig.keyed, "still keyed");
}
