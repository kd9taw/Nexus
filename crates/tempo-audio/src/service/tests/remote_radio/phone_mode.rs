use super::*;

#[test]
fn remote_phone_mode_owner_changes_the_actual_mode_then_restores_the_exact_dial_once() {
    for mode in ["USB", "LSB", "AM"] {
        let peer = retuning_peer(7_220_000, "CW", |line, _| {
            (line == "l RFPOWER").then(|| "0.1\n".into())
        });
        let mut s = Station::configured(&peer, |settings| {
            settings.operating_mode = tempo_app::settings::OperatingMode::Phone;
            settings.ensure_radio_profiles();
        });
        s.state.last_dial = 7_220_000;
        let receipt = {
            let mut e = engine_lock(&s.engine);
            e.set_frequency(7.22, "40m", "LSB");
            e.take_immediate_retune();
            s.state.last_mode = e.rig_mode_effective();
            e.settings().save(&s.path).unwrap();
            let read = e
                .remote_radio_read(s.state.remote_connection.as_ref().unwrap(), Instant::now())
                .unwrap();
            e.remote_observe_cat(Some(&read), Some(true));
            e.remote_observe_dial(Some(&read), Some(7_220_000));
            e.remote_observe_mode(Some(&read), Some("CW"));
            e.remote_observe_ptt(Some(&read), Some(false));
            let connection = e
                .remote_monitor_observation()
                .radio
                .readings
                .cat
                .unwrap()
                .connection_generation;
            e.queue_remote_phone_mode(
                None,
                Some(mode),
                connection,
                s.authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap(),
            )
            .unwrap()
        };
        let file = std::fs::read(&s.path).unwrap();
        s.step();
        assert_eq!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
        let commands = writes(&peer);
        assert_eq!(commands.len(), 2, "{commands:?}");
        assert!(commands[0].starts_with(&format!("M {mode} ")));
        assert_eq!(
            commands[1], "F 7220000",
            "the old CW convention must not pitch-shift the final dial"
        );
        assert_eq!(s.state.last_mode, mode);
        for _ in 0..3 {
            s.step();
        }
        assert_eq!(
            writes(&peer),
            commands,
            "native reconciliation cannot repeat the completed change"
        );
        let mut e = engine_lock(&s.engine);
        assert_eq!(e.snapshot().radio.sideband_override.as_deref(), Some(mode));
        assert_eq!(e.snapshot().radio.rig_mode.as_deref(), Some(mode));
        assert_eq!(e.snapshot().radio.dial_mhz, 7.22);
        assert!(!e.take_immediate_retune());
        assert!(!e.tx_enabled());
        assert_eq!(std::fs::read(&s.path).unwrap(), file);
    }
}

#[test]
fn remote_phone_am_owner_confirms_the_native_power_cap_without_raising_or_replaying_it() {
    for (initial, ignore_write) in [(0.8, false), (0.1, false), (0.8, true)] {
        let power = Mutex::new(initial);
        let peer = retuning_peer(7_220_000, "LSB", move |line, _| {
            let mut power = power.lock().unwrap();
            if line == "l RFPOWER" {
                return Some(format!("{power}\n"));
            }
            if let Some(value) = line.strip_prefix("L RFPOWER ") {
                if !ignore_write {
                    *power = value.parse().unwrap();
                }
                return Some("RPRT 0\n".into());
            }
            None
        });
        let mut s = Station::configured(&peer, |settings| {
            settings.operating_mode = tempo_app::settings::OperatingMode::Phone;
            settings.max_power_phone = None;
            settings.max_power_am = Some(0.25);
        });
        s.state.last_dial = 7_220_000;
        s.state.last_rf_power = Some(initial);
        let receipt = {
            let mut e = engine_lock(&s.engine);
            e.set_frequency(7.22, "40m", "LSB");
            e.take_immediate_retune();
            s.state.last_mode = e.rig_mode_effective();
            e.set_rf_power(initial);
            e.observe_rig_power(initial);
            let read = e
                .remote_radio_read(s.state.remote_connection.as_ref().unwrap(), Instant::now())
                .unwrap();
            e.remote_observe_cat(Some(&read), Some(true));
            e.remote_observe_dial(Some(&read), Some(7_220_000));
            e.remote_observe_mode(Some(&read), Some("LSB"));
            e.remote_observe_ptt(Some(&read), Some(false));
            let connection = e
                .remote_monitor_observation()
                .radio
                .readings
                .cat
                .unwrap()
                .connection_generation;
            e.queue_remote_phone_mode(
                None,
                Some("AM"),
                connection,
                s.authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap(),
            )
            .unwrap()
        };
        s.step();
        let commands = writes(&peer);
        assert_eq!(commands[0], "M AM 6000");
        assert_eq!(commands[1], "F 7220000");
        if initial > 0.25 {
            assert_eq!(commands[2], "L RFPOWER 0.250");
            assert_eq!(commands.len(), 3);
        } else {
            assert_eq!(commands.len(), 2, "lower power must not be increased");
        }
        if ignore_write {
            assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
        } else {
            assert_eq!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            );
        }
        for _ in 0..3 {
            s.step();
        }
        assert_eq!(
            writes(&peer),
            commands,
            "confirmed or uncertain work cannot become a later retry"
        );
        let mut e = engine_lock(&s.engine);
        assert_eq!(
            e.snapshot().radio.sideband_override.as_deref(),
            (!ignore_write).then_some("AM")
        );
        assert_eq!(
            e.rf_power(),
            Some(if ignore_write {
                initial
            } else {
                initial.min(0.25)
            })
        );
        assert!(!e.take_immediate_retune());
        assert!(!e.tx_enabled());
        assert!(!s.path.exists());
    }
}

#[test]
fn remote_phone_mode_owner_never_retries_an_unconfirmed_change() {
    let peer = retuning_peer(14_074_000, "USB", |line, _| {
        line.starts_with("M LSB ").then(|| "RPRT 0\n".into())
    });
    let mut s = Station::configured(&peer, |settings| {
        settings.operating_mode = tempo_app::settings::OperatingMode::Phone;
        settings.ensure_radio_profiles();
    });
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
            Some("LSB"),
            connection,
            s.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
        .unwrap()
    };
    s.step();
    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    let commands = writes(&peer);
    assert_eq!(commands.len(), 1);
    assert!(commands[0].starts_with("M LSB "));
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(writes(&peer), commands);
    let mut e = engine_lock(&s.engine);
    assert_eq!(e.snapshot().radio.sideband_override, None);
    assert!(!e.take_immediate_retune());
    assert!(!e.tx_enabled());
    assert!(!s.path.exists());
}
