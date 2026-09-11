use super::*;

fn queue(
    s: &mut Station,
    mode: &str,
    dial: f64,
    band: &str,
    call: &str,
) -> Result<Completion, Reason> {
    let connection = s
        .engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation;
    s.engine.queue_remote_spot(
        mode,
        dial,
        band,
        call,
        connection,
        s.authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap(),
    )
}

#[test]
fn spot_commit_matches_native_context_and_exact_frequency_without_arming() {
    for (mode, dial, band) in [("cw", 7.02345, "40m"), ("phone", 7.19876, "40m")] {
        let mut s = Station::new(OperatingMode::Digital);
        let mut native = Station::new(OperatingMode::Digital);
        let before = serde_json::to_value(&s.engine.settings).unwrap();
        let tick = s.engine.work_tick;
        let receipt = queue(&mut s, mode, dial, band, "n2spot/p").unwrap();
        assert_eq!(serde_json::to_value(&s.engine.settings).unwrap(), before);
        assert_eq!(s.engine.work_tick, tick);
        assert!(!s.path.exists());
        assert!(!s.engine.tx_enabled());
        let request = s.engine.take_remote_radio().unwrap();

        native.engine.work_spot(mode, dial, band);
        native.engine.note_work_call(Some("N2SPOT/P".into()));
        assert!(
            native.engine.tx_enabled(),
            "local manual entry retains its established arming"
        );
        assert_eq!(
            request.target(),
            (
                native.engine.settings.dial_hz(),
                native.engine.rig_mode_effective().as_str()
            )
        );
        let target = request.target().1.to_owned();
        let power = request.power_limit();
        request.permission().begin_write(Instant::now()).unwrap();
        s.sample((dial * 1e6).round() as u64, &target);
        assert!(request.commit_readback(&mut s.engine, power));
        assert!(matches!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        ));
        assert!(!s.engine.tx_enabled());
        assert!(
            !s.engine.take_immediate_retune(),
            "readback consumes the sole CAT QSY"
        );
        assert_eq!(s.engine.work_tick, tick + 1);
        assert_eq!(s.engine.work_view.as_deref(), Some(mode));
        assert_eq!(s.engine.work_call.as_deref(), Some("N2SPOT/P"));
        assert_eq!(s.engine.sideband_override, None);
        native.engine.settings.save(&native.path).unwrap();
        let saved: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&s.path).unwrap()).unwrap();
        let local: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&native.path).unwrap()).unwrap();
        assert_eq!(
            saved, local,
            "the full native profile and frequency memories round-trip"
        );
    }
}

#[test]
fn spot_rejects_invalid_targets_before_changing_the_station() {
    for (mode, dial, band, call) in [
        ("digital", 14.074, "20m", "N2SPOT"),
        ("phone", f64::NAN, "20m", "N2SPOT"),
        ("cw", 14.025, "40m", "N2SPOT"),
        ("cw", 10.0, "", "N2SPOT"),
        ("cw", 14.025, "20m", ""),
        ("cw", 14.025, "20m", "N2 SPOT"),
        ("cw", 14.025, "20m", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
    ] {
        let mut s = Station::new(OperatingMode::Digital);
        let before = serde_json::to_value(&s.engine.settings).unwrap();
        assert!(matches!(
            queue(&mut s, mode, dial, band, call),
            Err(Reason::InvalidAction)
        ));
        assert_eq!(serde_json::to_value(&s.engine.settings).unwrap(), before);
        assert!(s.engine.take_remote_radio().is_none());
        assert_eq!(s.engine.work_tick, 0);
        assert!(!s.engine.tx_enabled());
        assert!(!s.path.exists());
    }
}

#[test]
fn spot_needs_later_readback_and_live_permission_before_handoff() {
    for revoke in [false, true] {
        let mut s = Station::new(OperatingMode::Digital);
        let before = serde_json::to_value(&s.engine.settings).unwrap();
        let receipt = queue(&mut s, "cw", 7.025, "40m", "N2SPOT").unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        if revoke {
            s.authority.revoke();
            s.sample(7_025_000, "CW");
        }
        assert!(!request.commit(&mut s.engine));
        assert!(!matches!(receipt.outcome(), Outcome::Applied { .. }));
        assert_eq!(serde_json::to_value(&s.engine.settings).unwrap(), before);
        assert_eq!(s.engine.work_tick, 0);
        assert!(s.engine.work_call.is_none());
        assert!(!s.path.exists());
    }
}
