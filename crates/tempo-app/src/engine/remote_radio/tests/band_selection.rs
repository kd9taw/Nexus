use super::*;
use crate::settings::LicenseClass;

fn queue(s: &mut Station, band: &str, mode: &str) -> Result<Completion, Reason> {
    let connection = s
        .engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation;
    s.engine.queue_remote_band(
        band,
        mode,
        connection,
        s.authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap(),
    )
}

#[test]
fn band_selection_matches_native_defaults_for_every_desktop_choice_and_class() {
    for class in [
        LicenseClass::Technician,
        LicenseClass::General,
        LicenseClass::Extra,
        LicenseClass::Open,
    ] {
        for (mode, name) in [(OperatingMode::Cw, "cw"), (OperatingMode::Phone, "phone")] {
            for channel in crate::bandplan::licensed_bands(class, mode) {
                let mut s = Station::new(mode);
                let mut native = Station::new(mode);
                s.engine.settings.license_class = class;
                native.engine.settings.license_class = class;
                let expected = native.engine.prepare_band_pick(&channel.band, mode);
                // Prepare the independent native oracle before opening the
                // short remote permission window. Its setup is not radio I/O.
                native.engine.pick_band(&channel.band, Some(name));
                native.engine.take_immediate_retune();
                let before = serde_json::to_value(s.engine.settings()).unwrap();
                let memory = s.engine.freq_memory.clone();
                let current_mode = s.engine.rig_mode_effective();
                s.sample(s.engine.settings.dial_hz(), &current_mode);
                let queued_at = Instant::now();
                let result = queue(&mut s, &channel.band, name);
                assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), before);
                assert_eq!(s.engine.freq_memory, memory);
                if expected.is_none() {
                    assert!(matches!(result, Err(Reason::UnsupportedAction)));
                    assert_eq!(
                        native.engine.settings.dial_hz(),
                        s.engine.settings.dial_hz()
                    );
                    continue;
                }
                let receipt = result.unwrap();
                let request = s.engine.take_remote_radio().unwrap();
                assert_eq!(
                    request.target_hz,
                    native.engine.settings.dial_hz(),
                    "{class:?}/{name}/{}",
                    channel.band
                );
                assert_eq!(request.target_mode, native.engine.rig_mode_effective());
                s.sample(request.target_hz, &request.target_mode);
                let commit_started = Instant::now();
                assert!(
                    request.commit(&mut s.engine),
                    "{class:?}/{name}/{}: queue-to-commit {:?}, commit {:?}, outcome {:?}",
                    channel.band,
                    commit_started.duration_since(queued_at),
                    commit_started.elapsed(),
                    receipt.outcome()
                );
                assert_eq!(
                    receipt.outcome(),
                    Outcome::Applied {
                        evidence: Evidence::RadioReadback
                    },
                    "{class:?}/{name}/{}: queue-to-commit {:?}, commit {:?}",
                    channel.band,
                    commit_started.duration_since(queued_at),
                    commit_started.elapsed()
                );
                assert_eq!(
                    serde_json::to_value(s.engine.settings()).unwrap(),
                    serde_json::to_value(native.engine.settings()).unwrap()
                );
                assert_eq!(s.engine.freq_memory, native.engine.freq_memory);
                assert_eq!(s.engine.tx_gate_gen, native.engine.tx_gate_gen);
                assert_eq!(s.engine.tx_allowed(), native.engine.tx_allowed());
                assert!(!s.engine.tx_enabled());
                let saved: Settings =
                    serde_json::from_slice(&std::fs::read(&s.path).unwrap()).unwrap();
                assert_eq!(saved.dial_hz(), native.engine.settings.dial_hz());
                native.engine.settings.save(&native.path).unwrap();
                let native_saved: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&native.path).unwrap()).unwrap();
                let remote_saved: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&s.path).unwrap()).unwrap();
                assert_eq!(
                    remote_saved, native_saved,
                    "persist the same full profile/settings projection as the native pick"
                );
            }
        }
    }
}

#[test]
fn band_selection_recalls_mode_memory_and_banks_the_current_residency_like_native() {
    for (mode, name, dial) in [
        (OperatingMode::Cw, "cw", 14.055),
        (OperatingMode::Phone, "phone", 14.275),
    ] {
        let mut s = Station::new(mode);
        let mut native = Station::new(mode);
        for station in [&mut s, &mut native] {
            station.engine.set_frequency(dial, "20m", "USB");
            station.engine.take_immediate_retune();
            station.sample(
                station.engine.settings.dial_hz(),
                &station.engine.rig_mode_effective(),
            );
        }
        for band in ["20m", "40m", "20m", "20m"] {
            let memory = s.engine.freq_memory.clone();
            let receipt = queue(&mut s, band, name).unwrap();
            assert_eq!(s.engine.freq_memory, memory);
            let request = s.engine.take_remote_radio().unwrap();
            native.engine.pick_band(band, Some(name));
            native.engine.take_immediate_retune();
            if band == "20m" {
                assert_eq!(request.target_hz, (dial * 1e6).round() as u64);
            }
            assert_eq!(request.target_hz, native.engine.settings.dial_hz());
            s.sample(request.target_hz, &request.target_mode);
            assert!(request.commit(&mut s.engine));
            assert_eq!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            );
            assert_eq!(s.engine.freq_memory, native.engine.freq_memory);
            assert_eq!(
                serde_json::to_value(s.engine.settings()).unwrap(),
                serde_json::to_value(native.engine.settings()).unwrap()
            );
            assert!(!s.engine.take_immediate_retune());
        }
    }
}

#[test]
fn band_selection_refuses_invalid_context_and_a_changed_memory_before_commit() {
    let mut s = Station::new(OperatingMode::Phone);
    for (band, mode) in [
        ("", "phone"),
        ("20m;T 1", "phone"),
        ("40m", "cw"),
        ("40m", "digital"),
        ("60m", "phone"),
    ] {
        assert!(queue(&mut s, band, mode).is_err());
        assert!(s.engine.take_remote_radio().is_none());
    }
    let receipt = queue(&mut s, "40m", "phone").unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.engine.freq_memory.insert(
        ("40m".into(), OperatingMode::Phone),
        (7.255, Some("LSB".into())),
    );
    s.sample(request.target_hz, &request.target_mode);
    assert!(!request.commit(&mut s.engine));
    assert_eq!(
        receipt.outcome(),
        Outcome::Rejected {
            reason: Reason::ContextChanged
        }
    );
    assert!(!s.path.exists());
    // A fresh explicit gesture resolves the new station memory; no retry of the old request.
    s.sample(s.engine.settings.dial_hz(), &s.engine.rig_mode_effective());
    let receipt = queue(&mut s, "40m", "phone").unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    assert_eq!(request.target_hz, 7_255_000);
    s.authority.revoke();
    s.sample(request.target_hz, &request.target_mode);
    assert!(!request.commit(&mut s.engine));
    assert!(matches!(receipt.outcome(), Outcome::Rejected { .. }));
    assert!(!s.path.exists());
}
