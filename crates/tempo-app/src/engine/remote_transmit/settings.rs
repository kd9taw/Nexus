//! FT cockpit preferences reuse native setters without acquiring or extending
//! TX ownership. The displayed values and incarnation bind every gesture.
use super::*;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FtSettingsContext {
    pub key: String,
    pub tx_offset_hz: f32,
    pub rx_offset_hz: f32,
    pub hold_tx_freq: bool,
    pub tx_even: bool,
    pub tx_cycle_auto: bool,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FtSettingChange {
    TxOffset { hz: f32 },
    BothOffsets { hz: f32 },
    Hold { on: bool },
    Even { even: bool },
    Auto { auto: bool },
}

impl Engine {
    pub fn remote_ft_settings(&self) -> Option<FtSettingsContext> {
        if !matches!(self.tier(), Tier::Ft8 | Tier::Ft4)
            || self.remote_ft_settings_epoch == u64::MAX
            || self.tx_gate_gen == u64::MAX
        {
            return None;
        }
        Some(FtSettingsContext {
            key: format!(
                "{:016x}{:016x}",
                self.remote_ft_settings_epoch, self.tx_gate_gen
            ),
            tx_offset_hz: self.tx_offset_hz,
            rx_offset_hz: self.rx_offset_hz,
            hold_tx_freq: self.hold_tx_freq,
            tx_even: self.tx_even(),
            tx_cycle_auto: self.tx_cycle_auto,
        })
    }

    pub fn change_remote_ft_setting(
        &mut self,
        permit: &TransmitPermit,
        expected: &FtSettingsContext,
        change: &FtSettingChange,
    ) -> Result<(), Reason> {
        self.prepare_remote_ft(permit)?;
        if self.remote_ft_settings().as_ref() != Some(expected) {
            return Err(Reason::ContextChanged);
        }
        let mut next = self.settings.clone();
        match *change {
            FtSettingChange::TxOffset { hz } | FtSettingChange::BothOffsets { hz } => {
                let (lo, hi) = Self::tx_offset_bounds(self.tier());
                if !hz.is_finite() || !(lo..=hi).contains(&hz) {
                    return Err(Reason::InvalidAction);
                }
                next.tx_offset_hz = hz;
                if matches!(change, FtSettingChange::BothOffsets { .. }) {
                    next.rx_offset_hz = hz;
                }
            }
            FtSettingChange::Hold { on } => next.hold_tx_freq = on,
            FtSettingChange::Even { even } => next.tx_even = even,
            // This is a runtime preference in native Nexus, not a persisted field.
            FtSettingChange::Auto { .. } => {}
        }
        if !permit.valid(Instant::now()) {
            return Err(Reason::AuthorityExpired);
        }
        if !matches!(change, FtSettingChange::Auto { .. }) {
            next.save(
                self.remote_settings_path
                    .as_ref()
                    .ok_or(Reason::UnsupportedAction)?,
            )
            .map_err(|_| Reason::PersistenceFailed)?;
        }
        // Once admitted, a saved preference completes even if the connection
        // expires during disk I/O. It cannot arm or renew a transmission. The
        // existing permit remains subject to the native keying/expiry checks.
        match *change {
            FtSettingChange::TxOffset { hz } => self.set_tx_offset(hz),
            FtSettingChange::BothOffsets { hz } => {
                self.set_tx_offset(hz);
                self.set_rx_offset(hz);
            }
            FtSettingChange::Hold { on } => self.set_hold_tx_freq(on),
            FtSettingChange::Even { even } => self.set_tx_even(even),
            FtSettingChange::Auto { auto } => self.set_tx_cycle_auto(auto),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_control::transmit::TransmitAuthority;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    struct Fixture {
        engine: Engine,
        dir: std::path::PathBuf,
        authority: TransmitAuthority,
    }
    impl Fixture {
        fn new(tier: Tier) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let dir = std::env::temp_dir().join(format!(
                "ft-settings-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir(&dir).unwrap();
            let mut engine = Engine::new("KD9TAW", "EN52", 0);
            engine.set_tier(tier);
            engine.configure_remote_settings_store(dir.join("settings.json"));
            engine.set_tx_enabled(false);
            engine.take_immediate_retune();
            engine.take_slot_tx_abort();
            Self {
                engine,
                dir,
                authority: TransmitAuthority::default(),
            }
        }
        fn permit(&self) -> TransmitPermit {
            self.authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap()
        }
        fn apply(&mut self, change: &FtSettingChange) -> Result<(), Reason> {
            let expected = self.engine.remote_ft_settings().unwrap();
            self.engine
                .change_remote_ft_setting(&self.permit(), &expected, change)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn preferences_match_native_during_idle_and_remote_qso_without_arming() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for armed in [false, true] {
                let mut f = Fixture::new(tier);
                let mut native = Fixture::new(tier);
                if armed {
                    native.engine.start_cq(None).unwrap();
                    f.engine.start_remote_ft_cq(f.permit(), None).unwrap();
                    native.engine.take_immediate_retune();
                    f.engine.take_immediate_retune();
                }
                let changes = [
                    FtSettingChange::TxOffset { hz: 1800.0 },
                    FtSettingChange::Hold { on: true },
                    FtSettingChange::BothOffsets { hz: 2100.0 },
                    FtSettingChange::Even { even: false },
                    FtSettingChange::Auto { auto: true },
                    FtSettingChange::Hold { on: false },
                ];
                for change in changes {
                    match change {
                        FtSettingChange::TxOffset { hz } => native.engine.set_tx_offset(hz),
                        FtSettingChange::BothOffsets { hz } => {
                            native.engine.set_tx_offset(hz);
                            native.engine.set_rx_offset(hz);
                        }
                        FtSettingChange::Hold { on } => native.engine.set_hold_tx_freq(on),
                        FtSettingChange::Even { even } => native.engine.set_tx_even(even),
                        FtSettingChange::Auto { auto } => native.engine.set_tx_cycle_auto(auto),
                    }
                    f.apply(&change).unwrap();
                    assert_eq!(f.engine.settings(), native.engine.settings());
                    assert_eq!(f.engine.snapshot().qso, native.engine.snapshot().qso);
                    assert_eq!(f.engine.tx_cycle_auto(), native.engine.tx_cycle_auto());
                    assert_eq!(f.engine.tx_even(), native.engine.tx_even());
                    assert_eq!(f.engine.tx_enabled(), armed);
                    assert_eq!(f.engine.immediate_tx, native.engine.immediate_tx);
                    assert_eq!(
                        f.engine.take_slot_tx_abort(),
                        native.engine.take_slot_tx_abort()
                    );
                    if !matches!(change, FtSettingChange::Auto { .. }) {
                        let saved: crate::settings::Settings = serde_json::from_slice(
                            &std::fs::read(f.dir.join("settings.json")).unwrap(),
                        )
                        .unwrap();
                        assert_eq!(&saved, f.engine.settings());
                    }
                }
                f.authority.revoke();
                assert_eq!(f.engine.poll_remote_transmit(Instant::now()), armed);
                assert!(!f.engine.tx_enabled());
            }
        }
    }

    #[test]
    fn away_and_back_local_preferences_retire_the_captured_gesture() {
        for which in 0..5 {
            let mut f = Fixture::new(Tier::Ft8);
            let expected = f.engine.remote_ft_settings().unwrap();
            match which {
                0 => {
                    f.engine.set_tx_offset(2500.0);
                    f.engine.set_tx_offset(expected.tx_offset_hz);
                }
                1 => {
                    f.engine.set_rx_offset(2500.0);
                    f.engine.set_rx_offset(expected.rx_offset_hz);
                }
                2 => {
                    f.engine.set_hold_tx_freq(!expected.hold_tx_freq);
                    f.engine.set_hold_tx_freq(expected.hold_tx_freq);
                }
                3 => {
                    f.engine.set_tx_even(!expected.tx_even);
                    f.engine.set_tx_even(expected.tx_even);
                    f.engine.set_tx_cycle_auto(expected.tx_cycle_auto);
                }
                _ => {
                    f.engine.set_tx_cycle_auto(!expected.tx_cycle_auto);
                    f.engine.set_tx_cycle_auto(expected.tx_cycle_auto);
                }
            }
            let before = f.engine.settings().clone();
            assert_eq!(
                f.engine.change_remote_ft_setting(
                    &f.permit(),
                    &expected,
                    &FtSettingChange::Hold { on: true }
                ),
                Err(Reason::ContextChanged)
            );
            assert_eq!(f.engine.settings(), &before);
            assert!(!f.dir.join("settings.json").exists());
        }
    }

    #[test]
    fn failed_save_and_invalid_offset_change_neither_runtime_nor_choices() {
        let mut f = Fixture::new(Tier::Ft4);
        let expected = f.engine.remote_ft_settings().unwrap();
        let before = f.engine.settings().clone();
        for hz in [f32::NAN, f32::INFINITY, 199.0, 4001.0] {
            assert_eq!(
                f.apply(&FtSettingChange::TxOffset { hz }),
                Err(Reason::InvalidAction)
            );
        }
        std::fs::create_dir(f.dir.join("settings.json")).unwrap();
        assert_eq!(
            f.apply(&FtSettingChange::BothOffsets { hz: 2500.0 }),
            Err(Reason::PersistenceFailed)
        );
        assert_eq!(f.engine.remote_ft_settings(), Some(expected));
        assert_eq!(f.engine.settings(), &before);
        // Auto has the same runtime-only behavior as the native command.
        f.apply(&FtSettingChange::Auto { auto: false }).unwrap();
        assert!(!f.engine.tx_cycle_auto());
        assert_eq!(f.engine.settings(), &before);
    }

    #[test]
    fn expired_permission_local_transmit_and_exhausted_identity_refuse() {
        let mut f = Fixture::new(Tier::Ft8);
        let expected = f.engine.remote_ft_settings().unwrap();
        let permit = f.permit();
        f.authority.revoke();
        assert_eq!(
            f.engine.change_remote_ft_setting(
                &permit,
                &expected,
                &FtSettingChange::Even { even: false }
            ),
            Err(Reason::AuthorityExpired)
        );
        f.engine.start_cq(None).unwrap();
        assert_eq!(
            f.apply(&FtSettingChange::Hold { on: true }),
            Err(Reason::StationBusy)
        );
        assert!(f.engine.tx_enabled());
        assert!(!f.dir.join("settings.json").exists());
        f.engine.remote_ft_settings_epoch = u64::MAX;
        assert!(f.engine.remote_ft_settings().is_none());
    }
}
