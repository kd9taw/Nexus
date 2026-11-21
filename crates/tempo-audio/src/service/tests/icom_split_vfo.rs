//! ⭐ AN ICOM BEHIND HAMLIB IS ON VFO A AFTER EVERY SPLIT COMMAND, AND A B PRESS UNDER SPLIT IS
//! NOT A QSY.
//!
//! Hamlib's `rig_set_split_vfo` makes the receive VFO A for both `S 1 VFOB` and `S 0 VFOA`, and
//! `icom_set_split_vfo` selects it (`07 00`) before the split command. An IC-7300 cannot be asked
//! which VFO it is on, so after a B press and a split Nexus's indicator still said B while the
//! radio received on A. And with split on, a B press made the next dial read VFO B's frequency,
//! which Nexus took for a knob QSY: it turned split off (`S 0 VFOA`), and Hamlib put the radio
//! back on A. Each test runs the real `RadioLoop::step` against a rigctld that behaves as Hamlib's
//! Icom backend does to the radio's VFOs.

use super::cw_stop::{logging_rigctld, step};
use super::*;

/// The radio's VFOs, as the stub keeps them.
struct Vfos {
    on_b: bool,
    a: u64,
    b: u64,
}

/// A rigctld for an Icom behind Hamlib: `V` selects a VFO, every `S` selects VFO A first, `F`
/// sets the selected VFO, `I` sets VFO B, `f` reads the selected VFO. `m` answers USB, `t` not
/// keyed, anything else `RPRT 0` (so `\dump_caps` offers no VFO read, as on an IC-7300).
#[allow(clippy::type_complexity)] // (addr, wire log, the radio's VFOs) — a test-only triple
fn icom_rigctld() -> (String, Arc<Mutex<Vec<String>>>, Arc<Mutex<Vfos>>) {
    let vfos = Arc::new(Mutex::new(Vfos {
        on_b: false,
        a: 14_200_000,
        b: 14_200_000,
    }));
    let v = Arc::clone(&vfos);
    let (addr, log) = logging_rigctld(move |line| {
        let mut v = v.lock().unwrap();
        let mut words = line.split_whitespace();
        match (words.next()?, words.next()) {
            ("V", Some(vfo)) => {
                v.on_b = vfo == "VFOB";
                None
            }
            ("S", _) => {
                v.on_b = false;
                None
            }
            ("F", Some(hz)) => {
                let hz = hz.parse().ok()?;
                if v.on_b {
                    v.b = hz;
                } else {
                    v.a = hz;
                }
                None
            }
            ("I", Some(hz)) => {
                v.b = hz.parse().ok()?;
                None
            }
            ("f", None) => Some(format!("{}\n", if v.on_b { v.b } else { v.a })),
            ("m", None) => Some("USB\n2400\n".to_string()),
            ("t", None) => Some("0\n".to_string()),
            _ => None,
        }
    });
    (addr, log, vfos)
}

/// Phone at 14.200 USB on Hamlib model `model`, with Split Operation off.
fn station(model: u32) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("W9XYZ", "EN37", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.rig_model = model;
        s.split_mode = tempo_app::settings::SplitMode::None;
        e.apply_settings(s);
        e.set_license_class("extra");
        e.set_operating_mode("phone", false);
        e.set_frequency(14.200, "20m", "USB");
    }
    engine
}

/// Ticks of the loop 3 s apart, each with the heavy poll due, `between` run on the engine first.
fn run(engine: &Arc<Mutex<Engine>>, rig: &mut Rig, between: Vec<fn(&mut Engine)>) {
    let mut state = loop_state_for(engine);
    let mut backend = MockBackend::new();
    let mut t = 100.0;
    for f in between {
        f(&mut engine.lock().unwrap());
        state.last_rig_poll = -1_000_000.0;
        step(&mut state, engine, &mut backend, rig, t);
        t += 3_000.0;
    }
}

fn shown_vfo(engine: &Arc<Mutex<Engine>>) -> String {
    engine.lock().unwrap().snapshot().radio.active_vfo
}

fn idle(_: &mut Engine) {}

