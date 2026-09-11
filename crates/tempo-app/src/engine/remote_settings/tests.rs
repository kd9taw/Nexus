use super::*;
use crate::remote_control::{Outcome, Revocation};
use crate::remote_monitor::provenance::Connection;
use crate::settings::Settings;
use std::path::PathBuf;
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
    fn new(tier: Tier) -> Self {
        let path = std::env::temp_dir()
            .join(format!(
                "nexus-decoder-setting-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ))
            .join("settings.json");
        let mut settings = Settings::default();
        settings.ensure_radio_profiles();
        let mut other = settings.active_profile().unwrap().clone();
        other.id = 9;
        other.name = "Independent radio".into();
        other.amp_follow_band = !settings.amp_follow_band;
        settings.radios.push(other);
        let mut engine = Engine::with_settings(settings);
        engine.set_tier(tier);
        engine.take_immediate_retune();
        engine.set_tx_enabled(false);
        engine.configure_remote_settings_store(path.clone());
        let connection = engine.remote_open_radio().unwrap();
        let mut s = Self {
            engine,
            connection,
            authority: Default::default(),
            path,
        };
        s.sample(Some(false), Duration::ZERO);
        s
    }
    fn sample(&mut self, keyed: Option<bool>, age: Duration) {
        let read = self
            .engine
            .remote_radio_read(&self.connection, Instant::now() - age)
            .unwrap();
        self.engine.remote_observe_cat(Some(&read), Some(true));
        self.engine
            .remote_observe_dial(Some(&read), Some(self.engine.settings.dial_hz()));
        self.engine
            .remote_observe_mode(Some(&read), Some(&self.engine.rig_mode_effective()));
        self.engine.remote_observe_ptt(Some(&read), keyed);
    }
    fn setting(&self, next: u16) -> DecoderSetting {
        match self.engine.tier() {
            Tier::Js8 => DecoderSetting::Js8Speed {
                expected: self.engine.settings.js8_speed,
                speed: next as u8,
            },
            _ => DecoderSetting::Msk144Period {
                expected: self.engine.settings.msk144_period_s,
                secs: next,
            },
        }
    }
    fn apply(&mut self, setting: DecoderSetting) -> Result<(), Reason> {
        let connection = self.connection_generation();
        self.engine.save_remote_decoder_setting(
            setting,
            connection,
            &self
                .authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
        )
    }
    fn connection_generation(&self) -> u64 {
        self.engine
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation
    }
}
impl Drop for Station {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(self.path.parent().unwrap());
    }
}
fn settings(engine: &Engine) -> serde_json::Value {
    serde_json::to_value(engine.settings()).unwrap()
}

#[test]
fn decoder_settings_match_native_policy_and_preserve_all_other_saved_choices() {
    for (tier, choices) in [(Tier::Js8, [0, 2, 3, 1]), (Tier::Msk144, [5, 10, 30, 15])] {
        let mut remote = Station::new(tier);
        let mut native = Station::new(tier);
        let source = remote.engine.source.clone();
        for next in choices {
            remote.sample(Some(false), Duration::ZERO);
            let setting = remote.setting(next);
            remote.apply(setting).unwrap();
            match tier {
                Tier::Js8 => native.engine.js8_set_speed(next as u8).unwrap(),
                _ => native.engine.set_msk144_period(next),
            }
            assert_eq!(settings(&remote.engine), settings(&native.engine));
            let saved: Settings =
                serde_json::from_slice(&std::fs::read(&remote.path).unwrap()).unwrap();
            assert_eq!(
                serde_json::to_value(saved).unwrap(),
                settings(&native.engine)
            );
            assert_eq!(
                remote.engine.active_slot_secs(),
                native.engine.active_slot_secs()
            );
            assert_eq!(
                remote.engine.active_capture_samples(),
                native.engine.active_capture_samples()
            );
            assert_eq!(remote.engine.tx_gate_gen, native.engine.tx_gate_gen);
            assert_eq!(remote.engine.decode_epoch, native.engine.decode_epoch);
            assert_eq!(
                remote.engine.js8_state().speed,
                native.engine.js8_state().speed
            );
            assert_eq!(
                remote.engine.js8_state().rx_speeds,
                native.engine.js8_state().rx_speeds
            );
            assert!(std::sync::Arc::ptr_eq(&source, &remote.engine.source));
            assert_eq!(remote.engine.source_label, native.engine.source_label);
            assert!(!remote.engine.tx_enabled());
            assert!(!remote.engine.take_immediate_retune());
            assert!(remote.engine.take_remote_radio().is_none());
        }
    }
}

