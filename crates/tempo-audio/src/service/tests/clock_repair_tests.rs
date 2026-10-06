//! The clock repair asks Windows for administrator rights only when the operator presses
//! **Repair clock**.
//!
//! An operator's screenshot (2026-10-06) showed a Windows administrator prompt nobody had asked
//! for: "Windows Command Processor", `cmd.exe /c sc.exe config w32time start= auto && sc.exe
//! start w32time && w32tm.exe /resync` — the `StartService` repair. The background clock pass
//! ran it itself whenever the diagnosis said a repair would help, behind a limiter that lived in
//! memory and so started over at every launch.
//!
//! Every test here drives the real clock code with a stand-in machine whose diagnosis is fixed
//! and whose elevated helper only COUNTS, so "no prompt" is a number rather than a reading.

use super::*;
use crate::clockdiag::{decide, ClockDiagnosis, Findings, Repair};
use std::cell::Cell;

/// What the user in the screenshot had: Windows Time stopped (a "debloat" script, say) on a
/// machine that reaches a time server. Built by the real decision table, so the fixture is the
/// diagnosis that machine actually got.
fn stopped_time_service() -> ClockDiagnosis {
    let d = decide(&Findings {
        service_present: true,
        probe_reached_network: true,
        measured_offset_ms: Some(40),
        repairs_available: true,
        ..Default::default()
    });
    assert_eq!(
        d.repair,
        Repair::StartService,
        "premise: the screenshot's repair"
    );
    d
}

/// A default Windows PC: Windows Time running and synchronised, checking the time every 32768 s
/// (the Windows default) with the `0x1` flag set. The decision table names the poll write for
/// it, and that is a classification only: the operator's ruling (2026-10-06) offers it nowhere.
fn default_windows() -> ClockDiagnosis {
    let d = decide(&findings_of_default_windows());
    assert_eq!(
        d.repair,
        Repair::SetPollInterval(1_024),
        "premise: the table's poll write"
    );
    d
}

/// Windows Time running but never synchronised (its server blocked, say).
fn not_synchronised() -> ClockDiagnosis {
    let d = decide(&Findings {
        service_present: true,
        service_running: true,
        probe_reached_network: true,
        measured_offset_ms: Some(40),
        repairs_available: true,
        ..Default::default()
    });
    assert_eq!(d.repair, Repair::Resync { rediscover: true }, "premise");
    d
}

/// The default Windows PC just after the clock jumped (a resume from sleep).
fn just_jumped() -> ClockDiagnosis {
    let d = decide(&Findings {
        just_stepped: true,
        ..findings_of_default_windows()
    });
    assert_eq!(d.repair, Repair::Resync { rediscover: false }, "premise");
    d
}

/// What a default Windows PC's detection finds (see [`default_windows`]).
fn findings_of_default_windows() -> Findings {
    Findings {
        service_present: true,
        service_running: true,
        synced: true,
        poll_secs: Some(32_768),
        special_interval_flag: Some(true),
        min_poll_secs: Some(1_024),
        probe_reached_network: true,
        measured_offset_ms: Some(40),
        repairs_available: true,
        ..Default::default()
    }
}

/// A machine that needs nothing: its time service is running, synced and polling often.
fn healthy() -> ClockDiagnosis {
    let d = decide(&Findings {
        service_present: true,
        service_running: true,
        synced: true,
        poll_secs: Some(1_024),
        special_interval_flag: Some(true),
        min_poll_secs: Some(1_024),
        probe_reached_network: true,
        measured_offset_ms: Some(40),
        repairs_available: true,
        ..Default::default()
    });
    assert_eq!(d.repair, Repair::None, "premise: nothing to repair");
    d
}

/// A machine with a fixed diagnosis. Its elevated helper records each call and the repair it was
/// given, runs `during` (a second press arriving while Windows is still asking), and answers
/// `took`.
struct Machine<'a> {
    diag: ClockDiagnosis,
    took: bool,
    elevations: Cell<u32>,
    ran: Cell<Option<Repair>>,
    during: Option<&'a dyn Fn()>,
}

