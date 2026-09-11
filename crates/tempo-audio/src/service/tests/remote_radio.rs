//! Drive the actual step (including the following native reconciliation), not
//! just its Remote helper. CAT is an isolated loopback peer; no audio/RF devices.
use super::*;
use crate::rig::remote_tests::{retuning_peer, writes, Peer};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use tempo_app::remote_control::{Completion, Evidence, Outcome, Reason, Revocation};

mod dsp;
mod filter;
mod level;
mod phone_mode;
mod selection;
mod spot;

static NEXT: AtomicUsize = AtomicUsize::new(0);
struct Station {
    engine: Arc<Mutex<Engine>>,
    state: RadioLoop,
    rig: Rig,
    backend: MockBackend,
    authority: Revocation,
    path: PathBuf,
}
impl Station {
    fn new(peer: &Peer) -> Self {
        Self::configured(peer, |_| {})
    }
    fn configured(peer: &Peer, configure: impl FnOnce(&mut Settings)) -> Self {
        let path = std::env::temp_dir()
            .join(format!(
                "nexus-worker-frequency-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
            .join("settings.json");
        let mut settings = Settings {
            dial_mhz: 14.074,
            band: "20m".into(),
            sideband: "USB".into(),
            ..test_settings()
        };
        configure(&mut settings);
        let mut e = Engine::with_settings(settings);
        e.set_tx_enabled(false);
        e.configure_remote_settings_store(path.clone());
        let engine = Arc::new(Mutex::new(e));
        let mut state = loop_state();
        state.applied = Transport::from_settings(engine_lock(&engine).settings());
        state.remote_radio_id = Some(engine_lock(&engine).settings().active_radio);
        state.last_dial = 14_074_000;
        state.last_mode = engine_lock(&engine).rig_mode_effective();
        state.rig_asserted = true;
        let read = state.remote_read(&engine).unwrap();
        {
            let mut e = engine_lock(&engine);
            e.remote_observe_cat(Some(&read), Some(true));
            e.remote_observe_dial(Some(&read), Some(14_074_000));
            e.remote_observe_mode(Some(&read), Some(&state.last_mode));
            e.remote_observe_ptt(Some(&read), Some(false));
        }
        Self {
            engine,
            state,
            rig: Rig::rigctld(&peer.address),
            backend: MockBackend::new(),
            authority: Revocation::default(),
            path,
        }
    }
    fn queue(&mut self) -> Completion {
        self.queue_dial(7.074, "40m")
    }
    fn queue_dial(&mut self, dial: f64, band: &str) -> Completion {
        let mut e = engine_lock(&self.engine);
        let connection = e
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        e.queue_remote_frequency(
            dial,
            band,
            "USB",
            connection,
            self.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
        .unwrap()
    }
    fn queue_mode(&mut self, mode: &str, follow: bool) -> Completion {
        let mut e = engine_lock(&self.engine);
        let connection = e
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        e.queue_remote_mode(
            mode,
            follow,
            connection,
            self.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
        .unwrap()
    }
    fn queue_tier(&mut self, tier: tempo_app::dto::Tier) -> Completion {
        let mut e = engine_lock(&self.engine);
        let connection = e
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        e.queue_remote_tier(
            tier,
            connection,
            self.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
        .unwrap()
    }
    fn step(&mut self) {
        self.state
            .step(
                &self.engine,
                &mut self.backend,
                &mut self.rig,
                &no_sinks(),
                0.0,
                &mut mock_reopen_audio(),
                &mut mock_reopen_rig(),
                &mut StationSinks::new(),
            )
            .unwrap();
    }
}

#[test]
fn saved_remote_rx_gain_reaches_the_existing_audio_owner_once_without_tx_or_a_rebuild() {
    let peer = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
    let mut s = Station::configured(&peer, Settings::ensure_radio_profiles);
    let reopens = std::cell::Cell::new(0);
    let step = |s: &mut Station| {
        s.state
            .step(
                &s.engine,
                &mut s.backend,
                &mut s.rig,
                &no_sinks(),
                0.0,
                &mut |_: &Transport| {
                    reopens.set(reopens.get() + 1);
                    Err("gain must not reopen capture".into())
                },
                &mut mock_reopen_rig(),
                &mut StationSinks::new(),
            )
            .unwrap();
    };
    {
        let mut e = engine_lock(&s.engine);
        let connection = e
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        let radio = e.settings().active_radio;
        let expected = e.settings().rx_gain;
        assert_eq!(e.settings().active_profile().unwrap().id, radio);
        e.save_remote_rx_gain(
            radio,
            expected,
            2.5,
            connection,
            &s.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
        .unwrap();
    }
    assert!(
        s.backend.rx_gain_calls.is_empty(),
        "a saved setting alone is not an applied audio update"
    );
    assert!(s.backend.tx_level_calls.is_empty());
    step(&mut s);
    assert_eq!(s.backend.rx_gain_calls, [2.5]);
    assert!(s.backend.tx_level_calls.is_empty());
    assert_eq!(s.state.applied.rx_gain, 2.5);
    s.authority.revoke();
    for _ in 0..3 {
        step(&mut s);
    }
    assert_eq!(s.backend.rx_gain_calls, [2.5]);
    assert!(writes(&peer).is_empty());
    assert!(s.backend.played.is_empty());
    assert_eq!(s.backend.flush_calls, 0);
    engine_lock(&s.engine).set_rx_gain(1.5);
    step(&mut s);
    assert_eq!(
        s.backend.rx_gain_calls,
        [2.5, 1.5],
        "the local control uses the same live owner"
    );
    assert!(!engine_lock(&s.engine).tx_enabled());
    assert_eq!(reopens.get(), 0);
}

#[test]
fn tier_entry_uses_the_actual_owner_once_and_same_tier_never_sends_a_command() {
    use tempo_app::dto::Tier;
    let peer = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
    let mut s = Station::new(&peer);
    let unchanged = s.queue_tier(Tier::Ft8);
    assert_eq!(
        unchanged.outcome(),
        Outcome::Applied {
            evidence: Evidence::ReceiverState
        }
    );
    s.step();
    assert!(writes(&peer).is_empty());
    let receipt = s.queue_tier(Tier::Ft4);
    assert_eq!(engine_lock(&s.engine).tier(), Tier::Ft8);
    s.step();
    assert_eq!(
        receipt.outcome(),
        Outcome::Applied {
            evidence: Evidence::RadioReadback
        }
    );
    assert_eq!(engine_lock(&s.engine).tier(), Tier::Ft4);
    assert_eq!(engine_lock(&s.engine).settings().dial_hz(), 14_080_000);
    assert_eq!(writes(&peer), ["M PKTUSB -1", "F 14080000"]);
    s.authority.revoke();
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(writes(&peer), ["M PKTUSB -1", "F 14080000"]);
    assert!(!engine_lock(&s.engine).tx_enabled());
    assert!(
        !s.path.exists(),
        "native set_tier is a live transition, not a settings save"
    );
}

#[test]
fn a_failed_tier_qsy_keeps_the_old_decoder_and_does_not_retry_the_frequency() {
    use tempo_app::dto::Tier;
    let peer = retuning_peer(14_074_000, "PKTUSB", |line, _| {
        line.starts_with("F ").then(|| "RPRT -1\n".into())
    });
    let mut s = Station::new(&peer);
    let receipt = s.queue_tier(Tier::Ft4);
    s.step();
    assert_eq!(
        receipt.outcome(),
        Outcome::Unknown {
            reason: Reason::HardwareUnconfirmed
        }
    );
    assert_eq!(engine_lock(&s.engine).tier(), Tier::Ft8);
    assert_eq!(engine_lock(&s.engine).settings().dial_hz(), 14_074_000);
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(writes(&peer), ["M PKTUSB -1", "F 14080000"]);
    assert!(!s.path.exists());
}

#[test]
fn local_same_frequency_tier_round_trip_cancels_remote_tier_completion() {
    use tempo_app::dto::Tier;
    let change: Arc<Mutex<Option<Arc<Mutex<Engine>>>>> = Default::default();
    let local = change.clone();
    let peer = retuning_peer(14_074_000, "PKTUSB", move |line, _| {
        if line.starts_with("M ") {
            let engine = local.lock().unwrap().take();
            if let Some(engine) = engine {
                let mut e = engine_lock(&engine);
                e.set_tier(Tier::Ft4);
                e.set_tier(Tier::Ft8);
            }
        }
        None
    });
    let mut s = Station::configured(&peer, |settings| {
        settings
            .working_frequencies
            .push(tempo_app::settings::WorkingFreq {
                band: "20m".into(),
                mode: "FT4".into(),
                mhz: 14.074,
            })
    });
    *change.lock().unwrap() = Some(s.engine.clone());
    let receipt = s.queue_tier(Tier::Ft4);
    s.step();
    assert_eq!(
        receipt.outcome(),
        Outcome::Unknown {
            reason: Reason::HardwareUnconfirmed
        }
    );
    assert_eq!(engine_lock(&s.engine).tier(), Tier::Ft8);
    assert_eq!(engine_lock(&s.engine).settings().dial_hz(), 14_074_000);
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(writes(&peer), ["M PKTUSB -1"]);
    assert!(!s.path.exists());
}

#[test]
fn mode_entry_crosses_the_actual_worker_without_arming_or_deferred_retune() {
    let peer = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
    let mut s = Station::new(&peer);
    let receipt = s.queue_mode("cw", true);
    s.step();
    assert!(matches!(receipt.outcome(), Outcome::Applied { .. }));
    assert_eq!(
        Settings::load(&s.path).operating_mode,
        tempo_app::settings::OperatingMode::Cw
    );
    assert_eq!(writes(&peer), ["M CW -1", "F 14030000"]);
    s.authority.revoke();
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(writes(&peer), ["M CW -1", "F 14030000"]);
    assert!(!engine_lock(&s.engine).tx_enabled());
}

#[test]
fn mode_worker_adopts_confirmed_power_without_raising_or_reissuing_it() {
    for (initial, save_failure) in [(0.8, false), (0.2, false), (0.8, true)] {
        let peer = crate::rig::remote_tests::power_peer(initial);
        let mut s = Station::configured(&peer, |settings| {
            settings.operating_mode = tempo_app::settings::OperatingMode::Phone;
            settings.max_power_digital = Some(0.4);
        });
        {
            let mut e = engine_lock(&s.engine);
            e.observe_rig_power(initial);
            e.set_rf_power(initial);
        }
        s.state.last_rf_power = Some(initial);
        if save_failure {
            std::fs::create_dir_all(&s.path).unwrap();
        }
        let receipt = s.queue_mode("digital", false);
        s.step();
        if save_failure {
            assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
        } else {
            assert!(matches!(receipt.outcome(), Outcome::Applied { .. }));
        }
        let mut expected = vec!["M PKTUSB 3000", "F 14074000"];
        if initial > 0.4 {
            expected.push("L RFPOWER 0.400");
        }
        assert_eq!(writes(&peer), expected);
        s.authority.revoke();
        for _ in 0..3 {
            s.step();
        }
        assert_eq!(writes(&peer), expected);
        let e = engine_lock(&s.engine);
        assert_eq!(e.rf_power(), Some(initial.min(0.4)));
        assert_eq!(
            e.settings().operating_mode,
            tempo_app::settings::OperatingMode::Digital
        );
        assert!(!e.tx_enabled());
    }
}

#[test]
fn unconfirmed_power_does_not_commit_mode_or_defer_a_radio_retry() {
    let peer = retuning_peer(14_074_000, "USB", |line, _| {
        match line {
            "l RFPOWER" => Some("0.8\n".into()),
            "L RFPOWER 0.400" => Some("RPRT 0\n".into()), // accepted but ignored
            _ => None,
        }
    });
    let mut s = Station::configured(&peer, |settings| {
        settings.operating_mode = tempo_app::settings::OperatingMode::Phone;
        settings.max_power_digital = Some(0.4);
    });
    engine_lock(&s.engine).set_rf_power(0.8);
    s.state.last_rf_power = Some(0.8);
    let receipt = s.queue_mode("digital", false);
    s.step();
    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    let expected = ["M PKTUSB 3000", "F 14074000", "L RFPOWER 0.400"];
    assert_eq!(writes(&peer), expected);
    s.authority.revoke();
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(writes(&peer), expected);
    assert_eq!(
        engine_lock(&s.engine).settings().operating_mode,
        tempo_app::settings::OperatingMode::Phone
    );
    assert!(!engine_lock(&s.engine).tx_enabled());
    assert!(!s.path.exists());
}
impl Drop for Station {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.path.parent().unwrap());
    }
}

#[test]
fn frequency_crosses_the_actual_worker_once_then_persists_without_native_replay() {
    for (dial, band, hz) in [
        (7.074, "40m", 7_074_000),
        (10.0, "", 10_000_000),
        (9.5, "", 9_500_000),
    ] {
        let peer = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
        let mut s = Station::new(&peer);
        let receipt = s.queue_dial(dial, band);
        assert_eq!(engine_lock(&s.engine).settings().dial_hz(), 14_074_000);
        s.step();
        assert_eq!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
        assert_eq!(Settings::load(&s.path).dial_hz(), hz);
        assert_eq!(Settings::load(&s.path).band, band);
        let expected = vec!["M PKTUSB 3000".to_string(), format!("F {hz}")];
        assert_eq!(writes(&peer), expected);
        s.authority.revoke();
        for _ in 0..3 {
            s.step();
        }
        assert_eq!(writes(&peer), expected);
        assert!(!engine_lock(&s.engine).tx_enabled());
        assert_eq!(s.rig.read_freq().unwrap(), hz);
        assert!(s.backend.played.is_empty());
    }
}

#[test]
fn failed_frequency_does_not_leave_a_target_for_ordinary_worker_retries() {
    let peer = retuning_peer(14_074_000, "PKTUSB", |line, _| {
        line.starts_with("F ").then(|| "RPRT -1\n".into())
    });
    let mut s = Station::new(&peer);
    let receipt = s.queue();
    s.step();
    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(writes(&peer), ["M PKTUSB 3000", "F 7074000"]);
    assert_eq!(engine_lock(&s.engine).settings().dial_hz(), 14_074_000);
    assert!(!s.path.exists());
}

#[test]
fn worker_save_failure_is_unknown_and_does_not_reissue_the_confirmed_qsy() {
    let peer = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
    let mut s = Station::new(&peer);
    std::fs::create_dir_all(&s.path).unwrap();
    let receipt = s.queue();
    s.step();
    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(engine_lock(&s.engine).settings().dial_hz(), 7_074_000);
    assert_eq!(writes(&peer), ["M PKTUSB 3000", "F 7074000"]);
}

#[test]
fn expired_or_wrong_owner_request_never_reaches_cat() {
    for wrong_owner in [false, true] {
        let peer = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
        let mut s = Station::new(&peer);
        let receipt = s.queue();
        if wrong_owner {
            s.state.remote_radio_id = None;
        } else {
            s.authority.revoke();
        }
        s.step();
        assert!(matches!(
            receipt.outcome(),
            Outcome::Rejected {
                reason: Reason::AuthorityExpired | Reason::ContextChanged
            }
        ));
        assert!(writes(&peer).is_empty());
        assert!(!s.path.exists());
        // The native setter still operates on this same loop and socket.
        engine_lock(&s.engine).set_frequency(14.075, "20m", "USB");
        s.step();
        assert!(writes(&peer).iter().any(|w| w == "F 14075000"));
    }
}

#[test]
fn a_retired_worker_connection_cannot_use_a_new_connection_permission() {
    let peer = retuning_peer(14_074_000, "PKTUSB", |_, _| None);
    let mut s = Station::new(&peer);
    // Reopen BEFORE admission: the new permit is valid, but this worker still
    // owns the retired connection. Equal profile/transport values do not bind it.
    {
        let mut e = engine_lock(&s.engine);
        let current = e.remote_open_radio().unwrap();
        let read = e.remote_radio_read(&current, Instant::now()).unwrap();
        e.remote_observe_cat(Some(&read), Some(true));
        e.remote_observe_dial(Some(&read), Some(14_074_000));
        e.remote_observe_mode(Some(&read), Some("PKTUSB"));
        e.remote_observe_ptt(Some(&read), Some(false));
    }
    let receipt = s.queue();
    s.step();
    assert!(writes(&peer).is_empty());
    assert!(matches!(
        receipt.outcome(),
        Outcome::Rejected {
            reason: Reason::ContextChanged
        }
    ));
    assert!(!s.path.exists());
}

#[test]
fn local_source_selection_between_mode_and_dial_stops_the_remote_transaction() {
    let change: Arc<Mutex<Option<Arc<Mutex<Engine>>>>> = Default::default();
    let local = change.clone();
    let peer = retuning_peer(14_074_000, "PKTUSB", move |line, _| {
        if line.starts_with("M ") {
            let engine = local.lock().unwrap().take();
            if let Some(engine) = engine {
                engine_lock(&engine)
                    .set_source(tempo_app::dto::SourceKind::Native)
                    .unwrap();
            }
        }
        None
    });
    let mut s = Station::new(&peer);
    *change.lock().unwrap() = Some(s.engine.clone());
    let receipt = s.queue_mode("cw", true);
    s.step();
    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    assert_eq!(writes(&peer), ["M CW -1"]);
    s.authority.revoke();
    for _ in 0..3 {
        s.step();
    }
    assert_eq!(writes(&peer), ["M CW -1"]);
    assert_eq!(
        engine_lock(&s.engine).settings().operating_mode,
        tempo_app::settings::OperatingMode::Digital
    );
    assert!(!s.path.exists());
    assert!(!engine_lock(&s.engine).tx_enabled());
}

#[test]
fn a_local_qsy_between_cat_commands_cancels_the_remote_tail() {
    let change: Arc<Mutex<Option<Arc<Mutex<Engine>>>>> = Default::default();
    let local = change.clone();
    let peer = retuning_peer(14_074_000, "PKTUSB", move |line, _| {
        if line.starts_with("M ") {
            let engine = local.lock().unwrap().take();
            if let Some(engine) = engine {
                engine_lock(&engine).set_frequency(14.075, "20m", "USB");
            }
        }
        None
    });
    let mut s = Station::new(&peer);
    *change.lock().unwrap() = Some(s.engine.clone());
    let receipt = s.queue();
    s.step();
    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    let commands = writes(&peer);
    assert!(!commands.iter().any(|w| w == "F 7074000"));
    assert!(
        commands.iter().any(|w| w == "F 14075000"),
        "the local QSY must still reach the radio"
    );
    assert_eq!(engine_lock(&s.engine).settings().dial_hz(), 14_075_000);
    assert!(!s.path.exists());
}

#[test]
fn band_selection_uses_the_actual_radio_owner_and_never_replays_the_native_pick() {
    use tempo_app::settings::OperatingMode;
    for (om, mode, rig_mode) in [
        (OperatingMode::Cw, "cw", "CW"),
        (OperatingMode::Phone, "phone", "USB"),
    ] {
        let peer = retuning_peer(14_074_000, rig_mode, |_, _| None);
        let mut s = Station::configured(&peer, |settings| {
            settings.operating_mode = om;
            settings.ensure_radio_profiles();
        });
        let mut expected = Engine::with_settings(engine_lock(&s.engine).settings().clone());
        expected.set_tx_enabled(false);
        for (index, band) in ["40m", "20m", "40m"].into_iter().enumerate() {
            let receipt = {
                let mut e = engine_lock(&s.engine);
                let connection = e
                    .remote_monitor_observation()
                    .radio
                    .readings
                    .cat
                    .unwrap()
                    .connection_generation;
                e.queue_remote_band(
                    band,
                    mode,
                    connection,
                    s.authority
                        .permit(Instant::now() + Duration::from_secs(5))
                        .unwrap(),
                )
                .unwrap()
            };
            expected.pick_band(band, Some(mode));
            expected.take_immediate_retune();
            s.step();
            assert_eq!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            );
            assert_eq!(
                engine_lock(&s.engine).settings().dial_hz(),
                expected.settings().dial_hz()
            );
            assert_eq!(
                engine_lock(&s.engine).rig_mode_effective(),
                expected.rig_mode_effective()
            );
            let sent = writes(&peer);
            assert_eq!(sent.len(), (index + 1) * 2);
            assert!(
                sent[sent.len() - 2].starts_with(&format!("M {} ", expected.rig_mode_effective()))
            );
            assert_eq!(
                sent.last().unwrap(),
                &format!("F {}", expected.settings().dial_hz())
            );
            for _ in 0..3 {
                s.step();
            }
            assert_eq!(writes(&peer), sent);
            assert!(s.backend.played.is_empty());
            assert!(!engine_lock(&s.engine).tx_enabled());
            let saved: Settings = serde_json::from_slice(&std::fs::read(&s.path).unwrap()).unwrap();
            assert_eq!(saved.dial_hz(), expected.settings().dial_hz());
        }
    }
}
