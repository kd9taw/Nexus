use super::*;
use crate::remote_control::Revocation;
use crate::remote_monitor::provenance::Connection;
use crate::settings::Settings;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static NEXT: AtomicUsize = AtomicUsize::new(0);
mod band_selection;
mod dsp;
mod filter;
mod level;
mod phone_mode;
mod receive_tuning;
mod spot;
struct Station {
    engine: Engine,
    connection: Connection,
    authority: Revocation,
    path: PathBuf,
}
impl Station {
    fn new(mode: OperatingMode) -> Self {
        let path = std::env::temp_dir()
            .join(format!(
                "nexus-frequency-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
            .join("settings.json");
        let mut settings = Settings {
            operating_mode: mode,
            dial_mhz: 14.074,
            band: "20m".into(),
            sideband: "USB".into(),
            ..Default::default()
        };
        settings.ensure_radio_profiles();
        let mut engine = Engine::with_settings(settings);
        engine.set_tx_enabled(false);
        engine.configure_remote_settings_store(path.clone());
        let connection = engine.remote_open_radio().unwrap();
        let mut station = Self {
            engine,
            connection,
            authority: Revocation::default(),
            path,
        };
        let mode = station.engine.rig_mode_effective();
        station.sample(14_074_000, &mode);
        station
    }
    fn sample(&mut self, hz: u64, mode: &str) {
        let read = self
            .engine
            .remote_radio_read(&self.connection, Instant::now())
            .unwrap();
        self.engine.remote_observe_cat(Some(&read), Some(true));
        self.engine.remote_observe_dial(Some(&read), Some(hz));
        self.engine.remote_observe_mode(Some(&read), Some(mode));
        self.engine.remote_observe_ptt(Some(&read), Some(false));
    }
    fn queue(&mut self) -> Result<Completion, Reason> {
        let connection = self
            .engine
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        self.engine.queue_remote_frequency(
            7.074,
            "40m",
            "USB",
            connection,
            self.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
    }
    fn queue_mode(&mut self, mode: &str, follow_frequency: bool) -> Result<Completion, Reason> {
        let connection = self
            .engine
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        self.engine.queue_remote_mode(
            mode,
            follow_frequency,
            connection,
            self.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
    }
    fn queue_tier(&mut self, tier: Tier) -> Result<Completion, Reason> {
        let connection = self
            .engine
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        self.engine.queue_remote_tier(
            tier,
            connection,
            self.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
    }

    fn queue_workspace(&mut self, workspace: Workspace) -> Result<Completion, Reason> {
        let connection = self
            .engine
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        self.engine.queue_remote_workspace(
            workspace,
            connection,
            self.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
    }
}
impl Drop for Station {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.path.parent().unwrap());
    }
}

#[test]
fn workspace_entry_uses_native_area_memories_and_js8_session_policy() {
    let mut s = Station::new(OperatingMode::Digital);
    let mut native = Station::new(OperatingMode::Digital);
    for station in [&mut s, &mut native] {
        station.engine.set_tier(Tier::Ft4);
        station.engine.take_immediate_retune();
        let hz = station.engine.settings.dial_hz();
        let mode = station.engine.rig_mode_effective();
        station.sample(hz, &mode);
    }
    let source = s.engine.source.clone();
    for workspace in [
        Workspace::Tempo,
        Workspace::Ft,
        Workspace::Js8,
        Workspace::Tempo,
        Workspace::Js8,
        Workspace::Ft,
    ] {
        let before = serde_json::to_value(s.engine.settings()).unwrap();
        let generation = s.engine.tx_gate_gen;
        let receipt = s.queue_workspace(workspace).unwrap();
        assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), before);
        assert_eq!(s.engine.tx_gate_gen, generation, "preparation is passive");

        // The ordinary public native gestures are the operator-visible oracle.
        native.engine.set_operating_mode("digital", false);
        match workspace {
            Workspace::Tempo => native.engine.set_area("msg"),
            Workspace::Ft => {
                native.engine.set_area("dx");
                if native.engine.tier() == Tier::Js8 {
                    native.engine.set_tier(Tier::Ft8);
                }
            }
            Workspace::Js8 => native.engine.js8_enter(),
        }
        native.engine.take_immediate_retune();
        let request = s.engine.take_remote_radio().unwrap();
        assert_eq!(request.target_hz, native.engine.settings.dial_hz());
        assert_eq!(request.target_mode, native.engine.rig_mode_effective());
        s.sample(request.target_hz, &request.target_mode);
        let power = request.power_limit;
        assert!(request.commit_readback(&mut s.engine, power));
        assert_eq!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
        assert_eq!(s.engine.tier(), native.engine.tier());
        assert_eq!(
            serde_json::to_value(s.engine.snapshot().mode).unwrap(),
            serde_json::to_value(native.engine.snapshot().mode).unwrap()
        );
        assert_eq!(
            serde_json::to_value(s.engine.settings()).unwrap(),
            serde_json::to_value(native.engine.settings()).unwrap()
        );
        assert_eq!(s.engine.last_dx_tier, native.engine.last_dx_tier);
        assert_eq!(s.engine.last_msg_tier, native.engine.last_msg_tier);
        assert_eq!(s.engine.tx_gate_gen, native.engine.tx_gate_gen);
        assert!(std::sync::Arc::ptr_eq(&source, &s.engine.source));
        assert!(!s.engine.tx_enabled());
        assert!(!s.engine.take_immediate_retune());
        let saved: Settings = serde_json::from_slice(&std::fs::read(&s.path).unwrap()).unwrap();
        assert_eq!(
            serde_json::to_value(saved).unwrap(),
            serde_json::to_value(s.engine.settings()).unwrap()
        );
    }
}

#[test]
fn workspace_entries_match_native_modes_channels_memories_and_lower_power() {
    for (mode, name) in [
        (OperatingMode::Digital, "digital"),
        (OperatingMode::Cw, "cw"),
        (OperatingMode::Phone, "phone"),
        (OperatingMode::Rtty, "rtty"),
        (OperatingMode::Keyboard, "keyboard"),
    ] {
        for from in [Tier::Ft4, Tier::TempoDeep, Tier::Js8] {
            for workspace in [Workspace::Ft, Workspace::Tempo, Workspace::Js8] {
                let mut s = Station::new(mode);
                let mut native = Station::new(mode);
                for station in [&mut s, &mut native] {
                    station.engine.settings.max_power_digital = Some(0.2);
                    station.engine.settings.working_frequencies.push(
                        crate::settings::WorkingFreq {
                            band: "40m".into(),
                            mode: "FT4".into(),
                            mhz: 7.047,
                        },
                    );
                    station.engine.set_tier(from);
                    station.engine.set_operating_mode(name, false);
                    station.engine.set_frequency(7.123, "40m", "LSB");
                    station.engine.set_tx_enabled(false);
                    station.engine.take_immediate_retune();
                    station.engine.rf_power = Some(0.4);
                    station.sample(7_123_000, &station.engine.rig_mode_effective());
                }
                let follow = workspace != Workspace::Tempo && mode != OperatingMode::Digital;
                native.engine.set_operating_mode("digital", follow);
                match workspace {
                    Workspace::Ft => {
                        native.engine.set_area("dx");
                        if native.engine.tier() == Tier::Js8 {
                            native.engine.set_tier(Tier::Ft8);
                        }
                    }
                    Workspace::Tempo => native.engine.set_area("msg"),
                    Workspace::Js8 => native.engine.js8_enter(),
                }
                // Complete native comparison setup before publishing the fresh
                // positive-control reading or starting the command deadline.
                let prior = serde_json::to_value(s.engine.settings()).unwrap();
                s.sample(7_123_000, &s.engine.rig_mode_effective());
                let receipt = s.queue_workspace(workspace).unwrap();
                assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), prior);
                let request = s.engine.take_remote_radio().unwrap();
                assert_eq!(
                    request.target_hz,
                    native.engine.settings.dial_hz(),
                    "{name}/{from:?}/{workspace:?}"
                );
                assert_eq!(
                    request.target_mode,
                    native.engine.rig_mode_effective(),
                    "{name}/{from:?}/{workspace:?}"
                );
                assert_eq!(request.power_limit, Some(0.2));
                request.permission().begin_write(Instant::now()).unwrap();
                s.sample(request.target_hz, &request.target_mode);
                assert!(request.commit_readback(&mut s.engine, Some(0.2)));
                assert_eq!(
                    receipt.outcome(),
                    Outcome::Applied {
                        evidence: Evidence::RadioReadback
                    }
                );
                assert_eq!(s.engine.tier(), native.engine.tier());
                assert_eq!(s.engine.rf_power, native.engine.rf_power);
                assert_eq!(
                    serde_json::to_value(s.engine.settings()).unwrap(),
                    serde_json::to_value(native.engine.settings()).unwrap()
                );
                assert_eq!(
                    serde_json::to_value(s.engine.snapshot().mode).unwrap(),
                    serde_json::to_value(native.engine.snapshot().mode).unwrap()
                );
                assert!(!s.engine.tx_enabled());
                assert!(!s.engine.take_immediate_retune());
            }
        }
    }
}