impl Machine<'_> {
    fn new(diag: ClockDiagnosis) -> Self {
        Self {
            diag,
            took: true,
            elevations: Cell::new(0),
            ran: Cell::new(None),
            during: None,
        }
    }
}

impl ClockHost for Machine<'_> {
    fn detect(&self, _reached: bool, _offset: Option<i64>, _stepped: bool) -> ClockDiagnosis {
        self.diag.clone()
    }

    fn run_repair_elevated(&self, repair: Repair) -> bool {
        self.elevations.set(self.elevations.get() + 1);
        self.ran.set(Some(repair));
        if let Some(during) = self.during {
            during();
        }
        self.took
    }
}

fn engine() -> Arc<Mutex<Engine>> {
    Arc::new(Mutex::new(Engine::with_settings(Settings::default())))
}

/// An FT8 station that can transmit (its callsign and grid set), TX off.
fn station() -> Arc<Mutex<Engine>> {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::Ft8);
    Arc::new(Mutex::new(e))
}

/// A Tempo station (TempoFast, chat), TX off until something is sent.
fn tempo_station() -> Arc<Mutex<Engine>> {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.set_tier(Tier::TempoFast);
    Arc::new(Mutex::new(e))
}

/// A JS8 station on the 20 m watering hole, TX on.
fn js8_station() -> Arc<Mutex<Engine>> {
    let mut e = Engine::new("KD9TAW", "EN52", 0);
    e.js8_enter();
    e.set_frequency(14.078, "20m", "USB");
    e.set_tx_enabled(true);
    Arc::new(Mutex::new(e))
}

/// What the top bar reads: the note and whether the button shows.
fn shown(engine: &Arc<Mutex<Engine>>) -> (String, bool) {
    let radio = engine_lock(engine).snapshot().radio;
    (radio.clock_owner_note, radio.clock_repair_available)
}

/// ⛔ THE DEFECT. A pass runs on a timer and on resume, with nobody asking for anything: it
/// diagnoses and says what it found, and it never elevates. Nothing is on the air and nothing has
/// been tried, so every gate the old pass had in front of the elevation was open. Against that
/// pass this counted one elevation on the first pass.
#[test]
fn the_background_pass_never_elevates() {
    let engine = engine();
    let repair = ClockRepair::new();
    let machine = Machine::new(stopped_time_service());
    // An hour and a half of passes, one of them just after a clock jump, as a resume gives.
    for pass in 0..10 {
        clock_diagnose(&engine, &repair, &machine, Some(40), pass == 3);
    }
    assert_eq!(
        machine.elevations.get(),
        0,
        "the background clock pass asked Windows for administrator rights by itself"
    );
    // It still diagnoses and says so, and offers the repair for the operator to run.
    assert_eq!(shown(&engine), (stopped_time_service().detail, true));
}

/// The offer follows the latest diagnosis, both ways, and nothing on offer is nothing to run.
#[test]
fn the_pass_offers_the_repair_it_found_and_withdraws_it_when_none_is_needed() {
    let engine = engine();
    let repair = ClockRepair::new();
    clock_diagnose(
        &engine,
        &repair,
        &Machine::new(stopped_time_service()),
        None,
        false,
    );
    assert!(shown(&engine).1, "a repair that would help is offered");

    let fixed = Machine::new(healthy());
    clock_diagnose(&engine, &repair, &fixed, Some(40), false);
    assert_eq!(shown(&engine), (healthy().detail, false));
    assert_eq!(
        repair_clock_with(&engine, &repair, &fixed),
        Err(ClockRepairRefusal::NothingToRepair)
    );
    assert_eq!(fixed.elevations.get(), 0);
}

