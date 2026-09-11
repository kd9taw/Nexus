//! Exercise the real selection worker and its subsequent native tick against
//! two independent TCP radios. No sound card, serial device or RF is involved.
use super::*;
use std::sync::atomic::AtomicBool;

fn station(peer: &Peer) -> Station {
    Station::configured(peer, |settings| {
        settings.ensure_radio_profiles();
        let mut incoming = settings.radios[0].clone();
        incoming.id = 1;
        incoming.name = "Incoming".into();
        incoming.serial_port = "incoming-test-port".into();
        incoming.rigctld_port += 1;
        incoming.audio_in = "incoming-capture".into();
        incoming.last_dial_mhz = 7.074;
        incoming.last_band = "40m".into();
        incoming.last_sideband = "USB".into();
        settings.radios.push(incoming);
        // Match settings load: normalize CAT/rotator/broker ports before the
        // active owner opens them, rather than inventing a live collision.
        settings.ensure_distinct_radio_ports();
        settings.sync_flat_from_active();
    })
}

fn connection(s: &Station, peer: &Peer) -> MonitorConn {
    let e = engine_lock(&s.engine);
    let projection = e.preview_radio_selection(1).unwrap();
    let mut transport = Transport::from_settings(projection.settings());
    transport.broker_self_port = None;
    MonitorConn {
        id: 1,
        transport,
        rig: Rig::rigctld(&peer.address),
        rigctld_proc: None,
        last_poll: 0.0,
        ticks: 0,
        smeter_supported: None,
        freq_misses: 0,
        open_failures: 0,
        retry_after_ms: 0.0,
    }
}

fn queue(s: &Station, id: u32) -> Completion {
    let mut e = engine_lock(&s.engine);
    let generation = e
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation;
    e.queue_remote_radio_selection(
        id,
        generation,
        s.authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap(),
    )
    .unwrap()
}

fn apply(
    s: &mut Station,
    pool: &MonitorPool,
    open: impl FnMut(&Transport) -> (Rig, Option<CatDaemon>, Option<bool>),
) {
    let mut active = engine_lock(&s.engine).settings().active_radio;
    let before = active;
    let notifications = std::cell::Cell::new(0);
    let engine = s.engine.clone();
    let pending = AtomicBool::new(false);
    s.state.apply_remote_selection(
        &s.engine,
        pool,
        &mut s.rig,
        &mut s.backend,
        &mut active,
        &pending,
        open,
        || {
            assert!(
                engine.try_lock().is_ok(),
                "host notification cannot hold Engine"
            );
            assert!(
                pool.try_lock().is_ok(),
                "host notification cannot hold pool"
            );
            notifications.set(notifications.get() + 1);
        },
    );
    assert_eq!(notifications.get(), usize::from(active != before));
    assert!(!pending.load(Ordering::Relaxed));
    assert_eq!(active, engine_lock(&s.engine).settings().active_radio);
}

#[test]
fn selection_worker_adopts_warm_and_cold_connections_and_does_not_replay_the_retune() {
    for warm in [true, false] {
        let outgoing = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
        let incoming = retuning_peer(7_100_000, "LSB", |_, _| None);
        let mut s = station(&outgoing);
        let pool = Arc::new(MonitorConnections::new(if warm {
            vec![connection(&s, &incoming)]
        } else {
            vec![]
        }));
        let receipt = queue(&s, 1);
        let opens = std::cell::Cell::new(0);
        let engine = s.engine.clone();
        apply(&mut s, &pool, |_| {
            assert!(engine.try_lock().is_ok(), "cold opens must not hold Engine");
            assert!(pool.try_lock().is_ok(), "cold opens must not hold pool");
            opens.set(opens.get() + 1);
            (Rig::rigctld(&incoming.address), None, Some(true))
        });
        assert_eq!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
        assert_eq!(opens.get(), usize::from(!warm));
        assert_eq!(s.state.last_dial, 7_074_000);
        assert_eq!(s.state.last_mode, "PKTUSB");
        assert!(s.state.force_audio_rebuild);
        assert!(s.state.remote_connection.is_none());
        assert_eq!(s.state.remote_radio_id, Some(1));
        assert_eq!(
            pool.lock()
                .unwrap()
                .iter()
                .map(|c| c.id)
                .collect::<Vec<_>>(),
            [0]
        );
        let saved = Settings::load(&s.path);
        assert_eq!(saved.active_radio, 1);
        assert_eq!(saved.audio_in, "incoming-capture");
        assert_eq!(saved.radios[0].last_dial_mhz, 14.074);
        assert_eq!(writes(&outgoing), ["T 0"]);
        assert!(outgoing
            .lines
            .lock()
            .unwrap()
            .iter()
            .any(|line| line == "\\stop_morse"));
        let initial = writes(&incoming);
        assert!(initial.iter().any(|line| line == "F 7074000"));
        assert!(initial.iter().any(|line| line.starts_with("M PKTUSB ")));
        s.step();
        s.step();
        let following = writes(&incoming);
        assert!(
            following[initial.len()..].iter().all(|line| line == "T 0"),
            "native follow-up writes: {following:?}"
        );
        assert!(!s.state.force_audio_rebuild);
        assert!(!engine_lock(&s.engine).tx_enabled());
        assert!(s.backend.played.is_empty());
    }
}