#[test]
fn entering_ft_from_cw_uses_the_native_section_home_even_when_the_ft_tier_is_unchanged() {
    let mut s = Station::new(OperatingMode::Cw);
    s.engine.set_frequency(14.050, "20m", "USB");
    s.engine.take_immediate_retune();
    s.sample(14_050_000, "CW");
    assert_eq!(s.engine.tier(), Tier::Ft8);
    let mut native = Engine::with_settings(s.engine.settings.clone());
    // The actual FT view owns a frequency; rigModeTransition homes when the
    // last homed native section is CW. No tier change is needed in this case.
    native.set_operating_mode("digital", true);
    native.set_area("dx");
    let receipt = s.queue_workspace(Workspace::Ft).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    assert_eq!(request.target_hz, native.settings.dial_hz());
    assert_eq!(request.target_mode, native.rig_mode_effective());
    assert_eq!(request.target_hz, 14_074_000);
    s.sample(request.target_hz, &request.target_mode);
    let power = request.power_limit;
    assert!(request.commit_readback(&mut s.engine, power));
    assert_eq!(
        receipt.outcome(),
        Outcome::Applied {
            evidence: Evidence::RadioReadback
        }
    );
    assert_eq!(
        serde_json::to_value(s.engine.settings()).unwrap(),
        serde_json::to_value(native.settings()).unwrap()
    );
    assert!(!s.engine.tx_enabled());
}