/// The press runs the repair the pass found, through one elevation, and says what it achieved.
/// A repair that took comes off offer, so the button goes and a second press prompts for nothing.
#[test]
fn a_press_runs_the_offered_repair_once() {
    let engine = engine();
    let repair = ClockRepair::new();
    let machine = Machine::new(stopped_time_service());
    clock_diagnose(&engine, &repair, &machine, Some(40), false);

    assert_eq!(repair_clock_with(&engine, &repair, &machine), Ok(true));
    assert_eq!(machine.elevations.get(), 1);
    assert_eq!(machine.ran.get(), Some(Repair::StartService));
    assert_eq!(
        shown(&engine),
        ("started the Windows Time service".to_string(), false)
    );

    assert_eq!(
        repair_clock_with(&engine, &repair, &machine),
        Err(ClockRepairRefusal::NothingToRepair)
    );
    assert_eq!(machine.elevations.get(), 1, "no second prompt");
}

/// A prompt answered No (or a step that failed) says so and leaves the button, so the operator
/// can press again; the next press is theirs too.
#[test]
fn a_repair_that_did_not_take_stays_on_offer() {
    let engine = engine();
    let repair = ClockRepair::new();
    let machine = Machine {
        took: false,
        ..Machine::new(stopped_time_service())
    };
    clock_diagnose(&engine, &repair, &machine, Some(40), false);

    assert_eq!(repair_clock_with(&engine, &repair, &machine), Ok(false));
    let (note, available) = shown(&engine);
    assert!(note.contains("could not fix it"), "{note}");
    assert!(available, "still on offer after a failed repair");
    assert_eq!(repair_clock_with(&engine, &repair, &machine), Ok(false));
    assert_eq!(machine.elevations.get(), 2);
}

/// ⚠️ THE TX INTERLOCK. A repair can move the system clock, so nothing runs while anything is on
/// the air, here a rig keyed at the radio, which Nexus did not start. The offer survives, and the
/// same press with the rig unkeyed runs it (the control: the refusal was the interlock).
#[test]
fn refused_while_anything_is_on_the_air() {
    let engine = engine();
    let repair = ClockRepair::new();
    let machine = Machine::new(stopped_time_service());
    clock_diagnose(&engine, &repair, &machine, Some(40), false);

    engine_lock(&engine).observe_rig_ptt(true);
    assert_eq!(
        repair_clock_with(&engine, &repair, &machine),
        Err(ClockRepairRefusal::OnAir)
    );
    assert_eq!(machine.elevations.get(), 0);
    assert!(shown(&engine).1, "the offer survives the refusal");

    engine_lock(&engine).observe_rig_ptt(false);
    assert_eq!(repair_clock_with(&engine, &repair, &machine), Ok(true));
    assert_eq!(machine.elevations.get(), 1);
}

/// One repair at a time. Two presses arrive while Windows is still asking about the first: both
/// are refused without a prompt of their own, and the second refusal shows the first did not end
/// the running repair's claim. Once the first returns, a press runs again (the control).
#[test]
fn refused_while_a_repair_is_running() {
    let engine = engine();
    let repair = ClockRepair::new();
    let inner = Machine::new(stopped_time_service());
    let (second, third) = (Cell::new(None), Cell::new(None));
    let press_twice = || {
        second.set(Some(repair_clock_with(&engine, &repair, &inner)));
        third.set(Some(repair_clock_with(&engine, &repair, &inner)));
    };
    let outer = Machine {
        during: Some(&press_twice),
        ..Machine::new(stopped_time_service())
    };
    clock_diagnose(&engine, &repair, &outer, Some(40), false);

    assert_eq!(repair_clock_with(&engine, &repair, &outer), Ok(true));
    assert_eq!(second.get(), Some(Err(ClockRepairRefusal::Running)));
    assert_eq!(third.get(), Some(Err(ClockRepairRefusal::Running)));
    assert_eq!((outer.elevations.get(), inner.elevations.get()), (1, 0));

    clock_diagnose(&engine, &repair, &inner, Some(40), false);
    assert_eq!(repair_clock_with(&engine, &repair, &inner), Ok(true));
    assert_eq!(inner.elevations.get(), 1);
}