/// B pressed, then a split: the radio is on A (Hamlib's `07 00`), and so is the indicator.
fn b_then_split(model: u32) -> (String, bool, Vec<String>) {
    let (addr, log, vfos) = icom_rigctld();
    let engine = station(model);
    let mut rig = Rig::rigctld(&addr);
    run(
        &engine,
        &mut rig,
        vec![
            idle,
            |e| e.request_vfo(true),
            idle,
            |e| e.request_split(Some(14.205)),
            idle,
            idle,
        ],
    );
    let on_b = vfos.lock().unwrap().on_b;
    let lines = log.lock().unwrap().clone();
    (shown_vfo(&engine), on_b, lines)
}

#[test]
fn a_split_on_a_hamlib_icom_leaves_the_indicator_on_vfo_a() {
    for model in [3073u32, 3085, 3070, 3090] {
        let (shown, on_b, lines) = b_then_split(model);
        assert!(
            lines.iter().any(|l| l == "V VFOB") && lines.iter().any(|l| l == "S 1 VFOB"),
            "model {model}: precondition, B then split: {lines:?}"
        );
        assert!(!on_b, "model {model}: the stub radio is on A");
        assert_eq!(
            shown, "A",
            "model {model}: the indicator says where the radio is"
        );
    }
}

/// CONTROL: the IC-9700 (satellite mode, `RIG_FUNC_SATMODE`) keeps what it had: Hamlib does not
/// move its VFOs that way, and nothing here restates it.
#[test]
fn a_split_on_a_hamlib_ic9700_leaves_its_vfo_belief_alone() {
    let (shown, _, lines) = b_then_split(3081);
    assert!(lines.iter().any(|l| l == "S 1 VFOB"), "{lines:?}");
    assert_eq!(shown, "B");
}

/// With split on, a B press is the operator listening on VFO B: the dial follows it, split stays
/// on, and nothing sends `S 0 VFOA` (which on this radio is a press back to A).
#[test]
fn b_pressed_under_split_on_a_hamlib_icom_stays_on_b_with_split() {
    let (addr, log, vfos) = icom_rigctld();
    let engine = station(3073);
    let mut rig = Rig::rigctld(&addr);
    run(
        &engine,
        &mut rig,
        vec![
            idle,
            |e| e.request_split(Some(14.205)),
            idle,
            |e| e.request_vfo(true),
            idle,
            idle,
            idle,
            idle,
        ],
    );
    let lines = log.lock().unwrap().clone();
    assert!(
        lines.iter().any(|l| l == "V VFOB"),
        "precondition: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l.starts_with("S 0")),
        "no split off after the B press: {lines:?}"
    );
    assert!(vfos.lock().unwrap().on_b, "the radio stays on B: {lines:?}");
    assert_eq!(shown_vfo(&engine), "B");
    let e = engine.lock().unwrap();
    assert_eq!(e.split_tx_mhz(), Some(14.205), "the split stands");
    assert!(
        (e.settings().dial_mhz - 14.205).abs() < 1e-9,
        "the dial follows VFO B"
    );
}

/// A General in Phone at 14.200 USB (Extra-only phone) with split to 14.250 (inside General
/// phone), on Hamlib model `model`; `then` runs after the split, a tick apart each. Returns
/// whether the licence lets the transmitter key, and the frequency it judges the emission at.
fn general_split(model: u32, then: Vec<fn(&mut Engine)>) -> (bool, f64, Vec<String>) {
    let (addr, log, _) = icom_rigctld();
    let engine = station(model);
    engine.lock().unwrap().set_license_class("general");
    let mut rig = Rig::rigctld(&addr);
    let mut ticks: Vec<fn(&mut Engine)> = vec![idle, |e| e.request_split(Some(14.250)), idle];
    ticks.extend(then);
    ticks.extend([idle as fn(&mut Engine), idle, idle]);
    run(&engine, &mut rig, ticks);
    let e = engine.lock().unwrap();
    let lines = log.lock().unwrap().clone();
    (
        e.tx_allowed(),
        e.snapshot().radio.tx_emission_mhz.unwrap_or(0.0),
        lines,
    )
}