#[test]
fn workspace_entry_needs_the_actual_power_readback_and_carries_an_already_lower_level() {
    for power in [None, Some(0.3), Some(0.1)] {
        let mut s = Station::new(OperatingMode::Phone);
        s.engine.settings.max_power_digital = Some(0.2);
        s.engine.rf_power = Some(0.5);
        let before = serde_json::to_value(s.engine.settings()).unwrap();
        let receipt = s.queue_workspace(Workspace::Js8).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        assert_eq!(request.power_limit, Some(0.2));
        request.permission().begin_write(Instant::now()).unwrap();
        s.sample(request.target_hz, &request.target_mode);
        if power == Some(0.1) {
            assert!(request.commit_readback(&mut s.engine, power));
            assert_eq!(s.engine.rf_power, power);
            assert_eq!(s.engine.tier(), Tier::Js8);
            assert_eq!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            );
            assert!(s.path.exists());
        } else {
            assert!(!request.commit_readback(&mut s.engine, power));
            assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), before);
            assert_eq!(s.engine.tier(), Tier::Ft8);
            assert_eq!(
                receipt.outcome(),
                Outcome::Unknown {
                    reason: Reason::HardwareUnconfirmed
                }
            );
            assert!(!s.path.exists());
        }
        assert!(!s.engine.tx_enabled());
        assert!(!s.engine.take_immediate_retune());
        assert!(s.engine.take_remote_radio().is_none());
    }
}

#[test]
fn workspace_save_failure_keeps_confirmed_state_without_claiming_durability_or_retrying() {
    let mut s = Station::new(OperatingMode::Digital);
    std::fs::create_dir_all(s.path.parent().unwrap()).unwrap();
    let blocker = s.path.parent().unwrap().join("not-a-directory");
    std::fs::write(&blocker, b"preserve this file").unwrap();
    s.engine
        .configure_remote_settings_store(blocker.join("settings.json"));
    let receipt = s.queue_workspace(Workspace::Tempo).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    request.permission().begin_write(Instant::now()).unwrap();
    s.sample(request.target_hz, &request.target_mode);
    let power = request.power_limit;
    assert!(request.commit_readback(&mut s.engine, power));
    assert_eq!(s.engine.tier(), Tier::TempoFast);
    assert_eq!(s.engine.snapshot().mode, crate::dto::OpMode::Chat);
    assert_eq!(
        receipt.outcome(),
        Outcome::Unknown {
            reason: Reason::HardwareUnconfirmed
        }
    );
    assert_eq!(std::fs::read(blocker).unwrap(), b"preserve this file");
    assert!(!s.engine.tx_enabled());
    assert!(!s.engine.take_immediate_retune());
    assert!(s.engine.take_remote_radio().is_none());
}

#[test]
fn a_workspace_entry_never_waits_on_a_decoder_or_changes_state_before_confirmation() {
    let mut s = Station::new(OperatingMode::Digital);
    let source = s.engine.source.clone();
    let held = source.lock().unwrap();
    let before = serde_json::to_value(s.engine.settings()).unwrap();
    assert!(matches!(
        s.queue_workspace(Workspace::Js8),
        Err(Reason::StationBusy)
    ));
    assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), before);
    assert!(s.engine.take_remote_radio().is_none());
    drop(held);
    let receipt = s.queue_workspace(Workspace::Js8).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.sample(request.target_hz, &request.target_mode);
    let power = request.power_limit;
    let held = source.lock().unwrap();
    assert!(!request.commit_readback(&mut s.engine, power));
    assert!(matches!(
        receipt.outcome(),
        Outcome::Rejected {
            reason: Reason::StationBusy
        }
    ));
    assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), before);
    assert!(!s.path.exists());
    drop(held);
}