/// ⛔ ONLY FOR REAL FAULTS (operator ruling, 2026-10-06). A healthy default Windows PC shows no
/// button and a press runs nothing: the poll write is no longer offered at all.
#[test]
fn a_default_windows_clock_is_offered_nothing() {
    let engine = engine();
    let repair = ClockRepair::new();
    let machine = Machine::new(default_windows());
    clock_diagnose(&engine, &repair, &machine, Some(40), false);
    assert_eq!(shown(&engine), (default_windows().detail, false));
    assert_eq!(
        repair_clock_with(&engine, &repair, &machine),
        Err(ClockRepairRefusal::NothingToRepair)
    );
    assert_eq!(machine.elevations.get(), 0);
}

/// The three real faults are still offered, and a press runs exactly the repair that was found.
#[test]
fn each_real_fault_is_still_offered_and_runs_its_own_repair() {
    for (diag, want) in [
        (stopped_time_service(), Repair::StartService),
        (not_synchronised(), Repair::Resync { rediscover: true }),
        (just_jumped(), Repair::Resync { rediscover: false }),
    ] {
        let engine = engine();
        let repair = ClockRepair::new();
        let machine = Machine::new(diag);
        clock_diagnose(&engine, &repair, &machine, Some(40), false);
        assert!(shown(&engine).1, "{want:?}: on offer");
        assert_eq!(repair_clock_with(&engine, &repair, &machine), Ok(true));
        assert_eq!(
            (machine.elevations.get(), machine.ran.get()),
            (1, Some(want))
        );
    }
}

/// The command runs a real fault's repair and nothing else. Even with the poll write on offer
/// (planted here: no pass offers it), a press runs nothing and asks Windows for nothing.
#[test]
fn a_press_never_runs_the_poll_write() {
    let engine = engine();
    let repair = ClockRepair::new();
    let machine = Machine::new(default_windows());
    repair.offer(Some(default_windows()));
    assert_eq!(
        repair_clock_with(&engine, &repair, &machine),
        Err(ClockRepairRefusal::NothingToRepair)
    );
    assert_eq!((machine.elevations.get(), machine.ran.get()), (0, None));
}

/// ⚠️ THE TX INTERLOCK SEES EVERY OWNER OF THE TRANSMITTER, not only the slot flag, a tune and
/// the rig's own PTT: here a CW message still going out word by word, which none of those three
/// shows. Run then, the repair could move the clock under it. The same press once the words are
/// gone runs (the control).
#[test]
fn refused_while_cw_is_still_sending() {
    let engine = engine();
    let repair = ClockRepair::new();
    let machine = Machine::new(stopped_time_service());
    clock_diagnose(&engine, &repair, &machine, Some(40), false);

    engine_lock(&engine).send_cw("CQ CQ DE KD9TAW K");
    assert_eq!(
        engine_lock(&engine).tx_owner(),
        Some(tempo_app::engine::TxOwner::Cw),
        "premise: CW owns the transmitter"
    );
    assert_eq!(
        repair_clock_with(&engine, &repair, &machine),
        Err(ClockRepairRefusal::OnAir)
    );
    assert_eq!(machine.elevations.get(), 0);

    engine_lock(&engine).stop_cw();
    assert_eq!(repair_clock_with(&engine, &repair, &machine), Ok(true));
    assert_eq!(machine.elevations.get(), 1);
}

/// ⛔ A REPAIR NEVER PAUSES A MESSAGE (the operator's ruling, 2026-10-06: "Refuse the press
/// mid-message"). A Tempo broadcast that takes several overs, pressed in the slot between two of
/// them: nothing is on the air then, so the transmit interlock alone let the press through, and
/// the hold stopped the message half sent. It is refused, Windows is asked nothing, and the offer
/// survives. The same press once the message has gone runs (the control).
#[test]
fn refused_while_a_tempo_message_is_part_way_through() {
    let engine = tempo_station();
    let repair = ClockRepair::new();
    let machine = Machine::new(stopped_time_service());
    clock_diagnose(&engine, &repair, &machine, Some(40), false);

    let mut slot = {
        let mut e = engine_lock(&engine);
        e.broadcast("THIS ONE TAKES SEVERAL OVERS TO SAY");
        let own = u64::from(!e.tx_even());
        assert!(e.plan_tx(own).is_some(), "premise: its first over");
        assert!(
            e.plan_tx(own + 1).is_none(),
            "premise: the slot between two of its overs"
        );
        assert!(!e.on_air(), "premise: nothing on the air between its overs");
        own + 2
    };
    assert_eq!(
        repair_clock_with(&engine, &repair, &machine),
        Err(ClockRepairRefusal::MidMessage)
    );
    assert_eq!(machine.elevations.get(), 0);
    assert!(shown(&engine).1, "the offer survives the refusal");

    {
        let mut e = engine_lock(&engine);
        while e.plan_tx(slot).is_some() {
            slot += 2;
        }
        assert!(e.plan_tx(slot + 1).is_none());
        assert!(
            !e.on_air() && !e.message_in_progress(),
            "premise: the message has gone"
        );
    }
    assert_eq!(repair_clock_with(&engine, &repair, &machine), Ok(true));
    assert_eq!(machine.elevations.get(), 1);
}

