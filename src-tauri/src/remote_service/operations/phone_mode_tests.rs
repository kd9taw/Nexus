use super::receiver_filter::{run, station};
use super::*;

fn sample(f: &Fixture, connection: &tempo_app::remote_monitor::provenance::Connection, mode: &str) {
    let mut e = f.engine.lock().unwrap();
    let read = e.remote_radio_read(connection, Instant::now()).unwrap();
    e.remote_observe_cat(Some(&read), Some(true));
    e.remote_observe_dial(Some(&read), Some(14_275_000));
    e.remote_observe_mode(Some(&read), Some(mode));
    e.remote_observe_ptt(Some(&read), Some(false));
}

#[test]
fn remote_phone_mode_admission_requires_v3_and_readback_without_persistence_or_replay() {
    for (before, after, target) in [
        ("auto", "USB", "USB"),
        ("auto", "LSB", "LSB"),
        ("LSB", "auto", "USB"),
        ("USB", "USB", "USB"),
        ("auto", "FM", "FM"),
        ("FM", "auto", "USB"),
    ] {
        let (f, connection) = station("phone", 2400);
        {
            let mut e = f.engine.lock().unwrap();
            e.request_sideband_override((before != "auto").then_some(before));
            e.take_immediate_retune();
        }
        let file = std::fs::read(f.dir.join("settings.json")).unwrap();
        let state = acquire_controls_version(&f, Instant::now(), 3);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("phoneMode")));
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("fmTuning")));
        sample(&f, &connection, "LSB");
        let command = control_request(
            &state,
            json!({"action":"radio.phoneMode","expectedMode":before,"mode":after}),
        );
        assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        let pending = run(&f, 3, &command).unwrap();
        assert_eq!(pending["outcome"], "pending");
        assert_eq!(run(&f, 3, &command).unwrap(), pending);
        let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
        assert_eq!(work.expected(), (14_275_000, "LSB"));
        assert_eq!(work.target(), (14_275_000, target));
        sample(&f, &connection, target);
        let power = work.power_limit();
        // Admission/receipt recovery uses an explicit worker readback here;
        // the actual CAT transaction is covered by the audio owner tests.
        assert!(work.commit_tuning_readback(
            &mut f.engine.lock().unwrap(),
            power,
            (target == "FM").then_some(("simplex", 0, 0.0))
        ));
        let applied = run(&f, 3, &command).unwrap();
        assert_eq!(applied["outcome"], "applied");
        assert_eq!(applied["evidence"], "radioReadback");
        assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), file);
        {
            let mut e = f.engine.lock().unwrap();
            assert_eq!(
                e.snapshot().radio.sideband_override.as_deref(),
                (after != "auto").then_some(after)
            );
            assert!(!e.take_immediate_retune());
            assert!(!e.tx_enabled());
            e.request_sideband_override(None);
            e.take_immediate_retune();
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
        assert!(e.take_remote_radio().is_none());
        assert_eq!(e.snapshot().radio.sideband_override, None);
    }
}

#[test]
fn remote_phone_mode_refuses_logging_only_authority_and_malformed_or_stale_choices() {
    let (f, connection) = station("phone", 2400);
    let action = json!({"action":"radio.phoneMode","expectedMode":"auto","mode":"LSB"});
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(&f, 3, &control_request(&state, action.clone())),
        Err("localPermissionRequired")
    );
    for (field, value, reason) in [
        ("mode", "CW", "invalidAction"),
        ("mode", "AM", "invalidAction"),
        ("mode", "usb", "invalidAction"),
        ("mode", "USB\nT 1", "invalidAction"),
        ("expectedMode", "USB", "contextChanged"),
    ] {
        let state = acquire_controls_version(&f, Instant::now(), 3);
        sample(&f, &connection, "LSB");
        let mut bad = action.clone();
        bad[field] = json!(value);
        assert_eq!(
            run(&f, 3, &control_request(&state, bad)).unwrap()["reason"],
            reason
        );
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
    }
    for field in ["mode", "expectedMode"] {
        let mut bad = action.clone();
        bad.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<station::Action>(bad).is_err());
    }
    for field in [
        "command",
        "settings",
        "txEnabled",
        "dialMhz",
        "sideband",
        "radioId",
    ] {
        let mut bad = action.clone();
        bad[field] = json!(true);
        assert!(serde_json::from_value::<station::Action>(bad).is_err());
    }
}