/// CONTROL: split on, VFO A selected: the radio transmits on VFO B, 14.250, inside General phone.
#[test]
fn a_split_on_vfo_a_is_judged_at_vfo_b() {
    let (allowed, tx, lines) = general_split(3073, vec![]);
    assert!(lines.iter().any(|l| l == "S 1 VFOB"), "{lines:?}");
    assert!((tx - 14.250).abs() < 1e-9, "judged at {tx}");
    assert!(allowed, "14.250 is inside General phone");
}

/// ⛔ SPLIT WITH VFO B SELECTED TRANSMITS ON VFO A, AND IS JUDGED THERE (operator ruling: treat
/// VFO A as TX in that state). An IC-7300 behind Hamlib receives on B and keys A, here 14.200,
/// which a General may not use for phone, though B is inside. The gate used to judge B and let it
/// key.
#[test]
fn split_with_vfo_b_selected_on_a_hamlib_icom_is_judged_at_vfo_a() {
    for model in [3073u32, 3085, 3070, 3090] {
        let (allowed, tx, lines) = general_split(model, vec![|e| e.request_vfo(true)]);
        assert!(
            lines.iter().any(|l| l == "V VFOB"),
            "model {model}: {lines:?}"
        );
        assert!(
            !lines.iter().any(|l| l.starts_with("S 0")),
            "model {model}: split stays on: {lines:?}"
        );
        assert!(
            (tx - 14.200).abs() < 1e-9,
            "model {model}: judged at {tx}, not VFO A"
        );
        assert!(
            !allowed,
            "model {model}: VFO A, 14.200, is outside General phone"
        );
    }
}

/// …and back on VFO A, the transmitter is VFO B again.
#[test]
fn split_back_on_vfo_a_is_judged_at_vfo_b_again() {
    let (allowed, tx, _) = general_split(
        3073,
        vec![|e| e.request_vfo(true), idle, |e| e.request_vfo(false)],
    );
    assert!((tx - 14.250).abs() < 1e-9, "judged at {tx}");
    assert!(allowed);
}

/// CONTROL: the IC-9700 (satellite mode) is judged as before, at the split TX dial.
#[test]
fn split_with_vfo_b_selected_on_a_hamlib_ic9700_is_judged_as_before() {
    let (allowed, tx, _) = general_split(3081, vec![|e| e.request_vfo(true)]);
    assert!((tx - 14.250).abs() < 1e-9, "judged at {tx}");
    assert!(allowed);
}

/// ⛔ A SPLIT REQUEST UNDER A HELD PTT, WHILE THE SPLIT IS REVERSED, IS JUDGED AT THE REQUEST. A
/// General at 14.250 USB with split to 14.300, VFO B selected (the radio keys VFO A, 14.250, inside
/// General phone), PTT held: a request for 14.200 (Extra-only phone) is written into a keyed radio
/// only if the loop's own check passes it, and judged at VFO A it did.
#[test]
fn a_split_request_under_ptt_while_reversed_is_judged_at_the_request() {
    let (addr, log, _) = icom_rigctld();
    let engine = station(3073);
    {
        let mut e = engine.lock().unwrap();
        e.set_license_class("general");
        e.set_frequency(14.250, "20m", "USB");
    }
    let mut rig = Rig::rigctld(&addr);
    run(
        &engine,
        &mut rig,
        vec![
            idle,
            |e| e.request_split(Some(14.300)),
            idle,
            |e| e.request_vfo(true),
            idle,
            idle,
            |e| {
                assert!(
                    (e.snapshot().radio.tx_emission_mhz.unwrap() - 14.250).abs() < 1e-9,
                    "precondition: reversed, judged at VFO A"
                );
                e.set_tx_enabled(true);
                e.set_ptt(true);
                assert!(e.manual_ptt(), "precondition: PTT held in privileges");
            },
            |e| e.request_split(Some(14.200)),
            idle,
        ],
    );
    let lines = log.lock().unwrap().clone();
    assert!(
        lines.iter().any(|l| l == "T 1"),
        "precondition: keyed: {lines:?}"
    );
    assert!(
        !lines.iter().any(|l| l == "I 14200000"),
        "an Extra-only split never reaches the keyed radio: {lines:?}"
    );
}
