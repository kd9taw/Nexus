//! APRS tune over Remote: its own v3 action and hint, only the regional APRS channels, the FM
//! simplex readback transaction, and no transmit authority as a side effect.
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

fn action(dial: f64) -> Value {
    json!({"action":"radio.aprsTune","dialMhz":dial})
}

#[test]
fn aprs_tune_needs_v3_its_hint_and_fm_simplex_readback_and_never_arms_transmit() {
    let (f, connection) = station();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    assert!(state["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("aprsTuning")));
    let command = control_request(&state, action(144.39));
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    let pending = run(&f, 3, &command).unwrap();
    assert_eq!(pending["outcome"], "pending");
    assert_eq!(run(&f, 3, &command).unwrap(), pending);
    let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
    assert_eq!(work.target(), (144_390_000, "FM"));
    assert_eq!(work.repeater(), Some(("simplex", 0, 0.0)));
    work.permission().begin_write(Instant::now()).unwrap();
    sample(&f, &connection, 144_390_000, "FM");
    let power = work.power_limit();
    // The CAT transaction itself is covered by the audio owner tests; this is its readback.
    assert!(work.commit_tuning_readback(
        &mut f.engine.lock().unwrap(),
        power,
        Some(("simplex", 0, 0.0))
    ));
    let applied = run(&f, 3, &command).unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["evidence"], "radioReadback");
    assert_eq!(run(&f, 3, &command).unwrap(), applied);
    let mut e = f.engine.lock().unwrap();
    assert!(e.take_remote_radio().is_none(), "one receipt, no replay");
    assert!(!e.tx_enabled());
    assert!(!e.remote_ft_tx_owned());
    assert_eq!(e.settings().dial_hz(), 144_390_000);
}

#[test]
fn a_frequency_that_is_not_an_aprs_channel_is_refused_and_a_channel_is_not() {
    for (dial, allowed) in [(144.8, true), (146.52, false), (144.391, false)] {
        let (f, _connection) = station();
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let result = run(&f, 3, &control_request(&state, action(dial))).unwrap();
        if allowed {
            assert_eq!(result["outcome"], "pending", "positive control: {dial}");
            assert!(f.engine.lock().unwrap().take_remote_radio().is_some());
        } else {
            assert_eq!(result["outcome"], "rejected");
            assert_eq!(result["reason"], "invalidAction");
            assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        }
        assert!(!f.engine.lock().unwrap().tx_enabled());
    }
}

#[test]
fn aprs_tune_is_refused_while_transmit_is_armed_and_admitted_when_idle() {
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
        let result = run(&f, 3, &control_request(&state, action(144.39))).unwrap();
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
fn aprs_tune_is_refused_on_a_radio_that_cannot_receive_2m_and_admitted_on_one_that_can() {
    for (ranges, allowed) in [
        (vec![(1_800_000_u64, 54_000_000_u64)], false),
        (
            vec![(1_800_000, 54_000_000), (144_000_000, 148_000_000)],
            true,
        ),
    ] {
        let (f, _connection) = station();
        f.engine.lock().unwrap().observe_rig_rx_ranges(Some(ranges));
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let result = run(&f, 3, &control_request(&state, action(144.39))).unwrap();
        if allowed {
            assert_eq!(result["outcome"], "pending", "positive control");
            assert!(f.engine.lock().unwrap().take_remote_radio().is_some());
        } else {
            assert_eq!(result["outcome"], "rejected");
            assert_eq!(result["reason"], "hardwareUnavailable");
            assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
            assert_eq!(f.engine.lock().unwrap().settings().dial_hz(), 145_500_000);
        }
    }
}

#[test]
fn aprs_tune_rejects_logging_only_authority_and_unreviewed_fields() {
    let (f, _connection) = station();
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    let valid = action(144.39);
    assert_eq!(
        run(&f, 3, &control_request(&state, valid.clone())),
        Err("localPermissionRequired")
    );
    for field in [
        "band",
        "mode",
        "shift",
        "txEnabled",
        "settings",
        "radioId",
        "command",
    ] {
        let mut invalid = valid.clone();
        invalid[field] = json!(true);
        assert!(
            serde_json::from_value::<station::Action>(invalid).is_err(),
            "{field}"
        );
    }
    let mut invalid = valid.clone();
    invalid["dialMhz"] = json!("144.39");
    assert!(serde_json::from_value::<station::Action>(invalid).is_err());
    assert!(serde_json::from_value::<station::Action>(json!({"action":"radio.aprsTune"})).is_err());
    assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
}
