use super::*;
use crate::rig::remote_tests::level_peer;
use tempo_app::engine::remote_radio::RadioLevel;

#[test]
fn remote_levels_actual_owner_adopts_each_setting_and_never_replays_unconfirmed_work() {
    for level in [
        RadioLevel::Power,
        RadioLevel::MicGain,
        RadioLevel::NoiseReduction,
        RadioLevel::Compression,
        RadioLevel::NotchFrequency,
    ] {
        for confirmed in [false, true] {
            let before = if level == RadioLevel::NotchFrequency {
                600.0
            } else {
                0.5
            };
            let target = if level == RadioLevel::NotchFrequency {
                1500.0
            } else {
                0.35
            };
            let peer = level_peer(level, before, move |line, _, _| {
                (!confirmed && line.starts_with("L ")).then(|| "RPRT 0\n".into())
            });
            let mut station = Station::configured(&peer, |settings| {
                settings.operating_mode = tempo_app::settings::OperatingMode::Phone;
                settings.ensure_radio_profiles();
            });
            let read = station.state.remote_read(&station.engine).unwrap();
            let receipt = {
                let mut e = engine_lock(&station.engine);
                match level {
                    RadioLevel::Power => e.observe_rig_power(before),
                    RadioLevel::MicGain => e.observe_rig_mic_gain(before),
                    RadioLevel::NoiseReduction => e.observe_rig_nr_level(before),
                    RadioLevel::Compression => e.observe_rig_comp_level(before),
                    RadioLevel::NotchFrequency => e.observe_rig_notch_freq_hz(before),
                }
                e.remote_observe_mode(Some(&read), Some("LSB"));
                let connection = e
                    .remote_monitor_observation()
                    .radio
                    .readings
                    .cat
                    .unwrap()
                    .connection_generation;
                e.queue_remote_level(
                    "phone",
                    level,
                    before,
                    target,
                    connection,
                    station
                        .authority
                        .permit(Instant::now() + Duration::from_secs(5))
                        .unwrap(),
                )
                .unwrap()
            };
            station.step();
            assert_eq!(
                matches!(
                    receipt.outcome(),
                    Outcome::Applied {
                        evidence: Evidence::RadioReadback
                    }
                ),
                confirmed,
                "{level:?}"
            );
            if !confirmed {
                assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
            }
            for _ in 0..3 {
                station.step();
            }
            let commands = peer.lines.lock().unwrap();
            let changes: Vec<_> = commands.iter().filter(|s| s.starts_with("L ")).collect();
            assert_eq!(changes.len(), 1, "{level:?} {confirmed} {changes:?}");
            assert!(!engine_lock(&station.engine).tx_enabled());
        }
    }
}
