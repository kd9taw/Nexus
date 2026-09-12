use super::*;

fn queue(s: &mut Station, mode: &str, dial: f64) -> Completion {
    let mut engine = engine_lock(&s.engine);
    let connection = engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation;
    engine
        .queue_remote_spot(
            mode,
            dial,
            "40m",
            "N2SPOT/P",
            connection,
            s.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
        .unwrap()
}

#[test]
fn spot_crosses_the_actual_radio_owner_with_one_exact_qsy_and_no_tx_or_native_replay() {
    // 40 m CW commands plain `CW`, not `CWR`. The band rule was reversed on
    // 2026-09-11: there is no CW sideband convention, and `CWR` means opposite
    // sides on Yaesu and Icom. The preference lives on as `cw_reverse`, default OFF.
    for (mode, dial, cat) in [("cw", 7.02345_f64, "CW"), ("phone", 7.19876, "LSB")] {
        let peer = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
        let mut s = Station::new(&peer);
        let tick = engine_lock(&s.engine).snapshot().work_tick;
        let receipt = queue(&mut s, mode, dial);
        assert!(writes(&peer).is_empty());
        assert_eq!(engine_lock(&s.engine).snapshot().work_tick, tick);
        s.step();
        assert_eq!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
        let commands = writes(&peer);
        assert_eq!(commands.len(), 2, "{commands:?}");
        assert!(commands[0].starts_with(&format!("M {cat} ")));
        assert_eq!(commands[1], format!("F {}", (dial * 1e6).round() as u64));
        let stored = Settings::load(&s.path);
        assert_eq!(stored.dial_hz(), (dial * 1e6).round() as u64);
        assert_eq!(stored.band, "40m");
        assert_eq!(engine_lock(&s.engine).snapshot().work_tick, tick + 1);
        assert_eq!(
            engine_lock(&s.engine).snapshot().work_call.as_deref(),
            Some("N2SPOT/P")
        );
        s.authority.revoke();
        for _ in 0..3 {
            s.step();
        }
        assert_eq!(writes(&peer), commands);
        assert_eq!(s.rig.read_freq().unwrap(), stored.dial_hz());
        assert!(!engine_lock(&s.engine).tx_enabled());
        assert!(s.backend.played.is_empty());
    }
}

#[test]
fn unconfirmed_spot_never_prefills_navigates_saves_or_replays_the_remote_target() {
    let peer = retuning_peer(14_074_000, "PKTUSB", |line, _| {
        (line == "F 7023450").then(|| "RPRT -1\n".into())
    });
    let mut s = Station::new(&peer);
    let tick = engine_lock(&s.engine).snapshot().work_tick;
    let receipt = queue(&mut s, "cw", 7.02345);
    s.step();
    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    let commands = writes(&peer);
    assert!(
        commands.iter().any(|line| line == "F 7023450"),
        "positive control reached the radio boundary"
    );
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(writes(&peer), commands);
    assert!(!s.path.exists());
    let engine = engine_lock(&s.engine);
    assert_eq!(engine.settings().dial_hz(), 14_074_000);
    assert_eq!(engine.snapshot().work_tick, tick);
    assert_eq!(engine.snapshot().work_call, None);
    assert!(!engine.tx_enabled());
    assert!(s.backend.played.is_empty());
}
