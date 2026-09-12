use super::*;

pub(super) fn station(
    mode: &str,
    width: u32,
) -> (Fixture, tempo_app::remote_monitor::provenance::Connection) {
    let f = Fixture::new();
    let mut e = f.engine.lock().unwrap();
    e.configure_remote_settings_store(f.dir.join("settings.json"));
    e.set_operating_mode(mode, false);
    e.set_tx_enabled(false);
    e.set_frequency(14.275, "20m", "USB");
    e.take_immediate_retune();
    e.observe_rig_passband(Some(width));
    e.settings().save(&f.dir.join("settings.json")).unwrap();
    let connection = e.remote_open_radio().unwrap();
    drop(e);
    sample(&f, &connection, mode);
    (f, connection)
}

pub(super) fn sample(
    f: &Fixture,
    connection: &tempo_app::remote_monitor::provenance::Connection,
    mode: &str,
) {
    let mut e = f.engine.lock().unwrap();
    let read = e.remote_radio_read(connection, Instant::now()).unwrap();
    e.remote_observe_cat(Some(&read), Some(true));
    e.remote_observe_dial(Some(&read), Some(14_275_000));
    // The physical mode is authoritative for filter writes, including when it
    // differs from the commanded Phone sideband in Settings.
    e.remote_observe_mode(Some(&read), Some(if mode == "cw" { "CW" } else { "LSB" }));
    e.remote_observe_ptt(Some(&read), Some(false));
}

pub(super) fn run(f: &Fixture, version: u8, command: &Request) -> Result<Value, &'static str> {
    f.authority.handle_version(
        (f.connection, version),
        SESSION,
        DEVICE,
        command,
        &f.engine,
        Instant::now(),
    )
}

#[test]
fn receiver_filter_requires_v3_and_later_radio_readback_without_settings_or_replay() {
    for (mode, before, after) in [("cw", 500, 550), ("phone", 2400, 2300)] {
        let (f, connection) = station(mode, before);
        let settings = std::fs::read(f.dir.join("settings.json")).unwrap();
        let state = acquire_controls_version(&f, Instant::now(), 3);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("receiverFilter")));
        let command = control_request(
            &state,
            json!({"action":"radio.filterWidth", "mode":mode, "expectedHz":before, "hz":after}),
        );
        assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        let pending = run(&f, 3, &command).unwrap();
        assert_eq!(pending["outcome"], "pending");
        assert_eq!(run(&f, 3, &command).unwrap(), pending);
        let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
        assert_eq!(work.filter_width(), Some((before, after)));
        assert_eq!(
            f.engine.lock().unwrap().snapshot().radio.filter_width_hz,
            Some(before)
        );
        sample(&f, &connection, mode);
        assert!(work.commit_filter_readback(&mut f.engine.lock().unwrap(), Some(after)));
        let applied = run(&f, 3, &command).unwrap();
        assert_eq!(applied["outcome"], "applied");
        assert_eq!(applied["evidence"], "radioReadback");
        assert_eq!(
            std::fs::read(f.dir.join("settings.json")).unwrap(),
            settings
        );
        {
            let mut e = f.engine.lock().unwrap();
            e.request_filter_width(before);
            assert_eq!(e.take_passband_request(), Some(before));
        }
        assert_eq!(run(&f, 3, &command).unwrap(), applied);
        assert_eq!(
            run(
                &f,
                2,
                &Request::Result {
                    request_id: id(),
                    operation_id: command.id().into()
                }
            )
            .unwrap(),
            applied
        );
        let mut e = f.engine.lock().unwrap();
        assert_eq!(e.snapshot().radio.filter_width_hz, Some(before));
        assert!(e.take_remote_radio().is_none());
        assert_eq!(e.take_passband_request(), None);
        assert!(!e.tx_enabled());
    }
}

#[test]
fn receiver_filter_refuses_other_modes_stale_width_and_logging_only_grants() {
    let (f, _connection) = station("cw", 500);
    let action = json!({"action":"radio.filterWidth", "mode":"cw", "expectedHz":500, "hz":550});
    let settings = std::fs::read(f.dir.join("settings.json")).unwrap();
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(&f, 3, &control_request(&state, action.clone())),
        Err("localPermissionRequired")
    );
    for (field, value, reason) in [
        ("mode", json!("phone"), "contextChanged"),
        ("expectedHz", json!(600), "contextChanged"),
        ("hz", json!(2001), "invalidAction"),
    ] {
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let mut invalid = action.clone();
        invalid[field] = value;
        assert_eq!(
            run(&f, 3, &control_request(&state, invalid)).unwrap()["reason"],
            reason
        );
        let mut e = f.engine.lock().unwrap();
        assert!(e.take_remote_radio().is_none());
        assert_eq!(e.take_passband_request(), None);
    }
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(&f, 3, &control_request(&state, action.clone())).unwrap()["outcome"],
        "pending"
    );
    f.authority.permit_station(DEVICE, false).unwrap();
    assert_eq!(
        std::fs::read(f.dir.join("settings.json")).unwrap(),
        settings
    );
    for field in [
        "settings",
        "command",
        "sideband",
        "dialMhz",
        "radioId",
        "txEnabled",
    ] {
        let mut invalid = action.clone();
        invalid[field] = json!(true);
        assert!(serde_json::from_value::<station::Action>(invalid).is_err());
    }
}
