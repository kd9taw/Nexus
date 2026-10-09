//! The live engine drives the QSO and Field Day auto-sequencers end to end:
//! two engines over a virtual channel complete a ragchew QSO and a Field Day
//! exchange, and the results surface in each engine's snapshot (the UI contract).

use std::ops::ControlFlow;

use tempo_app::dto::Tier;
use tempo_app::engine::Engine;
use tempo_app::logstore::Freshness;
use tempo_core::channel::{VirtualAir, ON_TIME_OFFSET};
use tempo_core::logbook::sqlite::{call_norm_of, Narrow, Order, Scope};
use tempo_core::tempo_fast;

/// Whether `e`'s log holds a contact with `call`, asked as every reader of the log asks it: the
/// rows of that call (the store's call index; on the 1.13 path the log in memory), read with no
/// Engine guard held and only once every change made before the question is in.
fn log_holds(e: &Engine, call: &str) -> bool {
    const CALL: Narrow = Narrow {
        columns: &["call"],
        uploads: false,
    };
    let mut held = false;
    let fresh = e
        .log_rows()
        .each(
            CALL,
            Scope::CallNorm(&call_norm_of(call)),
            Order::Log,
            &mut |r| {
                held |= r.call == call;
                ControlFlow::Continue(())
            },
        )
        .expect("the log reads");
    assert!(matches!(fresh, Freshness::Current), "the log is current");
    held
}

/// Real audio capture normalizes the soundcard int16 to f32 (÷32768); the VirtualAir
/// harness instead emits f32 at the ×100 int16 scale (paired with channel::to_i16). The
/// engine.'s real decode path now uses capture_to_i16 (×32767), so convert harness output
/// to real-capture range before ingest — signal + noise together, so SNR is preserved.
fn to_capture(mut rx: Vec<f32>) -> Vec<f32> {
    for s in rx.iter_mut() {
        *s *= 100.0 / 32767.0;
    }
    rx
}

/// Shuttle frames between two engines over the channel for `slots` slots.
/// Engine `a` transmits on even slots, `b` on odd. Returns after `done(a,b)`.
fn run(a: &mut Engine, b: &mut Engine, slots: u64, done: impl Fn(&Engine, &Engine) -> bool) {
    let mut a2b = VirtualAir::new(tempo_fast::SAMPLE_RATE, 11);
    let mut b2a = VirtualAir::new(tempo_fast::SAMPLE_RATE, 22);
    for slot in 0..slots {
        if slot % 2 == 0 {
            for w in a.poll_tx(slot) {
                let rx = to_capture(a2b.receive(&w, ON_TIME_OFFSET, 15.0));
                b.ingest(&rx, slot);
            }
        } else {
            for w in b.poll_tx(slot) {
                let rx = to_capture(b2a.receive(&w, ON_TIME_OFFSET, 15.0));
                a.ingest(&rx, slot);
            }
        }
        if done(a, b) {
            break;
        }
    }
}

#[test]
fn qso_mode_completes_through_the_engine() {
    let mut a = Engine::new("W9XYZ", "EN37", 0);
    let mut b = Engine::new("K2DEF", "FN31", 1);
    a.set_tier(Tier::TempoFast); // FT1-modem loopback (default tier is now FT8)
    b.set_tier(Tier::TempoFast);
    a.set_mode("qso-run").unwrap(); // A RUNS (calls CQ)
                                    // B works A explicitly (Monitor is now passive — no auto-answer; the operator
                                    // double-clicks a decode, which is call_station).
    b.call_station("W9XYZ");

    let b_done = |e: &Engine| e.snapshot().qso.map(|q| q.state == "Done").unwrap_or(false);
    let a_logged = |e: &Engine| log_holds(e, "K2DEF");
    run(&mut a, &mut b, 60, |a, b| a_logged(a) && b_done(b));

    // The answerer (monitor) reaches Done with the runner as its DX.
    assert!(b_done(&b), "B qso: {:?}", b.snapshot().qso);
    assert_eq!(b.snapshot().qso.unwrap().dxcall.as_deref(), Some("W9XYZ"));
    // The runner logged the contact...
    assert!(a_logged(&a), "A logged K2DEF");
    assert!(
        !log_holds(&a, "W9XYZ"),
        "control: the question finds only a call A logged"
    );
    // ...and, because it was RUNNING, returns to calling CQ to work the next caller
    // (WSJT-X run workflow) — give it a few more periods to process its own RR73.
    run(&mut a, &mut b, 12, |a, _| {
        a.snapshot()
            .qso
            .map(|q| q.state == "CallingCq")
            .unwrap_or(false)
    });
    assert_eq!(
        a.snapshot().qso.unwrap().state,
        "CallingCq",
        "A resumed calling CQ after the QSO"
    );
}

