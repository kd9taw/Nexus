use super::*;

#[test]
fn remote_phone_am_pick_uses_the_native_am_ceiling_and_keeps_lower_power() {
    for (phone_cap, am_cap, before) in [
        (None, Some(0.25), 0.8),
        (Some(0.2), Some(0.5), 0.8),
        (None, Some(0.25), 0.1),
    ] {
        let mut s = Station::new(OperatingMode::Phone);
        s.engine.settings.max_power_phone = phone_cap;
        s.engine.settings.max_power_am = am_cap;
        s.engine.set_frequency(7.22, "40m", "LSB");
        s.engine.take_immediate_retune();
        s.engine.set_rf_power(before);
        s.engine.observe_rig_power(before);
        let mut native = Engine::with_settings(s.engine.settings.clone());
        native.request_sideband_override(Some("AM"));
        let limit = before.min(native.active_power_ceiling());
        s.sample(7_220_000, "LSB");
        let receipt = queue(&mut s, None, Some("AM")).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        assert_eq!(
            request.power_limit(),
            Some(limit),
            "a transient AM pick must inherit the native AM ceiling"
        );
        s.sample(7_220_000, "AM");
        assert!(request.commit_readback(&mut s.engine, Some(limit)));
        assert_eq!(s.engine.rf_power(), Some(limit));
        assert!(!s.engine.tx_enabled());
        assert!(!s.engine.take_immediate_retune());
        assert_eq!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
        assert!(!s.path.exists());
    }
}

fn queue(s: &mut Station, before: Option<&str>, mode: Option<&str>) -> Result<Completion, Reason> {
    let connection = s
        .engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation;
    s.engine.queue_remote_phone_mode(
        before,
        mode,
        connection,
        s.authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap(),
    )
}

#[test]
fn remote_phone_mode_matches_native_transient_picks_and_preserves_the_dial_and_settings() {
    for dial in [7.22, 14.275, 28.5] {
        for before in [None, Some("USB"), Some("LSB")] {
            for after in [None, Some("USB"), Some("LSB"), Some("AM")] {
                if after == Some("AM") && dial == 14.275 {
                    continue;
                }
                let mut s = Station::new(OperatingMode::Phone);
                let band = crate::bandplan::band_for_dial(dial).unwrap();
                s.engine.set_frequency(dial, band, "USB");
                s.engine.request_sideband_override(before);
                s.engine.take_immediate_retune();
                s.engine.settings.save(&s.path).unwrap();
                let file = std::fs::read(&s.path).unwrap();
                let mut native = Engine::with_settings(s.engine.settings.clone());
                native.request_sideband_override(after);
                let target = native.rig_mode_effective();
                // The physical front-panel choice is different from the native
                // desired mode. Bind the actual CAT position, not that belief.
                let hz = s.engine.settings.dial_hz();
                s.sample(hz, "CW");
                let receipt = queue(&mut s, before, after).unwrap();
                let request = s.engine.take_remote_radio().unwrap();
                assert_eq!(request.expected(), (hz, "CW"));
                assert_eq!(request.target(), (hz, target.as_str()));
                assert_eq!(s.engine.sideband_override.as_deref(), before);
                assert!(!s.engine.take_immediate_retune());
                s.sample(hz, &target);
                let power = request.power_limit();
                assert!(request.commit_readback(&mut s.engine, power));
                assert_eq!(
                    receipt.outcome(),
                    Outcome::Applied {
                        evidence: Evidence::RadioReadback
                    }
                );
                assert_eq!(s.engine.sideband_override, native.sideband_override);
                assert_eq!(s.engine.rig_mode_effective(), target);
                assert_eq!(s.engine.settings.dial_hz(), hz);
                assert!(!s.engine.take_immediate_retune());
                assert!(!s.engine.tx_enabled());
                assert_eq!(
                    serde_json::to_value(s.engine.settings()).unwrap(),
                    serde_json::to_value(native.settings()).unwrap()
                );
                assert_eq!(std::fs::read(&s.path).unwrap(), file);
            }
        }
    }
}

