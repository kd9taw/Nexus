//! Admission for a station-owned radio selection. The incoming Settings are a
//! passive native projection, never a browser payload or a state to apply wholesale.
//! The radio owner must acquire, configure and adopt the actual incoming connection.
use super::radio_selection::RadioSelection;
use super::Engine;
use crate::remote_control::{Completion, Evidence, Outcome, Permit, Reason, WritePermission};
use crate::settings::Settings;
use std::time::Instant;

pub struct Request {
    original: Settings,
    original_mode: String,
    connection: u64,
    selection: RadioSelection,
    permission: WritePermission,
    completion: Completion,
}

impl Engine {
    /// A configured id, not an address, port, profile patch or arbitrary command.
    /// No radio/profile/decoder mutation occurs at admission. The caller still
    /// needs a native worker completion before reporting a successful selection.
    pub fn queue_remote_radio_selection(
        &mut self,
        id: u32,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_idle()?;
        self.remote_radio_link(connection)?;
        if self.source_kind != crate::dto::SourceKind::Native {
            return Err(Reason::UnsupportedAction);
        }
        if self
            .remote_radio_command
            .as_ref()
            .is_some_and(|request| request.pending())
            || self
                .remote_radio_selection
                .as_ref()
                .is_some_and(|request| request.pending())
        {
            return Err(Reason::StationBusy);
        }
        if !self.settings.radios.iter().any(|profile| profile.id == id) {
            return Err(Reason::InvalidAction);
        }
        let completion = Completion::guarded(permit.clone());
        let permission = WritePermission::new(
            permit.clone(),
            self.remote_actuation_permit(permit.deadline())
                .ok_or(Reason::ContextChanged)?,
            completion.clone(),
        );
        permission.check(Instant::now())?;
        if id == self.settings.active_radio {
            // Preserve the local no-op: do not reset the decoder, touch a
            // connection or re-save settings merely to select the active radio.
            completion.finish(Outcome::Applied {
                evidence: Evidence::StationState,
            });
            return Ok(completion);
        }
        let request = Request {
            original: self.settings.clone(),
            original_mode: self.rig_mode_effective(),
            connection,
            selection: self
                .preview_radio_selection(id)
                .ok_or(Reason::InvalidAction)?,
            permission,
            completion: completion.clone(),
        };
        request.validate(self)?;
        self.remote_radio_selection = Some(request);
        Ok(completion)
    }

    /// Only the active radio owner consumes this slot, before local handoff.
    /// Taking a request does not transfer the active radio or grant TX permission.
    pub fn take_remote_radio_selection(&mut self) -> Option<Request> {
        self.remote_radio_selection.take()
    }
}

impl Request {
    pub(super) fn pending(&self) -> bool {
        matches!(self.completion.outcome(), Outcome::Pending)
    }

    pub fn settings(&self) -> &Settings {
        self.selection.settings()
    }

    pub fn permission(&self) -> &WritePermission {
        &self.permission
    }

    pub fn expected(&self) -> (u64, &str) {
        (self.original.dial_hz(), &self.original_mode)
    }

    pub fn validate(&self, engine: &Engine) -> Result<(), Reason> {
        self.permission.check(Instant::now())?;
        engine.remote_radio_idle()?;
        engine.remote_radio_link(self.connection)?;
        if engine.source_kind != crate::dto::SourceKind::Native
            || engine.settings != self.original
            || engine.rig_mode_effective() != self.original_mode
            || !engine
                .preview_radio_selection(self.settings().active_radio)
                .is_some_and(|selection| selection.settings() == self.settings())
        {
            return Err(Reason::ContextChanged);
        }
        Ok(())
    }

