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