#[test]
fn selection_worker_revocation_during_cold_open_returns_monitor_without_adopting_or_writing() {
    let outgoing = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
    let incoming = retuning_peer(7_100_000, "LSB", |_, _| None);
    let mut s = station(&outgoing);
    let original = engine_lock(&s.engine).settings().clone();
    let pool = Arc::new(MonitorConnections::new(vec![]));
    let receipt = queue(&s, 1);
    let authority = std::mem::take(&mut s.authority);
    apply(&mut s, &pool, |_| {
        authority.revoke();
        (Rig::rigctld(&incoming.address), None, Some(true))
    });
    assert!(matches!(receipt.outcome(), Outcome::Rejected { .. }));
    assert_eq!(engine_lock(&s.engine).settings(), &original);
    assert!(!s.path.exists());
    assert!(writes(&incoming).is_empty());
    assert!(writes(&outgoing).is_empty());
    assert_eq!(pool.lock().unwrap()[0].id, 1);
    assert!(pool.claims.try_claim(1).is_some());
    s.step();
    assert!(writes(&incoming).is_empty());
}

#[test]
fn selection_worker_failed_incoming_confirmation_preserves_outgoing_and_has_no_deferred_retry() {
    let outgoing = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
    let incoming = retuning_peer(7_100_000, "LSB", |line, _| {
        line.starts_with("F ").then(|| "RPRT -1\n".into())
    });
    let mut s = station(&outgoing);
    let pool = Arc::new(MonitorConnections::new(vec![connection(&s, &incoming)]));
    let receipt = queue(&s, 1);
    apply(&mut s, &pool, |_| panic!("warm radio must be reused"));
    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    assert_eq!(engine_lock(&s.engine).settings().active_radio, 0);
    assert!(!s.path.exists());
    assert_eq!(pool.lock().unwrap()[0].id, 1);
    let before = writes(&incoming);
    assert!(before.iter().any(|line| line.starts_with("F ")));
    s.step();
    s.step();
    assert_eq!(writes(&incoming), before);
    assert_eq!(s.state.remote_radio_id, Some(0));
}

#[test]
fn selection_worker_adopts_desired_levels_and_actual_readback_without_replaying_controls() {
    let outgoing = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
    let levels = Mutex::new(std::collections::HashMap::from([
        ("RFPOWER", 0.8f32),
        ("MICGAIN", 0.9),
        ("NR", 0.9),
        ("COMP", 0.9),
        ("NOTCHF", 800.0),
        ("AGC", 3.0),
    ]));
    let incoming = retuning_peer(7_100_000, "LSB", move |line, _| {
        let mut levels = levels.lock().unwrap();
        if let Some(token) = line.strip_prefix("l ") {
            return levels.get(token).map(|v| format!("{v}\n"));
        }
        if let Some((token, raw)) = line.strip_prefix("L ").and_then(|s| s.split_once(' ')) {
            if let Some(level) = levels.get_mut(token) {
                *level = raw.parse().unwrap();
                return Some("RPRT 0\n".into());
            }
        }
        None
    });
    let mut s = station(&outgoing);
    {
        let mut e = engine_lock(&s.engine);
        e.set_rf_power(0.2);
        e.set_mic_gain(0.3);
        e.set_nr_level(0.4);
        e.set_comp_level(0.5);
        e.set_notch_freq_hz(1200.0);
        e.set_agc("slow");
        e.observe_rig_power(0.95);
        e.observe_rig_mic_gain(0.95);
    }
    let pool = Arc::new(MonitorConnections::new(vec![connection(&s, &incoming)]));
    let receipt = queue(&s, 1);
    apply(&mut s, &pool, |_| panic!("warm radio must be reused"));
    assert_eq!(
        receipt.outcome(),
        Outcome::Applied {
            evidence: Evidence::RadioReadback
        }
    );
    assert_eq!(s.state.last_rf_power, Some(0.2));
    assert_eq!(s.state.last_mic_gain, Some(0.3));
    assert_eq!(s.state.last_nr_level, Some(0.4));
    assert_eq!(s.state.last_comp_level, Some(0.5));
    assert_eq!(s.state.last_notch_freq_hz, Some(1200.0));
    assert_eq!(s.state.last_agc.as_deref(), Some("slow"));
    assert_eq!(engine_lock(&s.engine).rf_power(), Some(0.2));
    let initial = writes(&incoming);
    assert_eq!(
        initial.iter().filter(|line| line.starts_with("L ")).count(),
        6
    );
    s.step();
    s.step();
    let following = writes(&incoming);
    assert!(
        following[initial.len()..].iter().all(|line| line == "T 0"),
        "native follow-up writes: {following:?}"
    );
    assert_eq!(engine_lock(&s.engine).rf_power(), Some(0.2));
}

