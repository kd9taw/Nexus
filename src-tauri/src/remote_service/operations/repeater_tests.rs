//! FM repeater tune over Remote (the Program section's Tune): its own v3 action and hint, the
//! readback transaction with the machine's shift, offset and tone, the licence judged at the
//! repeater INPUT, and no transmit authority as a side effect.
use super::receiver_filter::run;
use super::*;
use tempo_app::remote_monitor::provenance::Connection;

fn station() -> (Fixture, Connection) {
    let f = Fixture::new();
    let mut e = f.engine.lock().unwrap();
    e.configure_remote_settings_store(f.dir.join("settings.json"));
    e.set_operating_mode("phone", false);
    e.set_frequency(145.5, "2m", "USB");
    e.set_tx_enabled(false);
    e.take_immediate_retune();
    let connection = e.remote_open_radio().unwrap();
    drop(e);
    let (hz, mode) = current(&f);
    sample(&f, &connection, hz, &mode);
    (f, connection)
}

fn current(f: &Fixture) -> (u64, String) {
    let e = f.engine.lock().unwrap();
    (e.settings().dial_hz(), e.rig_mode_effective())
}

fn sample(f: &Fixture, connection: &Connection, hz: u64, mode: &str) {
    let mut e = f.engine.lock().unwrap();
    let read = e.remote_radio_read(connection, Instant::now()).unwrap();
    e.remote_observe_cat(Some(&read), Some(true));
    e.remote_observe_dial(Some(&read), Some(hz));
    e.remote_observe_mode(Some(&read), Some(mode));
    e.remote_observe_ptt(Some(&read), Some(false));
}

fn action(output: f64, shift: &str, offset: i64, tone: f64) -> Value {
    json!({"action":"radio.repeater","outputMhz":output,"shift":shift,"offsetHz":offset,"toneHz":tone})
}

#[test]
fn repeater_tune_needs_v3_its_hint_and_fm_readback_and_never_arms_transmit() {
    let (f, connection) = station();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    assert!(state["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("repeaterTuning")));
    let command = control_request(&state, action(146.94, "minus", 600_000, 100.0));
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    let pending = run(&f, 3, &command).unwrap();
    assert_eq!(pending["outcome"], "pending");
    assert_eq!(run(&f, 3, &command).unwrap(), pending);
    let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
    assert_eq!(work.target(), (146_940_000, "FM"));
    assert_eq!(work.repeater(), Some(("minus", 600_000, 100.0)));
    work.permission().begin_write(Instant::now()).unwrap();
    sample(&f, &connection, 146_940_000, "FM");
    let power = work.power_limit();
    // The CAT transaction itself is covered by the audio owner tests; this is its readback.
    assert!(work.commit_tuning_readback(
        &mut f.engine.lock().unwrap(),
        power,
        Some(("minus", 600_000, 100.0))
    ));
    let applied = run(&f, 3, &command).unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["evidence"], "radioReadback");
    assert_eq!(run(&f, 3, &command).unwrap(), applied);
    let mut e = f.engine.lock().unwrap();
    assert!(e.take_remote_radio().is_none(), "one receipt, no replay");
    assert!(!e.tx_enabled());
    assert!(!e.remote_ft_tx_owned());
    let settings = serde_json::to_value(e.settings()).unwrap();
    assert_eq!(settings["rptrShift"], "minus");
    assert_eq!(settings["rptrOffsetOverrideHz"], 600_000);
    assert_eq!(settings["phoneMode"], "fm");
}

#[test]
fn a_repeater_whose_input_is_outside_the_licence_is_refused_and_a_legal_one_is_not() {
    // Technician on 6 m: phone starts at 50.1 MHz. A 51.000 machine with the 1 MHz convention
    // keys 50.000, inside the CW-only slice; a 51.200 machine keys 50.200.
    for (output, allowed) in [(51.2, true), (51.0, false)] {
        let (f, _connection) = station();
        f.engine.lock().unwrap().set_license_class("technician");
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let result = run(
            &f,
            3,
            &control_request(&state, action(output, "minus", 0, 0.0)),
        )
        .unwrap();
        if allowed {
            assert_eq!(result["outcome"], "pending", "positive control: {output}");
            assert!(f.engine.lock().unwrap().take_remote_radio().is_some());
        } else {
            assert_eq!(result["outcome"], "rejected");
            assert_eq!(result["reason"], "outsidePrivileges");
            assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        }
        assert!(!f.engine.lock().unwrap().tx_enabled());
    }
}

#[test]
fn repeater_tune_is_refused_while_transmit_is_armed_and_admitted_when_idle() {
    for armed in [true, false] {
        let (f, connection) = station();
        if armed {
            let mut e = f.engine.lock().unwrap();
            e.set_tx_enabled(true);
            e.take_immediate_retune();
        }
        let (hz, mode) = current(&f);
        sample(&f, &connection, hz, &mode);
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let result = run(
            &f,
            3,
            &control_request(&state, action(146.94, "minus", 600_000, 0.0)),
        )
        .unwrap();
        if armed {
            assert_eq!(result["outcome"], "rejected");
            assert_eq!(result["reason"], "stationBusy");
            assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
            assert!(
                f.engine.lock().unwrap().tx_enabled(),
                "the latch is left as it was"
            );
        } else {
            assert_eq!(result["outcome"], "pending", "idle is the positive control");
            assert!(!f.engine.lock().unwrap().tx_enabled());
        }
    }
}

#[test]
fn repeater_tune_rejects_logging_only_authority_and_unreviewed_fields() {
    let (f, _connection) = station();
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    let valid = action(146.94, "minus", 600_000, 100.0);
    assert_eq!(
        run(&f, 3, &control_request(&state, valid.clone())),
        Err("localPermissionRequired")
    );
    for field in ["txEnabled", "settings", "radioId", "command", "band"] {
        let mut invalid = valid.clone();
        invalid[field] = json!(true);
        assert!(
            serde_json::from_value::<station::Action>(invalid).is_err(),
            "{field}"
        );
    }
    for (key, value) in [
        ("shift", json!("up")),
        ("offsetHz", json!(-1)),
        ("offsetHz", json!(1.5)),
    ] {
        let mut invalid = valid.clone();
        invalid[key] = value;
        assert!(
            serde_json::from_value::<station::Action>(invalid).is_err(),
            "{key}"
        );
    }
    assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
}
