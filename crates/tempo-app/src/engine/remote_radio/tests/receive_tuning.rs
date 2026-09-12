use super::*;

#[test]
fn off_band_listening_and_returning_keep_the_native_phone_sideband_override() {
    for sideband in ["LSB", "AM"] {
        let mut s = Station::new(OperatingMode::Phone);
        let mut native = Station::new(OperatingMode::Phone);
        for station in [&mut s, &mut native] {
            station.engine.request_sideband_override(Some(sideband));
            station.engine.take_immediate_retune();
            station.sample(14_074_000, sideband);
        }
        for (dial, band) in [(10.0, ""), (14.240, "20m"), (9.5, ""), (7.200, "40m")] {
            let receipt = queue_dial(&mut s, dial, band).unwrap();
            let request = s.engine.take_remote_radio().unwrap();
            native.engine.set_frequency(dial, band, "USB");
            native.engine.take_immediate_retune();
            assert_eq!(
                request.target_mode,
                native.engine.rig_mode_effective(),
                "the explicit Phone mode must follow native off-band context retention"
            );
            s.sample(request.target_hz, &request.target_mode);
            assert!(request.commit(&mut s.engine));
            assert_eq!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            );
            assert_eq!(s.engine.sideband_override, native.engine.sideband_override);
            assert_eq!(s.engine.off_band_from, native.engine.off_band_from);
            assert_eq!(s.engine.tx_gate_gen, native.engine.tx_gate_gen);
            assert!(!s.engine.tx_enabled());
        }
    }
}

fn queue_dial(s: &mut Station, dial: f64, band: &str) -> Result<Completion, Reason> {
    let connection = s
        .engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation;
    s.engine.queue_remote_frequency(
        dial,
        band,
        "USB",
        connection,
        s.authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap(),
    )
}

#[test]
fn off_band_remote_receive_tuning_matches_the_native_dial_and_memory_policy() {
    for mode in [
        OperatingMode::Digital,
        OperatingMode::Cw,
        OperatingMode::Phone,
        OperatingMode::Rtty,
        OperatingMode::Keyboard,
    ] {
        let mut s = Station::new(mode);
        let mut native = Station::new(mode);
        let source = s.engine.source.clone();
        for (dial, band) in [
            (10.0, ""),
            (9.5, ""),
            (7.074, "40m"),
            (14.240, "20m"),
            (10.000_000_4, ""),
        ] {
            let before = serde_json::to_value(s.engine.settings()).unwrap();
            let generation = s.engine.tx_gate_gen;
            // This compares native dial policy, not wall-clock expiry. Building
            // the other decoder and serializing full Settings can outlive the
            // initial sample under workspace load; sample the simulated idle
            // radio immediately before this explicit positive-control request.
            let hz = s.engine.settings.dial_hz();
            let mode = s.engine.rig_mode_effective();
            s.sample(hz, &mode);
            let queued_at = Instant::now();
            let receipt = queue_dial(&mut s, dial, band).unwrap();
            assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), before);
            assert_eq!(
                s.engine.tx_gate_gen, generation,
                "preparing a dial is passive"
            );
            let request = s.engine.take_remote_radio().unwrap();
            let rounded = (dial * 1e6).round() / 1e6;
            native.engine.set_frequency(rounded, band, "USB");
            native.engine.take_immediate_retune();
            assert_eq!(request.target_hz, native.engine.settings.dial_hz());
            assert_eq!(request.target_mode, native.engine.rig_mode_effective());
            s.sample(request.target_hz, &request.target_mode);
            let commit_started = Instant::now();
            assert!(
                request.commit(&mut s.engine),
                "{dial}/{band}: queue-to-commit {:?}, commit {:?}, outcome {:?}",
                commit_started.duration_since(queued_at),
                commit_started.elapsed(),
                receipt.outcome()
            );
            assert_eq!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                },
                "{dial}/{band}: queue-to-commit {:?}, commit {:?}",
                commit_started.duration_since(queued_at),
                commit_started.elapsed()
            );
            assert_eq!(
                serde_json::to_value(s.engine.settings()).unwrap(),
                serde_json::to_value(native.engine.settings()).unwrap()
            );
            assert_eq!(s.engine.freq_memory, native.engine.freq_memory);
            assert!(!s.engine.freq_memory.keys().any(|(band, _)| band.is_empty()));
            assert_eq!(s.engine.tx_gate_gen, native.engine.tx_gate_gen);
            assert_eq!(s.engine.tx_allowed(), native.engine.tx_allowed());
            assert!(!s.engine.tx_enabled());
            assert!(!s.engine.take_immediate_retune());
            assert!(std::sync::Arc::ptr_eq(&source, &s.engine.source));
            let saved: Settings = serde_json::from_slice(&std::fs::read(&s.path).unwrap()).unwrap();
            assert_eq!(saved.dial_hz(), native.engine.settings.dial_hz());
            assert_eq!(saved.band, band);
        }
    }
}

