use super::*;

fn apply(s: &mut Station, expected: f32, gain: f32) -> Result<(), Reason> {
    let connection = s.connection_generation();
    s.engine.save_remote_rx_gain(
        s.engine.settings.active_radio,
        expected,
        gain,
        connection,
        &s.authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap(),
    )
}

#[test]
fn rx_gain_preserves_settings_and_other_profiles_and_survives_native_radio_round_trips() {
    for mode in [
        OperatingMode::Digital,
        OperatingMode::Phone,
        OperatingMode::Cw,
        OperatingMode::Rtty,
        OperatingMode::Keyboard,
    ] {
        let mut s = Station::new(Tier::Ft8);
        s.engine.settings.operating_mode = mode;
        s.engine
            .settings
            .radios
            .iter_mut()
            .find(|p| p.id == 9)
            .unwrap()
            .rx_gain = 3.25;
        let source = s.engine.source.clone();
        let tx_generation = s.engine.tx_gate_gen;
        let epoch = s.engine.decode_epoch;
        for gain in [1.1, 8.0, 1.0, 2.75] {
            let mut expected = s.engine.settings.clone();
            expected.rx_gain = gain;
            let active = expected.active_radio;
            expected
                .radios
                .iter_mut()
                .find(|p| p.id == active)
                .unwrap()
                .rx_gain = gain;
            let previous = s.engine.settings.rx_gain;
            s.sample(Some(false), Duration::ZERO);
            apply(&mut s, previous, gain).unwrap();
            assert_eq!(
                settings(&s.engine),
                serde_json::to_value(&expected).unwrap()
            );
            let saved: Settings = serde_json::from_slice(&std::fs::read(&s.path).unwrap()).unwrap();
            assert_eq!(
                serde_json::to_value(saved).unwrap(),
                serde_json::to_value(&expected).unwrap()
            );
            assert_eq!(
                Settings::load(&s.path).active_profile().unwrap().rx_gain,
                gain
            );
            assert!(std::sync::Arc::ptr_eq(&source, &s.engine.source));
            assert_eq!(s.engine.tx_gate_gen, tx_generation);
            assert_eq!(s.engine.decode_epoch, epoch);
            assert!(!s.engine.take_immediate_retune());
            assert!(s.engine.take_remote_radio().is_none());
            assert!(!s.engine.tx_enabled());
        }
        let active = s.engine.settings.active_radio;
        s.engine.set_active_radio(9);
        assert_eq!(s.engine.settings.rx_gain, 3.25);
        s.engine.set_active_radio(active);
        assert_eq!(s.engine.settings.rx_gain, 2.75);
    }
}

#[test]
fn rx_gain_can_follow_a_local_narrow_edit_with_a_legitimately_older_profile_mirror() {
    let mut s = Station::new(Tier::Ft8);
    s.engine.set_rx_gain(2.5);
    assert_eq!(s.engine.settings.active_profile().unwrap().rx_gain, 1.0);
    s.engine.settings.save(&s.path).unwrap();
    assert_eq!(Settings::load(&s.path).rx_gain, 2.5);
    apply(&mut s, 2.5, 3.5).unwrap();
    assert_eq!(s.engine.settings.rx_gain, 3.5);
    assert_eq!(s.engine.settings.active_profile().unwrap().rx_gain, 3.5);
    assert_eq!(Settings::load(&s.path).rx_gain, 3.5);
}

#[test]
fn rx_gain_save_failure_leaves_runtime_and_the_existing_file_unchanged() {
    let mut s = Station::new(Tier::Ft8);
    s.engine.settings.save(&s.path).unwrap();
    let bytes = std::fs::read(&s.path).unwrap();
    let before = settings(&s.engine);
    let blocker = s.path.parent().unwrap().join("blocked-parent");
    std::fs::write(&blocker, b"retain").unwrap();
    s.engine
        .configure_remote_settings_store(blocker.join("settings.json"));
    assert_eq!(apply(&mut s, 1.0, 2.0), Err(Reason::PersistenceFailed));
    assert_eq!(std::fs::read(&s.path).unwrap(), bytes);
    assert_eq!(std::fs::read(&blocker).unwrap(), b"retain");
    assert_eq!(settings(&s.engine), before);
}

