use super::receiver_filter::{run, sample, station};
use super::*;
use tempo_app::engine::remote_radio::{AgcSpeed, ReceiverDsp, ReceiverFunction};

#[test]
fn receiver_dsp_requires_v3_and_actual_readback_and_never_replays_a_completed_gesture() {
    for mode in ["cw", "phone"] {
        let mut choices: Vec<_> = ["nb", "nr", "notch", "manualNotch"].into_iter().map(|func|
            (json!({"action":"radio.function","mode":mode,"func":func,"expectedOn":false,"on":true}),
             ReceiverDsp::Function { func: ReceiverFunction::from_name(func).unwrap(), on: true })).collect();
        choices.extend(["auto", "fast", "mid", "slow", "off"].map(|speed| {
            (
                json!({"action":"radio.agc","mode":mode,"expectedSpeed":"fast","speed":speed}),
                ReceiverDsp::Agc(AgcSpeed::from_name(speed).unwrap()),
            )
        }));
        for (action, after) in choices {
            let (f, connection) = station(mode, 500);
            {
                let mut e = f.engine.lock().unwrap();
                e.observe_rig_funcs([Some(false); 6]);
                e.observe_rig_agc("fast".into());
            }
            let file = std::fs::read(f.dir.join("settings.json")).unwrap();
            let state = acquire_controls_version(&f, Instant::now(), 3);
            assert!(state["controls"]["capabilities"]
                .as_array()
                .unwrap()
                .contains(&json!("receiverDsp")));
            let command = control_request(&state, action);
            assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
            assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
            let pending = run(&f, 3, &command).unwrap();
            assert_eq!(pending["outcome"], "pending");
            assert_eq!(run(&f, 3, &command).unwrap(), pending);
            let work = f.engine.lock().unwrap().take_remote_radio().unwrap();
            assert_eq!(work.receiver_dsp().unwrap().1, after);
            sample(&f, &connection, mode);
            assert!(work.commit_receiver_dsp_readback(&mut f.engine.lock().unwrap(), Some(after)));
            let applied = run(&f, 3, &command).unwrap();
            assert_eq!(applied["outcome"], "applied");
            assert_eq!(applied["evidence"], "radioReadback");
            {
                let mut e = f.engine.lock().unwrap();
                // A completed duplicate must not undo a later local pick.
                e.observe_rig_funcs([Some(false); 6]);
                e.observe_rig_agc("off".into());
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
            assert!(e.take_func_requests().iter().all(Option::is_none));
            assert!(!e.agc_to_command().is_some_and(|(_, picked)| picked));
            assert_eq!(e.snapshot().radio.nb, Some(false));
            assert_eq!(e.snapshot().radio.agc.as_deref(), Some("off"));
            assert!(!e.tx_enabled());
            assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), file);
        }
    }
}

#[test]
fn receiver_dsp_refuses_keying_functions_wrong_context_and_logging_only_grants() {
    let (f, connection) = station("phone", 2400);
    {
        let mut e = f.engine.lock().unwrap();
        e.observe_rig_funcs([Some(false); 6]);
        e.observe_rig_agc("fast".into());
    }
    let action =
        json!({"action":"radio.function","mode":"phone","func":"nb","expectedOn":false,"on":true});
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    assert_eq!(
        run(&f, 3, &control_request(&state, action.clone())),
        Err("localPermissionRequired")
    );
    for (field, value, reason) in [
        ("func", json!("vox"), "invalidAction"),
        ("func", json!("comp"), "invalidAction"),
        ("func", json!("tuner"), "invalidAction"),
        ("func", json!("nb\nT 1"), "invalidAction"),
        ("mode", json!("cw"), "contextChanged"),
        ("expectedOn", json!(true), "contextChanged"),
    ] {
        sample(&f, &connection, "phone");
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let mut bad = action.clone();
        bad[field] = value;
        assert_eq!(
            run(&f, 3, &control_request(&state, bad)).unwrap()["reason"],
            reason
        );
        assert!(f.engine.lock().unwrap().take_remote_radio().is_none());
    }
    for action in [
        action,
        json!({"action":"radio.agc","mode":"phone","expectedSpeed":"fast","speed":"slow"}),
    ] {
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
}
