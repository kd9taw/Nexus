//! The CW serial keyline and serial PTT on ONE port — CW on DTR, PTT on RTS, the conventional
//! keyline interface — key through one shared handle (`control_line::SharedControlLines`). The
//! keyer used to open the port for itself, and a COM port opens once: whichever of the keyer and
//! serial PTT came second was refused ("Access is denied.") and nothing keyed.
//!
//! Each scene runs the real `RadioLoop::step` with the real keyer thread over a
//! `control_line::fake_ports` port, which opens exclusively as a COM port does and records both
//! lines. Every way a tune or a message ends must leave the key up and PTT down; one holder going
//! mid-transmission must leave the other's line where it was. The keyer keys in REAL time while
//! the loop's clock is stepped, so each scene waits for the lines to go still (`fsk_settle`).
#![cfg(feature = "serial")]

use super::cw_stop::step;
use super::*;
use crate::control_line::fake_ports;

/// An engine in the CW section on 40 m, Extra class, keying CW on DTR of `port` with serial PTT on
/// RTS of the same port, and `set` applied.
fn engine_on(
    port: &str,
    set: impl FnOnce(&mut tempo_app::settings::Settings),
) -> Arc<Mutex<Engine>> {
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.cw_key_port = port.into();
        s.cw_key_line = "dtr".into();
        s.ptt_method = "rts".into();
        s.ptt_serial_port = port.into();
        set(&mut s);
        e.apply_settings(s);
        e.set_license_class("extra");
        e.set_cw_keyer("serial", 600.0);
        e.set_operating_mode("cw", false);
        e.set_frequency(7.03, "40m", "CW");
        e.set_cw_wpm(20);
    }
    engine
}

/// The scene: the fake port, the engine, and a loop that has already applied these settings, so it
/// keys this serial-PTT rig rather than reopening one.
fn scene(
    port: &str,
    set: impl FnOnce(&mut tempo_app::settings::Settings),
) -> (Arc<Mutex<Engine>>, RadioLoop, MockBackend, Rig) {
    fake_ports::install(port);
    let engine = engine_on(port, set);
    let mut state = loop_state();
    state.applied = Transport::from_settings(engine.lock().unwrap().settings());
    let rig = Rig::with_control(None, ptt_mode_for(&state.applied));
    assert_eq!(
        rig.ptt_mode(),
        &PttMode::Serial {
            port: port.into(),
            line: SerialLine::Rts
        },
        "premise: serial PTT on RTS of the keyline's port"
    );
    (engine, state, MockBackend::new(), rig)
}