#[test]
fn remote_phone_mode_cannot_borrow_a_local_away_and_back_pick() {
    let mut s = Station::new(OperatingMode::Phone);
    let receipt = queue(&mut s, None, Some("LSB")).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.engine.request_sideband_override(Some("LSB"));
    s.engine.request_sideband_override(None);
    s.engine.take_immediate_retune();
    s.sample(14_074_000, "USB");
    assert!(request.permission().begin_write(Instant::now()).is_err());
    assert!(!request.commit_readback(&mut s.engine, None));
    assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
    assert_eq!(s.engine.sideband_override, None);
    let fresh = queue(&mut s, None, Some("LSB")).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.sample(14_074_000, "LSB");
    let power = request.power_limit();
    assert!(request.commit_readback(&mut s.engine, power));
    assert!(matches!(fresh.outcome(), Outcome::Applied { .. }));
    assert!(!s.path.exists());
}

#[test]
fn remote_phone_mode_refuses_stale_override_and_modes_absent_from_the_native_picker() {
    let mut s = Station::new(OperatingMode::Phone);
    for bad in ["CW", "USB\nT 1", "usb", ""] {
        assert!(matches!(
            queue(&mut s, None, Some(bad)),
            Err(Reason::InvalidAction)
        ));
    }
    assert!(matches!(
        queue(&mut s, Some("USB"), None),
        Err(Reason::ContextChanged)
    ));
    assert!(
        matches!(queue(&mut s, None, Some("AM")), Err(Reason::InvalidAction)),
        "20m has no AM button"
    );
    s.sample(14_074_000, "USB");
    let receipt = queue(&mut s, None, Some("LSB")).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.sample(14_074_000, "USB");
    assert!(
        !request.commit_readback(&mut s.engine, None),
        "an unchanged mode cannot confirm the requested sideband"
    );
    assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
    assert_eq!(s.engine.sideband_override, None);
    assert!(!s.path.exists());
    for mode in [
        OperatingMode::Cw,
        OperatingMode::Digital,
        OperatingMode::Rtty,
        OperatingMode::Keyboard,
    ] {
        let mut s = Station::new(mode);
        assert!(matches!(
            queue(&mut s, None, Some("USB")),
            Err(Reason::UnsupportedAction)
        ));
    }
}

#[test]
fn remote_phone_fm_requires_repeater_readback_and_binds_saved_configuration() {
    for failure in ["none", "missing", "offset", "changed"] {
        let mut s = Station::new(OperatingMode::Phone);
        s.engine.set_frequency(145.5, "2m", "USB");
        s.engine.take_immediate_retune();
        s.engine.settings.rptr_shift = "+".into();
        s.engine.settings.ctcss_tone_hz = 88.55;
        s.sample(145_500_000, "USB");
        let receipt = queue(&mut s, None, Some("FM")).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        assert_eq!(request.target(), (145_500_000, "FM"));
        assert_eq!(request.repeater(), Some(("+", 600_000, 88.55)));
        s.sample(145_500_000, "FM");
        if failure == "changed" {
            s.engine.settings.ctcss_tone_hz = 100.0;
        }
        let actual = match failure {
            "missing" => None,
            "offset" => Some(("plus", 5_000_000, 88.6)),
            _ => Some(("plus", 600_000, 88.6)),
        };
        let power = request.power_limit();
        assert_eq!(
            request.commit_tuning_readback(&mut s.engine, power, actual),
            failure == "none"
        );
        assert_eq!(
            matches!(receipt.outcome(), Outcome::Applied { .. }),
            failure == "none"
        );
        assert_eq!(
            s.engine.sideband_override.as_deref(),
            (failure == "none").then_some("FM")
        );
        assert!(!s.engine.tx_enabled());
        assert!(!s.engine.take_immediate_retune());
        assert!(!s.path.exists());
    }
}
