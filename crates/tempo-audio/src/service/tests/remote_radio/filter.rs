use super::*;
use crate::rig::remote_tests::filtering_peer;

#[test]
fn remote_filter_uses_the_actual_radio_owner_without_replaying_or_overriding_a_front_panel_mode() {
    let peer = filtering_peer(14_074_000, "LSB", 2400, |_, _, _| None);
    let mut s = Station::configured(&peer, |settings| {
        settings.operating_mode = tempo_app::settings::OperatingMode::Phone;
        settings.ensure_radio_profiles();
    });
    let canonical_mode = s.state.last_mode.clone();
    assert_ne!(
        canonical_mode, "LSB",
        "positive control for a native mode mismatch"
    );
    let read = s.state.remote_read(&s.engine).unwrap();
    let receipt = {
        let mut e = engine_lock(&s.engine);
        e.settings().save(&s.path).unwrap();
        e.observe_rig_passband(Some(2400));
        e.remote_observe_mode(Some(&read), Some("LSB"));
        let connection = e
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        e.queue_remote_filter_width(
            "phone",
            2400,
            2300,
            connection,
            s.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
        .unwrap()
    };
    let file = std::fs::read(&s.path).unwrap();
    s.step();
    assert!(matches!(
        receipt.outcome(),
        Outcome::Applied {
            evidence: Evidence::RadioReadback
        }
    ));
    assert_eq!(
        engine_lock(&s.engine).snapshot().radio.filter_width_hz,
        Some(2300)
    );
    assert_eq!(s.state.last_mode, canonical_mode);
    assert_eq!(writes(&peer), vec!["M LSB 2300"]);
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(writes(&peer), vec!["M LSB 2300"]);
    assert_eq!(std::fs::read(&s.path).unwrap(), file);
    let mut e = engine_lock(&s.engine);
    assert_eq!(e.take_passband_request(), None);
    assert!(!e.tx_enabled());
}
