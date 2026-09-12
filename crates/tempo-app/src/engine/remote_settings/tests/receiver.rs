use super::*;

fn choices(s: &Station) -> [DecoderSetting; 2] {
    [
        DecoderSetting::Depth {
            tier: s.engine.tier(),
            expected: s.engine.settings.decode_depth,
            depth: 1,
        },
        DecoderSetting::RxOffset {
            tier: s.engine.tier(),
            expected: s.engine.rx_offset_hz(),
            hz: 725.25,
        },
    ]
}

#[test]
fn receiver_settings_match_native_across_digital_tiers_without_touching_tx_or_other_preferences() {
    for tier in [
        Tier::Ft8,
        Tier::Ft4,
        Tier::Ft2,
        Tier::Q65,
        Tier::Msk144,
        Tier::Jt65,
        Tier::Fst4,
        Tier::Fst4w,
        Tier::Wspr,
        Tier::Js8,
        Tier::TempoFast,
        Tier::TempoDeep,
    ] {
        for hold in [false, true] {
            let mut remote = Station::new(tier);
            let mut native = Station::new(tier);
            remote.engine.set_hold_tx_freq(hold);
            native.engine.set_hold_tx_freq(hold);
            let tx = remote.engine.tx_offset_hz();
            let source = remote.engine.source.clone();
            let generation = remote.engine.tx_gate_gen;
            let epoch = remote.engine.decode_epoch;
            for (depth, hz) in [(1, 200.0), (2, 4000.0), (3, 725.25)] {
                remote.sample(Some(false), Duration::ZERO);
                remote
                    .apply(DecoderSetting::Depth {
                        tier,
                        expected: remote.engine.settings.decode_depth,
                        depth,
                    })
                    .unwrap();
                native.engine.set_decode_depth(depth);
                remote.sample(Some(false), Duration::ZERO);
                remote
                    .apply(DecoderSetting::RxOffset {
                        tier,
                        expected: remote.engine.rx_offset_hz(),
                        hz,
                    })
                    .unwrap();
                native.engine.set_rx_offset(hz);
                assert_eq!(settings(&remote.engine), settings(&native.engine));
                let saved: Settings =
                    serde_json::from_slice(&std::fs::read(&remote.path).unwrap()).unwrap();
                assert_eq!(
                    serde_json::to_value(saved).unwrap(),
                    settings(&native.engine)
                );
                assert_eq!(remote.engine.rx_offset_hz(), hz);
                assert_eq!(
                    remote.engine.tx_offset_hz(),
                    tx,
                    "RX never follows or drags TX, even with Hold off"
                );
                assert_eq!(remote.engine.tx_gate_gen, generation);
                assert_eq!(remote.engine.decode_epoch, epoch);
                assert_eq!(
                    remote.engine.active_slot_secs(),
                    native.engine.active_slot_secs()
                );
                assert_eq!(
                    remote.engine.active_capture_samples(),
                    native.engine.active_capture_samples()
                );
                assert!(std::sync::Arc::ptr_eq(&source, &remote.engine.source));
                assert!(!remote.engine.tx_enabled());
                assert!(!remote.engine.take_immediate_retune());
                assert!(remote.engine.take_remote_radio().is_none());
            }
        }
    }
}

#[test]
fn receiver_choices_do_not_wait_for_the_decoder_source_or_replace_it() {
    let mut s = Station::new(Tier::Ft8);
    let source = s.engine.source.clone();
    let held = source.lock().unwrap();
    for setting in choices(&s) {
        s.sample(Some(false), Duration::ZERO);
        let started = Instant::now();
        s.apply(setting).unwrap();
        assert!(started.elapsed() < Duration::from_secs(1));
    }
    assert!(std::sync::Arc::ptr_eq(&source, &s.engine.source));
    drop(held);
    assert_eq!(s.engine.settings.decode_depth, 1);
    assert_eq!(s.engine.rx_offset_hz(), 725.25);
}

#[test]
fn receiver_save_failure_preserves_the_old_file_runtime_and_tx_marker() {
    for kind in 0..2 {
        let mut s = Station::new(Tier::Js8);
        s.engine.settings.save(&s.path).unwrap();
        let original_file = std::fs::read(&s.path).unwrap();
        let original_settings = settings(&s.engine);
        let rx = s.engine.rx_offset_hz();
        let tx = s.engine.tx_offset_hz();
        let blocker = s.path.parent().unwrap().join("not-a-directory");
        std::fs::write(&blocker, b"keep this file").unwrap();
        s.engine
            .configure_remote_settings_store(blocker.join("settings.json"));
        assert_eq!(s.apply(choices(&s)[kind]), Err(Reason::PersistenceFailed));
        assert_eq!(std::fs::read(&s.path).unwrap(), original_file);
        assert_eq!(std::fs::read(&blocker).unwrap(), b"keep this file");
        assert_eq!(settings(&s.engine), original_settings);
        assert_eq!(s.engine.rx_offset_hz(), rx);
        assert_eq!(s.engine.tx_offset_hz(), tx);
    }
}