#[test]
fn field_day_mode_logs_through_the_engine() {
    let mut run_st = Engine::new("W9XYZ", "EN37", 0);
    let mut sp = Engine::new("K2DEF", "FN31", 1);
    run_st.set_tier(Tier::TempoFast); // FT1-modem loopback (default tier is now FT8)
    sp.set_tier(Tier::TempoFast);
    // Configure exchanges via settings, then enter Field Day mode.
    {
        let mut s = run_st.settings().clone();
        s.fd_active = true; // master switch on — FD chrome visible in the snapshot
        s.fd_class = "3A".into();
        s.fd_section = "WI".into();
        run_st.apply_settings(s);
        let mut s = sp.settings().clone();
        s.fd_active = true;
        s.fd_class = "2A".into();
        s.fd_section = "IL".into();
        sp.apply_settings(s);
    }
    run_st.set_mode("fieldday-run").unwrap(); // run mode arms TX itself
    sp.set_mode("fieldday-sp").unwrap();
    // S&P is passive (no auto-CQ), so it doesn't arm TX — and TX is disarmed by default
    // now (WSJT-X Enable-Tx). Arm the S&P side so it answers the runner in this loopback.
    sp.set_tx_enabled(true);

    let logged = |e: &Engine| {
        e.snapshot()
            .field_day
            .map(|f| f.qso_count >= 1)
            .unwrap_or(false)
    };
    run(&mut run_st, &mut sp, 50, |a, b| logged(a) && logged(b));

    let fr = run_st.snapshot().field_day.expect("run field day status");
    let fs = sp.snapshot().field_day.expect("sp field day status");
    assert_eq!(fr.qso_count, 1, "runner log: {:?}", fr.log);
    assert_eq!(fs.qso_count, 1, "sp log: {:?}", fs.log);
    // Runner logged the S&P's exchange and vice versa.
    assert_eq!(fr.log[0].call, "K2DEF");
    assert_eq!(
        (fr.log[0].class.as_str(), fr.log[0].section.as_str()),
        ("2A", "IL")
    );
    assert_eq!(fs.log[0].call, "W9XYZ");
    assert_eq!(
        (fs.log[0].class.as_str(), fs.log[0].section.as_str()),
        ("3A", "WI")
    );
    assert_eq!(fr.points, 2); // one digital QSO = 2 points
}

/// ⛔ **Hard gate 2's boundary, pinned through FT itself.** What a club position measures of
/// its clock against the host's is SHOWN, and reaches no clock, slot or timestamp FT reads.
///
/// The runner is a club position whose host is 30 s ahead, told so by 100 round trips through
/// the real bridge; then it works a Field Day contact over the FT modem loopback. Its FT
/// steering offset (`Engine::clock_offset_ms`: the one number the radio loop subtracts from the
/// system clock for every slot, TX key and decode window) is exactly what SNTP left it, with an
/// offset held and with none; the contact the sequencer logged, and one logged by hand after
/// it, are stamped by this PC's own clock, 30 s from the host's.
#[test]
fn the_club_clock_reaches_no_clock_slot_or_stamp_ft_reads() {
    use std::sync::{Arc, Mutex};
    use tempo_app::fdbridge::EnginePositionSync;
    use tempo_net::fdsync::{ClockSample, PositionSync};
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
    };
    for held in [None, Some(1_234i64)] {
        let runner = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
        let mut sp = Engine::new("K2DEF", "FN31", 1);
        {
            let mut r = runner.lock().unwrap();
            r.set_tier(Tier::TempoFast);
            let mut s = r.settings().clone();
            s.fd_active = true;
            s.fd_class = "3A".into();
            s.fd_section = "WI".into();
            s.fd_position_id = "aaaa0001".into();
            s.fd_join_addr = "127.0.0.1:42073".into(); // a club position: the club block shows
            r.apply_settings(s);
            r.set_mode("fieldday-run").unwrap();
            if let Some(ms) = held {
                r.publish_clock_offset(ms, 3, None);
            }
        }
        sp.set_tier(Tier::TempoFast);
        let mut s = sp.settings().clone();
        s.fd_active = true;
        s.fd_class = "2A".into();
        s.fd_section = "IL".into();
        sp.apply_settings(s);
        sp.set_mode("fieldday-sp").unwrap();
        sp.set_tx_enabled(true);

        let (ft_offset, ft_chip) = {
            let r = runner.lock().unwrap();
            (r.clock_offset_ms(), r.snapshot().radio.clock_offset_ms)
        };
        assert_eq!(
            ft_offset, held,
            "scene: FT steers by what SNTP measured, or by nothing"
        );

        let club = EnginePositionSync(runner.clone());
        club.on_welcome(0, "TEST FD", "W9ABC", now().as_secs());
        let base = now().as_millis() as u64;
        for i in 0..100u64 {
            let t0 = base + i * 5_000;
            club.on_clock(ClockSample {
                t0,
                t1: t0 + 30_001,
                t2: t0 + 30_002,
                t3: t0 + 3,
            });
        }

        let mut r = runner.lock().unwrap();
        // Control: the club line did take the measurement.
        assert_eq!(
            r.snapshot().field_day.unwrap().club.unwrap().skew_secs,
            -30,
            "the position says it is 30 s behind the host"
        );
        let logged = |e: &Engine| {
            e.snapshot()
                .field_day
                .map(|f| f.qso_count >= 1)
                .unwrap_or(false)
        };
        let lo = now().as_secs();
        run(&mut r, &mut sp, 50, |a, b| logged(a) && logged(b));
        assert!(
            r.fd_log_manual("N0XYZ", "1D", "MN", "CW").unwrap(),
            "the hand-logged contact"
        );
        let hi = now().as_secs();

        let fd = r.snapshot().field_day.unwrap();
        assert_eq!(
            fd.qso_count, 2,
            "the FT contact and the hand-logged one: {:?}",
            fd.log
        );
        for q in &fd.log {
            assert!(
                lo <= q.when_unix && q.when_unix <= hi,
                "{} is stamped by this PC's clock: {lo} ≤ {} ≤ {hi}",
                q.call,
                q.when_unix
            );
        }
        assert_eq!(
            r.clock_offset_ms(),
            ft_offset,
            "FT's slot clock is untouched"
        );
        assert_eq!(
            r.snapshot().radio.clock_offset_ms,
            ft_chip,
            "and so is the clock chip"
        );
    }
}
