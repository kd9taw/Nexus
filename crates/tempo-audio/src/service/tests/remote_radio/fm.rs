//! Actual radio owner, including subsequent native ticks and persisted choices.
use super::*;

pub(super) fn writes(peer: &Peer) -> Vec<String> {
    peer.lines
        .lock()
        .unwrap()
        .iter()
        .filter(|s| {
            ["M ", "F ", "T ", "L ", "U ", "R ", "O ", "C "]
                .iter()
                .any(|p| s.starts_with(p))
        })
        .cloned()
        .collect()
}

fn peer(mode: &str, ignore_tone: bool) -> Peer {
    let fm = Mutex::new((String::from("None"), 0_i64, 0_u32));
    retuning_peer(145_500_000, mode, move |line, _| {
        let mut fm = fm.lock().unwrap();
        match line {
            "r" => return Some(format!("{}\n", fm.0)),
            "o" => return Some(format!("{}\n", fm.1)),
            "c" => return Some(format!("{}\n", fm.2)),
            "l RFPOWER" => return Some("0.1\n".into()),
            _ => {}
        }
        if let Some(value) = line.strip_prefix("R ") {
            fm.0 = value.into();
        } else if let Some(value) = line.strip_prefix("O ") {
            fm.1 = value.parse().unwrap();
        } else if let Some(value) = line.strip_prefix("C ") {
            if !ignore_tone {
                fm.2 = value.parse().unwrap();
            }
        } else {
            return None;
        }
        Some("RPRT 0\n".into())
    })
}

pub(super) fn station(peer: &Peer, fm: bool, offset: i64) -> Station {
    let mut s = Station::configured(peer, |settings| {
        settings.operating_mode = tempo_app::settings::OperatingMode::Phone;
        settings.dial_mhz = 145.5;
        settings.band = "2m".into();
        settings.phone_mode = if fm { "fm" } else { "ssb" }.into();
        settings.rptr_shift = "+".into();
        settings.rptr_offset_override_hz = offset;
        settings.ctcss_tone_hz = 88.55;
        settings.ensure_radio_profiles();
    });
    s.state.last_dial = 145_500_000;
    let read = s.state.remote_read(&s.engine).unwrap();
    let mut e = engine_lock(&s.engine);
    s.state.last_fm = fm.then(|| e.fm_repeater_config());
    e.remote_observe_cat(Some(&read), Some(true));
    e.remote_observe_dial(Some(&read), Some(145_500_000));
    e.remote_observe_mode(Some(&read), Some(if fm { "FM" } else { "USB" }));
    e.remote_observe_ptt(Some(&read), Some(false));
    drop(e);
    s
}

#[test]
fn remote_fm_picker_configures_once_and_preserves_raw_native_choices() {
    for ignore_tone in [false, true] {
        let peer = peer("USB", ignore_tone);
        let mut s = station(&peer, false, 0);
        let receipt = {
            let mut e = engine_lock(&s.engine);
            let connection = e
                .remote_monitor_observation()
                .radio
                .readings
                .cat
                .unwrap()
                .connection_generation;
            e.queue_remote_phone_mode(
                None,
                Some("FM"),
                connection,
                s.authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap(),
            )
            .unwrap()
        };
        s.step();
        assert_eq!(
            matches!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            ),
            !ignore_tone
        );
        if ignore_tone {
            assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
        }
        let sent = writes(&peer);
        for command in ["R +", "O 600000", "C 886"] {
            assert!(sent.iter().any(|s| s == command), "{sent:?}");
        }
        assert_eq!(
            s.state.last_fm,
            (!ignore_tone).then(|| ("+".into(), 600_000, 88.55))
        );
        for _ in 0..3 {
            s.step();
        }
        assert_eq!(
            writes(&peer),
            sent,
            "confirmed/unknown FM configuration cannot replay"
        );
        let mut e = engine_lock(&s.engine);
        assert_eq!(
            e.snapshot().radio.sideband_override.as_deref(),
            (!ignore_tone).then_some("FM")
        );
        assert_eq!(e.settings().ctcss_tone_hz, 88.55);
        assert_eq!(e.settings().rptr_offset_override_hz, 0);
        assert!(!e.tx_enabled());
        assert!(!e.take_immediate_retune());
        assert!(!s.path.exists(), "the native picker is transient");
    }
}

