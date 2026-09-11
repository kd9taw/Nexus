use super::*;

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
fn remote_phone_mode_refuses_stale_override_fm_and_modes_absent_from_the_native_picker() {
    let mut s = Station::new(OperatingMode::Phone);
    for bad in ["FM", "CW", "USB\nT 1", "usb", ""] {
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
    s.sample(14_074_000, "FM");
    assert!(matches!(
        queue(&mut s, None, Some("USB")),
        Err(Reason::UnsupportedAction)
    ));
    assert!(s.engine.take_remote_radio().is_none());
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
