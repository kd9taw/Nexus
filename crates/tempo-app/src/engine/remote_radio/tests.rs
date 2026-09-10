use super::*;
use crate::remote_control::Revocation;
use crate::remote_monitor::provenance::Connection;
use crate::settings::Settings;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

static NEXT: AtomicUsize = AtomicUsize::new(0);
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
}
impl Drop for Station {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.path.parent().unwrap());
    }
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
        let request = s.engine.take_remote_frequency().unwrap();
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
    let request = s.engine.take_remote_frequency().unwrap();
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
    let request = s.engine.take_remote_frequency().unwrap();
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
        let request = s.engine.take_remote_frequency().unwrap();
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
        let request = s.engine.take_remote_frequency().unwrap();
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
        assert!(s.engine.take_remote_frequency().is_none());
    }
}

#[test]
fn radio_handoffs_and_fm_contexts_require_their_own_transaction() {
    let mut s = Station::new(OperatingMode::Phone);
    let other = s.engine.add_radio();
    s.engine.set_radio_bands(other, vec!["40m".into()]);
    assert!(matches!(s.queue(), Err(Reason::UnsupportedAction)));
    // The same target is legal on the operator's pegged active radio.
    s.engine.set_radio_pegged(true);
    s.queue().unwrap();
    s.engine.take_remote_frequency().unwrap();
    s.engine.request_sideband_override(Some("FM"));
    s.engine.take_immediate_retune();
    assert!(matches!(s.queue(), Err(Reason::UnsupportedAction)));
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
        (10.0, "", "USB"),
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
        assert!(s.engine.take_remote_frequency().is_none());
    }
    assert_eq!(s.engine.settings.dial_hz(), 14_074_000);
    assert!(!s.path.exists());
}