/// …and a JS8 message of several frames, pressed between two of them. JS8 keys every period, so
/// the slot flag stays up for the whole message and the transmit interlock already refused this
/// press, as "on the air". It now says what to wait for: the message. The press once the message
/// has gone runs (the control).
#[test]
fn refused_while_a_js8_message_is_part_way_through() {
    let engine = js8_station();
    let repair = ClockRepair::new();
    let machine = Machine::new(stopped_time_service());
    clock_diagnose(&engine, &repair, &machine, Some(40), false);

    {
        let mut e = engine_lock(&engine);
        e.js8_send(None, "TEST MESSAGE WITH MULTIPLE FRAMES".into())
            .expect("queues");
        assert!(e.js8_state().queue.len() > 1, "premise: several frames");
        assert!(e.plan_tx(0).is_some(), "premise: its first frame");
    }
    assert_eq!(
        repair_clock_with(&engine, &repair, &machine),
        Err(ClockRepairRefusal::MidMessage)
    );
    assert_eq!(machine.elevations.get(), 0);

    {
        let mut e = engine_lock(&engine);
        let mut slot = 1;
        while e.plan_tx(slot).is_some() {
            slot += 1;
        }
        assert!(
            !e.on_air() && !e.message_in_progress(),
            "premise: the message has gone"
        );
    }
    assert_eq!(repair_clock_with(&engine, &repair, &machine), Ok(true));
    assert_eq!(machine.elevations.get(), 1);
}

/// ⛔ THE HOLD (the operator's ruling, 2026-10-06: "Yes, hold TX until it finishes"). From the
/// press until the elevated helper exits, nothing starts transmitting: TX On and Tune, tried while
/// Windows is still asking, are refused, and the station says a repair holds transmit. Once the
/// helper exits, whether the repair took or not (a failed step and a declined prompt answer
/// alike), both work again.
#[test]
fn nothing_starts_transmitting_while_the_repair_runs() {
    for took in [true, false] {
        let engine = station();
        let repair = ClockRepair::new();
        let tried = Cell::new(None);
        let try_to_transmit = || {
            let mut e = engine_lock(&engine);
            e.set_tx_enabled(true);
            e.set_tune(true);
            let held = e.snapshot().radio.clock_repair_tx_held;
            tried.set(Some((e.tx_enabled(), e.tuning(), held)));
        };
        let machine = Machine {
            took,
            during: Some(&try_to_transmit),
            ..Machine::new(stopped_time_service())
        };
        clock_diagnose(&engine, &repair, &machine, Some(40), false);

        assert_eq!(repair_clock_with(&engine, &repair, &machine), Ok(took));
        assert_eq!(
            tried.get(),
            Some((false, false, true)),
            "took: {took}: (TX On, Tune, held) while Windows was asking"
        );
        let mut e = engine_lock(&engine);
        assert!(!e.snapshot().radio.clock_repair_tx_held, "took: {took}");
        e.set_tx_enabled(true);
        e.set_tune(true);
        assert_eq!(
            (e.tx_enabled(), e.tuning()),
            (true, true),
            "took: {took}: TX On and Tune once the helper exited"
        );
    }
}