#[test]
fn selection_worker_refuses_busy_owner_before_open_or_hardware_cleanup() {
    for busy in 0..4 {
        let outgoing = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
        let mut s = station(&outgoing);
        let receipt = queue(&s, 1);
        match busy {
            0 => s.state.manual_ptt_applied = true,
            1 => s.state.cw_busy_until = now_unix_ms() + 60000.0,
            2 => s.state.rtty_busy_until = now_unix_ms() + 60000.0,
            _ => s.state.psk_busy_until = now_unix_ms() + 60000.0,
        }
        let pool = Arc::new(MonitorConnections::new(vec![]));
        apply(&mut s, &pool, |_| {
            panic!("busy owner cannot open incoming radio")
        });
        assert_eq!(
            receipt.outcome(),
            Outcome::Rejected {
                reason: Reason::StationBusy
            }
        );
        assert_eq!(engine_lock(&s.engine).settings().active_radio, 0);
        assert!(!s.path.exists());
        assert!(outgoing.lines.lock().unwrap().is_empty());
        assert_eq!(s.backend.flush_calls, 0);
    }
}

#[test]
fn selection_worker_switches_back_using_the_original_connection_and_retires_old_readings() {
    let outgoing = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
    let incoming = retuning_peer(7_100_000, "LSB", |_, _| None);
    let mut s = station(&outgoing);
    let pool = Arc::new(MonitorConnections::new(vec![connection(&s, &incoming)]));
    let old_read = s.state.remote_read(&s.engine).unwrap();
    let first = queue(&s, 1);
    apply(&mut s, &pool, |_| panic!("warm selection cannot reopen"));
    assert_eq!(
        first.outcome(),
        Outcome::Applied {
            evidence: Evidence::RadioReadback
        }
    );
    {
        let mut e = engine_lock(&s.engine);
        e.remote_observe_cat(Some(&old_read), Some(true));
        e.remote_observe_dial(Some(&old_read), Some(14_074_000));
        let observation = e.remote_monitor_observation();
        assert_eq!(observation.radio.id, 1);
        assert_eq!(observation.radio.cat_connected, None);
        assert_eq!(observation.radio.rig_dial_mhz, None);
    }
    s.step();
    // A new actual read is bound BEFORE its I/O on the now-owned Rig. This
    // positive control distinguishes retired provenance from missing support.
    let read = s.state.remote_read(&s.engine).unwrap();
    let hz = s.rig.read_freq().unwrap();
    let mode = s.rig.read_mode().unwrap();
    let keyed = s.rig.read_ptt().unwrap();
    {
        let mut e = engine_lock(&s.engine);
        e.remote_observe_cat(Some(&read), Some(true));
        e.remote_observe_dial(Some(&read), Some(hz));
        e.remote_observe_mode(Some(&read), Some(&mode));
        e.remote_observe_ptt(Some(&read), Some(keyed));
        assert_eq!(
            e.remote_monitor_observation().radio.rig_dial_mhz,
            Some(7.074)
        );
    }
    {
        let e = engine_lock(&s.engine);
        let projection = e.preview_radio_selection(0).unwrap();
        let mut want = Transport::from_settings(projection.settings());
        want.broker_self_port = None;
        let connections = pool.lock().unwrap();
        assert!(!connections[0].transport.rig_differs(&want));
        assert!(connections[0].rig.has_control());
    }
    let second = queue(&s, 0);
    apply(&mut s, &pool, |_| {
        panic!("original connection must remain reusable")
    });
    assert_eq!(
        second.outcome(),
        Outcome::Applied {
            evidence: Evidence::RadioReadback
        }
    );
    assert_eq!(s.state.remote_radio_id, Some(0));
    assert_eq!(s.rig.read_freq().unwrap(), 14_074_000);
    let saved = Settings::load(&s.path);
    assert_eq!(saved.active_radio, 0);
    assert_eq!(saved.audio_in, "USB Audio CODEC");
    assert_eq!(saved.radios[1].last_dial_mhz, 7.074);
    assert_eq!(pool.lock().unwrap()[0].id, 1);
}
