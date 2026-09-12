use super::*;

fn station() -> Fixture {
    let f = Fixture::new();
    let mut e = f.engine.lock().unwrap();
    e.configure_remote_settings_store(f.dir.join("settings.json"));
    e.take_immediate_retune();
    e.set_tx_enabled(false);
    let connection = e.remote_open_radio().unwrap();
    let read = e.remote_radio_read(&connection, Instant::now()).unwrap();
    e.remote_observe_cat(Some(&read), Some(true));
    e.remote_observe_ptt(Some(&read), Some(false));
    drop(e);
    f
}
fn action(f: &Fixture) -> Value {
    let e = f.engine.lock().unwrap();
    json!({"action":"receiver.rxGain", "radioId": e.settings().active_radio,
        "expectedSettingsRevision": crate::remote_service::query::settings_revision(e.settings()).unwrap(),
        "expectedGain":e.settings().rx_gain,"gain":2.5})
}
fn run(f: &Fixture, version: u8, command: &Request) -> Result<Value, &'static str> {
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
fn gain_has_its_own_v3_capability_and_recovered_receipts_never_replay_a_later_local_choice() {
    let f = station();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    assert!(state["controls"]["capabilities"]
        .as_array()
        .unwrap()
        .contains(&json!("receiverGain")));
    let command = control_request(&state, action(&f));
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    assert!(!f.dir.join("settings.json").exists());
    let receipt = run(&f, 3, &command).unwrap();
    assert_eq!(receipt["outcome"], "applied");
    assert_eq!(receipt["evidence"], "settingsSaved");
    {
        let mut e = f.engine.lock().unwrap();
        assert_eq!(e.settings().rx_gain, 2.5);
        e.set_rx_gain(1.5);
        e.settings().save(&f.dir.join("settings.json")).unwrap();
    }
    let bytes = std::fs::read(f.dir.join("settings.json")).unwrap();
    assert_eq!(run(&f, 3, &command).unwrap(), receipt);
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
        receipt
    );
    assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), bytes);
    f.authority.permit_station(DEVICE, false).unwrap();
    let mut e = f.engine.lock().unwrap();
    assert_eq!(e.settings().rx_gain, 1.5);
    assert!(!e.tx_enabled());
    assert!(e.take_remote_radio().is_none());
}

#[test]
fn a_fresh_lease_never_rebases_a_stale_gain_radio_or_configuration() {
    for changed in ["revision", "prior", "radio"] {
        let f = station();
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let mut intent = action(&f);
        match changed {
            "revision" => intent["expectedSettingsRevision"] = json!("b".repeat(64)),
            "prior" => intent["expectedGain"] = json!(1.5),
            _ => intent["radioId"] = json!(99),
        }
        let refused = run(&f, 3, &control_request(&state, intent)).unwrap();
        assert_eq!(refused["outcome"], "rejected");
        assert_eq!(refused["reason"], "contextChanged");
        assert!(!f.dir.join("settings.json").exists());
        let state = control_state_version(&f, Instant::now(), 3);
        assert_eq!(
            run(&f, 3, &control_request(&state, action(&f))).unwrap()["evidence"],
            "settingsSaved"
        );
    }
}

#[test]
fn gain_refuses_logging_only_grants_invalid_revision_and_native_form_arguments() {
    let f = station();
    for extra in ["settings", "command", "txLevel", "path", "expectedTier"] {
        let mut intent = action(&f);
        intent[extra] = json!(true);
        assert!(serde_json::from_value::<station::Action>(intent).is_err());
    }
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(&f, 3, &control_request(&state, action(&f))),
        Err("localPermissionRequired")
    );
    assert!(!f.dir.join("settings.json").exists());
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let mut invalid = action(&f);
    invalid["expectedSettingsRevision"] = json!("A".repeat(64));
    assert_eq!(
        run(&f, 3, &control_request(&state, invalid)).unwrap()["reason"],
        "invalidAction"
    );
    assert!(!f.dir.join("settings.json").exists());
}

#[test]
fn a_native_gain_gesture_retires_already_displayed_remote_authority() {
    let f = station();
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let old = action(&f);
    let command = control_request(&state, old.clone());
    f.engine.lock().unwrap().set_rx_gain(1.5);
    assert_eq!(run(&f, 3, &command), Err("staleContext"));
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(&f, 3, &control_request(&state, old)).unwrap()["reason"],
        "contextChanged"
    );
    assert!(!f.dir.join("settings.json").exists());
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(&f, 3, &control_request(&state, action(&f))).unwrap()["evidence"],
        "settingsSaved"
    );
}
