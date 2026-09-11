//! Native ownership of an explicitly authorized Remote FT transmission.
//! The existing FT sequencer, slot timing and waveform code remain authoritative.
//! No browser action is exposed by this internal foundation alone.
use super::Engine;
use crate::dto::{SourceKind, Tier};
use crate::remote_control::{transmit::TransmitPermit, Reason};
use crate::settings::OperatingMode;
use std::time::Instant;

impl Engine {
    /// Begin CQ using exactly the native operating verb. Existing local arming
    /// is not permission for a remote browser to take over that transmission.
    /// The host still owns device approval, command context and hardware checks.
    pub fn start_remote_ft_cq(
        &mut self,
        permit: TransmitPermit,
        direction: Option<&str>,
    ) -> Result<(), Reason> {
        if !permit.valid(Instant::now()) {
            return Err(Reason::AuthorityExpired);
        }
        if self.source_kind != SourceKind::Native
            || self.settings.operating_mode != OperatingMode::Digital
            || !matches!(self.tier(), Tier::Ft8 | Tier::Ft4)
        {
            return Err(Reason::UnsupportedAction);
        }
        self.remote_radio_idle()?;
        self.start_cq(direction)
            .map_err(|_| Reason::InvalidAction)?;
        // Native entry may spend time resetting a decoder. Recheck before any
        // future TX plan can observe the newly armed state.
        if !permit.valid(Instant::now()) {
            self.halt_tx();
            return Err(Reason::AuthorityExpired);
        }
        self.remote_transmit = Some(permit);
        Ok(())
    }

    pub fn renew_remote_transmit(&mut self, permit: TransmitPermit, now: Instant) -> bool {
        self.remote_transmit
            .as_mut()
            .is_some_and(|current| current.renew(permit, now))
    }

    /// Called by the real radio loop and at both FT planning and commit. A
    /// disconnected host cannot leave the station waiting for another request
    /// to notice expiry. Native halt supplies the existing flush/unkey signals.
    pub fn poll_remote_transmit(&mut self, now: Instant) -> bool {
        if self.remote_transmit.as_ref().is_some_and(|p| !p.valid(now)) {
            self.halt_tx();
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_control::transmit::TransmitAuthority;
    use std::time::Duration;

    fn station(tier: Tier) -> Engine {
        let mut engine = Engine::new("KD9TAW", "EN52", 0);
        engine.set_tier(tier);
        engine.configure_remote_settings_store(
            std::env::temp_dir().join("nexus-remote-ft-authority-test.json"),
        );
        engine.set_tx_enabled(false);
        engine.take_immediate_retune();
        engine.take_slot_tx_abort();
        engine
    }

    #[test]
    fn remote_cq_preserves_native_ft_state_and_expiry_uses_the_existing_halt() {
        for tier in [Tier::Ft8, Tier::Ft4] {
            let mut native = station(tier);
            let mut remote = station(tier);
            let authority = TransmitAuthority::default();
            let now = Instant::now();
            let deadline = now + Duration::from_secs(5);
            native.start_cq(Some("DX")).unwrap();
            remote
                .start_remote_ft_cq(authority.permit(deadline).unwrap(), Some("DX"))
                .unwrap();
            assert_eq!(remote.settings, native.settings);
            assert_eq!(remote.snapshot().qso, native.snapshot().qso);
            assert_eq!(remote.tx_even(), native.tx_even());
            assert_eq!(remote.tx_gate_gen, native.tx_gate_gen);
            assert_eq!(remote.immediate_tx, native.immediate_tx);
            assert_eq!(remote.immediate_retune, native.immediate_retune);
            assert!(remote.tx_enabled());
            assert!(!remote.poll_remote_transmit(now));
            assert!(remote.poll_remote_transmit(deadline));
            assert!(!remote.tx_enabled());
            assert!(remote.take_slot_tx_abort());
            assert!(!remote.immediate_tx);
            assert!(!remote.renew_remote_transmit(
                authority.permit(deadline + Duration::from_secs(5)).unwrap(),
                deadline
            ));
            assert!(!remote.poll_remote_transmit(deadline));
        }
    }

    #[test]
    fn revoked_authority_cannot_commit_a_previously_planned_ft_over() {
        let mut engine = station(Tier::Ft8);
        let authority = TransmitAuthority::default();
        let permit = authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap();
        engine.start_remote_ft_cq(permit, None).unwrap();
        let slot = if engine.tx_even() { 0 } else { 1 };
        let plan = engine
            .plan_tx(slot)
            .expect("positive control: native CQ plans an over");
        authority.revoke();
        assert!(engine.commit_tx(&plan, vec![0.0; 120], slot).is_empty());
        assert!(!engine.tx_enabled());
        assert!(engine.take_slot_tx_abort());
    }

    #[test]
    fn native_rearm_takes_ownership_and_cannot_be_stopped_by_an_old_remote_session() {
        let mut engine = station(Tier::Ft8);
        let authority = TransmitAuthority::default();
        engine
            .start_remote_ft_cq(
                authority
                    .permit(Instant::now() + Duration::from_secs(5))
                    .unwrap(),
                None,
            )
            .unwrap();
        engine.set_tx_enabled(true);
        authority.revoke();
        assert!(!engine.poll_remote_transmit(Instant::now()));
        assert!(engine.tx_enabled());
        assert!(!engine.renew_remote_transmit(
            authority
                .permit(Instant::now() + Duration::from_secs(5))
                .unwrap(),
            Instant::now()
        ));
    }
}
