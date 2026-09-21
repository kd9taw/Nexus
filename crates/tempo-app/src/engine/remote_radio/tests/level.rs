use super::*;

fn choices() -> [(RadioLevel, f32, f32); 8] {
    [
        (RadioLevel::Power, 0.5, 0.35),
        (RadioLevel::MicGain, 0.5, 0.35),
        (RadioLevel::NoiseReduction, 0.5, 0.35),
        (RadioLevel::Compression, 0.5, 0.35),
        (RadioLevel::NotchFrequency, 600.0, 1500.0),
        (RadioLevel::AfGain, 0.5, 0.35),
        (RadioLevel::RfGain, 0.5, 0.35),
        (RadioLevel::Squelch, 0.5, 0.35),
    ]
}
fn observe(s: &mut Station, level: RadioLevel, value: f32) {
    match level {
        RadioLevel::Power => s.engine.observe_rig_power(value),
        RadioLevel::MicGain => s.engine.observe_rig_mic_gain(value),
        RadioLevel::NoiseReduction => s.engine.observe_rig_nr_level(value),
        RadioLevel::Compression => s.engine.observe_rig_comp_level(value),
        RadioLevel::NotchFrequency => s.engine.observe_rig_notch_freq_hz(value),
        RadioLevel::AfGain => s.engine.observe_rig_af_gain(value),
        RadioLevel::RfGain => s.engine.observe_rig_rf_gain(value),
        RadioLevel::Squelch => s.engine.observe_rig_squelch(value),
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
        RadioLevel::AfGain => s.engine.set_af_gain(value),
        RadioLevel::RfGain => s.engine.set_rf_gain(value),
        RadioLevel::Squelch => s.engine.set_squelch(value),
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
        s.authority.permit(unexpired_deadline()).unwrap(),
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
            RadioLevel::AfGain => snapshot.radio.af_gain,
            RadioLevel::RfGain => snapshot.radio.rf_gain,
            RadioLevel::Squelch => snapshot.radio.squelch,
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

#[test]
fn remote_notch_does_not_confirm_out_of_range_readback_with_same_display_value() {
    for (target, actual) in [(300.0, 299.6), (3400.0, 3400.4)] {
        let mut s = Station::new(OperatingMode::Phone);
        observe(&mut s, RadioLevel::NotchFrequency, 600.0);
        let receipt = queue(&mut s, RadioLevel::NotchFrequency, 600.0, target).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        request.permission().begin_write(Instant::now()).unwrap();
        observe(&mut s, RadioLevel::NotchFrequency, 600.0);
        assert!(!request.commit_level_readback(&mut s.engine, Some(actual)));
        assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
        assert_eq!(
            s.engine.remote_level_desired(RadioLevel::NotchFrequency),
            None
        );
        let receipt = queue(&mut s, RadioLevel::NotchFrequency, 600.0, target).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        observe(&mut s, RadioLevel::NotchFrequency, 600.0);
        assert!(request.commit_level_readback(&mut s.engine, Some(target)));
        assert!(matches!(receipt.outcome(), Outcome::Applied { .. }));
    }
}

#[test]
fn remote_power_keeps_each_native_mode_ceiling_and_rechecks_a_lowered_limit() {
    for (name, mode, cap) in [
        ("digital", OperatingMode::Digital, 0.2),
        ("phone", OperatingMode::Phone, 0.4),
        ("cw", OperatingMode::Cw, 0.6),
        ("rtty", OperatingMode::Rtty, 0.2),
        ("keyboard", OperatingMode::Keyboard, 0.2),
    ] {
        let mut s = Station::new(mode);
        s.engine.settings.max_power_digital = Some(0.2);
        s.engine.settings.max_power_phone = Some(0.4);
        s.engine.settings.max_power_cw = Some(0.6);
        observe(&mut s, RadioLevel::Power, 0.1);
        let connection = s
            .engine
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        let receipt = s
            .engine
            .queue_remote_level(
                name,
                RadioLevel::Power,
                0.1,
                1.0,
                connection,
                s.authority.permit(unexpired_deadline()).unwrap(),
            )
            .unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        assert_eq!(request.level().unwrap().2, cap);
        observe(&mut s, RadioLevel::Power, 0.1);
        assert!(request.commit_level_readback(&mut s.engine, Some(cap)));
        assert!(matches!(receipt.outcome(), Outcome::Applied { .. }));
        observe(&mut s, RadioLevel::Power, cap);
        let receipt = s
            .engine
            .queue_remote_level(
                name,
                RadioLevel::Power,
                cap,
                1.0,
                connection,
                s.authority.permit(unexpired_deadline()).unwrap(),
            )
            .unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        s.engine.settings.max_power_digital = Some(0.1);
        s.engine.settings.max_power_phone = Some(0.1);
        s.engine.settings.max_power_cw = Some(0.1);
        assert!(!request.commit_level_readback(&mut s.engine, Some(cap)));
        assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
        assert!(!s.engine.tx_enabled());
    }
}

/// The snapshot value for one of the three analog levels — the number a slider would show.
fn shown(s: &mut Station, level: RadioLevel) -> Option<f32> {
    let snapshot = s.engine.snapshot();
    match level {
        RadioLevel::AfGain => snapshot.radio.af_gain,
        RadioLevel::RfGain => snapshot.radio.rf_gain,
        RadioLevel::Squelch => snapshot.radio.squelch,
        other => unreachable!("this helper covers the three analog levels, not {other:?}"),
    }
}

/// AF GAIN, RF GAIN AND SQUELCH SHOW THE RADIO, NOT OUR MEMORY OF IT.
///
/// ⚠️ THE TWO NUMBERS DISAGREE ON PURPOSE, and that is the whole test. With the rig reporting
/// back the same value that was commanded, this passes on a snapshot wired to the COMMANDED
/// field that never asks the radio at all — the assertion cannot tell the two apart. 0.30
/// commanded against 0.80 reported can: drop the observation, or read the desired field, and
/// it reads 0.30.
///
/// The first assertion is the other half, and it is why these controls are not a memory
/// dressed as a reading: before the rig has said anything there is no value at all, so the
/// slider does not render. The commanded value can only ever appear AFTER a reading has.
#[test]
fn af_rf_and_squelch_display_the_rigs_reading_over_the_commanded_value() {
    const COMMANDED: f32 = 0.30;
    const REPORTED: f32 = 0.80;
    for level in [RadioLevel::AfGain, RadioLevel::RfGain, RadioLevel::Squelch] {
        let mut s = Station::new(OperatingMode::Phone);
        assert_eq!(
            shown(&mut s, level),
            None,
            "{level:?}: nothing read and nothing commanded must show nothing"
        );
        set(&mut s, level, COMMANDED);
        assert_eq!(
            shown(&mut s, level),
            Some(COMMANDED),
            "{level:?}: an in-flight command stands until the poll answers"
        );
        observe(&mut s, level, REPORTED);
        assert_eq!(
            shown(&mut s, level),
            Some(REPORTED),
            "{level:?}: the rig's own reading must beat the commanded value"
        );
    }
}

/// The three tokens are Hamlib's, verified against `rig.h` and a live `rigctl -m 1 l ?`:
/// `AF` is volume, `RF` is RF GAIN (not TX power — `RFPOWER` is that), `SQL` is squelch.
/// A transposition here would move a control the operator did not touch, so each is pinned
/// by name and the pair that is easy to swap is pinned against each other.
#[test]
fn the_analog_level_tokens_are_the_hamlib_names_and_are_not_transposed() {
    assert_eq!(
        RadioLevel::from_name("afGain").map(RadioLevel::token),
        Some("AF")
    );
    assert_eq!(
        RadioLevel::from_name("rfGain").map(RadioLevel::token),
        Some("RF")
    );
    assert_eq!(
        RadioLevel::from_name("squelch").map(RadioLevel::token),
        Some("SQL")
    );
    // RF GAIN is a receive control and RFPOWER is a transmit one. Hamlib spells them
    // differently for that reason and so must we.
    assert_ne!(RadioLevel::RfGain.token(), RadioLevel::Power.token());
    assert_eq!(RadioLevel::Power.token(), "RFPOWER");
    // The wire token is not an accepted name — only the camelCase field names cross the
    // browser boundary, so a caller cannot smuggle a raw Hamlib token through.
    for token in ["AF", "RF", "SQL"] {
        assert!(
            RadioLevel::from_name(token).is_none(),
            "{token} is a wire token, not a name"
        );
    }
    // All three are plain 0..1 fractions, so they take the shared percentage display rule
    // rather than the notch's hertz one.
    for level in [RadioLevel::AfGain, RadioLevel::RfGain, RadioLevel::Squelch] {
        assert!(level.valid_reading(0.0) && level.valid_reading(1.0));
        assert!(!level.valid_reading(1.5) && !level.valid_reading(f32::NAN));
        assert!(level.valid_target(0.5));
        assert!(level.same_display_value(0.501, 0.5));
        assert!(!level.same_display_value(0.51, 0.5));
    }
}
