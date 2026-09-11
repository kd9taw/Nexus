use super::*;

fn choices() -> [(RadioLevel, f32, f32); 5] {
    [
        (RadioLevel::Power, 0.5, 0.35),
        (RadioLevel::MicGain, 0.5, 0.35),
        (RadioLevel::NoiseReduction, 0.5, 0.35),
        (RadioLevel::Compression, 0.5, 0.35),
        (RadioLevel::NotchFrequency, 600.0, 1500.0),
    ]
}
fn observe(s: &mut Station, level: RadioLevel, value: f32) {
    match level {
        RadioLevel::Power => s.engine.observe_rig_power(value),
        RadioLevel::MicGain => s.engine.observe_rig_mic_gain(value),
        RadioLevel::NoiseReduction => s.engine.observe_rig_nr_level(value),
        RadioLevel::Compression => s.engine.observe_rig_comp_level(value),
        RadioLevel::NotchFrequency => s.engine.observe_rig_notch_freq_hz(value),
    }
    let mode = s.engine.rig_mode_effective();
    s.sample(14_074_000, &mode);
}
fn set(s: &mut Station, level: RadioLevel, value: f32) {
    match level {
        RadioLevel::Power => s.engine.set_rf_power(value),
        RadioLevel::MicGain => s.engine.set_mic_gain(value),
        RadioLevel::NoiseReduction => s.engine.set_nr_level(value),
        RadioLevel::Compression => s.engine.set_comp_level(value),
        RadioLevel::NotchFrequency => s.engine.set_notch_freq_hz(value),
    }
}
fn queue(
    s: &mut Station,
    level: RadioLevel,
    before: f32,
    value: f32,
) -> Result<Completion, Reason> {
    let connection = s
        .engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation;
    s.engine.queue_remote_level(
        "phone",
        level,
        before,
        value,
        connection,
        s.authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap(),
    )
}

#[test]
fn remote_levels_commit_actual_readback_preserving_settings_decoder_and_tx() {
    for (level, before, desired) in choices() {
        let mut s = Station::new(OperatingMode::Phone);
        s.engine.settings.save(&s.path).unwrap();
        let bytes = std::fs::read(&s.path).unwrap();
        let settings = s.engine.settings.clone();
        let decoder = s.engine.source.clone();
        observe(&mut s, level, before);
        let receipt = queue(&mut s, level, before, desired).unwrap();
        assert_eq!(s.engine.remote_level_desired(level), None);
        let request = s.engine.take_remote_radio().unwrap();
        assert_eq!(request.level(), Some((level, before, desired)));
        let actual = if level == RadioLevel::NotchFrequency {
            desired
        } else {
            desired + 0.001
        };
        observe(&mut s, level, before);
        assert!(request.commit_level_readback(&mut s.engine, Some(actual)));
        assert_eq!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
        assert_eq!(s.engine.remote_level_desired(level), Some(desired));
        let snapshot = s.engine.snapshot();
        let displayed = match level {
            RadioLevel::Power => snapshot.radio.rf_power,
            RadioLevel::MicGain => snapshot.radio.mic_gain,
            RadioLevel::NoiseReduction => snapshot.radio.nr_level,
            RadioLevel::Compression => snapshot.radio.comp_level,
            RadioLevel::NotchFrequency => snapshot.radio.notch_freq_hz,
        };
        assert_eq!(displayed, Some(actual));
        assert_eq!(s.engine.settings, settings);
        assert_eq!(std::fs::read(&s.path).unwrap(), bytes);
        assert!(std::sync::Arc::ptr_eq(&decoder, &s.engine.source));
        assert!(!s.engine.tx_enabled());
        assert!(!s.engine.take_immediate_retune());
    }
}

#[test]
fn remote_levels_local_away_and_back_retires_old_work_and_fresh_action_succeeds() {
    for (level, before, desired) in choices() {
        let mut s = Station::new(OperatingMode::Phone);
        set(&mut s, level, before);
        observe(&mut s, level, before);
        let receipt = queue(&mut s, level, before, desired).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        set(&mut s, level, desired);
        set(&mut s, level, before);
        observe(&mut s, level, before);
        assert!(request.permission().begin_write(Instant::now()).is_err());
        assert!(!request.commit_level_readback(&mut s.engine, Some(desired)));
        assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
        assert_eq!(s.engine.remote_level_desired(level), Some(before));
        let receipt = queue(&mut s, level, before, desired).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        observe(&mut s, level, before);
        assert!(request.commit_level_readback(&mut s.engine, Some(desired)));
        assert!(matches!(receipt.outcome(), Outcome::Applied { .. }));
    }
}

#[test]
fn remote_levels_require_the_reported_value_and_current_authority() {
    for (level, before, desired) in choices() {
        for failure in ["missing", "different", "nan", "revoked"] {
            let mut s = Station::new(OperatingMode::Phone);
            observe(&mut s, level, before);
            let receipt = queue(&mut s, level, before, desired).unwrap();
            let request = s.engine.take_remote_radio().unwrap();
            request.permission().begin_write(Instant::now()).unwrap();
            observe(&mut s, level, before);
            if failure == "revoked" {
                s.authority.revoke();
            }
            let value = match failure {
                "missing" => None,
                "different" => Some(before),
                "nan" => Some(f32::NAN),
                _ => Some(desired),
            };
            assert!(!request.commit_level_readback(&mut s.engine, value));
            assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
            assert_eq!(s.engine.remote_level_desired(level), None);
            assert!(!s.engine.take_immediate_retune());
        }
    }
}

#[test]
fn remote_level_targets_keep_native_limits_and_reject_nonfinite_inputs() {
    let mut s = Station::new(OperatingMode::Phone);
    observe(&mut s, RadioLevel::Power, 0.5);
    assert!(queue(&mut s, RadioLevel::Power, 0.5, f32::NAN).is_err());
    queue(&mut s, RadioLevel::Power, 0.5, 2.0).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    assert_eq!(request.level().unwrap().2, s.engine.active_power_ceiling());
    request.refuse(Reason::ContextChanged);
    observe(&mut s, RadioLevel::NotchFrequency, 600.0);
    queue(&mut s, RadioLevel::NotchFrequency, 600.0, 10.0).unwrap();
    assert_eq!(
        s.engine.take_remote_radio().unwrap().level().unwrap().2,
        super::super::super::NOTCH_MIN_HZ
    );
    assert!(RadioLevel::from_name("VOX").is_none());
    assert!(!RadioLevel::MicGain.valid_reading(-1.0));
    assert!(!RadioLevel::Power.same_display_value(0.35, 0.37));
    assert!(!RadioLevel::NotchFrequency.valid_target(1e20));
}
