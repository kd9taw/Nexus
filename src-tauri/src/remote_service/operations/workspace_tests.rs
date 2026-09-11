use super::*;
use tempo_app::dto::Tier;

#[test]
fn workspace_entry_requires_v3_and_preserves_one_receipt_through_readback_or_revocation() {
    for revoke in [false, true] {
        let f = Fixture::new();
        let connection = {
            let mut e = f.engine.lock().unwrap();
            e.configure_remote_settings_store(f.dir.join("settings.json"));
            e.set_tx_enabled(false);
            e.set_frequency(14.074, "20m", "USB");
            e.take_immediate_retune();
            e.remote_open_radio().unwrap()
        };
        let sample = |hz, mode: &str| {
            let mut e = f.engine.lock().unwrap();
            let read = e.remote_radio_read(&connection, Instant::now()).unwrap();
            e.remote_observe_cat(Some(&read), Some(true));
            e.remote_observe_dial(Some(&read), Some(hz));
            e.remote_observe_mode(Some(&read), Some(mode));
            e.remote_observe_ptt(Some(&read), Some(false));
        };
        sample(14_074_000, "PKTUSB");
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let command = control_request(
            &state,
            json!({"action":"radio.workspace","workspace":"js8"}),
        );
        let run = |version, request: &Request| {
            f.authority.handle_version(
                (f.connection, version),
                SESSION,
                DEVICE,
                request,
                &f.engine,
                Instant::now(),
            )
        };
        assert_eq!(run(2, &command), Err("stationUnsupported"));
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        let pending = run(3, &command).unwrap();
        assert_eq!(pending["outcome"], "pending");
        assert_eq!(run(3, &command).unwrap(), pending);
        let work = {
            let mut e = f.engine.lock().unwrap();
            assert_eq!(e.tier(), Tier::Ft8);
            assert!(!e.tx_enabled());
            let work = e.take_remote_radio().unwrap();
            assert!(e.take_remote_radio().is_none());
            work
        };
        let result = Request::Result {
            request_id: id(),
            operation_id: command.id().into(),
        };
        assert!(!f.dir.join("settings.json").exists());
        if revoke {
            f.authority.permit_station(DEVICE, false).unwrap();
            assert!(work.permission().begin_write(Instant::now()).is_err());
            sample(work.target().0, work.target().1);
            assert!(!work.commit_readback(&mut f.engine.lock().unwrap(), None));
            assert_eq!(f.engine.lock().unwrap().tier(), Tier::Ft8);
            assert!(!f.dir.join("settings.json").exists());
        } else {
            work.permission().begin_write(Instant::now()).unwrap();
            sample(work.target().0, work.target().1);
            let power = work.power_limit();
            assert!(work.commit_readback(&mut f.engine.lock().unwrap(), power));
            let applied = run(3, &result).unwrap();
            assert_eq!(applied["outcome"], "applied");
            assert_eq!(applied["evidence"], "radioReadback");
            let saved = std::fs::read(f.dir.join("settings.json")).unwrap();
            assert_eq!(run(3, &command).unwrap(), applied);
            assert_eq!(run(2, &result).unwrap(), applied);
            assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), saved);
            assert_eq!(f.engine.lock().unwrap().tier(), Tier::Js8);
        }
        assert!(!f.engine.lock().unwrap().tx_enabled());
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        assert!(!f.engine.lock().unwrap().take_immediate_retune());
    }
}

#[test]
fn workspace_grammar_and_local_grants_cannot_be_bypassed_with_native_arguments() {
    for action in [
        json!({"action":"radio.workspace","workspace":"FT"}),
        json!({"action":"radio.workspace","workspace":"phone"}),
        json!({"action":"radio.workspace","workspace":"ft","tier":"WSPR"}),
        json!({"action":"radio.workspace","workspace":"tempo","txEnabled":true}),
        json!({"action":"radio.workspace","workspace":"js8","settings":{}}),
        json!({"action":"radio.workspace","workspace":"js8","command":"js8_enter"}),
    ] {
        assert!(serde_json::from_value::<station::Action>(action).is_err());
    }
    let f = Fixture::new();
    let now = Instant::now();
    f.acquire(now); // Logging alone cannot enter a station workspace.
    let state = control_state_version(&f, now, 3);
    let command = control_request(
        &state,
        json!({"action":"radio.workspace","workspace":"tempo"}),
    );
    assert_eq!(
        f.authority
            .handle_version((f.connection, 3), SESSION, DEVICE, &command, &f.engine, now),
        Err("localPermissionRequired")
    );
    assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
}
