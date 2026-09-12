use super::*;

fn queue(s: &mut Station, expected: u32, hz: u32) -> Result<Completion, Reason> {
    let connection = s
        .engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation;
    let mode = match s.engine.settings.operating_mode {
        OperatingMode::Cw => "cw",
        OperatingMode::Phone => "phone",
        _ => "digital",
    };
    s.engine.queue_remote_filter_width(
        mode,
        expected,
        hz,
        connection,
        s.authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap(),
    )
}

#[test]
fn remote_filter_commits_only_confirmed_width_without_native_retry_or_settings_write() {
    for (mode, actual_mode, before, after) in [
        (OperatingMode::Cw, "CW", 500, 550),
        // A physical front-panel mode may differ from Nexus's commanded mode.
        // Native filter adjustment preserves the actual mode; Remote must too.
        (OperatingMode::Phone, "LSB", 2400, 2300),
    ] {
        let mut s = Station::new(mode);
        s.sample(14_074_000, actual_mode);
        s.engine.observe_rig_passband(Some(before));
        s.engine.settings.save(&s.path).unwrap();
        let settings = serde_json::to_value(s.engine.settings()).unwrap();
        let file = std::fs::read(&s.path).unwrap();
        let decoder = s.engine.source.clone();
        let receipt = queue(&mut s, before, after).unwrap();
        assert!(matches!(receipt.outcome(), Outcome::Pending));
        assert_eq!(s.engine.snapshot().radio.filter_width_hz, Some(before));
        assert_eq!(s.engine.take_passband_request(), None);
        assert_eq!(std::fs::read(&s.path).unwrap(), file);
        let request = s.engine.take_remote_radio().unwrap();
        assert_eq!(request.expected(), (14_074_000, actual_mode));
        assert_eq!(request.target(), request.expected());
        s.sample(14_074_000, actual_mode);
        assert!(request.commit_filter_readback(&mut s.engine, Some(after)));
        assert!(matches!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        ));
        assert_eq!(s.engine.snapshot().radio.filter_width_hz, Some(after));
        assert_eq!(s.engine.take_passband_request(), None);
        assert!(!s.engine.take_immediate_retune());
        assert!(!s.engine.tx_enabled());
        assert!(std::sync::Arc::ptr_eq(&decoder, &s.engine.source));
        assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), settings);
        assert_eq!(std::fs::read(&s.path).unwrap(), file);
    }
}

#[test]
fn remote_filter_cannot_borrow_a_local_width_gesture_even_when_it_returns_to_the_prior_value() {
    let mut s = Station::new(OperatingMode::Cw);
    s.engine.observe_rig_passband(Some(500));
    let receipt = queue(&mut s, 500, 550).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.engine.request_filter_width(600);
    assert_eq!(s.engine.take_passband_request(), Some(600));
    s.engine.request_filter_width(500);
    assert_eq!(s.engine.take_passband_request(), Some(500));
    let mode = s.engine.rig_mode_effective();
    s.sample(14_074_000, &mode);
    assert!(request.validate(&s.engine).is_err());
    assert!(!request.commit_filter_readback(&mut s.engine, Some(550)));
    assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
    assert_eq!(s.engine.snapshot().radio.filter_width_hz, Some(500));
    assert_eq!(s.engine.take_passband_request(), None);
    // A new operator action, with new authority capture, still works.
    let fresh = queue(&mut s, 500, 600).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.sample(14_074_000, &mode);
    assert!(request.commit_filter_readback(&mut s.engine, Some(600)));
    assert!(matches!(fresh.outcome(), Outcome::Applied { .. }));
}

#[test]
fn remote_filter_refuses_unavailable_changed_and_out_of_native_range_widths() {
    for mode in [OperatingMode::Cw, OperatingMode::Phone] {
        let mut s = Station::new(mode);
        assert!(queue(&mut s, 500, 550).is_err());
        s.engine.observe_rig_passband(Some(500));
        assert!(queue(&mut s, 501, 550).is_err());
        for bad in [0, 49, 4001, u32::MAX] {
            assert!(queue(&mut s, 500, bad).is_err());
            assert!(s.engine.take_remote_radio().is_none());
            assert_eq!(s.engine.take_passband_request(), None);
        }
        let receipt = queue(&mut s, 500, 550).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        let mode = s.engine.rig_mode_effective();
        s.sample(14_074_000, &mode);
        assert!(!request.commit_filter_readback(&mut s.engine, None));
        assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
        assert_eq!(s.engine.snapshot().radio.filter_width_hz, Some(500));
    }
    let mut s = Station::new(OperatingMode::Digital);
    s.engine.observe_rig_passband(Some(2400));
    assert!(queue(&mut s, 2400, 2300).is_err());
}