#[test]
fn receiver_choices_refuse_bad_values_stale_tiers_and_expired_authority_before_saving() {
    let tier = Tier::Ft8;
    let mut s = Station::new(tier);
    let original = settings(&s.engine);
    for depth in [0, 3, 4, 255] {
        assert_eq!(
            s.apply(DecoderSetting::Depth {
                tier,
                expected: 3,
                depth
            }),
            Err(Reason::InvalidAction)
        );
    }
    for hz in [
        f32::NAN,
        f32::INFINITY,
        f32::NEG_INFINITY,
        199.0,
        4001.0,
        1500.0,
    ] {
        assert_eq!(
            s.apply(DecoderSetting::RxOffset {
                tier,
                expected: 1500.0,
                hz
            }),
            Err(Reason::InvalidAction)
        );
    }
    assert_eq!(
        s.apply(DecoderSetting::RxOffset {
            tier,
            expected: f32::NAN,
            hz: 800.0
        }),
        Err(Reason::InvalidAction)
    );
    for setting in [
        DecoderSetting::Depth {
            tier,
            expected: 2,
            depth: 1,
        },
        DecoderSetting::Depth {
            tier: Tier::Js8,
            expected: 3,
            depth: 1,
        },
        DecoderSetting::RxOffset {
            tier,
            expected: 700.0,
            hz: 800.0,
        },
        DecoderSetting::RxOffset {
            tier: Tier::Js8,
            expected: 1500.0,
            hz: 800.0,
        },
    ] {
        assert_eq!(s.apply(setting), Err(Reason::ContextChanged));
    }
    let connection = s.connection_generation();
    let permit = s
        .authority
        .permit(Instant::now() + Duration::from_secs(5))
        .unwrap();
    for setting in choices(&s) {
        assert_eq!(
            s.engine
                .save_remote_decoder_setting(setting, connection + 1, &permit),
            Err(Reason::ContextChanged)
        );
    }
    s.authority.revoke();
    for setting in choices(&s) {
        assert_eq!(
            s.engine
                .save_remote_decoder_setting(setting, connection, &permit),
            Err(Reason::AuthorityExpired)
        );
    }
    assert_eq!(settings(&s.engine), original);
    assert!(!s.path.exists());
    for setting in choices(&s) {
        s.apply(setting).unwrap();
    }
    assert!(
        s.path.is_file(),
        "a fresh authority positively admits both choices"
    );
}

#[test]
fn receiver_choices_refuse_unknown_keyed_stale_armed_or_non_native_context() {
    for kind in 0..2 {
        let mut s = Station::new(Tier::Ft8);
        let setting = choices(&s)[kind];
        let original = settings(&s.engine);
        for (keyed, age, reason) in [
            (None, Duration::ZERO, Reason::ReadingUnavailable),
            (Some(true), Duration::ZERO, Reason::StationBusy),
            (
                Some(false),
                Duration::from_secs(2),
                Reason::ReadingUnavailable,
            ),
        ] {
            s.sample(keyed, age);
            assert_eq!(s.apply(setting), Err(reason));
        }
        s.sample(Some(false), Duration::ZERO);
        s.engine.source_kind = SourceKind::Companion;
        assert_eq!(s.apply(setting), Err(Reason::UnsupportedAction));
        s.engine.source_kind = SourceKind::Native;
        s.engine.settings.operating_mode = OperatingMode::Cw;
        assert_eq!(s.apply(setting), Err(Reason::UnsupportedAction));
        s.engine.settings.operating_mode = OperatingMode::Digital;
        s.engine.set_tx_enabled(true);
        assert!(s.engine.tx_enabled());
        assert_eq!(s.apply(setting), Err(Reason::StationBusy));
        s.engine.set_tx_enabled(false);
        assert_eq!(s.apply(setting), Err(Reason::StationBusy));
        assert_eq!(settings(&s.engine), original);
        assert!(!s.path.exists());
        assert!(s.engine.take_immediate_retune());
        s.sample(Some(false), Duration::ZERO);
        s.apply(setting).unwrap();
        assert!(s.path.is_file());
    }
}

#[test]
fn displayed_legacy_receiver_values_can_be_deliberately_repaired() {
    for depth in [0, 255] {
        let mut s = Station::new(Tier::Ft8);
        s.engine.settings.decode_depth = depth;
        let displayed = s.engine.snapshot().radio.decode_depth;
        assert_eq!(displayed, depth.clamp(1, 3));
        s.apply(DecoderSetting::Depth {
            tier: Tier::Ft8,
            expected: displayed,
            depth: 2,
        })
        .unwrap();
        assert_eq!(s.engine.settings.decode_depth, 2);
    }
    let mut s = Station::new(Tier::Ft8);
    s.engine.rx_offset_hz = 50.0;
    s.engine.settings.rx_offset_hz = 50.0;
    s.apply(DecoderSetting::RxOffset {
        tier: Tier::Ft8,
        expected: 50.0,
        hz: 200.0,
    })
    .unwrap();
    assert_eq!(s.engine.rx_offset_hz(), 200.0);
    assert_eq!(s.engine.tx_offset_hz(), 1500.0);
}

#[test]
fn native_receiver_adjustment_invalidates_pending_remote_radio_work_even_for_the_same_value() {
    for kind in 0..2 {
        let mut s = Station::new(Tier::Ft8);
        let connection = s.connection_generation();
        let receipt = s
            .engine
            .queue_remote_frequency(
                7.074,
                "40m",
                "USB",
                connection,
                s.authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap(),
            )
            .unwrap();
        let work = s.engine.take_remote_radio().unwrap();
        assert_eq!(receipt.outcome(), Outcome::Pending);
        assert!(work.permission().check(Instant::now()).is_ok());
        let tx_generation = s.engine.tx_gate_gen;
        if kind == 0 {
            s.engine.set_decode_depth(s.engine.settings.decode_depth);
        } else {
            s.engine.set_rx_offset(s.engine.rx_offset_hz());
        }
        assert_eq!(
            work.permission().begin_write(Instant::now()),
            Err(Reason::ContextChanged)
        );
        assert_eq!(s.engine.tx_gate_gen, tx_generation);
        assert!(!s.engine.tx_enabled());
    }
}
