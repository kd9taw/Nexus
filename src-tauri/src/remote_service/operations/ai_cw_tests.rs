//! Remote parity batch 1: the AI-CW switch and the FT Decode button. Neither may arm or key
//! anything, and each travels only under its own v3 capability.
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

fn saved(f: &Fixture) -> tempo_app::settings::Settings {
    serde_json::from_slice(&std::fs::read(f.dir.join("settings.json")).unwrap()).unwrap()
}

#[test]
fn ai_cw_needs_v3_saves_once_and_a_replay_never_mutates_again() {
    let f = station(Tier::Ft8);
    let current = f.engine.lock().unwrap().settings().ai_cw_enabled;
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let capabilities = state["controls"]["capabilities"].as_array().unwrap();
    assert!(capabilities.contains(&json!("aiCw")));
    assert!(capabilities.contains(&json!("redecode")));
    let command = control_request(
        &state,
        json!({"action":"decoder.aiCw","expectedOn":current,"on":!current}),
    );
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    assert!(!f.dir.join("settings.json").exists());
    let applied = run(&f, 3, &command).unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["evidence"], "settingsSaved");
    assert_eq!(saved(&f).ai_cw_enabled, !current);
    {
        let mut e = f.engine.lock().unwrap();
        assert_eq!(e.settings().ai_cw_enabled, !current);
        assert_eq!(e.snapshot().ai_cw.enabled, !current);
        assert!(!e.tx_enabled());
        assert!(e.take_remote_radio().is_none());
        // A later local choice must survive a replay of the remote one.
        e.set_ai_cw_enabled(current);
    }
    let bytes = std::fs::read(f.dir.join("settings.json")).unwrap();
    assert_eq!(run(&f, 3, &command).unwrap(), applied);
    assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), bytes);
    let e = f.engine.lock().unwrap();
    assert_eq!(e.settings().ai_cw_enabled, current);
    assert!(!e.tx_enabled());
}

#[test]
fn ai_cw_refuses_a_stale_or_unchanged_choice_and_a_logging_only_grant() {
    for (stale, reason) in [(true, "contextChanged"), (false, "invalidAction")] {
        let f = station(Tier::Ft8);
        let current = f.engine.lock().unwrap().settings().ai_cw_enabled;
        let state = acquire_controls_version(&f, Instant::now(), 3);
        let action = if stale {
            json!({"action":"decoder.aiCw","expectedOn":!current,"on":current})
        } else {
            json!({"action":"decoder.aiCw","expectedOn":current,"on":current})
        };
        let result = run(&f, 3, &control_request(&state, action)).unwrap();
        assert_eq!(result["outcome"], "rejected");
        assert_eq!(result["reason"], reason);
        assert!(!f.dir.join("settings.json").exists());
        let e = f.engine.lock().unwrap();
        assert_eq!(e.settings().ai_cw_enabled, current);
        assert!(!e.tx_enabled());
    }
    let f = station(Tier::Ft8);
    f.acquire(Instant::now());
    let state = control_state_version(&f, Instant::now(), 3);
    let action = json!({"action":"decoder.aiCw","expectedOn":false,"on":true});
    assert_eq!(
        run(&f, 3, &control_request(&state, action.clone())),
        Err("localPermissionRequired")
    );
    assert!(!f.dir.join("settings.json").exists());
    assert!(serde_json::from_value::<station::Action>(action.clone()).is_ok());
    for field in ["settings", "command", "txEnabled", "radioId", "model"] {
        let mut invalid = action.clone();
        invalid[field] = json!(true);
        assert!(serde_json::from_value::<station::Action>(invalid).is_err());
    }
    let redecode = json!({"action":"decoder.redecode","expectedTier":"FT8"});
    assert!(serde_json::from_value::<station::Action>(redecode.clone()).is_ok());
    for field in ["depth", "tier", "txEnabled", "slot"] {
        let mut invalid = redecode.clone();
        invalid[field] = json!(1);
        assert!(serde_json::from_value::<station::Action>(invalid).is_err());
    }
}

#[test]
fn redecode_is_receive_only_and_refused_while_transmit_is_enabled() {
    // Positive control: an idle FT8 station accepts the redecode.
    let f = station(Tier::Ft8);
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let command = control_request(
        &state,
        json!({"action":"decoder.redecode","expectedTier":"FT8"}),
    );
    assert_eq!(run(&f, 2, &command), Err("stationUnsupported"));
    let applied = run(&f, 3, &command).unwrap();
    assert_eq!(applied["outcome"], "applied");
    assert_eq!(applied["evidence"], "receiverState");
    {
        let mut e = f.engine.lock().unwrap();
        assert!(!e.tx_enabled());
        assert!(e.take_remote_radio().is_none());
    }

    let f = station(Tier::Ft8);
    f.engine.lock().unwrap().set_tx_enabled(true);
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let busy = run(
        &f,
        3,
        &control_request(
            &state,
            json!({"action":"decoder.redecode","expectedTier":"FT8"}),
        ),
    )
    .unwrap();
    assert_eq!(busy["outcome"], "rejected");
    assert_eq!(busy["reason"], "stationBusy");
    assert!(
        f.engine.lock().unwrap().tx_enabled(),
        "the TX latch is left exactly as it was"
    );

    let f = station(Tier::Ft8);
    let state = acquire_controls_version(&f, Instant::now(), 3);
    let other = run(
        &f,
        3,
        &control_request(
            &state,
            json!({"action":"decoder.redecode","expectedTier":"FT4"}),
        ),
    )
    .unwrap();
    assert_eq!(other["outcome"], "rejected");
    assert_eq!(other["reason"], "contextChanged");
    assert!(!f.engine.lock().unwrap().tx_enabled());
}
