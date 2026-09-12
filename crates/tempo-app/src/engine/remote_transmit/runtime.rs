//! Live FT receive selection and Skip Tx1 share the native station state. A
//! receive adjustment during an owned FT session cannot move the TX marker.
use super::settings::FtSettingsContext;
use super::*;

#[derive(Clone, Debug, PartialEq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FtRuntimeContext {
    pub settings: FtSettingsContext,
    pub skip_tx1: bool,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum FtRuntimeChange {
    RxOffset { hz: f32 },
    SkipTx1 { on: bool },
}

impl Engine {
    pub fn remote_ft_runtime(&self) -> Option<FtRuntimeContext> {
        Some(FtRuntimeContext {
            settings: self.remote_ft_settings()?,
            skip_tx1: self.skip_tx1(),
        })
    }

    pub fn change_remote_ft_runtime(
        &mut self,
        permit: &TransmitPermit,
        expected: &FtRuntimeContext,
        change: &FtRuntimeChange,
    ) -> Result<(), Reason> {
        self.prepare_remote_ft(permit)?;
        if self.remote_ft_runtime().as_ref() != Some(expected) {
            return Err(Reason::ContextChanged);
        }
        match *change {
            FtRuntimeChange::RxOffset { hz } => {
                if !hz.is_finite() || !(200.0..=4000.0).contains(&hz) {
                    return Err(Reason::InvalidAction);
                }
                let mut next = self.settings.clone();
                next.rx_offset_hz = hz;
                if !permit.valid(Instant::now()) {
                    return Err(Reason::AuthorityExpired);
                }
                next.save(
                    self.remote_settings_path
                        .as_ref()
                        .ok_or(Reason::UnsupportedAction)?,
                )
                .map_err(|_| Reason::PersistenceFailed)?;
                // Native RX is independent of TX even with Hold off. Complete
                // the admitted saved choice without renewing or acquiring TX.
                self.set_rx_offset(hz);
            }
            FtRuntimeChange::SkipTx1 { on } => {
                if !permit.valid(Instant::now()) {
                    return Err(Reason::AuthorityExpired);
                }
                self.set_skip_tx1(on);
            }
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
                "ft-runtime-{}-{}",
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
        fn apply(&mut self, change: &FtRuntimeChange) -> Result<(), Reason> {
            let expected = self.engine.remote_ft_runtime().unwrap();
            self.engine
                .change_remote_ft_runtime(&self.permit(), &expected, change)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn rx_and_skip_match_native_during_an_owned_qso_and_preserve_transmit_frequency() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            for hold in [false, true] {
                let mut f = Fixture::new(tier);
                let mut native = Fixture::new(tier);
                native.engine.start_cq(None).unwrap();
                f.engine.start_remote_ft_cq(f.permit(), None).unwrap();
                for e in [&mut f.engine, &mut native.engine] {
                    e.set_hold_tx_freq(hold);
                    e.set_tx_offset(1700.0);
                    e.take_immediate_retune();
                }
                native.engine.set_rx_offset(900.0);
                f.apply(&FtRuntimeChange::RxOffset { hz: 900.0 }).unwrap();
                assert_eq!(f.engine.rx_offset_hz(), 900.0);
                assert_eq!(f.engine.tx_offset_hz(), 1700.0);
                assert_eq!(f.engine.settings(), native.engine.settings());
                let saved = std::fs::read(f.dir.join("settings.json")).unwrap();
                for on in [true, false] {
                    native.engine.set_skip_tx1(on);
                    f.apply(&FtRuntimeChange::SkipTx1 { on }).unwrap();
                    assert_eq!(f.engine.skip_tx1(), on);
                    assert_eq!(f.engine.snapshot().qso, native.engine.snapshot().qso);
                    assert_eq!(f.engine.tx_even(), native.engine.tx_even());
                    assert_eq!(f.engine.immediate_tx, native.engine.immediate_tx);
                    assert_eq!(
                        f.engine.take_slot_tx_abort(),
                        native.engine.take_slot_tx_abort()
                    );
                    assert!(f.engine.tx_enabled());
                    assert_eq!(std::fs::read(f.dir.join("settings.json")).unwrap(), saved);
                }
                f.authority.revoke();
                assert!(f.engine.poll_remote_transmit(Instant::now()));
                assert!(!f.engine.tx_enabled());
            }
        }
    }

    #[test]
    fn idle_skip_is_session_only_and_local_away_and_back_retires_remote_intent() {
        let mut f = Fixture::new(Tier::Ft8);
        let expected = f.engine.remote_ft_runtime().unwrap();
        f.engine.set_skip_tx1(true);
        f.engine.set_skip_tx1(false);
        assert_eq!(
            f.engine.change_remote_ft_runtime(
                &f.permit(),
                &expected,
                &FtRuntimeChange::SkipTx1 { on: true }
            ),
            Err(Reason::ContextChanged)
        );
        f.apply(&FtRuntimeChange::SkipTx1 { on: true }).unwrap();
        assert!(!f.engine.tx_enabled());
        assert!(f.engine.skip_tx1());
        assert!(!f.dir.join("settings.json").exists());
        assert!(!Engine::with_settings(f.engine.settings().clone()).skip_tx1());
    }

    #[test]
    fn invalid_rx_and_failed_save_leave_both_markers_unchanged() {
        let mut f = Fixture::new(Tier::Ft4);
        let before = f.engine.remote_ft_runtime();
        for hz in [f32::NAN, f32::INFINITY, 199.0, 4001.0] {
            assert_eq!(
                f.apply(&FtRuntimeChange::RxOffset { hz }),
                Err(Reason::InvalidAction)
            );
        }
        std::fs::create_dir(f.dir.join("settings.json")).unwrap();
        assert_eq!(
            f.apply(&FtRuntimeChange::RxOffset { hz: 900.0 }),
            Err(Reason::PersistenceFailed)
        );
        assert_eq!(f.engine.remote_ft_runtime(), before);
    }

    #[test]
    fn local_transmission_and_expired_authority_cannot_be_taken_over() {
        let mut f = Fixture::new(Tier::Ft8);
        let expected = f.engine.remote_ft_runtime().unwrap();
        let permit = f.permit();
        f.authority.revoke();
        assert_eq!(
            f.engine.change_remote_ft_runtime(
                &permit,
                &expected,
                &FtRuntimeChange::SkipTx1 { on: true }
            ),
            Err(Reason::AuthorityExpired)
        );
        f.engine.start_cq(None).unwrap();
        assert_eq!(
            f.apply(&FtRuntimeChange::RxOffset { hz: 900.0 }),
            Err(Reason::StationBusy)
        );
        assert!(f.engine.tx_enabled());
        assert!(!f.dir.join("settings.json").exists());
    }
}