#[test]
fn a_local_operating_spec_change_retires_pending_radio_work_with_the_same_cat_context() {
    for workspace in [false, true] {
        let mut s = Station::new(OperatingMode::Digital);
        s.engine.set_mode("qso-monitor").unwrap();
        let receipt = if workspace {
            s.queue_workspace(Workspace::Tempo)
        } else {
            s.queue()
        }
        .unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        // The public native verb resets the local operating spec even if its
        // label and CAT mode are unchanged. That newer operator action wins.
        s.engine.set_mode("qso-monitor").unwrap();
        let local = serde_json::to_value(s.engine.settings()).unwrap();
        s.sample(request.target_hz, &request.target_mode);
        let power = request.power_limit;
        assert!(
            !request.commit_readback(&mut s.engine, power),
            "workspace={workspace}"
        );
        assert!(matches!(
            receipt.outcome(),
            Outcome::Rejected {
                reason: Reason::ContextChanged
            }
        ));
        assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), local);
        assert!(!s.path.exists());
    }
    let mut s = Station::new(OperatingMode::Digital);
    let receipt = s.queue_workspace(Workspace::Tempo).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    assert!(s.engine.set_mode("not-a-mode").is_err());
    s.sample(request.target_hz, &request.target_mode);
    let power = request.power_limit;
    assert!(request.commit_readback(&mut s.engine, power));
    assert_eq!(
        receipt.outcome(),
        Outcome::Applied {
            evidence: Evidence::RadioReadback
        }
    );
}

#[test]
fn remote_tiers_reuse_native_channel_decoder_and_offset_transitions() {
    for tier in Tier::ALL {
        let mut s = Station::new(OperatingMode::Digital);
        s.engine.settings.q65_period_s = 30;
        s.engine.settings.q65_submode = 2;
        s.engine.settings.fst4_period_s = 300;
        s.engine.settings.msk144_period_s = 30;
        s.engine.settings.jt65_submode = 1;
        let from = if tier == Tier::Ft8 {
            Tier::Ft4
        } else {
            Tier::Ft8
        };
        s.engine.set_tier(from);
        s.engine.take_immediate_retune();
        s.sample(s.engine.settings.dial_hz(), &s.engine.rig_mode_effective());
        let source = s.engine.source.clone();
        let before = serde_json::to_value(s.engine.settings()).unwrap();
        let mut native = Engine::with_settings(s.engine.settings.clone());
        native.set_tier(from);
        native.set_tier(tier);
        let receipt = s.queue_tier(tier).unwrap();
        assert_eq!(s.engine.tier(), from, "admission cannot select the decoder");
        assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), before);
        let request = s.engine.take_remote_radio().unwrap();
        assert_eq!(request.target().0, native.settings.dial_hz());
        assert_eq!(request.target().1, native.rig_mode_effective());
        if tier == Tier::Ft4 {
            assert_eq!(request.target().0, 14_080_000);
        }
        if tier == Tier::Msk144 {
            assert_eq!(request.target().0, 50_260_000);
        }
        request.permission().begin_write(Instant::now()).unwrap();
        s.sample(request.target().0, request.target().1);
        assert!(request.commit(&mut s.engine));
        assert_eq!(s.engine.tier(), tier);
        assert_eq!(
            serde_json::to_value(s.engine.settings()).unwrap(),
            serde_json::to_value(native.settings()).unwrap()
        );
        assert_eq!(s.engine.source_label, native.source_label);
        assert_eq!(s.engine.rx_offset_hz, native.rx_offset_hz);
        assert_eq!(s.engine.tx_offset_hz, native.tx_offset_hz);
        assert!(std::sync::Arc::ptr_eq(&source, &s.engine.source));
        assert!(!s.engine.tx_enabled());
        assert!(!s.engine.take_immediate_retune());
        assert!(
            !s.path.exists(),
            "native tier selection does not save Settings"
        );
        assert_eq!(
            receipt.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
    }
}

#[test]
fn same_remote_tier_is_a_complete_noop_even_during_decode_away_from_the_default_dial() {
    let mut s = Station::new(OperatingMode::Digital);
    s.engine.set_frequency(14.076, "20m", "USB");
    s.engine.take_immediate_retune();
    s.sample(14_076_000, "PKTUSB");
    let before = serde_json::to_value(s.engine.settings()).unwrap();
    let epoch = s.engine.decode_epoch;
    let source = s.engine.source.clone();
    let _decode = super::super::source_lock(&source);
    let receipt = s.queue_tier(Tier::Ft8).unwrap();
    assert_eq!(
        receipt.outcome(),
        Outcome::Applied {
            evidence: Evidence::ReceiverState
        }
    );
    assert!(s.engine.take_remote_radio().is_none());
    assert_eq!(s.engine.decode_epoch, epoch);
    assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), before);
    assert!(!s.path.exists());
}

