use super::*;
use tempo_app::dto::Tier;

fn station(tier: Tier) -> Fixture {
    let f = Fixture::new();
    let mut e = f.engine.lock().unwrap();
    e.configure_remote_settings_store(f.dir.join("settings.json"));
    e.set_tier(tier);
    e.take_immediate_retune();
    e.set_tx_enabled(false);
    let connection = e.remote_open_radio().unwrap();
    let read = e.remote_radio_read(&connection, Instant::now()).unwrap();
    e.remote_observe_cat(Some(&read), Some(true));
    e.remote_observe_ptt(Some(&read), Some(false));
    drop(e);
    f
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
fn cases() -> [(Tier, Value); 4] {
    [
        (
            Tier::Js8,
            json!({"action":"decoder.js8Speed","expectedSpeed":1,"speed":3}),
        ),
        (
            Tier::Msk144,
            json!({"action":"decoder.msk144Period","expectedPeriodSecs":15,"periodSecs":5}),
        ),
        (
            Tier::Ft8,
            json!({"action":"decoder.depth","expectedTier":"FT8","expectedDepth":3,"depth":1}),
        ),
        (
            Tier::Ft8,
            json!({"action":"receiver.rxOffset","expectedTier":"FT8","expectedHz":1500.0,"hz":725.25}),
        ),
    ]
}

#[test]
fn decoder_choices_need_v3_and_recover_the_saved_receipt_without_a_second_mutation() {
    for (tier, action) in cases() {
        let name = action["action"].as_str().unwrap().to_owned();
        let f = station(tier);
        let state = acquire_controls_version(&f, Instant::now(), 3);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!(
                if matches!(name.as_str(), "decoder.depth" | "receiver.rxOffset") {
                    "receiverSettings"
                } else {
                    "decoderSettings"
                }
            )));
        let command = control_request(&state, action);
        assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
        assert!(!f.dir.join("settings.json").exists());
        let saved = run(&f, 3, &command).unwrap();
        assert_eq!(saved["outcome"], "applied");
        assert_eq!(saved["evidence"], "settingsSaved");
        let bytes = std::fs::read(f.dir.join("settings.json")).unwrap();
        let generation = f
            .engine
            .lock()
            .unwrap()
            .remote_actuation_context_generation();
        assert_eq!(run(&f, 3, &command).unwrap(), saved);
        let lookup = Request::Result {
            request_id: id(),
            operation_id: command.id().into(),
        };
        assert_eq!(run(&f, 2, &lookup).unwrap(), saved);
        assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), bytes);
        assert_eq!(
            f.engine
                .lock()
                .unwrap()
                .remote_actuation_context_generation(),
            generation
        );
        f.authority.permit_station(DEVICE, false).unwrap();
        let mut e = f.engine.lock().unwrap();
        assert!(!e.tx_enabled());
        assert!(e.take_remote_radio().is_none());
        assert!(!e.take_immediate_retune());
        match name.as_str() {
            "decoder.js8Speed" => assert_eq!(
                serde_json::to_value(e.js8_state().speed).unwrap(),
                json!("turbo")
            ),
            "decoder.msk144Period" => assert_eq!(e.settings().msk144_period_s, 5),
            "decoder.depth" => assert_eq!(e.settings().decode_depth, 1),
            _ => {
                assert_eq!(e.rx_offset_hz(), 725.25);
                assert_eq!(e.tx_offset_hz(), 1500.0);
            }
        }
    }
}

#[test]
fn local_decoder_change_invalidates_an_already_displayed_command() {
    for (tier, action) in cases() {
        let name = action["action"].as_str().unwrap().to_owned();
        let f = station(tier);
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let command = control_request(&state, action.clone());
        {
            let mut e = f.engine.lock().unwrap();
            match name.as_str() {
                "decoder.js8Speed" => e.js8_set_speed(0).unwrap(),
                "decoder.msk144Period" => e.set_msk144_period(30),
                "decoder.depth" => e.set_decode_depth(2),
                _ => e.set_rx_offset(900.0),
            }
        }
        assert_eq!(run(&f, 3, &command), Err("staleContext"));
        assert!(!f.dir.join("settings.json").exists());
        let fresh = control_state_version(&f, Instant::now(), 3);
        let command = control_request(&fresh, action);
        let refusal = run(&f, 3, &command).unwrap();
        assert_eq!(refusal["outcome"], "rejected");
        assert_eq!(
            refusal["reason"], "contextChanged",
            "a fresh lease cannot bless a stale displayed value"
        );
        assert!(!f.dir.join("settings.json").exists());
    }
}

#[test]
fn decoder_settings_do_not_accept_extra_native_arguments_or_a_logging_grant() {
    for (_, mut action) in cases() {
        for extra in ["settings", "command", "txEnabled", "radioId"] {
            action[extra] = json!(true);
            assert!(serde_json::from_value::<station::Action>(action.clone()).is_err());
            action.as_object_mut().unwrap().remove(extra);
        }
    }
    for (tier, action) in cases() {
        let f = station(tier);
        f.acquire(Instant::now());
        let state = control_state_version(&f, Instant::now(), 3);
        let command = control_request(&state, action);
        assert_eq!(run(&f, 3, &command), Err("localPermissionRequired"));
        assert!(!f.dir.join("settings.json").exists());
    }
}
