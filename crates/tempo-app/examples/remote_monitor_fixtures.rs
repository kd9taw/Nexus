//! Explicit observer preview fixtures, serialized by the same Rust contract as native IPC.
//! Run with --check in verification; without it, print the refreshed JSON to stdout.
use std::collections::BTreeMap;
use tempo_app::dto::AmpStatusDto;
use tempo_app::engine::Engine;
use tempo_app::remote_monitor::{Frame, Source, VERSION};
use tempo_app::settings::Settings;

fn main() {
    let mut settings = Settings {
        mycall: "N0CALL".into(),
        mygrid: "AA00".into(),
        dial_mhz: 14.074,
        band: "20m".into(),
        amp_model: "spe".into(),
        amp_port: "fixture-only".into(),
        amp_follow_band: true,
        ..Default::default()
    };
    settings.ensure_radio_profiles();
    let mut engine = Engine::with_settings(settings);
    engine.set_cat_status(Some(true), String::new());
    let id = engine.settings().active_radio;
    let mut fixtures = BTreeMap::new();
    let mut capture = |name: &str, engine: &Engine| {
        fixtures.insert(
            name.to_string(),
            Frame {
                version: VERSION,
                source: Source::Fixture,
                epoch: "example-station-v1".into(),
                sequence: 1,
                generated_at_ms: 1_788_768_000_000,
                station: engine.remote_monitor_observation(),
            },
        );
    };
    capture("waiting", &engine);
    let spe = AmpStatusDto {
        family: "spe".into(),
        model: "13K".into(),
        linked: true,
        operate: Some(true),
        transmitting: Some(false),
        output_watts: Some(0),
        band_label: Some("20m".into()),
        swr: None,
        swr_atu: None,
        volts: Some(48.0),
        amps: Some(0.0),
        temp: Some(41),
        temp_celsius: false,
        alarm: "none".into(),
        warning: "none".into(),
        ..Default::default()
    };
    engine.observe_amp_status(id, spe.clone());
    capture("spe", &engine);
    engine.observe_amp_miss(id, "spe", "noAnswer");
    capture("firstMiss", &engine);
    engine.observe_amp_miss(id, "spe", "noAnswer");
    engine.observe_amp_miss(id, "spe", "noAnswer");
    capture("ampLost", &engine);
    engine.observe_amp_status(
        id,
        AmpStatusDto {
            alarm: "unknown".into(),
            alarm_raised: true,
            warning: "powerLimitExceeded".into(),
            warning_raised: true,
            ..spe.clone()
        },
    );
    capture("fault", &engine);
    engine.observe_amp_status(
        id,
        AmpStatusDto {
            alarm: "swrExceedingLimits".into(),
            alarm_raised: true,
            ..spe
        },
    );
    capture("knownFault", &engine);
    let mut kpa_settings = engine.settings().clone();
    kpa_settings.amp_model = "kpa".into();
    kpa_settings.radios[0].amp_model = "kpa".into();
    engine = Engine::with_settings(kpa_settings);
    engine.set_cat_status(Some(true), String::new());
    engine.observe_amp_status(
        id,
        AmpStatusDto {
            family: "kpa".into(),
            linked: true,
            operate: Some(false),
            output_watts: Some(0),
            band_label: Some("20m".into()),
            temp: Some(52),
            temp_celsius: true,
            kpa_fault: Some(0),
            ..Default::default()
        },
    );
    capture("kpa", &engine);
    engine.set_cat_status(Some(false), "fixture link loss".into());
    capture("catLost", &engine);
    let other = engine.add_radio();
    engine.set_active_radio(other);
    capture("noAmp", &engine);
    let output = serde_json::to_string_pretty(&fixtures).unwrap() + "\n";
    if std::env::args().any(|arg| arg == "--check") {
        assert_eq!(
            output,
            include_str!("../../../ui/src/remote-monitor/fixtures.v1.json"),
            "regenerate remote monitor fixtures with this example"
        );
    } else {
        print!("{output}");
    }
}