#[test]
fn tier_admission_honors_custom_channels_and_refuses_a_busy_decoder_before_cat() {
    let mut s = Station::new(OperatingMode::Digital);
    s.engine
        .settings
        .working_frequencies
        .push(crate::settings::WorkingFreq {
            band: "20m".into(),
            mode: "FT4".into(),
            mhz: 14.082,
        });
    let source = s.engine.source.clone();
    let guard = super::super::source_lock(&source);
    assert!(matches!(s.queue_tier(Tier::Ft4), Err(Reason::StationBusy)));
    assert!(s.engine.take_remote_radio().is_none());
    assert_eq!(s.engine.tier(), Tier::Ft8);
    drop(guard);
    let receipt = s.queue_tier(Tier::Ft4).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    assert_eq!(request.target(), (14_082_000, "PKTUSB"));
    request.permission().begin_write(Instant::now()).unwrap();
    s.sample(14_082_000, "PKTUSB");
    assert!(request.commit(&mut s.engine));
    assert_eq!(s.engine.settings.dial_hz(), 14_082_000);
    assert_eq!(s.engine.tier(), Tier::Ft4);
    assert!(matches!(receipt.outcome(), Outcome::Applied { .. }));
}

#[test]
fn decoder_starting_during_cat_cannot_block_or_commit_the_remote_tier() {
    let mut s = Station::new(OperatingMode::Digital);
    let receipt = s.queue_tier(Tier::Ft4).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    request.permission().begin_write(Instant::now()).unwrap();
    s.sample(14_080_000, "PKTUSB");
    let source = s.engine.source.clone();
    let guard = super::super::source_lock(&source);
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::scope(|scope| {
        let handle = scope.spawn(|| tx.send(request.commit(&mut s.engine)).unwrap());
        let prompt = rx.recv_timeout(Duration::from_millis(250));
        // Always release and join, even if a regression used a blocking lock.
        drop(guard);
        handle.join().unwrap();
        assert_eq!(prompt, Ok(false));
    });
    assert_eq!(
        receipt.outcome(),
        Outcome::Unknown {
            reason: Reason::HardwareUnconfirmed
        }
    );
    assert_eq!(s.engine.tier(), Tier::Ft8);
    assert_eq!(s.engine.settings.dial_hz(), 14_074_000);
    assert!(!s.engine.take_immediate_retune());
    assert!(s.engine.take_remote_radio().is_none());
    assert!(!s.path.exists());
}

#[test]
fn a_tier_cannot_mislabel_a_custom_dial_or_borrow_a_retired_cat_connection() {
    let mut s = Station::new(OperatingMode::Digital);
    s.engine
        .settings
        .working_frequencies
        .push(crate::settings::WorkingFreq {
            band: "20m".into(),
            mode: "FT4".into(),
            mhz: 7.074,
        });
    assert!(matches!(
        s.queue_tier(Tier::Ft4),
        Err(Reason::InvalidAction)
    ));
    let stale = s
        .engine
        .remote_monitor_observation()
        .radio
        .readings
        .cat
        .unwrap()
        .connection_generation
        + 1;
    let result = s.engine.queue_remote_tier(
        Tier::Ft8,
        stale,
        s.authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap(),
    );
    assert!(
        matches!(result, Err(Reason::ContextChanged)),
        "a no-op still requires its current radio binding"
    );
    assert!(s.engine.take_remote_radio().is_none());
    assert_eq!(s.engine.tier(), Tier::Ft8);
    assert!(!s.path.exists());
}

#[test]
fn local_decoder_or_source_changes_cancel_a_pending_mode_even_if_the_dial_returns() {
    use crate::dto::{SourceKind, Tier};
    for change_source in [false, true] {
        let mut s = Station::new(OperatingMode::Digital);
        // Both native tiers deliberately use the current dial. A frequency
        // callback cannot accidentally provide the cancellation being tested.
        s.engine
            .settings
            .working_frequencies
            .push(crate::settings::WorkingFreq {
                band: "20m".into(),
                mode: "FT4".into(),
                mhz: 14.074,
            });
        let receipt = s.queue_mode("cw", true).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        request.permission().check(Instant::now()).unwrap();
        if change_source {
            // Rebuilding Native is also a local source choice; no UDP port or
            // network provider is needed to exercise the actual source setter.
            s.engine.set_source(SourceKind::Native).unwrap();
        } else {
            s.engine.set_tier(Tier::Ft4);
            assert_eq!(s.engine.tier(), Tier::Ft4);
            s.engine.set_tier(Tier::Ft8);
        }
        assert_eq!(s.engine.settings.dial_hz(), 14_074_000);
        assert!(request.permission().begin_write(Instant::now()).is_err());
        request.refuse(Reason::ContextChanged);
        assert_eq!(
            receipt.outcome(),
            Outcome::Rejected {
                reason: Reason::ContextChanged
            }
        );
        assert!(!s.path.exists());
        assert!(!s.engine.tx_enabled());
    }
}