#[test]
fn remote_fm_cross_band_tuning_uses_target_band_or_saved_odd_split_without_replay() {
    for offset in [0, 1_000_000] {
        let peer = peer("FM", false);
        let mut s = station(&peer, true, offset);
        let receipt = s.queue_dial(433.5, "70cm");
        s.step();
        assert_eq!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
        let expected_offset = if offset == 0 { 5_000_000 } else { offset };
        assert_eq!(s.state.last_fm, Some(("+".into(), expected_offset, 88.55)));
        let sent = writes(&peer);
        assert!(sent.contains(&format!("O {expected_offset}")), "{sent:?}");
        assert!(sent.contains(&"F 433500000".into()), "{sent:?}");
        for _ in 0..3 {
            s.step();
        }
        assert_eq!(writes(&peer), sent);
        let saved: Settings = serde_json::from_slice(&std::fs::read(&s.path).unwrap()).unwrap();
        assert_eq!(saved.dial_mhz, 433.5);
        assert_eq!(saved.rptr_offset_override_hz, offset);
        assert_eq!(saved.ctcss_tone_hz, 88.55);
        assert_eq!(saved.phone_mode, "fm");
        assert!(!engine_lock(&s.engine).tx_enabled());
        assert!(s.backend.played.is_empty());
    }
}

#[test]
fn remote_fm_partial_cross_band_failure_cannot_replay_after_later_dial_polling() {
    for (ignore_tone, remote_recovery) in [(false, false), (true, false), (true, true)] {
        let peer = peer("FM", ignore_tone);
        let mut s = station(&peer, true, 0);
        // This scene advances an explicit clock from zero. A failed request
        // correctly leaves the ordinary poll timers untouched, so initialize
        // those timers to the same clock instead of their wall-clock defaults.
        s.state.last_rig_poll = 0.0;
        s.state.last_freq_poll = 0.0;
        let receipt = s.queue_dial(433.5, "70cm");
        s.step();
        assert_eq!(
            matches!(receipt.outcome(), Outcome::Applied { .. }),
            !ignore_tone
        );
        if ignore_tone {
            assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
        }
        let initial = writes(&peer);
        let initial_reads = peer
            .lines
            .lock()
            .unwrap()
            .iter()
            .filter(|s| *s == "f")
            .count();
        for now in [200.0, 400.0, 800.0, 1000.0, 1600.0, 2400.0] {
            s.state
                .step(
                    &s.engine,
                    &mut s.backend,
                    &mut s.rig,
                    &no_sinks(),
                    now,
                    &mut mock_reopen_audio(),
                    &mut mock_reopen_rig(),
                    &mut StationSinks::new(),
                )
                .unwrap();
        }
        let later_reads = peer
            .lines
            .lock()
            .unwrap()
            .iter()
            .filter(|s| *s == "f")
            .count();
        assert!(
            later_reads > initial_reads,
            "the test must actually run later native dial polls: control={} cat={:?} last_dial={} freq_poll={} rig_poll={} lines={:?}", s.rig.has_control(), s.state.cat_ok, s.state.last_dial, s.state.last_freq_poll, s.state.last_rig_poll, peer.lines.lock().unwrap()
        );
        assert_eq!(writes(&peer).into_iter().filter(|s| s != "T 0").collect::<Vec<_>>(),
            initial.into_iter().filter(|s| s != "T 0").collect::<Vec<_>>(),
            "later CAT observations must not replay an uncertain remote transaction; ignored tone: {ignore_tone}");
        assert_eq!(s.state.remote_retune_uncertain, ignore_tone);
        if ignore_tone {
            let before_local = writes(&peer).len();
            let recovery = if remote_recovery {
                let mut e = engine_lock(&s.engine);
                let connection = e
                    .remote_monitor_observation()
                    .radio
                    .readings
                    .cat
                    .unwrap()
                    .connection_generation;
                Some(
                    e.queue_remote_phone_mode(
                        None,
                        Some("USB"),
                        connection,
                        s.authority
                            .permit(Instant::now() + Duration::from_secs(5))
                            .unwrap(),
                    )
                    .unwrap(),
                )
            } else {
                engine_lock(&s.engine).set_frequency(433.5, "70cm", "USB");
                None
            };
            s.state
                .step(
                    &s.engine,
                    &mut s.backend,
                    &mut s.rig,
                    &no_sinks(),
                    2600.0,
                    &mut mock_reopen_audio(),
                    &mut mock_reopen_rig(),
                    &mut StationSinks::new(),
                )
                .unwrap();
            if let Some(recovery) = recovery {
                assert_eq!(
                    recovery.outcome(),
                    Outcome::Applied {
                        evidence: Evidence::RadioReadback
                    }
                );
            }
            assert!(
                !s.state.remote_retune_uncertain,
                "an explicit local or confirmed remote pick restores reconciliation"
            );
            assert!(
                writes(&peer).len() > before_local,
                "the operator's new action must actually command the rig"
            );
            assert!(
                matches!(receipt.outcome(), Outcome::Unknown { .. }),
                "new intent does not rewrite the old receipt"
            );
        }
        assert!(!engine_lock(&s.engine).tx_enabled());
        assert!(s.backend.played.is_empty());
    }
}