#[test]
fn off_band_receive_refuses_an_expired_idle_sample_then_accepts_a_fresh_gesture() {
    let mut s = Station::new(OperatingMode::Phone);
    let old = s
        .engine
        .remote_radio_read(&s.connection, Instant::now() - Duration::from_secs(2))
        .unwrap();
    s.engine.remote_observe_ptt(Some(&old), Some(false));
    assert!(matches!(
        queue_dial(&mut s, 10.0, ""),
        Err(Reason::ReadingUnavailable)
    ));
    assert!(s.engine.take_remote_radio().is_none());
    assert!(!s.path.exists());
    let mode = s.engine.rig_mode_effective();
    s.sample(14_074_000, &mode);
    let receipt = queue_dial(&mut s, 10.0, "").unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.sample(request.target_hz, &request.target_mode);
    assert!(request.commit(&mut s.engine));
    assert!(matches!(receipt.outcome(), Outcome::Applied { .. }));
    assert!(!s.engine.tx_enabled());
}

#[test]
fn a_bandless_remote_dial_uses_native_bandless_routing_instead_of_catch_all_coverage() {
    let mut s = Station::new(OperatingMode::Phone);
    let mut other = s.engine.settings.radios[0].clone();
    other.id = 9;
    other.name = "Other receive radio".into();
    other.bands.clear();
    s.engine.settings.radios[0].bands = vec!["20m".into()];
    s.engine.settings.radios.push(other);
    s.engine.settings.default_radio = Some(9);
    let route_mode = s.engine.route_mode("", 10.0);
    assert_eq!(
        s.engine.settings.route_radio("", route_mode),
        Some(9),
        "the wrong resolver would hand off"
    );
    assert_eq!(s.engine.settings.route_radio_bandless(route_mode), None);
    let receipt = queue_dial(&mut s, 10.0, "").unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.sample(request.target_hz, &request.target_mode);
    assert!(request.commit(&mut s.engine));
    assert_eq!(
        receipt.outcome(),
        Outcome::Applied {
            evidence: Evidence::RadioReadback
        }
    );
    assert_eq!(s.engine.settings.active_radio, 0);

    s.engine
        .settings
        .routing_rules
        .push(crate::settings::RoutingRule {
            bands: vec![],
            mode: None,
            radio: 9,
            context: None,
        });
    assert_eq!(s.engine.settings.route_radio_bandless(route_mode), Some(9));
    assert!(matches!(
        queue_dial(&mut s, 9.5, ""),
        Err(Reason::UnsupportedAction)
    ));
    assert!(s.engine.take_remote_radio().is_none());
    assert_eq!(s.engine.settings.dial_hz(), 10_000_000);
    s.engine.set_radio_pegged(true);
    queue_dial(&mut s, 9.5, "").unwrap();
    assert!(
        s.engine.take_remote_radio().is_some(),
        "pegging keeps the intended radio"
    );
}

#[test]
fn bandless_tuning_keeps_exact_band_validation_and_native_idle_authority() {
    let mut s = Station::new(OperatingMode::Digital);
    for (dial, band) in [
        (10.0, "20m"),
        (14.074, ""),
        (0.000_000_1, ""),
        (f64::NAN, ""),
        (f64::INFINITY, ""),
        (0.0, ""),
        (250_001.0, ""),
    ] {
        assert!(matches!(
            queue_dial(&mut s, dial, band),
            Err(Reason::InvalidAction)
        ));
        assert!(s.engine.take_remote_radio().is_none());
        assert!(!s.path.exists());
    }
    let receipt = queue_dial(&mut s, 10.0, "").unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.authority.revoke();
    s.sample(request.target_hz, &request.target_mode);
    assert!(!request.commit(&mut s.engine));
    assert!(matches!(receipt.outcome(), Outcome::Rejected { .. }));
    assert_eq!(s.engine.settings.dial_hz(), 14_074_000);
    assert!(!s.path.exists());
    // Restore the actual idle read, then demonstrate that a fresh gesture works.
    let mode = s.engine.rig_mode_effective();
    s.sample(14_074_000, &mode);
    queue_dial(&mut s, 10.0, "").unwrap();
    s.engine.take_remote_radio().unwrap();
    s.engine.set_tx_enabled(true);
    assert!(matches!(
        queue_dial(&mut s, 9.5, ""),
        Err(Reason::StationBusy)
    ));
}