#[test]
fn remote_section_targets_match_native_entry_and_do_not_arm_transmit() {
    for from in [
        OperatingMode::Digital,
        OperatingMode::Phone,
        OperatingMode::Cw,
        OperatingMode::Rtty,
        OperatingMode::Keyboard,
    ] {
        for mode in ["digital", "phone", "cw", "rtty", "keyboard"] {
            for follow in [false, true] {
                let mut s = Station::new(from);
                assert_eq!(s.engine.settings.operating_mode, from);
                let before = serde_json::to_value(s.engine.settings()).unwrap();
                let mut native = Engine::with_settings(s.engine.settings.clone());
                native.set_operating_mode(mode, follow);
                assert_eq!(
                    native.tx_enabled(),
                    mode != "digital",
                    "local arming stays native"
                );
                let receipt = s.queue_mode(mode, follow).unwrap();
                assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), before);
                assert!(!s.engine.tx_enabled());
                assert!(!s.path.exists());
                let request = s.engine.take_remote_radio().unwrap();
                assert_eq!(
                    request.target(),
                    (
                        native.settings.dial_hz(),
                        native.rig_mode_effective().as_str()
                    )
                );
                request.permission().begin_write(Instant::now()).unwrap();
                s.sample(request.target().0, request.target().1);
                assert!(request.commit(&mut s.engine));
                assert_eq!(
                    serde_json::to_value(s.engine.settings()).unwrap(),
                    serde_json::to_value(native.settings()).unwrap()
                );
                assert!(
                    !s.engine.tx_enabled(),
                    "Remote section entry cannot grant transmit"
                );
                assert!(!s.engine.take_immediate_retune());
                assert!(matches!(receipt.outcome(), Outcome::Applied { .. }));
            }
        }
    }
}

#[test]
fn remote_mode_restores_native_session_memory_and_saves_current_preferences() {
    let mut s = Station::new(OperatingMode::Phone);
    s.engine.set_frequency(14.240, "20m", "USB");
    s.engine.set_operating_mode("cw", true);
    s.engine.set_tx_enabled(false);
    s.engine.take_immediate_retune();
    s.sample(s.engine.settings.dial_hz(), &s.engine.rig_mode_effective());
    let receipt = s.queue_mode("phone", true).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    assert_eq!(request.target(), (14_240_000, "USB"));
    let other = s.engine.settings.active_radio;
    s.engine.rename_radio(other, "Main station");
    request.permission().begin_write(Instant::now()).unwrap();
    s.sample(14_240_000, "USB");
    assert!(request.commit(&mut s.engine));
    assert!(matches!(receipt.outcome(), Outcome::Applied { .. }));
    let saved = Settings::load(&s.path);
    assert_eq!(saved.operating_mode, OperatingMode::Phone);
    assert_eq!(saved.dial_hz(), 14_240_000);
    assert_eq!(saved.active_profile().unwrap().name, "Main station");
    assert!(!s.engine.tx_enabled());
}

#[test]
fn capped_remote_mode_needs_confirmed_lower_power_and_never_queues_an_increase() {
    for observed in [
        None,
        Some(0.8),
        Some(f32::NAN),
        Some(-0.1),
        Some(0.4),
        Some(0.2),
    ] {
        let mut s = Station::new(OperatingMode::Phone);
        s.engine.settings.max_power_digital = Some(0.4);
        s.engine.set_rf_power(0.8);
        let receipt = s.queue_mode("digital", false).unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        assert_eq!(request.power_limit(), Some(0.4));
        request.permission().begin_write(Instant::now()).unwrap();
        s.sample(14_074_000, "PKTUSB");
        let good = observed.is_some_and(|p| p == 0.4 || p == 0.2);
        assert_eq!(request.commit_readback(&mut s.engine, observed), good);
        if good {
            assert_eq!(s.engine.rf_power(), observed);
            assert_eq!(s.engine.rf_power_to_command(), observed.map(|p| (p, false)));
            assert!(!s.engine.tx_enabled());
            assert!(!s.engine.take_immediate_retune());
            assert!(matches!(receipt.outcome(), Outcome::Applied { .. }));
        } else {
            assert_eq!(s.engine.settings.operating_mode, OperatingMode::Phone);
            assert_eq!(s.engine.rf_power(), Some(0.8));
            assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
            assert!(!s.path.exists());
        }
    }
}

