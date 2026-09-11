//! Admission for a station-owned radio selection. The incoming Settings are a
//! passive native projection, never a browser payload or a state to apply wholesale.
//! The radio owner must acquire, configure and adopt the actual incoming connection.
use super::radio_selection::RadioSelection;
use super::remote_radio::{AgcSpeed, RadioLevel};
use super::Engine;
use crate::remote_control::{Completion, Evidence, Outcome, Permit, Reason, WritePermission};
use crate::settings::Settings;
pub use modes::Ft8A7ResetGuard;
use std::time::Instant;

pub struct Request {
    original: Settings,
    original_mode: String,
    connection: u64,
    selection: RadioSelection,
    tune: Option<RoutedTune>,
    permission: WritePermission,
    completion: Completion,
}

struct RoutedTune {
    dial_mhz: f64,
    band: String,
    sideband: String,
    band_mode: Option<String>,
}

/// Final position read by the incoming connection owner, never a browser DTO.
/// The owner must also verify the complete requested CAT configuration and the
/// outgoing radio's position/idle state before entering the Engine commit.
pub struct Readback<'a> {
    pub radio: u32,
    pub dial_hz: u64,
    pub mode: &'a str,
    pub sampled_at: Instant,
}

/// Desired controls the native loop would reapply on the selected radio.
/// Preparation does not consume a pending AGC pick or adopt observed levels
/// as operator preferences. The worker retains these targets separately from
/// rounded hardware readback for its reconciliation caches.
#[derive(Debug, PartialEq)]
pub struct Configuration {
    pub levels: Vec<(RadioLevel, f32)>,
    pub agc: Option<AgcSpeed>,
    pub power_limit: Option<f32>,
}

impl Engine {
    /// Host wiring, not a saved operator setting or a browser permission.
    pub fn configure_remote_selection_host(&mut self, ready: bool) {
        self.remote_selection_host_ready = ready;
    }

    pub fn remote_selection_host_ready(&self) -> bool {
        self.remote_selection_host_ready
    }

    /// A configured id, not an address, port, profile patch or arbitrary command.
    /// No radio/profile/decoder mutation occurs at admission. The caller still
    /// needs a native worker completion before reporting a successful selection.
    pub fn queue_remote_radio_selection(
        &mut self,
        id: u32,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.queue_remote_selection(id, None, connection, permit)
    }

    pub(super) fn queue_remote_routed_frequency(
        &mut self,
        id: u32,
        dial_mhz: f64,
        band: &str,
        sideband: &str,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        if !self.remote_selection_host_ready {
            return Err(Reason::UnsupportedAction);
        }
        self.queue_remote_selection(
            id,
            Some(RoutedTune {
                dial_mhz,
                band: band.into(),
                sideband: sideband.into(),
                band_mode: None,
            }),
            connection,
            permit,
        )
    }

