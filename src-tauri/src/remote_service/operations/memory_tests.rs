//! Memory recall over Remote: its own v3 action and hint, the readback transaction for the
//! memory's section, dial, sideband and FM machine, the licence judged at an FM machine's input,
//! and no transmit authority as a side effect.
use super::receiver_filter::run;
use super::*;
use tempo_app::remote_monitor::provenance::Connection;

fn station() -> (Fixture, Connection) {
    let f = Fixture::new();
    let mut e = f.engine.lock().unwrap();
    e.configure_remote_settings_store(f.dir.join("settings.json"));
    e.set_operating_mode("digital", false);
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

fn ssb() -> Value {
    json!({"action":"radio.memoryRecall","section":"phone","dialMhz":7.188,"band":"40m","sideband":"USB"})
}

fn fm(dial: f64, band: &str) -> Value {
    json!({"action":"radio.memoryRecall","section":"phone","dialMhz":dial,"band":band,"sideband":null,
        "fm":{"shift":"minus","offsetHz":1_000_000,"toneHz":0.0}})
}

#[test]
fn memory_recall_needs_v3_its_hint_and_readback_and_never_arms_transmit() {
    let (f, connection) = station();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    assert!(state["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("memoryRecall")));
    let command = control_request(&state, ssb());
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    let pending = run(&f, 3, &command).unwrap();
    assert_eq!(pending["outcome"], "pending");
    assert_eq!(run(&f, 3, &command).unwrap(), pending);
    let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
    assert_eq!(work.target(), (7_188_000, "USB"));
    assert_eq!(work.repeater(), None);
    work.permission().begin_write(Instant::now()).unwrap();
    sample(&f, &connection, 7_188_000, "USB");
    let power = work.power_limit();
    assert!(work.commit_tuning_readback(&mut f.engine.lock().unwrap(), power, None));
    let applied = run(&f, 3, &command).unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["evidence"], "radioReadback");
    assert_eq!(run(&f, 3, &command).unwrap(), applied);
    let mut e = f.engine.lock().unwrap();
    assert!(e.take_remote_radio().is_none(), "one receipt, no replay");
    assert!(
        !e.tx_enabled(),
        "the desktop recall arms Phone; a browser recall never does"
    );
    assert!(!e.remote_ft_tx_owned());
    let settings = serde_json::to_value(e.settings()).unwrap();
    assert_eq!(settings["operatingMode"], "phone");
    assert_eq!(settings["phoneMode"], "ssb");
    assert_eq!(e.snapshot().radio.sideband_override.as_deref(), Some("USB"));
}

#[test]
fn an_fm_memory_whose_input_is_outside_the_licence_is_refused_and_a_legal_one_is_not() {
    for (dial, allowed) in [(51.2, true), (51.0, false)] {
        let (f, _connection) = station();
        f.engine.lock().unwrap().set_license_class("technician");
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let result = run(&f, 3, &control_request(&state, fm(dial, "6m"))).unwrap();
        if allowed {
            assert_eq!(result["outcome"], "pending", "positive control: {dial}");
            let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
            assert_eq!(work.target().1, "FM");
            assert_eq!(work.repeater(), Some(("minus", 1_000_000, 0.0)));
        } else {
            assert_eq!(result["outcome"], "rejected");
            assert_eq!(result["reason"], "outsidePrivileges");
            assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        }
        assert!(!f.engine.lock().unwrap().tx_enabled());
    }
}

#[test]
fn memory_recall_is_refused_while_transmit_is_armed_and_admitted_when_idle() {
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
        let result = run(&f, 3, &control_request(&state, ssb())).unwrap();
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
fn memory_recall_rejects_logging_only_authority_and_unreviewed_fields() {
    let (f, _connection) = station();
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(&f, 3, &control_request(&state, ssb())),
        Err("localPermissionRequired")
    );
    for field in ["txEnabled", "settings", "radioId", "call", "tier"] {
        let mut invalid = ssb();
        invalid[field] = json!(true);
        assert!(
            serde_json::from_value::<station::Action>(invalid).is_err(),
            "{field}"
        );
    }
    for (key, value) in [
        ("section", json!("rtty")),
        ("sideband", json!("usb")),
        ("fm", json!({"shift":"up","offsetHz":600_000,"toneHz":0.0})),
        ("fm", json!({"shift":"minus","offsetHz":-1,"toneHz":0.0})),
        (
            "fm",
            json!({"shift":"minus","offsetHz":600_000,"toneHz":0.0,"txEnabled":true}),
        ),
    ] {
        let mut invalid = ssb();
        invalid[key] = value;
        assert!(
            serde_json::from_value::<station::Action>(invalid).is_err(),
            "{key}"
        );
    }
    assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
}