#[test]
fn local_power_gesture_cancels_remote_mode_even_if_returned_to_same_level() {
    let mut s = Station::new(OperatingMode::Phone);
    s.engine.settings.max_power_digital = Some(0.4);
    s.engine.set_rf_power(0.8);
    s.queue_mode("digital", false).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    s.engine.set_rf_power(0.2);
    s.engine.set_rf_power(0.8);
    assert_eq!(request.validate(&s.engine), Err(Reason::ContextChanged));
    assert!(!s.path.exists());
}

#[test]
fn mode_save_failure_keeps_confirmed_section_disarmed_without_retry() {
    let mut s = Station::new(OperatingMode::Digital);
    std::fs::create_dir_all(&s.path).unwrap();
    let receipt = s.queue_mode("cw", true).unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    request.permission().begin_write(Instant::now()).unwrap();
    s.sample(request.target().0, request.target().1);
    assert!(request.commit(&mut s.engine));
    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    assert_eq!(s.engine.settings.operating_mode, OperatingMode::Cw);
    assert!(!s.engine.tx_enabled());
    assert!(!s.engine.take_immediate_retune());
}

#[test]
fn frequency_preparation_preserves_live_settings_and_uses_native_mode_policy() {
    for mode in [
        OperatingMode::Digital,
        OperatingMode::Phone,
        OperatingMode::Cw,
        OperatingMode::Rtty,
        OperatingMode::Keyboard,
    ] {
        let mut s = Station::new(mode);
        let before = serde_json::to_value(s.engine.settings()).unwrap();
        let receipt = s.queue().unwrap();
        assert_eq!(serde_json::to_value(s.engine.settings()).unwrap(), before);
        assert!(!s.path.exists());
        assert_eq!(receipt.outcome(), Outcome::Pending);
        let request = s.engine.take_remote_radio().unwrap();
        let prepared = request.target().1.to_string();
        s.engine.set_frequency(7.074, "40m", "USB");
        assert_eq!(prepared, s.engine.rig_mode_effective());
        assert!(!s.engine.tx_enabled());
    }
}

#[test]
fn confirmed_frequency_saves_current_settings_and_has_no_deferred_cat_retry() {
    let mut s = Station::new(OperatingMode::Phone);
    let other = s.engine.add_radio();
    let receipt = s.queue().unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    assert!(request.permission().begin_write(Instant::now()).is_ok());
    // A concurrent unrelated preference must survive; no old whole-settings
    // clone may overwrite it at the transaction's persistence boundary.
    s.engine.rename_radio(other, "Desk receiver");
    s.sample(7_074_000, "LSB");
    assert!(request.commit(&mut s.engine));
    assert_eq!(
        receipt.outcome(),
        Outcome::Applied {
            evidence: Evidence::RadioReadback
        }
    );
    let saved: Settings = serde_json::from_slice(&std::fs::read(&s.path).unwrap()).unwrap();
    assert_eq!(saved.dial_hz(), 7_074_000);
    assert_eq!(saved.band, "40m");
    assert_eq!(
        saved.radios.iter().find(|p| p.id == other).unwrap().name,
        "Desk receiver"
    );
    assert_eq!(Settings::load(&s.path).dial_hz(), 7_074_000);
    assert!(!s.engine.take_immediate_retune());
    // Profile last_* is banked on radio departure, exactly as for a local QSY.
    let active = s.engine.settings.active_radio;
    s.engine.set_active_radio(other);
    assert_eq!(
        s.engine
            .settings
            .radios
            .iter()
            .find(|p| p.id == active)
            .unwrap()
            .last_dial_mhz,
        7.074
    );
    s.engine.set_active_radio(active);
    assert_eq!(s.engine.settings.dial_hz(), 7_074_000);
    assert!(!s.engine.tx_enabled());
}

#[test]
fn persistence_failure_keeps_confirmed_live_position_but_never_claims_success_or_retries() {
    let mut s = Station::new(OperatingMode::Digital);
    std::fs::create_dir_all(&s.path).unwrap(); // atomic rename cannot replace a directory
    let receipt = s.queue().unwrap();
    let request = s.engine.take_remote_radio().unwrap();
    request.permission().begin_write(Instant::now()).unwrap();
    s.sample(7_074_000, "PKTUSB");
    assert!(request.commit(&mut s.engine));
    assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
    assert_eq!(s.engine.settings.dial_hz(), 7_074_000);
    assert!(!s.engine.take_immediate_retune());
    assert!(s.path.is_dir());
}