    fn queue_remote_selection(
        &mut self,
        id: u32,
        tune: Option<RoutedTune>,
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
        let mut selection = self
            .preview_radio_selection(id)
            .ok_or(Reason::InvalidAction)?;
        if let Some(tune) = &tune {
            selection = selection.with_frequency(tune.dial_mhz, &tune.band, &tune.sideband);
        }
        let request = Request {
            original: self.settings.clone(),
            original_mode: self.rig_mode_effective(),
            connection,
            selection,
            tune,
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
    pub(super) fn bind_band_pick(&mut self, mode: &str) {
        self.tune.as_mut().expect("routed tune").band_mode = Some(mode.into());
    }

    fn preview(&self, engine: &Engine) -> Option<RadioSelection> {
        let mut selection = engine.preview_radio_selection(self.settings().active_radio)?;
        if let Some(tune) = &self.tune {
            if !engine.remote_selection_host_ready
                || engine.remote_frequency_route(tune.dial_mhz, &tune.band)
                    != Some(self.settings().active_radio)
            {
                return None;
            }
            if let Some(mode) = &tune.band_mode {
                let om = match mode.as_str() {
                    "cw" => crate::settings::OperatingMode::Cw,
                    "phone" => crate::settings::OperatingMode::Phone,
                    _ => return None,
                };
                if engine.settings.operating_mode != om
                    || engine.prepare_band_pick(&tune.band, om)
                        != Some((tune.dial_mhz, tune.sideband.clone()))
                {
                    return None;
                }
            }
            selection = selection.with_frequency(tune.dial_mhz, &tune.band, &tune.sideband);
        }
        Some(selection)
    }

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

    pub fn configuration(&self, engine: &Engine) -> Result<Configuration, Reason> {
        self.validate(engine)?;
        let limit = self.settings().rf_power_ceiling();
        let levels = [
            (
                RadioLevel::Power,
                engine.rf_power.map(|power| power.min(limit)),
            ),
            (RadioLevel::MicGain, engine.mic_gain),
            (RadioLevel::NoiseReduction, engine.nr_level),
            (RadioLevel::Compression, engine.comp_level),
            (RadioLevel::NotchFrequency, engine.notch_freq_hz),
        ]
        .into_iter()
        .filter_map(|(kind, value)| value.map(|value| (kind, value)))
        .collect();
        let agc = engine
            .agc
            .as_deref()
            .map(|speed| AgcSpeed::from_name(speed).ok_or(Reason::InvalidAction))
            .transpose()?;
        Ok(Configuration {
            levels,
            agc,
            power_limit: (limit < 1.0).then_some(limit),
        })
    }

    pub fn validate(&self, engine: &Engine) -> Result<(), Reason> {
        self.permission.check(Instant::now())?;
        engine.remote_radio_idle()?;
        engine.remote_radio_link(self.connection)?;
        if engine.pending_func.iter().any(Option::is_some)
            || engine.pending_passband.is_some()
            || engine.pending_atu_tune.is_some()
            || engine.pending_scope_span.is_some()
            || engine.pending_yaesu_scope_mode.is_some()
            || engine.pending_scope_ref.is_some()
            || engine.pending_scope_fixed.is_some()
        {
            // Let the native owner finish the local radio's pending gesture.
            // Selection must neither consume it nor apply it on another rig.
            return Err(Reason::StationBusy);
        }
        if engine.source_kind != crate::dto::SourceKind::Native
            || engine.settings != self.original
            || engine.rig_mode_effective() != self.original_mode
            || !self
                .preview(engine)
                .is_some_and(|selection| selection.settings() == self.settings())
        {
            return Err(Reason::ContextChanged);
        }
        Ok(())
    }

    pub fn refuse(&self, reason: Reason) {
        self.completion.refuse(reason);
    }

    /// Commit native selection and install the already-prepared connection
    /// under the caller's Engine mutex. The installation closure must contain
    /// no hardware I/O; it transfers the owned Rig and confirmed owner caches.
    /// Physical release and complete CAT verification must already be done.
    ///
    /// True means canonical selection changed, including when persistence
    /// failed. The caller must retain that adoption and never replay or roll it
    /// back. Host device synchronization still belongs outside the Engine lock.
    pub fn commit_with_install(
        self,
        engine: &mut Engine,
        readback: Readback<'_>,
        mut decoder: modes::Ft8A7ResetGuard,
        install: impl FnOnce(&mut Engine),
    ) -> bool {
        let ready = (|| {
            self.validate(engine)?;
            let age = Instant::now()
                .checked_duration_since(readback.sampled_at)
                .ok_or(Reason::HardwareUnconfirmed)?;
            if age >= std::time::Duration::from_secs(1)
                || readback.radio != self.settings().active_radio
                || readback.dial_hz != self.settings().dial_hz()
                || readback.mode != self.settings().rig_mode()
            {
                return Err(Reason::HardwareUnconfirmed);
            }
            // Selection itself changes station state, even if CAT setup was a
            // no-op. Abandonment after this boundary cannot report rejection.
            self.permission.begin_write(Instant::now())
        })();
        if let Err(reason) = ready {
            self.refuse(reason);
            return false;
        }
        let incoming = self.settings().active_radio;
        if let Some(tune) = &self.tune {
            if let Some(mode) = &tune.band_mode {
                engine.pick_band_with_reset(&tune.band, Some(mode), || decoder.reset_held());
            } else {
                engine.tune_dial_with_reset(
                    tune.dial_mhz,
                    &tune.band,
                    &tune.sideband,
                    super::DialOrigin::Operator,
                    || decoder.reset_held(),
                );
            }
            drop(decoder);
        } else {
            engine.set_active_radio_with_decoder_guard(incoming, decoder);
        }
        engine.forget_radio_live(incoming);
        engine.clear_rig_smeter();
        // These observations belong to the outgoing hardware. In particular,
        // its RF reading cannot force a later power write on the incoming rig.
        // Desired controls remain intact and the owner installs actual new
        // readings in the callback before ordinary reconciliation can run.
        engine.rig_rf_power = None;
        engine.rig_mic_gain = None;
        engine.rig_nr_level = None;
        engine.rig_comp_level = None;
        engine.rig_notch_freq_hz = None;
        engine.rig_agc = None;
        engine.set_rig_refused_agc(None);
        engine.clear_rig_mode();
        engine.clear_rig_funcs();
        engine.clear_rig_tuner();
        engine.clear_rig_passband();
        engine.clear_rig_tx_meters();
        // This request's CAT transaction already established the selected dial
        // and mode. Consume only its own retune while local setters are excluded.
        // Do this before installation too: unwinding may not leave a retry behind.
        engine.take_immediate_retune();
        install(engine);
        let saved = engine.settings.save(
            engine
                .remote_settings_path
                .as_ref()
                .expect("validated settings store"),
        );
        self.completion.finish(if saved.is_ok() {
            Outcome::Applied {
                evidence: Evidence::RadioReadback,
            }
        } else {
            Outcome::Unknown {
                reason: Reason::HardwareUnconfirmed,
            }
        });
        true
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

    fn decoder_guard() -> modes::Ft8A7ResetGuard {
        loop {
            if let Some(guard) = modes::Ft8A7ResetGuard::try_acquire() {
                return guard;
            }
            std::thread::yield_now();
        }
    }

    fn commit(request: Request, engine: &mut Engine, install: impl FnOnce(&mut Engine)) -> bool {
        let mode = request.settings().rig_mode();
        let readback = Readback {
            radio: request.settings().active_radio,
            dial_hz: request.settings().dial_hz(),
            mode: &mode,
            sampled_at: Instant::now(),
        };
        request.commit_with_install(engine, readback, decoder_guard(), install)
    }

    #[test]
    fn selection_commit_banks_profiles_installs_before_save_and_consumes_its_retune() {
        let (mut engine, incoming, connection) = station();
        engine.settings.audio_in = "outgoing-live-input".into();
        engine.settings.amp_follow_band = true;
        let profile = engine
            .settings
            .radios
            .iter_mut()
            .find(|p| p.id == incoming)
            .unwrap();
        profile.audio_in = "incoming-input".into();
        profile.amp_follow_band = false;
        let authority = Revocation::default();
        let completion = engine
            .queue_remote_radio_selection(incoming, connection, permit(&authority))
            .unwrap();
        let request = engine.take_remote_radio_selection().unwrap();
        let expected = request.settings().clone();
        let path = engine.remote_settings_path.clone().unwrap();
        assert!(!path.exists());
        let epoch = engine.decode_epoch;
        let mut installed = false;
        assert!(commit(request, &mut engine, |engine| {
            assert_eq!(engine.settings, expected);
            assert!(!path.exists(), "installation precedes persistence");
            assert_eq!(
                engine.decode_epoch,
                epoch.wrapping_add(1),
                "native handoff ran"
            );
            assert!(!engine.immediate_retune);
            installed = true;
        }));
        assert!(installed);
        assert_eq!(engine.decode_epoch, epoch.wrapping_add(1));
        assert_eq!(engine.settings, expected);
        let loaded = Settings::load(&path);
        assert_eq!(loaded.active_radio, incoming);
        assert_eq!(loaded.audio_in, "incoming-input");
        assert!(!loaded.amp_follow_band);
        let outgoing = loaded.radios.iter().find(|p| p.id == 0).unwrap();
        assert_eq!(outgoing.audio_in, "outgoing-live-input");
        assert!(outgoing.amp_follow_band);
        assert!(!engine.take_immediate_retune());
        assert!(!engine.tx_enabled());
        assert_eq!(
            completion.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn selection_commit_refuses_wrong_or_aged_incoming_readback_before_installation() {
        for fault in ["radio", "dial", "mode", "stale", "future"] {
            let (mut engine, incoming, connection) = station();
            let authority = Revocation::default();
            let completion = engine
                .queue_remote_radio_selection(incoming, connection, permit(&authority))
                .unwrap();
            let request = engine.take_remote_radio_selection().unwrap();
            let before = engine.settings.clone();
            let mode = request.settings().rig_mode();
            let mut readback = Readback {
                radio: incoming,
                dial_hz: request.settings().dial_hz(),
                mode: &mode,
                sampled_at: Instant::now(),
            };
            match fault {
                "radio" => readback.radio = 0,
                "dial" => readback.dial_hz += 1000,
                "mode" => readback.mode = "invalid",
                "stale" => readback.sampled_at -= Duration::from_secs(2),
                "future" => readback.sampled_at += Duration::from_secs(2),
                _ => unreachable!(),
            }
            assert!(!request.commit_with_install(
                &mut engine,
                readback,
                decoder_guard(),
                |_| panic!("bad reading must not install")
            ));
            assert_eq!(engine.settings, before);
            assert!(!engine.remote_settings_path.as_ref().unwrap().exists());
            assert_eq!(
                completion.outcome(),
                Outcome::Rejected {
                    reason: Reason::HardwareUnconfirmed
                }
            );
        }
    }

    #[test]
    fn selection_commit_checks_original_authority_and_native_context_before_installation() {
        for local in [false, true] {
            let (mut engine, incoming, connection) = station();
            let authority = Revocation::default();
            let completion = engine
                .queue_remote_radio_selection(incoming, connection, permit(&authority))
                .unwrap();
            let request = engine.take_remote_radio_selection().unwrap();
            let before = engine.settings.clone();
            if local {
                engine.set_radio_pegged(!before.radio_pegged);
                engine.set_radio_pegged(before.radio_pegged);
            } else {
                authority.revoke();
            }
            assert!(!commit(request, &mut engine, |_| panic!(
                "revoked request must not install"
            )));
            assert_eq!(engine.settings, before);
            assert!(!engine.remote_settings_path.as_ref().unwrap().exists());
            assert_eq!(
                completion.outcome(),
                Outcome::Rejected {
                    reason: if local {
                        Reason::ContextChanged
                    } else {
                        Reason::AuthorityExpired
                    }
                }
            );
        }
    }

    #[test]
    fn selection_save_failure_keeps_adopted_native_state_without_a_retune_retry() {
        let (mut engine, incoming, connection) = station();
        // A file where a parent directory is required makes persistence fail
        // without touching any real operator settings.
        let blocker = engine.remote_settings_path.clone().unwrap();
        std::fs::write(&blocker, b"fixture").unwrap();
        engine.configure_remote_settings_store(blocker.join("settings.json"));
        let authority = Revocation::default();
        let completion = engine
            .queue_remote_radio_selection(incoming, connection, permit(&authority))
            .unwrap();
        let request = engine.take_remote_radio_selection().unwrap();
        let mut installed = false;
        assert!(commit(request, &mut engine, |_| installed = true));
        assert!(installed);
        assert_eq!(engine.settings.active_radio, incoming);
        assert!(!engine.take_immediate_retune());
        assert!(!engine.tx_enabled());
        assert_eq!(
            completion.outcome(),
            Outcome::Unknown {
                reason: Reason::HardwareUnconfirmed
            }
        );
        assert_eq!(std::fs::read(&blocker).unwrap(), b"fixture");
        std::fs::remove_file(blocker).unwrap();
    }

    #[test]
    fn authority_loss_during_installation_keeps_the_actual_selection_and_reports_unknown() {
        let (mut engine, incoming, connection) = station();
        let authority = Revocation::default();
        let completion = engine
            .queue_remote_radio_selection(incoming, connection, permit(&authority))
            .unwrap();
        let request = engine.take_remote_radio_selection().unwrap();
        let path = engine.remote_settings_path.clone().unwrap();
        assert!(commit(request, &mut engine, |_| authority.revoke()));
        assert_eq!(engine.settings.active_radio, incoming);
        assert_eq!(Settings::load(&path).active_radio, incoming);
        assert!(!engine.take_immediate_retune());
        assert_eq!(
            completion.outcome(),
            Outcome::Unknown {
                reason: Reason::HardwareUnconfirmed
            }
        );
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn abandoned_installation_cannot_leave_a_pending_receipt_or_a_deferred_retune() {
        let (mut engine, incoming, connection) = station();
        let authority = Revocation::default();
        let completion = engine
            .queue_remote_radio_selection(incoming, connection, permit(&authority))
            .unwrap();
        let request = engine.take_remote_radio_selection().unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            commit(request, &mut engine, |_| panic!("installation abandoned"));
        }));
        assert!(result.is_err());
        assert_eq!(engine.settings.active_radio, incoming);
        assert!(!engine.take_immediate_retune());
        assert_eq!(
            completion.outcome(),
            Outcome::Unknown {
                reason: Reason::HardwareUnconfirmed
            }
        );
        assert!(!engine.remote_settings_path.as_ref().unwrap().exists());
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
    fn selection_leaves_pending_local_hardware_commands_with_the_original_radio() {
        let pending: [fn(&mut Engine); 7] = [
            |e| e.pending_func[0] = Some(true),
            |e| e.pending_passband = Some(1800),
            |e| e.pending_atu_tune = Some(1),
            |e| e.pending_scope_span = Some(25000),
            |e| e.pending_yaesu_scope_mode = Some(1),
            |e| e.pending_scope_ref = Some(-10),
            |e| e.pending_scope_fixed = Some(true),
        ];
        for set in pending {
            let (mut engine, incoming, connection) = station();
            let authority = Revocation::default();
            let completion = engine
                .queue_remote_radio_selection(incoming, connection, permit(&authority))
                .unwrap();
            let request = engine.take_remote_radio_selection().unwrap();
            set(&mut engine);
            assert_eq!(request.validate(&engine), Err(Reason::StationBusy));
            assert!(!commit(request, &mut engine, |_| panic!(
                "pending local command cannot transfer"
            )));
            assert_eq!(engine.settings.active_radio, 0);
            assert_eq!(
                completion.outcome(),
                Outcome::Rejected {
                    reason: Reason::StationBusy
                }
            );
            assert!(matches!(
                engine.queue_remote_radio_selection(incoming, connection, permit(&authority)),
                Err(Reason::StationBusy)
            ));
        }
    }

    #[test]
    fn selection_clears_outgoing_level_readings_but_preserves_operator_preferences() {
        let (mut engine, incoming, connection) = station();
        engine.set_rf_power(0.2);
        engine.set_mic_gain(0.3);
        engine.set_nr_level(0.4);
        engine.set_comp_level(0.5);
        engine.set_notch_freq_hz(1200.0);
        engine.set_agc("slow");
        engine.observe_rig_power(0.95);
        engine.observe_rig_mic_gain(0.9);
        engine.observe_rig_nr_level(0.9);
        engine.observe_rig_comp_level(0.9);
        engine.observe_rig_notch_freq_hz(900.0);
        engine.observe_rig_agc("fast".into());
        let authority = Revocation::default();
        let completion = engine
            .queue_remote_radio_selection(incoming, connection, permit(&authority))
            .unwrap();
        let request = engine.take_remote_radio_selection().unwrap();
        assert!(commit(request, &mut engine, |e| {
            assert_eq!(
                (
                    e.rig_rf_power,
                    e.rig_mic_gain,
                    e.rig_nr_level,
                    e.rig_comp_level,
                    e.rig_notch_freq_hz
                ),
                (None, None, None, None, None)
            );
            assert_eq!(e.rig_agc, None);
            assert_eq!(
                (
                    e.rf_power,
                    e.mic_gain,
                    e.nr_level,
                    e.comp_level,
                    e.notch_freq_hz
                ),
                (Some(0.2), Some(0.3), Some(0.4), Some(0.5), Some(1200.0))
            );
            assert_eq!(e.agc.as_deref(), Some("slow"));
            e.observe_rig_power(0.21);
        }));
        assert_eq!(engine.rig_rf_power, Some(0.21));
        assert_eq!(
            completion.outcome(),
            Outcome::Applied {
                evidence: Evidence::RadioReadback
            }
        );
    }

    #[test]
    fn selection_configuration_keeps_unset_controls_and_does_not_consume_an_agc_pick() {
        let (mut engine, incoming, connection) = station();
        let authority = Revocation::default();
        let completion = engine
            .queue_remote_radio_selection(incoming, connection, permit(&authority))
            .unwrap();
        let request = engine.take_remote_radio_selection().unwrap();
        let configuration = request.configuration(&engine).unwrap();
        assert!(configuration.levels.is_empty());
        assert_eq!(configuration.agc, None);
        drop(request);
        assert!(matches!(completion.outcome(), Outcome::Rejected { .. }));

        engine.settings.max_power_digital = Some(0.4);
        engine.set_rf_power(0.25);
        engine.set_mic_gain(0.3);
        engine.set_nr_level(0.4);
        engine.set_comp_level(0.5);
        engine.set_notch_freq_hz(1200.0);
        engine.set_agc("slow");
        let _completion = engine
            .queue_remote_radio_selection(incoming, connection, permit(&authority))
            .unwrap();
        let request = engine.take_remote_radio_selection().unwrap();
        let before = engine.settings.clone();
        let configuration = request.configuration(&engine).unwrap();
        assert_eq!(
            configuration.levels,
            vec![
                (RadioLevel::Power, 0.25),
                (RadioLevel::MicGain, 0.3),
                (RadioLevel::NoiseReduction, 0.4),
                (RadioLevel::Compression, 0.5),
                (RadioLevel::NotchFrequency, 1200.0),
            ]
        );
        assert_eq!(configuration.power_limit, Some(0.4));
        assert_eq!(configuration.agc, Some(AgcSpeed::Slow));
        assert!(engine.agc_picked);
        assert_eq!(engine.settings, before);
        assert_eq!(engine.rf_power(), Some(0.25));
        engine.set_agc("fast");
        assert_eq!(request.configuration(&engine), Err(Reason::ContextChanged));
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

    fn refresh(engine: &mut Engine) -> u64 {
        let connection = engine.remote_open_radio().unwrap();
        let read = engine
            .remote_radio_read(&connection, Instant::now())
            .unwrap();
        engine.remote_observe_cat(Some(&read), Some(true));
        engine.remote_observe_dial(Some(&read), Some(engine.settings.dial_hz()));
        engine.remote_observe_mode(Some(&read), Some(&engine.rig_mode_effective()));
        engine.remote_observe_ptt(Some(&read), Some(false));
        engine
            .remote_monitor_observation()
            .radio
            .readings
            .cat
            .unwrap()
            .connection_generation
    }

    #[test]
    fn routed_frequency_preparation_and_commit_match_the_native_qsy_for_each_mode() {
        use crate::settings::OperatingMode;
        for (mode, phone_mode) in [
            (OperatingMode::Digital, "ssb"),
            (OperatingMode::Cw, "ssb"),
            (OperatingMode::Rtty, "ssb"),
            (OperatingMode::Keyboard, "ssb"),
            (OperatingMode::Phone, "ssb"),
            (OperatingMode::Phone, "fm"),
            (OperatingMode::Phone, "am"),
        ] {
            let (mut engine, incoming, _) = station();
            engine.settings.operating_mode = mode;
            engine.settings.phone_mode = phone_mode.into();
            engine
                .settings
                .radios
                .iter_mut()
                .find(|p| p.id == incoming)
                .unwrap()
                .bands = vec!["2m".into()];
            engine.configure_remote_selection_host(true);
            let mut native = Engine::with_settings(engine.settings.clone());
            native.set_tx_enabled(false);
            for e in [&mut engine, &mut native] {
                e.set_frequency(14.250, "20m", "USB");
                e.take_immediate_retune();
            }
            native.set_frequency(145.225, "2m", "USB");
            native.take_immediate_retune();
            let generation = refresh(&mut engine);
            let original = engine.settings.clone();
            let memory = engine.freq_memory.clone();
            let authority = Revocation::default();
            let receipt = engine
                .queue_remote_frequency(145.225, "2m", "USB", generation, permit(&authority))
                .unwrap();
            assert_eq!(engine.settings, original);
            assert_eq!(engine.freq_memory, memory);
            assert!(engine.take_remote_radio().is_none());
            let request = engine.take_remote_radio_selection().unwrap();
            assert_eq!(
                request.settings(),
                native.settings(),
                "{mode:?} {phone_mode}"
            );
            assert!(commit(request, &mut engine, |_| {}));
            assert_eq!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            );
            assert_eq!(engine.settings, native.settings);
            assert_eq!(engine.freq_memory, native.freq_memory);
            assert_eq!(engine.settings.active_radio, incoming);
            assert_eq!(engine.settings.dial_mhz, 145.225);
            assert!(!engine.tx_enabled());
            assert!(!engine.immediate_retune);
            let saved: Settings = serde_json::from_slice(
                &std::fs::read(engine.remote_settings_path.as_ref().unwrap()).unwrap(),
            )
            .unwrap();
            assert_eq!(saved, engine.settings);
        }
    }

    #[test]
    fn routed_band_selection_uses_native_memory_and_outgoing_profile_banking() {
        use crate::settings::OperatingMode;
        for (mode, name, dial) in [
            (OperatingMode::Phone, "phone", 145.275),
            (OperatingMode::Cw, "cw", 144.055),
        ] {
            let (mut engine, incoming, _) = station();
            engine.settings.operating_mode = mode;
            engine.settings.license_class = crate::settings::LicenseClass::Open;
            engine
                .settings
                .radios
                .iter_mut()
                .find(|p| p.id == incoming)
                .unwrap()
                .bands = vec!["2m".into()];
            engine.configure_remote_selection_host(true);
            let mut native = Engine::with_settings(engine.settings.clone());
            native.set_tx_enabled(false);
            for e in [&mut engine, &mut native] {
                e.set_frequency(dial, "2m", "USB");
                e.set_frequency(14.250, "20m", "USB");
                e.take_immediate_retune();
            }
            native.pick_band("2m", Some(name));
            native.take_immediate_retune();
            let generation = refresh(&mut engine);
            let before = engine.settings.clone();
            let authority = Revocation::default();
            let receipt = engine
                .queue_remote_band("2m", name, generation, permit(&authority))
                .unwrap();
            assert_eq!(engine.settings, before);
            let request = engine.take_remote_radio_selection().unwrap();
            assert_eq!(request.settings().dial_mhz, dial);
            assert!(commit(request, &mut engine, |_| {}));
            assert_eq!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            );
            assert_eq!(engine.settings, native.settings);
            assert_eq!(engine.freq_memory, native.freq_memory);
            assert!(!engine.tx_enabled());
        }
    }

    #[test]
    fn routed_frequency_needs_host_support_and_rechecks_the_original_route_before_commit() {
        for changed in ["host", "route", "authority"] {
            let (mut engine, incoming, _) = station();
            engine
                .settings
                .radios
                .iter_mut()
                .find(|p| p.id == incoming)
                .unwrap()
                .bands = vec!["2m".into()];
            let authority = Revocation::default();
            let generation = refresh(&mut engine);
            assert!(matches!(
                engine.queue_remote_frequency(145.225, "2m", "USB", generation, permit(&authority)),
                Err(Reason::UnsupportedAction)
            ));
            engine.configure_remote_selection_host(true);
            let receipt = engine
                .queue_remote_frequency(145.225, "2m", "USB", generation, permit(&authority))
                .unwrap();
            let request = engine.take_remote_radio_selection().unwrap();
            match changed {
                "host" => engine.configure_remote_selection_host(false),
                "route" => engine.settings.radio_pegged = true,
                _ => authority.revoke(),
            }
            let expected = engine.settings.clone();
            assert!(!commit(request, &mut engine, |_| panic!(
                "refused selection cannot install"
            )));
            assert!(matches!(receipt.outcome(), Outcome::Rejected { .. }));
            assert_eq!(engine.settings, expected);
            assert_eq!(engine.settings.active_radio, 0);
        }
    }

    #[test]
    fn routed_am_configuration_uses_the_native_am_power_ceiling_before_any_write() {
        let (mut engine, incoming, _) = station();
        engine.settings.operating_mode = crate::settings::OperatingMode::Phone;
        engine.settings.phone_mode = "am".into();
        engine.settings.max_power_phone = Some(0.8);
        engine.settings.max_power_am = Some(0.25);
        engine.rf_power = Some(0.7);
        engine
            .settings
            .radios
            .iter_mut()
            .find(|p| p.id == incoming)
            .unwrap()
            .bands = vec!["2m".into()];
        engine.configure_remote_selection_host(true);
        let generation = refresh(&mut engine);
        let authority = Revocation::default();
        engine
            .queue_remote_frequency(145.225, "2m", "USB", generation, permit(&authority))
            .unwrap();
        let request = engine.take_remote_radio_selection().unwrap();
        assert_eq!(request.settings().rig_mode(), "AM");
        let configuration = request.configuration(&engine).unwrap();
        assert_eq!(configuration.power_limit, Some(0.25));
        assert!(configuration.levels.contains(&(RadioLevel::Power, 0.25)));
    }
}
