use super::receiver_filter::{run, sample, station};
use super::*;

#[test]
fn remote_levels_host_requires_v3_and_physical_confirmation_with_stable_result_identity() {
    for (name, before, target) in [
        ("power", 0.5, 0.35),
        ("micGain", 0.5, 0.35),
        ("nr", 0.5, 0.35),
        ("compression", 0.5, 0.35),
        ("notch", 600.0, 1500.0),
    ] {
        let (f, connection) = station("phone", 2400);
        {
            let mut e = f.engine.lock().unwrap();
            match name {
                "power" => e.observe_rig_power(before),
                "micGain" => e.observe_rig_mic_gain(before),
                "nr" => e.observe_rig_nr_level(before),
                "compression" => e.observe_rig_comp_level(before),
                _ => e.observe_rig_notch_freq_hz(before),
            }
        }
        let bytes = std::fs::read(f.dir.join("settings.json")).unwrap();
        let state = acquire_controls_version(&f, Instant::now(), 3);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("radioLevels")));
        let command = control_request(
            &state,
            json!({"action":"radio.level","mode":"phone","level":name,"expected":before,"value":target}),
        );
        assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        assert_eq!(run(&f, 3, &command).unwrap()["outcome"], "pending");
        let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
        assert_eq!(run(&f, 3, &command).unwrap()["outcome"], "pending");
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        sample(&f, &connection, "phone");
        assert!(work.commit_level_readback(&mut f.engine.lock().unwrap(), Some(target)));
        let result = run(&f, 3, &command).unwrap();
        assert_eq!(result["outcome"], "applied");
        assert_eq!(result["evidence"], "radioReadback");
        f.engine.lock().unwrap().set_mic_gain(0.8);
        assert_eq!(run(&f, 3, &command).unwrap(), result);
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
        assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), bytes);
        assert!(!f.engine.lock().unwrap().tx_enabled());
    }
}

#[test]
fn remote_levels_host_refuses_logging_only_authority_and_open_command_fields() {
    let (f, connection) = station("phone", 2400);
    let action = json!({"action":"radio.level","mode":"phone","level":"micGain","expected":0.5,"value":0.35});
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(&f, 3, &control_request(&state, action.clone())),
        Err("localPermissionRequired")
    );
    for field in ["settings", "command", "txEnabled", "radioId"] {
        let mut bad = action.clone();
        bad[field] = json!(true);
        assert!(serde_json::from_value::<station::Action>(bad).is_err());
    }
    for name in ["VOX", "TUNER", "MICGAIN\nT 1", ""] {
        let state = acquire_controls_version(&f, Instant::now(), 3);
        sample(&f, &connection, "phone");
        let mut bad = action.clone();
        bad["level"] = json!(name);
        assert_eq!(
            run(&f, 3, &control_request(&state, bad)).unwrap()["reason"],
            "invalidAction"
        );
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
    }
}