#[test]
fn local_changes_cancel_a_pending_frequency_even_after_returning_to_the_old_value() {
    let changes: &[fn(&mut Engine)] = &[
        |e| {
            e.set_radio_pegged(true);
            e.set_radio_pegged(false);
        },
        |e| {
            e.request_sideband_override(Some("AM"));
            e.request_sideband_override(None);
            e.take_immediate_retune();
        },
        |e| {
            e.request_split(Some(14.1));
            e.request_split(None);
            e.split_dirty = false;
        },
        |e| {
            e.set_default_radio(Some(e.settings.active_radio));
            e.set_default_radio(None);
        },
        |e| {
            e.set_routing_rules(Vec::new());
        },
        |e| {
            e.set_radio_bands(e.settings.active_radio, vec!["40m".into()]);
            e.set_radio_bands(e.settings.active_radio, Vec::new());
        },
        |e| {
            e.hold_cat_port();
            e.release_cat_port();
        },
        |e| {
            e.set_tx_enabled(true);
            e.set_tx_enabled(false);
        },
        |e| {
            e.request_vfo(true);
            e.request_vfo(false);
            e.take_vfo_apply();
        },
        |e| {
            e.request_rit(100);
            e.request_rit(0);
            e.take_rit_apply();
        },
        |e| {
            e.request_xit(100);
            e.request_xit(0);
            e.take_xit_apply();
        },
    ];
    for change in changes {
        let mut s = Station::new(OperatingMode::Digital);
        s.queue().unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        assert!(
            request.validate(&s.engine).is_ok(),
            "positive control: valid before gesture"
        );
        change(&mut s.engine);
        assert_eq!(request.validate(&s.engine), Err(Reason::ContextChanged));
        assert!(request.permission().begin_write(Instant::now()).is_err());
        assert!(!s.path.exists());
    }
}

#[test]
fn late_or_wrong_readback_cannot_commit_a_target_or_erase_a_local_gesture() {
    for failure in 0..5 {
        let mut s = Station::new(OperatingMode::Digital);
        let receipt = s.queue().unwrap();
        let request = s.engine.take_remote_radio().unwrap();
        request.permission().begin_write(Instant::now()).unwrap();
        match failure {
            0 => s.sample(7_074_000, "CW"),
            1 => s.sample(7_075_000, "PKTUSB"),
            2 => {
                s.sample(7_074_000, "PKTUSB");
                s.authority.revoke();
            }
            3 => {
                s.engine.set_frequency(14.2, "20m", "USB");
                s.sample(7_074_000, "PKTUSB");
            }
            _ => {
                s.connection = s.engine.remote_open_radio().unwrap();
                s.sample(7_074_000, "PKTUSB");
            }
        }
        assert!(!request.commit(&mut s.engine));
        assert!(matches!(receipt.outcome(), Outcome::Unknown { .. }));
        assert_eq!(
            s.engine.settings.dial_mhz,
            if failure == 3 { 14.2 } else { 14.074 }
        );
        assert_eq!(s.engine.take_immediate_retune(), failure == 3);
        assert!(!s.path.exists());
    }
}

#[test]
fn pending_local_tunes_and_held_channels_are_refused_without_mutation() {
    for context in 0..6 {
        let mut s = Station::new(OperatingMode::Digital);
        match context {
            0 => s.engine.immediate_retune = true,
            1 => s.engine.aprs_fm = true,
            2 => s.engine.fm_channel = true,
            3 => s.engine.split_dirty = true,
            4 => s.engine.sat_dial_owner = Some(("test pass".into(), 0)),
            _ => s.engine.set_tx_enabled(true),
        }
        assert!(matches!(s.queue(), Err(Reason::StationBusy)));
        assert_eq!(s.engine.settings.dial_hz(), 14_074_000);
        assert!(s.engine.take_remote_radio().is_none());
    }
}

#[test]
fn frequency_handoffs_still_require_their_own_transaction() {
    let mut s = Station::new(OperatingMode::Phone);
    let other = s.engine.add_radio();
    s.engine.set_radio_bands(other, vec!["40m".into()]);
    assert!(matches!(s.queue(), Err(Reason::UnsupportedAction)));
    // The same target is legal on the operator's pegged active radio.
    s.engine.set_radio_pegged(true);
    s.queue().unwrap();
    s.engine.take_remote_radio().unwrap();
    assert_eq!(s.engine.settings.dial_hz(), 14_074_000);
    assert!(!s.path.exists());
}

#[test]
fn the_station_rejects_invalid_frequency_payloads_without_queuing_or_saving() {
    let mut s = Station::new(OperatingMode::Digital);
    for (dial, band, sideband) in [
        (f64::NAN, "40m", "USB"),
        (0.0, "40m", "USB"),
        (7.074, "20m", "USB"),
        (7.074, "40m", "USB\nT 1"),
        (10.0, "30m", "USB"),
    ] {
        let connection = s
            .engine
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        assert!(matches!(
            s.engine.queue_remote_frequency(
                dial,
                band,
                sideband,
                connection,
                s.authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap()
            ),
            Err(Reason::InvalidAction)
        ));
        assert!(s.engine.take_remote_radio().is_none());
    }
    assert_eq!(s.engine.settings.dial_hz(), 14_074_000);
    assert!(!s.path.exists());
}