#[test]
fn speed_refuses_a_busy_decoder_but_the_native_narrow_msk_period_never_takes_its_lock() {
    for (tier, next) in [(Tier::Js8, 3), (Tier::Msk144, 5)] {
        let mut s = Station::new(tier);
        let original = settings(&s.engine);
        let generation = s.engine.tx_gate_gen;
        let source = s.engine.source.clone();
        let held = source.lock().unwrap();
        let setting = s.setting(next);
        let start = Instant::now();
        let result = s.apply(setting);
        assert!(start.elapsed() < Duration::from_secs(1));
        assert_eq!(s.engine.tx_gate_gen, generation);
        if tier == Tier::Js8 {
            assert_eq!(result, Err(Reason::StationBusy));
            assert_eq!(settings(&s.engine), original);
            assert!(!s.path.exists());
        } else {
            assert_eq!(result, Ok(()));
            assert_eq!(s.engine.settings.msk144_period_s, 5);
            assert!(s.path.is_file());
        }
        drop(held);
        if tier == Tier::Js8 {
            s.apply(setting).unwrap();
        }
    }
}

#[test]
fn decoder_save_failure_publishes_neither_runtime_nor_settings_changes() {
    for (tier, next) in [(Tier::Js8, 3), (Tier::Msk144, 5)] {
        let mut s = Station::new(tier);
        std::fs::create_dir_all(s.path.parent().unwrap()).unwrap();
        let blocker = s.path.parent().unwrap().join("not-a-directory");
        std::fs::write(&blocker, b"retain this file").unwrap();
        s.engine
            .configure_remote_settings_store(blocker.join("settings.json"));
        let original = settings(&s.engine);
        let epoch = s.engine.decode_epoch;
        let generation = s.engine.tx_gate_gen;
        let setting = s.setting(next);
        assert_eq!(s.apply(setting), Err(Reason::PersistenceFailed));
        assert_eq!(settings(&s.engine), original);
        assert_eq!(s.engine.decode_epoch, epoch);
        assert_eq!(s.engine.tx_gate_gen, generation);
        assert_eq!(std::fs::read(blocker).unwrap(), b"retain this file");
        assert!(!s.path.exists());
        assert!(!s.engine.tx_enabled());
    }
}

#[test]
fn decoder_choice_uses_the_native_displayed_fallback_for_legacy_invalid_settings() {
    for (tier, next) in [(Tier::Js8, 3), (Tier::Msk144, 5)] {
        let mut s = Station::new(tier);
        let setting = s.setting(next);
        if tier == Tier::Js8 {
            s.engine.settings.js8_speed = 200;
            assert_eq!(s.engine.js8_state().speed, modes::Js8Speed::Normal);
        } else {
            s.engine.settings.msk144_period_s = 14;
            assert_eq!(s.engine.active_slot_secs(), 15.0);
        }
        s.apply(setting).unwrap();
        assert_eq!(
            s.engine.active_slot_secs(),
            if tier == Tier::Js8 { 6.0 } else { 5.0 }
        );
        assert!(!s.engine.tx_enabled());
        assert!(s.path.is_file());
    }
}