/// Wait (at most 2 s) for `port`'s lines to satisfy `want`.
fn wait_for(port: &str, want: impl Fn(fake_ports::State) -> bool) -> fake_ports::State {
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_secs(2) {
        let s = fake_ports::state(port);
        if want(s) {
            return s;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    fake_ports::state(port)
}

/// Press Tune and run the tick that keys it: PTT up on RTS and the key held down on DTR.
fn tune(
    engine: &Arc<Mutex<Engine>>,
    state: &mut RadioLoop,
    backend: &mut MockBackend,
    rig: &mut Rig,
    port: &str,
) {
    engine.lock().unwrap().set_tune(true);
    step(state, engine, backend, rig, 100.0);
    assert_eq!(
        engine.lock().unwrap().cw_keyer_error(),
        None,
        "{port}: the keyer and PTT both open the one port"
    );
    assert!(state.tuning_keyed && rig.keyed, "{port}: the tune keyed");
    let s = wait_for(port, |s| s.dtr && s.rts);
    assert!(
        s.dtr && s.rts,
        "{port}: key down on DTR under PTT on RTS: {s:?}"
    );
}

/// The key is up and PTT is down; the port is still held while `held`.
fn assert_both_down(port: &str, held: bool, how: &str) {
    fsk_settle(port);
    let s = fake_ports::state(port);
    assert!(!s.dtr, "{how}: the key is up: {s:?}");
    assert!(!s.rts, "{how}: PTT is down: {s:?}");
    assert_eq!(
        s.held, held,
        "{how}: the port is held exactly while a holder remains"
    );
}

/// ★ THE BUG, AND EVERY WAY A TUNE ENDS (#500's exits): the key is let up and PTT drops.
#[test]
fn a_cw_tune_keys_the_keyline_and_serial_ptt_on_one_port_and_every_ending_leaves_both_down() {
    // Tune off.
    let port = "tempo-test-cw-shared-tune-off";
    let (engine, mut state, mut backend, mut rig) = scene(port, |_| {});
    tune(&engine, &mut state, &mut backend, &mut rig, port);
    engine.lock().unwrap().set_tune(false);
    step(&mut state, &engine, &mut backend, &mut rig, 140.0);
    assert!(!rig.keyed && !state.tuning_keyed);
    assert_both_down(port, true, "Tune off");

    // Stop TX.
    let port = "tempo-test-cw-shared-tune-stop";
    let (engine, mut state, mut backend, mut rig) = scene(port, |_| {});
    tune(&engine, &mut state, &mut backend, &mut rig, port);
    engine.lock().unwrap().halt_tx();
    step(&mut state, &engine, &mut backend, &mut rig, 140.0);
    assert!(!rig.keyed && !state.tuning_keyed);
    assert_both_down(port, true, "Stop TX");

    // The operator's Tune timeout.
    let port = "tempo-test-cw-shared-tune-timeout";
    let (engine, mut state, mut backend, mut rig) = scene(port, |s| s.tune_timeout_secs = 1);
    tune(&engine, &mut state, &mut backend, &mut rig, port);
    step(&mut state, &engine, &mut backend, &mut rig, 1_200.0);
    assert!(!rig.keyed && !state.tuning_keyed);
    assert_both_down(port, true, "the Tune timeout");

    // The 60 s ceiling, whatever the setting says.
    let port = "tempo-test-cw-shared-tune-ceiling";
    let (engine, mut state, mut backend, mut rig) = scene(port, |s| s.tune_timeout_secs = 600);
    tune(&engine, &mut state, &mut backend, &mut rig, port);
    step(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        100.0 + MAX_TUNE_MS - 100.0,
    );
    assert!(state.tuning_keyed, "control: still inside the ceiling");
    step(
        &mut state,
        &engine,
        &mut backend,
        &mut rig,
        100.0 + MAX_TUNE_MS + 100.0,
    );
    assert!(!rig.keyed && !state.tuning_keyed);
    assert_both_down(port, true, "the tune ceiling");

    // The keyline's port setting cleared mid-tune: the keyer goes, and the tune with it.
    let port = "tempo-test-cw-shared-tune-port-gone";
    let (engine, mut state, mut backend, mut rig) = scene(port, |_| {});
    tune(&engine, &mut state, &mut backend, &mut rig, port);
    {
        let mut e = engine.lock().unwrap();
        let mut s = e.settings().clone();
        s.cw_key_port = String::new();
        e.apply_settings(s);
    }
    step(&mut state, &engine, &mut backend, &mut rig, 140.0);
    step(&mut state, &engine, &mut backend, &mut rig, 160.0);
    assert!(!engine.lock().unwrap().tuning(), "the tune ends");
    assert!(!rig.keyed && !state.tuning_keyed);
    assert_both_down(port, true, "the keyline's port closing");

    // App shutdown.
    let port = "tempo-test-cw-shared-tune-shutdown";
    let (engine, mut state, mut backend, mut rig) = scene(port, |_| {});
    tune(&engine, &mut state, &mut backend, &mut rig, port);
    state.stop_for_shutdown(&engine, &mut backend, &mut rig);
    state.release_keyers_for_shutdown();
    assert!(!rig.keyed);
    assert_both_down(port, true, "shutdown");

    // A context change (a CAT or audio rebuild, the Test-CAT hold).
    let port = "tempo-test-cw-shared-tune-context";
    let (engine, mut state, mut backend, mut rig) = scene(port, |_| {});
    tune(&engine, &mut state, &mut backend, &mut rig, port);
    state.clear_loop_tx_for_context(&mut backend, &mut rig, "test: a rebuild");
    assert!(!rig.keyed && !state.tuning_keyed);
    assert_both_down(port, true, "a context change");

    // A radio switch mid-tune.
    let port = "tempo-test-cw-shared-tune-switch";
    let (engine, mut state, mut backend, mut rig) = scene(port, |_| {});
    let r1 = engine.lock().unwrap().add_radio();
    let mut last_active = engine.lock().unwrap().settings().active_radio;
    assert_ne!(last_active, r1, "control: a second radio to switch to");
    tune(&engine, &mut state, &mut backend, &mut rig, port);
    engine.lock().unwrap().set_active_radio(r1);
    let pool: MonitorPool = Arc::new(MonitorConnections::new(Vec::new()));
    let pending = std::sync::atomic::AtomicBool::new(false);
    handoff_if_switched(
        &engine,
        &pool,
        &mut rig,
        &mut backend,
        &mut state,
        &mut last_active,
        &pending,
    );
    assert!(!state.tuning_keyed, "the tune is over");
    fsk_settle(port);
    let s = fake_ports::state(port);
    assert!(
        !s.dtr && !s.rts,
        "a radio switch: the key is up and PTT down: {s:?}"
    );
}

/// ★ ONE HOLDER GOING MID-TRANSMISSION LEAVES THE OTHER'S LINE. The rig lets go of its PTT port
/// mid-tune (the dual-radio handoff demoting it): PTT drops at once, and the key — the keyer's
/// line — stays down until the tune lets it up. Then the tune's own end leaves both down.
#[test]
fn ptt_letting_go_mid_tune_does_not_move_the_key() {
    let port = "tempo-test-cw-shared-tune-ptt-goes";
    let (engine, mut state, mut backend, mut rig) = scene(port, |_| {});
    tune(&engine, &mut state, &mut backend, &mut rig, port);
    rig.release_ptt_port();
    let s = fake_ports::state(port);
    assert!(!s.rts, "the released PTT line drops at once: {s:?}");
    assert!(
        s.dtr,
        "the PTT holder going did not let up the key, the keyer's line: {s:?}"
    );
    assert!(s.held, "the keyer still holds the port");
    engine.lock().unwrap().set_tune(false);
    step(&mut state, &engine, &mut backend, &mut rig, 140.0);
    assert!(!state.tuning_keyed);
    assert_both_down(port, true, "Tune off after PTT let go");
}

/// The keyline on the very line serial PTT keys is refused before anything opens or keys: a tune
/// refused with neither line moved, and a keyline whose port will not open refuses its tune with
/// no PTT either.
#[test]
fn a_cw_tune_on_the_ptt_line_or_a_dead_keyline_port_is_refused_with_no_ptt() {
    let port = "tempo-test-cw-shared-same-line";
    let (engine, mut state, mut backend, mut rig) = scene(port, |s| s.cw_key_line = "rts".into());
    engine.lock().unwrap().set_tune(true);
    step(&mut state, &engine, &mut backend, &mut rig, 100.0);
    step(&mut state, &engine, &mut backend, &mut rig, 140.0);
    assert!(!rig.keyed && !state.tuning_keyed, "the tune is refused");
    assert!(!engine.lock().unwrap().tuning());
    let said = engine.lock().unwrap().cw_keyer_error().unwrap_or_default();
    assert!(
        said.contains("same control line"),
        "the cockpit says why: {said:?}"
    );
    assert!(
        fake_ports::log(port).iter().all(|s| !s.rts && !s.dtr),
        "neither line was ever raised (the rig's own PTT-off may write a line low): {:?}",
        fake_ports::log(port)
    );

    let port = "tempo-test-cw-shared-dead-keyline";
    let (engine, mut state, mut backend, mut rig) =
        scene(port, |s| s.cw_key_port = "NO_SUCH_KEYLINE_PORT".into());
    engine.lock().unwrap().set_tune(true);
    step(&mut state, &engine, &mut backend, &mut rig, 100.0);
    step(&mut state, &engine, &mut backend, &mut rig, 140.0);
    assert!(!rig.keyed && !state.tuning_keyed, "the tune is refused");
    let said = engine.lock().unwrap().cw_keyer_error().unwrap_or_default();
    assert!(
        said.contains("Serial keyline"),
        "the cockpit says why: {said:?}"
    );
    assert!(
        fake_ports::log(port).iter().all(|s| !s.rts),
        "PTT never went up: {:?}",
        fake_ports::log(port)
    );
}

/// A message scene: serial PTT has keyed once already (an over in another mode), so the rig holds
/// the port — as it does for the rest of the session — when CW is sent on the keyline.
fn message_scene(port: &str, text: &str) -> (Arc<Mutex<Engine>>, RadioLoop, MockBackend, Rig, f64) {
    let (engine, mut state, mut backend, mut rig) = scene(port, |_| {});
    rig.ptt(true).expect("premise: serial PTT keys");
    rig.ptt(false).expect("premise: and unkeys");
    assert!(
        fake_ports::state(port).held,
        "premise: the rig holds the port"
    );
    engine.lock().unwrap().send_cw(text);
    let mut t = 100.0;
    step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, 200.0);
    assert_eq!(
        engine.lock().unwrap().cw_keyer_error(),
        None,
        "{port}: the keyer opens the port the rig holds"
    );
    let s = wait_for(port, |s| s.dtr);
    assert!(s.dtr, "{port}: the keyline keys: {s:?}");
    (engine, state, backend, rig, t)
}

/// ★ A MESSAGE ON THE KEYLINE WHILE SERIAL PTT HOLDS THE PORT, and every way it ends — its own end,
/// Stop, Halt TX — leaves the key up and PTT down.
#[test]
fn a_cw_message_keys_on_the_port_serial_ptt_holds_and_every_ending_leaves_both_down() {
    let port = "tempo-test-cw-shared-msg-end";
    let (engine, mut state, mut backend, mut rig, mut t) = message_scene(port, "E E");
    fsk_settle(port);
    step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, 5_000.0);
    assert_eq!(
        fake_ports::log(port).iter().filter(|s| s.dtr).count(),
        2,
        "both words keyed: {:?}",
        fake_ports::log(port)
    );
    assert_both_down(port, true, "the message's end");

    let port = "tempo-test-cw-shared-msg-stop";
    let (engine, mut state, mut backend, mut rig, mut t) = message_scene(port, "0000 0000");
    engine.lock().unwrap().stop_cw();
    let until = t + 40.0;
    step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, until);
    assert_both_down(port, true, "Stop");

    let port = "tempo-test-cw-shared-msg-halt";
    let (engine, mut state, mut backend, mut rig, mut t) = message_scene(port, "0000 0000");
    engine.lock().unwrap().halt_tx();
    let until = t + 40.0;
    step_to(&engine, &mut state, &mut backend, &mut rig, &mut t, until);
    assert_both_down(port, true, "Halt TX");
}