    pub fn refuse(&self, reason: Reason) {
        self.completion.refuse(reason);
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        // An abandoned request must not remain apparently queued. Completion
        // preserves attempted-write uncertainty and never overwrites a terminal result.
        self.completion.refuse(Reason::HardwareUnconfirmed);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_control::Revocation;
    use std::time::Duration;
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

    fn station() -> (Engine, u32, u64) {
        let mut engine = Engine::with_settings(Settings::default());
        let incoming = engine.add_radio();
        engine.set_active_radio(0);
        engine.set_tx_enabled(false);
        engine.take_immediate_retune();
        engine.configure_remote_settings_store(std::env::temp_dir().join(format!(
            "nexus-selection-admission-{}-{}.json",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        )));
        let connection = engine.remote_open_radio().unwrap();
        let read = engine
            .remote_radio_read(&connection, Instant::now())
            .unwrap();
        engine.remote_observe_cat(Some(&read), Some(true));
        engine.remote_observe_dial(Some(&read), Some(engine.settings.dial_hz()));
        let mode = engine.rig_mode_effective();
        engine.remote_observe_mode(Some(&read), Some(&mode));
        engine.remote_observe_ptt(Some(&read), Some(false));
        let generation = engine
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation;
        (engine, incoming, generation)
    }

    fn permit(authority: &Revocation) -> Permit {
        authority
            .permit(Instant::now() + Duration::from_secs(5))
            .unwrap()
    }

    #[test]
    fn selection_admission_is_passive_and_configured_disabled_radios_remain_selectable() {
        for enabled in [false, true] {
            let (mut engine, incoming, connection) = station();
            engine
                .settings
                .radios
                .iter_mut()
                .find(|p| p.id == incoming)
                .unwrap()
                .enabled = enabled;
            let before = engine.settings.clone();
            let authority = Revocation::default();
            let completion = engine
                .queue_remote_radio_selection(incoming, connection, permit(&authority))
                .unwrap();
            assert_eq!(
                engine.settings, before,
                "admission cannot switch or rewrite profiles"
            );
            assert_eq!(completion.outcome(), Outcome::Pending);
            let request = engine.take_remote_radio_selection().unwrap();
            assert_eq!(request.settings().active_radio, incoming);
            assert_eq!(request.validate(&engine), Ok(()));
            assert!(engine.take_remote_radio_selection().is_none());
            drop(request);
            assert!(
                matches!(completion.outcome(), Outcome::Rejected { .. }),
                "abandonment ends a request instead of leaving it queued"
            );
        }
    }

    #[test]
    fn active_id_is_a_no_op_and_unknown_id_is_refused() {
        let (mut engine, _, connection) = station();
        let authority = Revocation::default();
        let before = engine.settings.clone();
        let completion = engine
            .queue_remote_radio_selection(before.active_radio, connection, permit(&authority))
            .unwrap();
        assert_eq!(
            completion.outcome(),
            Outcome::Applied {
                evidence: Evidence::StationState
            }
        );
        assert!(engine.take_remote_radio_selection().is_none());
        assert_eq!(engine.settings, before);
        assert!(matches!(
            engine.queue_remote_radio_selection(u32::MAX, connection, permit(&authority)),
            Err(Reason::InvalidAction)
        ));
        assert_eq!(engine.settings, before);
    }

    #[test]
    fn local_away_and_back_and_monitored_tune_changes_invalidate_selection() {
        for monitor_change in [false, true] {
            let (mut engine, incoming, connection) = station();
            let authority = Revocation::default();
            let _completion = engine
                .queue_remote_radio_selection(incoming, connection, permit(&authority))
                .unwrap();
            let request = engine.take_remote_radio_selection().unwrap();
            assert_eq!(request.validate(&engine), Ok(()));
            if monitor_change {
                engine.observe_radio_freq(incoming, 7_142_000);
            } else {
                let before = engine.settings.clone();
                engine.set_radio_pegged(!before.radio_pegged);
                engine.set_radio_pegged(before.radio_pegged);
                assert_eq!(
                    engine.settings, before,
                    "same values do not restore old authority"
                );
            }
            assert_eq!(request.validate(&engine), Err(Reason::ContextChanged));
        }
    }

    #[test]
    fn selection_and_tuning_share_one_pending_action_and_revocation_stays_terminal() {
        let (mut engine, incoming, connection) = station();
        let authority = Revocation::default();
        let completion = engine
            .queue_remote_radio_selection(incoming, connection, permit(&authority))
            .unwrap();
        assert!(matches!(
            engine.queue_remote_frequency(14.075, "20m", "USB", connection, permit(&authority)),
            Err(Reason::StationBusy)
        ));
        let request = engine.take_remote_radio_selection().unwrap();
        authority.revoke();
        assert_eq!(request.validate(&engine), Err(Reason::AuthorityExpired));
        assert!(matches!(
            completion.outcome(),
            Outcome::Rejected {
                reason: Reason::AuthorityExpired
            }
        ));
        let fresh = engine
            .queue_remote_radio_selection(incoming, connection, permit(&authority))
            .unwrap();
        assert_eq!(fresh.outcome(), Outcome::Pending);
        assert_eq!(
            request.validate(&engine),
            Err(Reason::AuthorityExpired),
            "new authority never revives the old command"
        );
        drop(engine.take_remote_radio_selection());
        let tuning = engine
            .queue_remote_frequency(14.075, "20m", "USB", connection, permit(&authority))
            .unwrap();
        assert_eq!(tuning.outcome(), Outcome::Pending);
        assert!(matches!(
            engine.queue_remote_radio_selection(incoming, connection, permit(&authority)),
            Err(Reason::StationBusy)
        ));
        engine
            .take_remote_radio()
            .unwrap()
            .refuse(Reason::ContextChanged);
    }

    #[test]
    fn stale_connection_and_expired_authority_cannot_enter_the_selection_queue() {
        let (mut engine, incoming, connection) = station();
        let authority = Revocation::default();
        let before = engine.settings.clone();
        assert!(matches!(
            engine.queue_remote_radio_selection(incoming, connection + 1, permit(&authority)),
            Err(Reason::ContextChanged)
        ));
        assert!(engine.take_remote_radio_selection().is_none());
        let expired = authority
            .permit(Instant::now() - Duration::from_millis(1))
            .unwrap();
        assert!(matches!(
            engine.queue_remote_radio_selection(incoming, connection, expired),
            Err(Reason::AuthorityExpired)
        ));
        assert!(engine.take_remote_radio_selection().is_none());
        assert_eq!(engine.settings, before);
    }
}