#[test]
fn rx_gain_rejects_invalid_values_wrong_radio_or_connection_and_expired_authority() {
    let mut s = Station::new(Tier::Ft8);
    let before = settings(&s.engine);
    for gain in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.9, 8.1, 1.0] {
        assert_eq!(apply(&mut s, 1.0, gain), Err(Reason::InvalidAction));
    }
    assert_eq!(apply(&mut s, f32::NAN, 2.0), Err(Reason::InvalidAction));
    assert_eq!(apply(&mut s, 1.5, 2.0), Err(Reason::ContextChanged));
    let connection = s.connection_generation();
    let radio = s.engine.settings.active_radio;
    let permit = s
        .authority
        .permit(Instant::now() + Duration::from_secs(5))
        .unwrap();
    assert_eq!(
        s.engine
            .save_remote_rx_gain(9, 1.0, 2.0, connection, &permit),
        Err(Reason::ContextChanged)
    );
    assert_eq!(
        s.engine
            .save_remote_rx_gain(radio, 1.0, 2.0, connection + 1, &permit),
        Err(Reason::ContextChanged)
    );
    s.authority.revoke();
    assert_eq!(
        s.engine
            .save_remote_rx_gain(radio, 1.0, 2.0, connection, &permit),
        Err(Reason::AuthorityExpired)
    );
    assert_eq!(settings(&s.engine), before);
    assert!(!s.path.exists());
    apply(&mut s, 1.0, 2.0).unwrap();
    assert!(s.path.is_file());
}

#[test]
fn rx_gain_refuses_stale_keyed_unknown_armed_and_companion_contexts() {
    let mut s = Station::new(Tier::Ft8);
    let before = settings(&s.engine);
    for (keyed, age, reason) in [
        (None, Duration::ZERO, Reason::ReadingUnavailable),
        (Some(true), Duration::ZERO, Reason::StationBusy),
        (
            Some(false),
            Duration::from_secs(2),
            Reason::ReadingUnavailable,
        ),
    ] {
        s.sample(keyed, age);
        assert_eq!(apply(&mut s, 1.0, 2.0), Err(reason));
    }
    s.sample(Some(false), Duration::ZERO);
    s.engine.source_kind = SourceKind::Companion;
    assert_eq!(apply(&mut s, 1.0, 2.0), Err(Reason::UnsupportedAction));
    s.engine.source_kind = SourceKind::Native;
    s.engine.set_tx_enabled(true);
    assert!(s.engine.tx_enabled());
    assert_eq!(apply(&mut s, 1.0, 2.0), Err(Reason::StationBusy));
    s.engine.set_tx_enabled(false);
    assert_eq!(apply(&mut s, 1.0, 2.0), Err(Reason::StationBusy));
    assert_eq!(settings(&s.engine), before);
    assert!(!s.path.exists());
    assert!(s.engine.take_immediate_retune());
    s.sample(Some(false), Duration::ZERO);
    apply(&mut s, 1.0, 2.0).unwrap();
}

#[test]
fn rx_gain_does_not_acquire_or_replace_the_decoder_and_can_repair_a_legacy_value() {
    let mut s = Station::new(Tier::Js8);
    s.engine.settings.rx_gain = 0.5;
    let source = s.engine.source.clone();
    let guard = source.lock().unwrap();
    let started = Instant::now();
    apply(&mut s, 0.5, 1.0).unwrap();
    assert!(started.elapsed() < Duration::from_secs(1));
    assert!(std::sync::Arc::ptr_eq(&source, &s.engine.source));
    drop(guard);
    assert_eq!(Settings::load(&s.path).rx_gain, 1.0);
}

#[test]
fn native_rx_gain_takeover_cancels_pending_remote_hardware_work_without_changing_tx_generation() {
    let mut s = Station::new(Tier::Ft8);
    let connection = s.connection_generation();
    s.engine
        .queue_remote_frequency(
            7.074,
            "40m",
            "USB",
            connection,
            s.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
        .unwrap();
    let work = s.engine.take_remote_radio().unwrap();
    assert!(work.permission().check(Instant::now()).is_ok());
    let generation = s.engine.tx_gate_gen;
    s.engine.set_rx_gain(1.0);
    assert_eq!(
        work.permission().begin_write(Instant::now()),
        Err(Reason::ContextChanged)
    );
    assert_eq!(s.engine.tx_gate_gen, generation);
}
