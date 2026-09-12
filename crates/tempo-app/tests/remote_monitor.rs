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
fn legacy_amp_cache_cannot_certify_a_current_remote_measurement() {
    let mut engine = station();
    let id = engine.settings().active_radio;
    engine.observe_amp_status(
        id,
        AmpStatusDto {
            family: "spe".into(),
            linked: true,
            output_watts: Some(900),
            ..Default::default()
        },
    );
    let other = engine.add_radio();
    engine.set_active_radio(other);
    engine.set_active_radio(id);
    assert_eq!(
        engine
            .remote_monitor_observation()
            .amplifier
            .unwrap()
            .output_watts,
        None,
        "an unattributed cached poll must not become a fresh reading after switching back"
    );
}

#[test]
fn amp_read_failure_and_recovery_preserve_the_desktop_cache_and_remote_association() {
    let mut engine = station();
    let id = engine.settings().active_radio;
    assert!(
        !engine
            .remote_monitor_observation()
            .amplifier
            .unwrap()
            .linked
    );
    let connection = engine.remote_open_amp().unwrap();
    let now = std::time::Instant::now();
    let good = AmpStatusDto {
        family: "spe".into(),
        model: "13K".into(),
        linked: true,
        output_watts: Some(900),
        operate: Some(true),
        ..Default::default()
    };
    let first = engine.remote_amp_read(&connection, now).unwrap();
    engine.observe_amp_status(id, good.clone());
    engine.remote_observe_amp(Some(&first), good.clone());
    assert_eq!(
        engine
            .remote_monitor_observation_at(now)
            .amplifier
            .unwrap()
            .output_watts,
        Some(900)
    );
    let miss = engine.remote_amp_read(&connection, now).unwrap();
    engine.observe_amp_miss(id, "spe", "noAnswer");
    engine.remote_observe_amp(
        Some(&miss),
        AmpStatusDto {
            family: "spe".into(),
            reason: "noAnswer".into(),
            ..Default::default()
        },
    );
    assert!(
        engine.amp_live(id).unwrap().linked,
        "desktop retains its existing debounce"
    );
    let unavailable = engine.remote_monitor_observation_at(now).amplifier.unwrap();
    assert!(!unavailable.linked);
    assert_eq!(unavailable.output_watts, None);
    engine.remote_observe_amp(Some(&first), good.clone());
    assert_eq!(
        engine
            .remote_monitor_observation_at(now)
            .amplifier
            .unwrap()
            .output_watts,
        None
    );
    let fresh = engine.remote_amp_read(&connection, now).unwrap();
    engine.remote_observe_amp(Some(&fresh), good);
    assert_eq!(
        engine
            .remote_monitor_observation_at(now)
            .amplifier
            .unwrap()
            .output_watts,
        Some(900)
    );
    let other = engine.add_radio();
    engine.set_active_radio(other);
    assert!(engine.remote_monitor_observation().amplifier.is_none());
    engine.set_active_radio(id);
    assert!(engine.remote_amp_read(&connection, now).is_none());
    assert_eq!(
        engine
            .remote_monitor_observation()
            .amplifier
            .unwrap()
            .output_watts,
        None
    );
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

#[test]
fn unattributed_legacy_readbacks_are_unknown_including_across_radio_handoff() {
    let mut engine = station();
    engine.set_cat_status(Some(true), String::new());
    engine.observe_rig_mode("USB".into());
    engine.observe_rig_ptt(true);
    let before = engine.remote_monitor_observation();
    // These legacy mirrors are available to the desktop but have no producer
    // identity. Reporting them as this radio's measurements would guess provenance.
    assert_eq!(before.radio.cat_connected, None);
    assert_eq!(before.radio.rig_mode, None);
    assert_eq!(before.radio.rig_keyed, None);
    let second = engine.add_radio();
    engine.set_active_radio(second);
    let changed = engine.remote_monitor_observation();
    assert_eq!(changed.radio.id, second);
    assert_eq!(changed.radio.cat_connected, None);
    assert_eq!(changed.radio.rig_mode, None);
    assert_eq!(changed.radio.rig_keyed, None);
    engine.set_cat_status(Some(true), String::new());
    engine.observe_rig_mode("FM".into());
    let mode_only = engine.remote_monitor_observation();
    assert_eq!(mode_only.radio.rig_mode, None);
    assert_eq!(
        mode_only.radio.rig_keyed, None,
        "a successful CAT open is not a PTT reading"
    );
    engine.observe_rig_ptt(false);
    assert_eq!(engine.remote_monitor_observation().radio.rig_keyed, None);
}

#[test]
fn transport_read_age_is_independent_of_publication_and_fields_expire_independently() {
    use std::time::{Duration, Instant};
    use tempo_app::remote_monitor::{MEASUREMENT_STALE_MS, MODE_STALE_MS};
    let mut engine = station();
    let now = Instant::now();
    let connection = engine.remote_open_radio().unwrap();
    let read = engine.remote_radio_read(&connection, now).unwrap();
    engine.remote_observe_cat(Some(&read), Some(true));
    engine.remote_observe_dial(Some(&read), Some(14_074_000));
    engine.remote_observe_mode(Some(&read), Some("USB"));
    engine.remote_observe_ptt(Some(&read), Some(false));
    let before = serde_json::to_value(engine.snapshot()).unwrap();
    let frame = engine.remote_monitor_observation_at(now + Duration::from_millis(800));
    assert_eq!(frame.radio.rig_dial_mhz, Some(14.074));
    assert_eq!(frame.radio.rig_keyed, Some(false));
    assert_eq!(frame.radio.readings.ptt.unwrap().age_ms, 800);
    let next = engine
        .remote_radio_read(&connection, now + Duration::from_millis(2000))
        .unwrap();
    engine.remote_observe_cat(Some(&next), Some(true));
    let stale =
        engine.remote_monitor_observation_at(now + Duration::from_millis(MEASUREMENT_STALE_MS));
    assert_eq!(stale.radio.cat_connected, Some(true));
    assert_eq!(
        stale.radio.rig_keyed, None,
        "a new CAT answer cannot renew PTT"
    );
    assert_eq!(stale.radio.rig_dial_mhz, None);
    assert_eq!(stale.radio.rig_mode.as_deref(), Some("USB"));
    assert_eq!(
        engine
            .remote_monitor_observation_at(now + Duration::from_millis(MODE_STALE_MS))
            .radio
            .rig_mode,
        None
    );
    assert_eq!(
        serde_json::to_value(engine.snapshot()).unwrap(),
        before,
        "remote evidence cannot change desktop or TX state"
    );
}

#[test]
fn delayed_old_reads_fail_after_away_back_settings_port_change_reconnect_and_engine_restart() {
    let mut engine = station();
    let now = std::time::Instant::now();
    let id = engine.settings().active_radio;
    for transition in 0..5 {
        let connection = engine.remote_open_radio().unwrap();
        let blocked = engine.remote_radio_read(&connection, now).unwrap();
        let amp_connection = engine.remote_open_amp().unwrap();
        let blocked_amp = engine.remote_amp_read(&amp_connection, now).unwrap();
        match transition {
            0 => {
                let other = engine.add_radio();
                engine.set_active_radio(other);
                engine.set_active_radio(id);
            }
            1 => {
                let mut s = engine.settings().clone();
                s.serial_port = "changed-test-port".into();
                engine.apply_settings(s);
            }
            2 => {
                let mut s = engine.settings().clone();
                s.amp_port = "changed-amp-port".into();
                engine.apply_settings(s);
            }
            3 => {
                engine.remote_close_radio();
                engine.remote_open_amp();
            }
            _ => {
                engine = station();
            }
        }
        let fresh_connection = engine.remote_open_radio().unwrap();
        let fresh_amp = engine.remote_open_amp().unwrap();
        engine.remote_observe_cat(Some(&blocked), Some(true));
        engine.remote_observe_mode(Some(&blocked), Some("FM"));
        engine.remote_observe_ptt(Some(&blocked), Some(true));
        engine.remote_observe_amp(
            Some(&blocked_amp),
            AmpStatusDto {
                family: "spe".into(),
                linked: true,
                output_watts: Some(900),
                ..Default::default()
            },
        );
        let rejected = engine.remote_monitor_observation_at(now);
        assert_eq!(
            rejected.radio.cat_connected, None,
            "transition {transition}"
        );
        assert_eq!(rejected.radio.rig_mode, None);
        assert_eq!(rejected.radio.rig_keyed, None);
        assert_eq!(rejected.amplifier.unwrap().output_watts, None);
        let fresh = engine.remote_radio_read(&fresh_connection, now).unwrap();
        engine.remote_observe_cat(Some(&fresh), Some(true));
        assert_eq!(
            engine
                .remote_monitor_observation_at(now)
                .radio
                .cat_connected,
            Some(true)
        );
        let fresh = engine.remote_amp_read(&fresh_amp, now).unwrap();
        engine.remote_observe_amp(
            Some(&fresh),
            AmpStatusDto {
                family: "spe".into(),
                linked: true,
                output_watts: Some(100),
                ..Default::default()
            },
        );
        assert_eq!(
            engine
                .remote_monitor_observation_at(now)
                .amplifier
                .unwrap()
                .output_watts,
            Some(100)
        );
    }
}

#[test]
fn unsupported_failed_and_slow_reads_never_resurrect_old_successes() {
    let mut engine = station();
    let now = std::time::Instant::now();
    let connection = engine.remote_open_radio().unwrap();
    let older = engine.remote_radio_read(&connection, now).unwrap();
    let latest = engine.remote_radio_read(&connection, now).unwrap();
    engine.remote_observe_ptt(Some(&latest), None);
    engine.remote_observe_ptt(Some(&older), Some(false));
    engine.remote_observe_mode(Some(&latest), Some("USB"));
    engine.remote_observe_mode(Some(&older), Some("FM"));
    assert_eq!(
        engine.remote_monitor_observation_at(now).radio.rig_keyed,
        None
    );
    assert_eq!(
        engine
            .remote_monitor_observation_at(now)
            .radio
            .rig_mode
            .as_deref(),
        Some("USB")
    );
    let failed = engine.remote_radio_read(&connection, now).unwrap();
    engine.remote_observe_cat(Some(&failed), Some(false));
    engine.remote_observe_mode(Some(&latest), Some("USB"));
    assert_eq!(
        engine.remote_monitor_observation_at(now).radio.rig_mode,
        None
    );
    let slow = engine.remote_radio_read(&connection, now).unwrap();
    engine.remote_observe_cat(Some(&slow), Some(true));
    assert_eq!(
        engine
            .remote_monitor_observation_at(now + std::time::Duration::from_secs(6))
            .radio
            .cat_connected,
        None
    );
}
