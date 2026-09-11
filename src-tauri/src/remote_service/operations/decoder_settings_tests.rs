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
fn cases() -> [(Tier, Value); 2] {
    [
        (
            Tier::Js8,
            json!({"action":"decoder.js8Speed","expectedSpeed":1,"speed":3}),
        ),
        (
            Tier::Msk144,
            json!({"action":"decoder.msk144Period","expectedPeriodSecs":15,"periodSecs":5}),
        ),
    ]
}

#[test]
fn decoder_choices_need_v3_and_recover_the_saved_receipt_without_a_second_mutation() {
    for (tier, action) in cases() {
        let f = station(tier);
        let state = acquire_controls_version(&f, Instant::now(), 3);
        assert!(state["controls"]["capabilities"]
            .as_array()
            .unwrap()
            .contains(&json!("decoderSettings")));
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
        if tier == Tier::Js8 {
            assert_eq!(
                serde_json::to_value(e.js8_state().speed).unwrap(),
                json!("turbo")
            );
        } else {
            assert_eq!(e.settings().msk144_period_s, 5);
        }
    }
}

#[test]
fn local_decoder_change_invalidates_an_already_displayed_command() {
    for (tier, action) in cases() {
        let f = station(tier);
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let command = control_request(&state, action);
        {
            let mut e = f.engine.lock().unwrap();
            if tier == Tier::Js8 {
                e.js8_set_speed(0).unwrap();
            } else {
                e.set_msk144_period(30);
            }
        }
        assert_eq!(run(&f, 3, &command), Err("staleContext"));
        assert!(!f.dir.join("settings.json").exists());
        let fresh = control_state_version(&f, Instant::now(), 3);
        let command = control_request(
            &fresh,
            cases().into_iter().find(|(t, _)| *t == tier).unwrap().1,
        );
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