#[test]
fn decoder_setting_refuses_stale_unknown_keyed_and_armed_station_state() {
    for (tier, next) in [(Tier::Js8, 3), (Tier::Msk144, 5)] {
        let mut s = Station::new(tier);
        let original = settings(&s.engine);
        let setting = s.setting(next);
        for (ptt, age, reason) in [
            (None, Duration::ZERO, Reason::ReadingUnavailable),
            (Some(true), Duration::ZERO, Reason::StationBusy),
            (
                Some(false),
                Duration::from_secs(2),
                Reason::ReadingUnavailable,
            ),
        ] {
            s.sample(ptt, age);
            assert_eq!(s.apply(setting), Err(reason));
            assert_eq!(settings(&s.engine), original);
            assert!(!s.path.exists());
        }
        s.sample(Some(false), Duration::ZERO);
        s.engine.set_tx_enabled(true);
        assert!(
            s.engine.tx_enabled(),
            "armed negative case has a positive control"
        );
        assert_eq!(s.apply(setting), Err(Reason::StationBusy));
        s.engine.set_tx_enabled(false);
        assert_eq!(s.apply(setting), Err(Reason::StationBusy));
        assert!(!s.path.exists());
        // The normal radio worker must first settle the retune requested by
        // native TX arming. Disarming must not erase that outstanding context.
        assert!(s.engine.take_immediate_retune());
        s.sample(Some(false), Duration::ZERO);
        s.apply(setting).unwrap();
    }
}

#[test]
fn decoder_setting_requires_exact_values_context_and_live_permission() {
    for (tier, next) in [(Tier::Js8, 3), (Tier::Msk144, 5)] {
        let mut s = Station::new(tier);
        let setting = s.setting(next);
        let original = settings(&s.engine);
        let malformed = if tier == Tier::Js8 {
            [
                DecoderSetting::Js8Speed {
                    expected: 4,
                    speed: 0,
                },
                DecoderSetting::Js8Speed {
                    expected: 1,
                    speed: 4,
                },
                DecoderSetting::Js8Speed {
                    expected: 1,
                    speed: 1,
                },
            ]
        } else {
            [
                DecoderSetting::Msk144Period {
                    expected: 14,
                    secs: 5,
                },
                DecoderSetting::Msk144Period {
                    expected: 15,
                    secs: 14,
                },
                DecoderSetting::Msk144Period {
                    expected: 15,
                    secs: 15,
                },
            ]
        };
        for invalid in malformed {
            assert_eq!(s.apply(invalid), Err(Reason::InvalidAction));
        }
        let conflict = if tier == Tier::Js8 {
            DecoderSetting::Js8Speed {
                expected: 0,
                speed: 2,
            }
        } else {
            DecoderSetting::Msk144Period {
                expected: 30,
                secs: 5,
            }
        };
        assert_eq!(s.apply(conflict), Err(Reason::ContextChanged));
        let connection = s.connection_generation();
        let permit = s
            .authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap();
        assert_eq!(
            s.engine
                .save_remote_decoder_setting(setting, connection + 1, &permit),
            Err(Reason::ContextChanged)
        );
        s.authority.revoke();
        assert_eq!(
            s.engine
                .save_remote_decoder_setting(setting, connection, &permit),
            Err(Reason::AuthorityExpired)
        );
        s.engine.settings.operating_mode = OperatingMode::Cw;
        assert_eq!(s.apply(setting), Err(Reason::UnsupportedAction));
        s.engine.settings.operating_mode = OperatingMode::Digital;
        assert_eq!(settings(&s.engine), original);
        s.engine.set_tier(Tier::Ft8);
        s.engine.take_immediate_retune();
        assert_eq!(s.apply(setting), Err(Reason::ContextChanged));
        s.engine.set_tier(tier);
        s.engine.take_immediate_retune();
        assert!(!s.path.exists());
        s.apply(setting).unwrap();
    }
}

#[test]
fn local_decoder_choices_retire_pending_remote_hardware_work_without_rewriting_tx_policy() {
    for tier in [Tier::Js8, Tier::Msk144] {
        let mut s = Station::new(tier);
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
        let tx_generation = s.engine.tx_gate_gen;
        if tier == Tier::Js8 {
            assert!(s.engine.js8_set_speed(4).is_err());
            assert!(work.permission().check(Instant::now()).is_ok());
            s.engine.js8_set_speed(s.engine.settings.js8_speed).unwrap();
        } else {
            s.engine
                .set_msk144_period(s.engine.settings.msk144_period_s);
        }
        assert_eq!(
            work.permission().begin_write(Instant::now()),
            Err(Reason::ContextChanged)
        );
        assert_eq!(s.engine.tx_gate_gen, tx_generation);
        assert!(!s.engine.tx_enabled());
    }
}
