//! Admission for a station-owned radio selection. The incoming Settings are a
//! passive native projection, never a browser payload or a state to apply wholesale.
//! The radio owner must acquire, configure and adopt the actual incoming connection.
use super::radio_selection::RadioSelection;
use super::remote_radio::{AgcSpeed, RadioLevel, Workspace};
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
    mode_entry: Option<ModeEntry>,
    tier: Option<(crate::dto::Tier, crate::dto::Tier)>,
    workspace: Option<WorkspaceEntry>,
}

struct WorkspaceEntry {
    workspace: Workspace,
    original_tier: crate::dto::Tier,
    target_tier: crate::dto::Tier,
    follow_frequency: bool,
    power_limit: Option<f32>,
    prior_power: (Option<f32>, Option<f32>),
}

struct ModeEntry {
    mode: String,
    follow_frequency: bool,
    spot_call: Option<String>,
    power_limit: Option<f32>,
    prior_power: (Option<f32>, Option<f32>),
}

/// Validated native spot intent; never a profile patch or hardware command.
pub(super) struct RoutedSpot {
    pub mode: String,
    pub dial_mhz: f64,
    pub band: String,
    pub call: String,
    pub power_limit: Option<f32>,
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
    /// A mode ceiling may carry a lower incoming level; preserve that readback
    /// instead of turning the ceiling into a later requested power increase.
    pub adopt_limited_power: bool,
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
                mode_entry: None,
                tier: None,
                workspace: None,
            }),
            connection,
            permit,
        )
    }

    pub(super) fn queue_remote_routed_mode(
        &mut self,
        id: u32,
        mode: &str,
        follow_frequency: bool,
        power_limit: Option<f32>,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        if !self.remote_selection_host_ready {
            return Err(Reason::UnsupportedAction);
        }
        let entry = self.prepare_mode_entry(mode, follow_frequency);
        let (dial_mhz, sideband) = entry.frequency.ok_or(Reason::ContextChanged)?;
        self.queue_remote_selection(
            id,
            Some(RoutedTune {
                dial_mhz,
                band: self.settings.band.clone(),
                sideband,
                band_mode: None,
                mode_entry: Some(ModeEntry {
                    mode: mode.into(),
                    follow_frequency,
                    spot_call: None,
                    power_limit,
                    prior_power: (self.rf_power, self.rig_rf_power),
                }),
                tier: None,
                workspace: None,
            }),
            connection,
            permit,
        )
    }

    pub(super) fn queue_remote_routed_spot(
        &mut self,
        id: u32,
        spot: RoutedSpot,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        if !self.remote_selection_host_ready {
            return Err(Reason::UnsupportedAction);
        }
        self.queue_remote_selection(
            id,
            Some(RoutedTune {
                dial_mhz: spot.dial_mhz,
                band: spot.band,
                sideband: "USB".into(),
                band_mode: None,
                mode_entry: Some(ModeEntry {
                    mode: spot.mode,
                    follow_frequency: false,
                    spot_call: Some(spot.call),
                    power_limit: spot.power_limit,
                    prior_power: (self.rf_power, self.rig_rf_power),
                }),
                tier: None,
                workspace: None,
            }),
            connection,
            permit,
        )
    }

    pub(super) fn queue_remote_routed_tier(
        &mut self,
        id: u32,
        tier: crate::dto::Tier,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        if !self.remote_selection_host_ready {
            return Err(Reason::UnsupportedAction);
        }
        let channel = self
            .prepare_tier_frequency(tier)
            .ok_or(Reason::ContextChanged)?;
        self.queue_remote_selection(
            id,
            Some(RoutedTune {
                dial_mhz: channel.dial_mhz,
                band: channel.band,
                sideband: channel.mode,
                band_mode: None,
                mode_entry: None,
                tier: Some((tier, self.tier())),
                workspace: None,
            }),
            connection,
            permit,
        )
    }

    pub(super) fn queue_remote_routed_workspace(
        &mut self,
        workspace: Workspace,
        power_limit: Option<f32>,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        if !self.remote_selection_host_ready {
            return Err(Reason::UnsupportedAction);
        }
        let plan = self.prepare_remote_workspace(workspace);
        let settings = plan.selection.settings();
        if !plan.routed {
            return Err(Reason::UnsupportedAction);
        }
        self.queue_remote_selection(
            settings.active_radio,
            Some(RoutedTune {
                dial_mhz: settings.dial_mhz,
                band: settings.band.clone(),
                sideband: settings.sideband.clone(),
                band_mode: None,
                mode_entry: None,
                tier: None,
                workspace: Some(WorkspaceEntry {
                    workspace,
                    original_tier: self.tier(),
                    target_tier: plan.tier,
                    follow_frequency: plan.follow_frequency,
                    power_limit,
                    prior_power: (self.rf_power, self.rig_rf_power),
                }),
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
        if id == self.settings.active_radio && tune.is_none() {
            // Preserve the local no-op: do not reset the decoder, touch a
            // connection or re-save settings merely to select the active radio.
            completion.finish(Outcome::Applied {
                evidence: Evidence::StationState,
            });
            return Ok(completion);
        }
        let workspace = tune.as_ref().and_then(|tune| tune.workspace.as_ref());
        let mut selection = if let Some(entry) = workspace {
            self.prepare_remote_workspace(entry.workspace).selection
        } else {
            self.preview_radio_selection(id)
                .ok_or(Reason::InvalidAction)?
        };
        if let Some(tune) = &tune {
            selection = selection.with_frequency(tune.dial_mhz, &tune.band, &tune.sideband);
            if let Some(entry) = &tune.mode_entry {
                selection = selection.with_operating_mode(
                    self.prepare_mode_entry(&entry.mode, entry.follow_frequency)
                        .mode,
                );
            }
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
        if let Some(entry) = self.tune.as_ref().and_then(|tune| tune.workspace.as_ref()) {
            if !engine.remote_selection_host_ready
                || engine.tier() != entry.original_tier
                || (engine.rf_power, engine.rig_rf_power) != entry.prior_power
            {
                return None;
            }
            let plan = engine.prepare_remote_workspace(entry.workspace);
            return (plan.routed
                && plan.tier == entry.target_tier
                && plan.follow_frequency == entry.follow_frequency
                && plan.selection.settings().active_radio == self.settings().active_radio)
                .then_some(plan.selection);
        }
        let mut selection = engine.preview_radio_selection(self.settings().active_radio)?;
        if let Some(tune) = &self.tune {
            if !engine.remote_selection_host_ready {
                return None;
            }
            let routed = if let Some(entry) = &tune.mode_entry {
                let prepared = engine.prepare_mode_entry(&entry.mode, entry.follow_frequency);
                if (entry.spot_call.is_none()
                    && (prepared.frequency != Some((tune.dial_mhz, tune.sideband.clone()))
                        || engine.settings.band != tune.band))
                    || engine.settings.radio_pegged
                    || (engine.rf_power, engine.rig_rf_power) != entry.prior_power
                {
                    return None;
                }
                selection = selection.with_operating_mode(prepared.mode);
                engine.settings.route_radio(
                    &tune.band,
                    engine.route_mode_for(&tune.band, tune.dial_mhz, prepared.mode),
                )
            } else {
                engine.remote_frequency_route(tune.dial_mhz, &tune.band)
            };
            if routed != Some(self.settings().active_radio) {
                return None;
            }
            if let Some((tier, original)) = tune.tier {
                if engine.tier() != original
                    || tier == original
                    || engine.settings.operating_mode != crate::settings::OperatingMode::Digital
                    || !engine.prepare_tier_frequency(tier).is_some_and(|channel| {
                        channel.dial_mhz == tune.dial_mhz
                            && channel.band == tune.band
                            && channel.mode == tune.sideband
                    })
                {
                    return None;
                }
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

    /// A workspace may logically visit another profile and end at its original
    /// radio. The existing owner must keep that connection, never claim itself
    /// from the monitor pool or reopen its physical CAT/audio devices.
    pub fn retains_connection(&self) -> bool {
        self.original.active_radio == self.settings().active_radio
    }

    pub fn uncertain(&self) -> bool {
        matches!(self.completion.outcome(), Outcome::Unknown { .. })
    }

    pub fn configuration(&self, engine: &Engine) -> Result<Configuration, Reason> {
        self.validate(engine)?;
        let mode_limit = self.tune.as_ref().and_then(|tune| {
            tune.mode_entry
                .as_ref()
                .and_then(|entry| entry.power_limit)
                .or_else(|| tune.workspace.as_ref().and_then(|entry| entry.power_limit))
        });
        let limit = self
            .settings()
            .rf_power_ceiling()
            .min(mode_limit.unwrap_or(1.0));
        let levels = [
            (
                RadioLevel::Power,
                engine
                    .rf_power
                    .filter(|_| mode_limit.is_none())
                    .map(|power| power.min(limit)),
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
            adopt_limited_power: mode_limit.is_some(),
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
            self.permission.check(Instant::now())
        })();
        if let Err(reason) = ready {
            self.refuse(reason);
            return false;
        }
        let source = engine.source.clone();
        let mut source_slot = if self
            .tune
            .as_ref()
            .is_some_and(|tune| tune.tier.is_some() || tune.workspace.is_some())
        {
            match source.try_lock() {
                Ok(slot) => Some(slot),
                Err(std::sync::TryLockError::Poisoned(error)) => Some(error.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) => {
                    self.refuse(Reason::StationBusy);
                    return false;
                }
            }
        } else {
            None
        };
        // Selection changes station state even when CAT setup was a no-op.
        // Never wait behind a decoder after crossing this boundary.
        if let Err(reason) = self.permission.begin_write(Instant::now()) {
            self.refuse(reason);
            return false;
        }
        let incoming = self.settings().active_radio;
        if let Some(tune) = &self.tune {
            if let Some(entry) = &tune.workspace {
                // Both synchronous callbacks use the same held modem lock.
                // RefCell coordinates Rust borrows; it never reacquires MODEM_LOCK.
                let modem = std::cell::RefCell::new(&mut decoder);
                engine.enter_remote_workspace_with_decoder(
                    entry.workspace,
                    entry.follow_frequency,
                    |engine, mutation| match mutation {
                        super::DecoderMutation::Install(source) => engine.install_source_into(
                            source_slot.as_mut().expect("workspace source lock"),
                            source,
                        ),
                        super::DecoderMutation::ResetHarq => {
                            let _source = source_slot.as_ref().expect("workspace source lock");
                            modem.borrow_mut().reset_tempo_harq_held();
                        }
                    },
                    || modem.borrow_mut().reset_held(),
                );
            } else if let Some((tier, _)) = tune.tier {
                engine.set_tier_with_installer_and_reset(
                    tier,
                    |engine, decoder| {
                        engine.install_source_into(
                            source_slot.as_mut().expect("tier source lock"),
                            decoder,
                        )
                    },
                    || decoder.reset_held(),
                );
            } else if let Some(entry) = &tune.mode_entry {
                if let Some(call) = &entry.spot_call {
                    engine.work_spot_split_with_reset(
                        &entry.mode,
                        tune.dial_mhz,
                        &tune.band,
                        None,
                        false,
                        || decoder.reset_held(),
                    );
                    engine.note_work_call(Some(call.clone()));
                } else {
                    engine.set_operating_mode_with_reset(
                        &entry.mode,
                        entry.follow_frequency,
                        false,
                        || decoder.reset_held(),
                    );
                }
            } else if let Some(mode) = &tune.band_mode {
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
    fn routed_tier_preserves_native_decoder_channel_offsets_and_profile_policy() {
        use crate::dto::Tier;
        use crate::settings::{RouteMode, RoutingRule};
        for tier in [
            Tier::Ft4,
            Tier::Ft2,
            Tier::Wspr,
            Tier::Q65,
            Tier::Msk144,
            Tier::Jt65,
            Tier::Fst4,
            Tier::Fst4w,
            Tier::TempoFast,
            Tier::TempoDeep,
            Tier::Js8,
        ] {
            let (mut engine, incoming, _) = station();
            engine.set_frequency(14.250, "20m", "USB");
            engine.take_immediate_retune();
            engine.configure_remote_selection_host(true);
            engine.settings.routing_rules = vec![RoutingRule {
                mode: Some(RouteMode::Digital),
                radio: incoming,
                ..RoutingRule::default()
            }];
            let mut native = Engine::with_settings(engine.settings.clone());
            native.settings.radio_pegged = true;
            native.set_frequency(14.250, "20m", "USB");
            native.settings.radio_pegged = false;
            native.freq_memory = engine.freq_memory.clone();
            for e in [&mut engine, &mut native] {
                e.set_rx_offset(2300.0);
                e.set_tx_offset(2400.0);
                e.set_tx_enabled(false);
                e.take_immediate_retune();
            }
            native.set_tier(tier);
            native.take_immediate_retune();
            assert_eq!(native.settings.active_radio, incoming, "{tier:?}");
            let original = engine.settings.clone();
            let source = engine.source.clone();
            let generation = refresh(&mut engine);
            let authority = Revocation::default();
            let receipt = engine
                .queue_remote_tier(tier, generation, permit(&authority))
                .unwrap();
            assert_eq!(engine.settings, original);
            assert_eq!(engine.tier(), Tier::Ft8);
            let request = engine.take_remote_radio_selection().unwrap();
            // The projection configures hardware. The native tier owner still
            // owns decoder and offset changes at commit; it is never replaced
            // wholesale by this temporary Settings value.
            assert_eq!(request.settings().active_radio, incoming);
            assert_eq!(request.settings().dial_hz(), native.settings.dial_hz());
            assert_eq!(request.settings().rig_mode(), native.settings.rig_mode());
            assert!(commit(request, &mut engine, |_| {}));
            assert!(matches!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            ));
            assert_eq!(engine.settings, native.settings, "{tier:?}");
            assert_eq!(engine.freq_memory, native.freq_memory, "{tier:?}");
            assert_eq!(engine.tier(), native.tier());
            assert_eq!(engine.source_label, native.source_label);
            assert!(std::sync::Arc::ptr_eq(&source, &engine.source));
            assert_eq!(engine.rx_offset_hz, native.rx_offset_hz);
            assert_eq!(engine.tx_offset_hz, native.tx_offset_hz);
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
    fn routed_tier_refuses_a_busy_decoder_at_admission_and_final_commit() {
        use crate::dto::Tier;
        use crate::settings::{RouteMode, RoutingRule};
        let (mut engine, incoming, generation) = station();
        engine.configure_remote_selection_host(true);
        engine.settings.routing_rules = vec![RoutingRule {
            mode: Some(RouteMode::Digital),
            radio: incoming,
            ..RoutingRule::default()
        }];
        let source = engine.source.clone();
        let slot = source.lock().unwrap();
        let authority = Revocation::default();
        assert!(matches!(
            engine.queue_remote_tier(Tier::Msk144, generation, permit(&authority)),
            Err(Reason::StationBusy)
        ));
        drop(slot);
        let original = engine.settings.clone();
        let label = engine.source_label.clone();
        let receipt = engine
            .queue_remote_tier(Tier::Msk144, generation, permit(&authority))
            .unwrap();
        let request = engine.take_remote_radio_selection().unwrap();
        let slot = source.lock().unwrap();
        assert!(!commit(request, &mut engine, |_| panic!(
            "busy decoder installed"
        )));
        assert!(matches!(
            receipt.outcome(),
            Outcome::Rejected {
                reason: Reason::StationBusy
            }
        ));
        assert_eq!(engine.settings, original);
        assert_eq!(engine.source_label, label);
        assert_eq!(engine.tier(), Tier::Ft8);
        assert!(!engine.remote_settings_path.as_ref().unwrap().exists());
        drop(slot);
        // Positive control after releasing that same stable source lock.
        let receipt = engine
            .queue_remote_tier(Tier::Msk144, generation, permit(&authority))
            .unwrap();
        let request = engine.take_remote_radio_selection().unwrap();
        assert!(commit(request, &mut engine, |_| {}));
        assert!(matches!(receipt.outcome(), Outcome::Applied { .. }));
    }

    #[test]
    fn routed_spot_matches_native_exact_dial_contact_context_and_profile_memory() {
        use crate::settings::{OperatingMode, RouteMode, RoutingRule};
        for (mode, dial, band, am) in [
            ("cw", 7.031, "40m", false),
            ("phone", 7.183, "40m", false),
            ("phone", 14.267, "20m", true),
        ] {
            let (mut engine, incoming, _) = station();
            engine.settings.operating_mode = OperatingMode::Phone;
            engine.set_frequency(14.250, "20m", "USB");
            engine.take_immediate_retune();
            engine.settings.routing_rules = vec![RoutingRule {
                mode: Some(if mode == "cw" {
                    RouteMode::Cw
                } else {
                    RouteMode::Ssb
                }),
                radio: incoming,
                ..RoutingRule::default()
            }];
            engine.configure_remote_selection_host(true);
            engine.rf_power = Some(0.8);
            engine.sideband_override = am.then(|| "AM".into());
            let mut native = Engine::with_settings(engine.settings.clone());
            // The initial dial must not run the destination routing rule.
            native.settings.radio_pegged = true;
            native.set_frequency(14.250, "20m", "USB");
            native.settings.radio_pegged = false;
            native.freq_memory = engine.freq_memory.clone();
            native.rf_power = engine.rf_power;
            native.sideband_override = engine.sideband_override.clone();
            native.set_tx_enabled(false);
            native.work_spot_split_with_arming(mode, dial, band, None, false);
            native.note_work_call(Some("W1AW/P".into()));
            native.take_immediate_retune();
            let original = engine.settings.clone();
            let tick = engine.work_tick;
            let generation = refresh(&mut engine);
            let authority = Revocation::default();
            let receipt = engine
                .queue_remote_spot(mode, dial, band, "w1aw/p", generation, permit(&authority))
                .unwrap();
            assert_eq!(engine.settings, original);
            assert_eq!(engine.work_tick, tick);
            let request = engine.take_remote_radio_selection().unwrap();
            assert_eq!(request.settings(), native.settings());
            if am {
                assert_eq!(
                    request.configuration(&engine).unwrap().power_limit,
                    Some(0.25)
                );
            }
            assert!(commit(request, &mut engine, |_| {}));
            assert!(matches!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            ));
            assert_eq!(engine.settings, native.settings);
            assert_eq!(engine.freq_memory, native.freq_memory);
            assert_eq!(engine.rf_power, native.rf_power);
            assert_eq!(engine.work_view, native.work_view);
            assert_eq!(engine.work_call, native.work_call);
            assert_eq!(engine.work_tick, tick + 1);
            assert!(engine.sideband_override.is_none());
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
    fn spot_leaving_am_prepares_the_native_entry_power_on_the_same_radio() {
        let (mut engine, _, _) = station();
        engine.settings.operating_mode = crate::settings::OperatingMode::Phone;
        engine.settings.max_power_phone = Some(0.8);
        engine.rf_power = Some(0.8);
        engine.sideband_override = Some("AM".into());
        let generation = refresh(&mut engine);
        let authority = Revocation::default();
        let _receipt = engine
            .queue_remote_spot(
                "phone",
                14.267,
                "20m",
                "W1AW",
                generation,
                permit(&authority),
            )
            .unwrap();
        let request = engine.take_remote_radio().unwrap();
        assert_eq!(request.power_limit(), Some(0.25));
        assert_eq!(engine.rf_power, Some(0.8), "preparation remains passive");
    }

    #[test]
    fn routed_mode_entry_matches_native_mode_memory_profiles_and_disarming() {
        use crate::settings::{OperatingMode, RouteMode, RoutingRule};
        for (name, route, phone, dial, band) in [
            ("digital", RouteMode::Digital, "ssb", 14.250, "20m"),
            ("keyboard", RouteMode::Digital, "ssb", 14.250, "20m"),
            ("cw", RouteMode::Cw, "ssb", 14.250, "20m"),
            ("rtty", RouteMode::Rtty, "ssb", 14.250, "20m"),
            ("phone", RouteMode::Ssb, "ssb", 14.074, "20m"),
            ("phone", RouteMode::Fm, "fm", 145.550, "2m"),
        ] {
            let (mut engine, incoming, _) = station();
            engine.settings.operating_mode = if name == "phone" {
                OperatingMode::Digital
            } else {
                OperatingMode::Phone
            };
            engine.settings.phone_mode = phone.into();
            engine.set_frequency(dial, band, "USB");
            engine.take_immediate_retune();
            engine.settings.routing_rules = vec![RoutingRule {
                mode: Some(route),
                radio: incoming,
                ..RoutingRule::default()
            }];
            engine.configure_remote_selection_host(true);
            let mut native = Engine::with_settings(engine.settings.clone());
            // A settings load intentionally has no operator dial residency.
            // Give the native oracle the same explicit dial gesture as Remote.
            native.set_frequency(dial, band, "USB");
            native.freq_memory = engine.freq_memory.clone();
            native.set_tx_enabled(false);
            native.set_operating_mode_with_arming(name, true, false);
            native.take_immediate_retune();
            assert_eq!(native.settings.active_radio, incoming, "{name} {phone}");
            let generation = refresh(&mut engine);
            let original = engine.settings.clone();
            let memory = engine.freq_memory.clone();
            let authority = Revocation::default();
            let receipt = engine
                .queue_remote_mode(name, true, generation, permit(&authority))
                .unwrap();
            assert_eq!(engine.settings, original);
            assert_eq!(engine.freq_memory, memory);
            assert!(engine.take_remote_radio().is_none());
            let request = engine.take_remote_radio_selection().unwrap();
            assert_eq!(request.settings(), native.settings(), "{name} {phone}");
            assert!(commit(request, &mut engine, |_| {}));
            assert!(matches!(
                receipt.outcome(),
                Outcome::Applied {
                    evidence: Evidence::RadioReadback
                }
            ));
            assert_eq!(engine.settings, native.settings, "{name} {phone}");
            assert_eq!(engine.freq_memory, native.freq_memory, "{name} {phone}");
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
    fn routed_mode_rechecks_recall_power_host_and_authority_before_commit() {
        use crate::settings::{RouteMode, RoutingRule};
        for change in ["host", "power", "authority", "peg", "local_qsy"] {
            let (mut engine, incoming, _) = station();
            engine.settings.routing_rules = vec![RoutingRule {
                mode: Some(RouteMode::Cw),
                radio: incoming,
                ..RoutingRule::default()
            }];
            engine.configure_remote_selection_host(true);
            let generation = refresh(&mut engine);
            let authority = Revocation::default();
            let receipt = engine
                .queue_remote_mode("cw", true, generation, permit(&authority))
                .unwrap();
            let request = engine.take_remote_radio_selection().unwrap();
            match change {
                "host" => engine.configure_remote_selection_host(false),
                "power" => engine.observe_rig_power(0.2),
                "authority" => authority.revoke(),
                "peg" => engine.settings.radio_pegged = true,
                "local_qsy" => {
                    engine.set_frequency(14.040, "20m", "USB");
                    engine.take_immediate_retune();
                }
                _ => unreachable!(),
            }
            let original = engine.settings.clone();
            assert!(!commit(request, &mut engine, |_| panic!(
                "stale mode installed"
            )));
            assert_eq!(engine.settings, original);
            assert!(matches!(receipt.outcome(), Outcome::Rejected { .. }));
            assert!(!engine.remote_settings_path.as_ref().unwrap().exists());
        }
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
    fn routed_frequency_leaves_temporary_am_mode_under_native_handoff_policy() {
        let (mut engine, incoming, _) = station();
        engine.settings.operating_mode = crate::settings::OperatingMode::Phone;
        engine.settings.phone_mode = "ssb".into();
        engine.sideband_override = Some("AM".into());
        engine.settings.max_power_phone = Some(0.8);
        engine.settings.max_power_am = Some(0.25);
        engine.rf_power = Some(0.2);
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
        assert_eq!(request.settings().rig_mode(), "USB");
        let configuration = request.configuration(&engine).unwrap();
        assert_eq!(configuration.power_limit, Some(0.8));
        assert!(configuration.levels.contains(&(RadioLevel::Power, 0.2)));
    }
}
