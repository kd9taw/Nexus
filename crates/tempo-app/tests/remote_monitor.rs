use tempo_app::dto::AmpStatusDto;
use tempo_app::engine::Engine;
use tempo_app::settings::Settings;

fn station() -> Engine {
    let mut settings = Settings {
        mycall: "N0CALL".into(),
        mygrid: "AA00".into(),
        amp_model: "spe".into(),
        amp_port: "test-only".into(),
        amp_follow_band: true,
        ..Default::default()
    };
    settings.ensure_radio_profiles();
    Engine::with_settings(settings)
}

#[test]
fn waiting_first_miss_loss_recovery_and_radio_handoff_keep_the_amp_association() {
    let mut engine = station();
    let id = engine.settings().active_radio;
    let waiting = engine.remote_monitor_observation().amplifier.unwrap();
    assert_eq!(waiting.family, "spe");
    assert!(!waiting.linked);
    assert_eq!(waiting.operate, None);
    let live = AmpStatusDto {
        family: "spe".into(),
        model: "13K".into(),
        linked: true,
        operate: Some(true),
        output_watts: Some(900),
        temp: Some(41),
        alarm: "unknown".into(),
        alarm_raised: true,
        ..Default::default()
    };
    engine.observe_amp_status(id, live.clone());
    let before = engine.remote_monitor_observation();
    assert_eq!(before.amplifier.as_ref().unwrap().output_watts, Some(900));
    assert!(before.amplifier.unwrap().alarm_raised);
    engine.observe_amp_miss(id, "spe", "noAnswer");
    let missed = engine.remote_monitor_observation().amplifier.unwrap();
    assert!(missed.linked);
    assert_eq!(missed.reason, "noAnswer");
    assert_eq!(missed.output_watts, None);
    assert_eq!(missed.operate, None);
    for _ in 0..2 {
        engine.observe_amp_miss(id, "spe", "noAnswer");
    }
    assert!(
        !engine
            .remote_monitor_observation()
            .amplifier
            .unwrap()
            .linked
    );
    engine.observe_amp_status(id, live);
    assert!(
        engine
            .remote_monitor_observation()
            .amplifier
            .unwrap()
            .linked
    );
    let other = engine.add_radio();
    engine.set_active_radio(other);
    let changed = engine.remote_monitor_observation();
    assert_eq!(changed.radio.id, other);
    assert!(changed.amplifier.is_none());
    engine.set_active_radio(id);
    let returned = engine.remote_monitor_observation();
    assert_eq!(returned.radio.id, id);
    assert_eq!(returned.amplifier.unwrap().family, "spe");
}

#[test]
fn observations_preserve_state_and_have_a_fixed_small_shape() {
    let engine = station();
    let before = serde_json::to_value(engine.snapshot()).unwrap();
    let started = std::time::Instant::now();
    for _ in 0..10_000 {
        std::hint::black_box(engine.remote_monitor_observation());
    }
    eprintln!(
        "10,000 bounded observer projections: {:?}",
        started.elapsed()
    );
    assert_eq!(serde_json::to_value(engine.snapshot()).unwrap(), before);
    let value = serde_json::to_value(engine.remote_monitor_observation()).unwrap();
    let keys: Vec<_> = value
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(keys, ["amplifier", "call", "grid", "radio"]);
    let json = serde_json::to_string(&value).unwrap();
    assert!(!json.contains("test-only"));
    assert!(!json.contains("password"));
    assert!(json.len() < 2048);
}