/// An FT8 run armed before the press misses its over while Windows is asking, and is still
/// armed: the hold refuses a start and disarms nothing. The run's next over plans once the
/// helper exits.
#[test]
fn an_armed_run_misses_its_overs_while_the_repair_runs_and_keeps_its_arm() {
    let engine = station();
    let slot = {
        let mut e = engine_lock(&engine);
        e.start_cq(None).unwrap();
        if e.tx_even() {
            0
        } else {
            1
        }
    };
    let repair = ClockRepair::new();
    let at_the_slot = Cell::new(None);
    let plan_an_over = || {
        let mut e = engine_lock(&engine);
        at_the_slot.set(Some((e.plan_tx(slot).is_some(), e.tx_enabled())));
    };
    let machine = Machine {
        during: Some(&plan_an_over),
        ..Machine::new(stopped_time_service())
    };
    clock_diagnose(&engine, &repair, &machine, Some(40), false);

    assert_eq!(repair_clock_with(&engine, &repair, &machine), Ok(true));
    assert_eq!(
        at_the_slot.get(),
        Some((false, true)),
        "(an over planned, still armed) while Windows was asking"
    );
    assert!(
        engine_lock(&engine).plan_tx(slot).is_some(),
        "the run's next over, once the helper exited"
    );
}

/// ★ A hung repair never holds transmit forever: past its bound the hold lets go while the helper
/// is still running, and the outcome says so. With a bound of zero, TX On arms while Windows is
/// still asking. (With the real bound the note has no such words: `a_press_runs_the_offered_repair_once`.)
#[test]
fn the_hold_lets_go_at_its_bound_and_the_outcome_says_so() {
    let engine = station();
    let repair = ClockRepair {
        tx_hold: Duration::ZERO,
        ..ClockRepair::new()
    };
    let tried = Cell::new(None);
    let try_tx_on = || {
        let mut e = engine_lock(&engine);
        e.set_tx_enabled(true);
        tried.set(Some(e.tx_enabled()));
    };
    let machine = Machine {
        during: Some(&try_tx_on),
        ..Machine::new(stopped_time_service())
    };
    clock_diagnose(&engine, &repair, &machine, Some(40), false);

    assert_eq!(repair_clock_with(&engine, &repair, &machine), Ok(true));
    assert_eq!(tried.get(), Some(true), "TX On past the bound");
    assert_eq!(
        shown(&engine).0,
        "started the Windows Time service \
         (transmit was released after 0 s, before the repair finished)"
    );
}

/// The hold comes off however the press ends, a helper that panics included: TX On arms after.
#[test]
fn the_hold_comes_off_however_the_press_ends() {
    let engine = station();
    let repair = ClockRepair::new();
    let fail = || panic!("the helper failed");
    let machine = Machine {
        during: Some(&fail),
        ..Machine::new(stopped_time_service())
    };
    clock_diagnose(&engine, &repair, &machine, Some(40), false);

    let pressed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        repair_clock_with(&engine, &repair, &machine)
    }));
    assert!(pressed.is_err(), "premise: the press ended in a panic");
    let mut e = engine_lock(&engine);
    assert!(!e.clock_repair_holds_tx(), "the hold is off");
    e.set_tx_enabled(true);
    assert!(e.tx_enabled(), "TX On after a press that panicked");
}

/// Off means off: with the clock check switched off the loop withdraws the offer, so the button
/// goes and a press runs nothing from a diagnosis nobody is taking any more.
#[test]
fn nothing_is_offered_once_the_clock_check_is_off() {
    let engine = engine();
    let repair = ClockRepair::new();
    let machine = Machine::new(stopped_time_service());
    clock_diagnose(&engine, &repair, &machine, Some(40), false);
    assert!(shown(&engine).1, "premise: on offer");

    clock_repair_withdraw(&engine, &repair);
    assert!(!shown(&engine).1);
    assert_eq!(
        repair_clock_with(&engine, &repair, &machine),
        Err(ClockRepairRefusal::NothingToRepair)
    );
    assert_eq!(machine.elevations.get(), 0);
}
