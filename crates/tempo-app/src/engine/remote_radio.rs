//! One radio intent owned by the existing radio worker. Preparation never
//! changes station state; only fresh, matching hardware readings permit the
//! native commit. Frequency/section changes also save Settings; tier selection
//! keeps native live-state semantics. A failed/expired intent is never replayed.
use super::{DecoderMutation, Engine};
use crate::dto::Tier;
use crate::remote_control::{Completion, Evidence, Outcome, Permit, Reason, WritePermission};
use crate::settings::OperatingMode;
use std::path::PathBuf;
use std::sync::TryLockError;
use std::time::Instant;

mod dsp;
mod filter;
mod level;
pub use level::RadioLevel;
mod phone_mode;
mod spot;
pub use dsp::{AgcSpeed, ReceiverDsp, ReceiverFunction};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Workspace {
    Ft,
    Tempo,
    Js8,
}

enum Intent {
    Level {
        mode: OperatingMode,
        level: RadioLevel,
        expected: f32,
        value: f32,
        expected_native: Option<f32>,
    },
    PhoneMode {
        expected_override: Option<String>,
        override_mode: Option<String>,
        expected_native_mode: String,
        expected_cat_mode: String,
    },
    Frequency,
    Spot {
        mode: String,
        call: String,
    },
    FilterWidth {
        mode: OperatingMode,
        expected: u32,
        hz: u32,
    },
    ReceiverDsp {
        mode: OperatingMode,
        expected: ReceiverDsp,
        value: ReceiverDsp,
    },
    Band {
        mode: String,
    },
    Tier(Tier),
    Workspace {
        workspace: Workspace,
        tier: Tier,
        follow_frequency: bool,
    },
    Mode {
        mode: String,
        follow_frequency: bool,
    },
}

struct Target {
    hz: u64,
    mode: String,
    band: String,
    sideband: String,
    power_limit: Option<f32>,
    intent: Intent,
}

pub struct Request {
    expected_hz: u64,
    expected_mode: String,
    target_hz: u64,
    target_mode: String,
    band: String,
    sideband: String,
    power_limit: Option<f32>,
    expected_power: Option<f32>,
    intent: Intent,
    radio: u32,
    connection: u64,
    prior_read: u64,
    permission: WritePermission,
    completion: Completion,
}

impl Engine {
    /// Installed by the native host, never supplied over Remote. Tests and
    /// embedded hosts must explicitly provide their own isolated settings file.
    pub fn configure_remote_settings_store(&mut self, path: PathBuf) {
        self.remote_actuation.revoke();
        self.remote_settings_path = Some(path);
    }

    pub(super) fn remote_radio_idle(&self) -> Result<(), Reason> {
        if self.tx_enabled() || self.tx_owner().is_some() || self.sstv_in_flight() {
            return Err(Reason::StationBusy);
        }
        if self.remote_settings_path.is_none() {
            return Err(Reason::UnsupportedAction);
        }
        // These contexts require their own transaction: do not silently clear
        // a held channel, split, pending local retune or radio-routing intent.
        if self.sat_dial_owner.is_some()
            || self.sat_mode.is_some()
            || self.aprs_fm
            || self.fm_channel
            || self.machinery_park.is_some()
            || self.split_tx_mhz.is_some()
            || self.split_dirty
            || self.observed_split.is_some_and(|s| s.0)
            || self.route_intent.is_some()
            || self.route_target.is_some()
            || self.rit_dirty
            || self.xit_dirty
            || self.vfo_dirty
            || self.immediate_retune
            || self.cat_port_hold()
        {
            return Err(Reason::StationBusy);
        }
        Ok(())
    }

    pub fn queue_remote_frequency(
        &mut self,
        dial_mhz: f64,
        band: &str,
        sideband: &str,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_idle()?;
        if !dial_mhz.is_finite()
            || !(0.0..=250000.0).contains(&dial_mhz)
            || crate::bandplan::band_for_dial(dial_mhz).unwrap_or("") != band
            || !matches!(sideband, "USB" | "LSB" | "FM" | "AM")
        {
            return Err(Reason::InvalidAction);
        }
        let target_hz = (dial_mhz * 1e6).round() as u64;
        let dial_mhz = target_hz as f64 / 1e6;
        // Match the native dial owner: an unnamed receive frequency uses the
        // bandless resolver, never catch-all band coverage or default-radio
        // fallback. A real requested handoff still needs its own transaction.
        let route_mode = self.route_mode(band, dial_mhz);
        let route = if band.is_empty() {
            self.settings.route_radio_bandless(route_mode)
        } else {
            self.settings.route_radio(band, route_mode)
        };
        if !self.settings.radio_pegged && route.is_some_and(|id| id != self.settings.active_radio) {
            return Err(Reason::UnsupportedAction);
        }
        if target_hz == 0
            || crate::bandplan::band_for_dial(target_hz as f64 / 1e6).unwrap_or("") != band
        {
            return Err(Reason::InvalidAction);
        }
        let target_mode = if self.settings.operating_mode == OperatingMode::Phone
            && !self.context_band_transition(band).0
        {
            self.sideband_override
                .clone()
                .unwrap_or_else(|| self.settings.rig_mode_at(dial_mhz, sideband))
        } else {
            self.settings.rig_mode_at(dial_mhz, sideband)
        };
        self.queue_remote_target(
            Target {
                hz: target_hz,
                mode: target_mode,
                band: band.into(),
                sideband: sideband.into(),
                power_limit: None,
                intent: Intent::Frequency,
            },
            connection,
            permit,
        )
    }

    pub fn queue_remote_band(
        &mut self,
        band: &str,
        mode: &str,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_idle()?;
        let om = match mode {
            "cw" => OperatingMode::Cw,
            "phone" => OperatingMode::Phone,
            _ => return Err(Reason::InvalidAction),
        };
        if self.settings.operating_mode != om || self.source_kind != crate::dto::SourceKind::Native
        {
            return Err(Reason::ContextChanged);
        }
        if !crate::bandplan::licensed_bands(self.settings.license_class, om)
            .iter()
            .any(|c| c.band == band)
        {
            return Err(Reason::InvalidAction);
        }
        let (dial, sideband) = self
            .prepare_band_pick(band, om)
            .ok_or(Reason::UnsupportedAction)?;
        // The existing frequency transaction owns routing, authority, mode and
        // readback. Only its final verb differs: native pick must bank/recall.
        let receipt = self.queue_remote_frequency(dial, band, &sideband, connection, permit)?;
        self.remote_radio_command
            .as_mut()
            .expect("queued frequency")
            .intent = Intent::Band { mode: mode.into() };
        Ok(receipt)
    }

    pub fn queue_remote_mode(
        &mut self,
        mode: &str,
        follow_frequency: bool,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_idle()?;
        if !matches!(mode, "digital" | "phone" | "cw" | "rtty" | "keyboard") {
            return Err(Reason::InvalidAction);
        }
        if self.source_kind != crate::dto::SourceKind::Native {
            return Err(Reason::UnsupportedAction);
        }
        let entry = self.prepare_mode_entry(mode, follow_frequency);
        // A temporary policy projection only: never apply/save this clone over
        // the live station. Commit calls the existing native verb under its lock.
        let mut projected = self.settings.clone();
        projected.operating_mode = entry.mode;
        if let Some((dial, sideband)) = entry.frequency {
            projected.dial_mhz = dial;
            projected.sideband = sideband;
        } else if entry.mode == OperatingMode::Digital
            && projected.sideband.eq_ignore_ascii_case("LSB")
        {
            projected.sideband = "USB".into();
        }
        let mode_override = (entry.mode == OperatingMode::Phone)
            .then_some(self.sideband_override.as_deref())
            .flatten();
        let target_mode = mode_override
            .map(str::to_string)
            .unwrap_or_else(|| projected.rig_mode());
        if !projected.radio_pegged
            && projected
                .route_radio(
                    &projected.band,
                    self.route_mode_for(&projected.band, projected.dial_mhz, entry.mode),
                )
                .is_some_and(|id| id != projected.active_radio)
        {
            return Err(Reason::UnsupportedAction);
        }
        let ceiling = if mode_override.is_some_and(|m| m.eq_ignore_ascii_case("AM")) {
            projected.rf_power_ceiling_am()
        } else {
            projected.rf_power_ceiling()
        };
        let power_limit = (ceiling < 1.0).then(|| {
            self.rf_power
                .or(self.rig_rf_power)
                .unwrap_or(ceiling)
                .min(ceiling)
        });
        if power_limit.is_some_and(|p| !p.is_finite() || !(0.0..=1.0).contains(&p)) {
            return Err(Reason::ReadingUnavailable);
        }
        self.queue_remote_target(
            Target {
                hz: projected.dial_hz(),
                mode: target_mode,
                band: projected.band,
                sideband: projected.sideband,
                power_limit,
                intent: Intent::Mode {
                    mode: mode.into(),
                    follow_frequency,
                },
            },
            connection,
            permit,
        )
    }

    pub fn queue_remote_tier(
        &mut self,
        tier: Tier,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_idle()?;
        if !permit.valid(Instant::now()) {
            return Err(Reason::AuthorityExpired);
        }
        if self.source_kind != crate::dto::SourceKind::Native
            || self.settings.operating_mode != OperatingMode::Digital
        {
            return Err(Reason::UnsupportedAction);
        }
        if self.tier() == tier {
            // Native same-tier selection is a complete no-op, including when
            // the operator has tuned away from the tier's default channel.
            if self
                .remote_radio_command
                .as_ref()
                .is_some_and(|r| matches!(r.completion.outcome(), Outcome::Pending))
            {
                return Err(Reason::StationBusy);
            }
            self.remote_radio_link(connection)?;
            let receipt = Completion::guarded(permit);
            receipt.finish(Outcome::Applied {
                evidence: Evidence::ReceiverState,
            });
            return Ok(receipt);
        }
        // Decline an already-running decode before any CAT write. A decode can
        // still start after this check; commit therefore uses try_lock again.
        if matches!(self.source.try_lock(), Err(TryLockError::WouldBlock)) {
            return Err(Reason::StationBusy);
        }
        let target = self.prepare_tier_frequency(tier);
        let (dial, band, sideband) = target.as_ref().map_or(
            (
                self.settings.dial_mhz,
                self.settings.band.as_str(),
                self.settings.sideband.as_str(),
            ),
            |ch| (ch.dial_mhz, ch.band.as_str(), ch.mode.as_str()),
        );
        if !self.settings.radio_pegged
            && self
                .settings
                .route_radio(band, self.route_mode(band, dial))
                .is_some_and(|id| id != self.settings.active_radio)
        {
            return Err(Reason::UnsupportedAction);
        }
        self.queue_remote_target(
            Target {
                hz: (dial * 1e6).round() as u64,
                mode: self.settings.rig_mode_at(dial, sideband),
                band: band.into(),
                sideband: sideband.into(),
                power_limit: None,
                intent: Intent::Tier(tier),
            },
            connection,
            permit,
        )
    }

    /// Explicit entry into a complete digital workspace. Opening a browser tab
    /// never calls this. Resolve the native destination before any state change.
    pub fn queue_remote_workspace(
        &mut self,
        workspace: Workspace,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_idle()?;
        if self.source_kind != crate::dto::SourceKind::Native {
            return Err(Reason::UnsupportedAction);
        }
        if matches!(self.source.try_lock(), Err(TryLockError::WouldBlock)) {
            return Err(Reason::StationBusy);
        }
        let tier = match workspace {
            Workspace::Ft => match self.area_tier("dx") {
                Tier::Js8 => Tier::Ft8, // JS8 owns its own cockpit, outside FT.
                tier => tier,
            },
            Workspace::Tempo => self.area_tier("msg"),
            Workspace::Js8 => Tier::Js8,
        };
        let mut projected = self.settings.clone();
        projected.operating_mode = OperatingMode::Digital;
        // FT and JS8 own their section's frequency. When leaving a manual
        // section, reuse native mode memory/home policy even if its digital
        // decoder was already selected. Tempo keeps its own band selection.
        let follow_frequency =
            workspace != Workspace::Tempo && self.settings.operating_mode != OperatingMode::Digital;
        if let Some((dial, sideband)) = self
            .prepare_mode_entry("digital", follow_frequency)
            .frequency
        {
            projected.dial_mhz = dial;
            projected.sideband = sideband;
        }
        // A same-tier return preserves an operator-tuned channel, like the
        // native setter. A changed tier uses its existing override/fallback rule.
        if tier != self.tier() {
            if let Some(channel) =
                self.prepare_tier_frequency_at(tier, &projected.band, projected.dial_mhz)
            {
                projected.dial_mhz = channel.dial_mhz;
                projected.band = channel.band;
                projected.sideband = channel.mode;
            }
        }
        if projected.sideband.eq_ignore_ascii_case("LSB") {
            projected.sideband = "USB".into();
        }
        if !projected.radio_pegged
            && projected
                .route_radio(
                    &projected.band,
                    self.route_mode_for(
                        &projected.band,
                        projected.dial_mhz,
                        OperatingMode::Digital,
                    ),
                )
                .is_some_and(|id| id != projected.active_radio)
        {
            return Err(Reason::UnsupportedAction);
        }
        let ceiling = projected.rf_power_ceiling();
        let power_limit = (ceiling < 1.0).then(|| {
            self.rf_power
                .or(self.rig_rf_power)
                .unwrap_or(ceiling)
                .min(ceiling)
        });
        if power_limit.is_some_and(|p| !p.is_finite() || !(0.0..=1.0).contains(&p)) {
            return Err(Reason::ReadingUnavailable);
        }
        self.queue_remote_target(
            Target {
                hz: projected.dial_hz(),
                mode: projected.rig_mode(),
                band: projected.band,
                sideband: projected.sideband,
                power_limit,
                intent: Intent::Workspace {
                    workspace,
                    tier,
                    follow_frequency,
                },
            },
            connection,
            permit,
        )
    }

    fn queue_remote_target(
        &mut self,
        target: Target,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_idle()?;
        if self
            .remote_radio_command
            .as_ref()
            .is_some_and(|r| matches!(r.completion.outcome(), Outcome::Pending))
        {
            return Err(Reason::StationBusy);
        }
        let named_band = crate::bandplan::band_for_dial(target.hz as f64 / 1e6);
        // Native FST4/FST4W/WSPR plans include LF/MF channels which the
        // arbitrary-dial band table intentionally does not resolve. A tier
        // request carries no client frequency: admit only an exact stock
        // channel in that case, without widening arbitrary tuning or TX guards.
        let native_channel = match &target.intent {
            Intent::Tier(tier) | Intent::Workspace { tier, .. } if named_band.is_none() => {
                crate::bandplan::band_plan_for(*tier).iter().any(|c| {
                    c.band == target.band
                        && (c.dial_mhz * 1e6).round() as u64 == target.hz
                        && c.mode == target.sideband
                })
            }
            _ => false,
        };
        let bandless_receive = matches!(
            &target.intent,
            Intent::Frequency
                | Intent::Level { .. }
                | Intent::FilterWidth { .. }
                | Intent::ReceiverDsp { .. }
                | Intent::PhoneMode { .. }
        ) && target.band.is_empty()
            && named_band.is_none();
        if target.hz == 0
            || (named_band != Some(target.band.as_str()) && !native_channel && !bandless_receive)
        {
            return Err(Reason::InvalidAction);
        }
        // FM's repeater offset/tone reconciliation is a separate transaction.
        // Do not complete a QSY that leaves unguarded FM writes for the next tick.
        if matches!(target.mode.as_str(), "FM" | "PKTFM")
            || matches!(self.rig_mode_effective().as_str(), "FM" | "PKTFM")
        {
            return Err(Reason::UnsupportedAction);
        }
        let o = self.remote_monitor_observation();
        let cat = o.radio.readings.cat.ok_or(Reason::ReadingUnavailable)?;
        let completion = Completion::guarded(permit.clone());
        let request = Request {
            expected_hz: self.settings.dial_hz(),
            expected_mode: match &target.intent {
                Intent::PhoneMode {
                    expected_cat_mode, ..
                } => expected_cat_mode.clone(),
                Intent::FilterWidth { .. } | Intent::ReceiverDsp { .. } | Intent::Level { .. } => {
                    target.mode.clone()
                }
                _ => self.rig_mode_effective(),
            },
            target_hz: target.hz,
            target_mode: target.mode,
            band: target.band,
            sideband: target.sideband,
            power_limit: target.power_limit,
            expected_power: self.rf_power,
            intent: target.intent,
            radio: self.settings.active_radio,
            connection,
            prior_read: [
                Some(cat),
                o.radio.readings.dial,
                o.radio.readings.mode,
                o.radio.readings.ptt,
            ]
            .into_iter()
            .flatten()
            .map(|r| r.read_sequence)
            .max()
            .unwrap_or(cat.read_sequence),
            permission: WritePermission::new(
                permit.clone(),
                self.remote_actuation_permit(permit.deadline())
                    .ok_or(Reason::ContextChanged)?,
                completion.clone(),
            ),
            completion: completion.clone(),
        };
        request.validate(self)?;
        self.remote_radio_command = Some(request);
        Ok(completion)
    }

    /// Only the active radio worker consumes this slot, before its settings
    /// reconciliation. There is no retry or transfer to a different worker.
    pub fn take_remote_radio(&mut self) -> Option<Request> {
        self.remote_radio_command.take()
    }

    pub(super) fn remote_radio_link(&self, connection: u64) -> Result<(), Reason> {
        let o = self.remote_monitor_observation();
        let cat = o.radio.readings.cat.ok_or(Reason::ReadingUnavailable)?;
        let ptt = o.radio.readings.ptt.ok_or(Reason::ReadingUnavailable)?;
        if cat.connection_generation != connection || ptt.connection_generation != connection {
            return Err(Reason::ContextChanged);
        }
        if o.radio.rig_keyed == Some(true) {
            return Err(Reason::StationBusy);
        }
        if o.radio.cat_connected != Some(true)
            || o.radio.rig_keyed != Some(false)
            || ptt.age_ms >= 1000
        {
            return Err(Reason::ReadingUnavailable);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

impl Request {
    pub fn expected(&self) -> (u64, &str) {
        (self.expected_hz, &self.expected_mode)
    }
    pub fn target(&self) -> (u64, &str) {
        (self.target_hz, &self.target_mode)
    }
    pub fn permission(&self) -> &WritePermission {
        &self.permission
    }
    pub fn power_limit(&self) -> Option<f32> {
        self.power_limit
    }
    pub fn filter_width(&self) -> Option<(u32, u32)> {
        match self.intent {
            Intent::FilterWidth { expected, hz, .. } => Some((expected, hz)),
            _ => None,
        }
    }
    pub fn level(&self) -> Option<(RadioLevel, f32, f32)> {
        match self.intent {
            Intent::Level {
                level,
                expected,
                value,
                ..
            } => Some((level, expected, value)),
            _ => None,
        }
    }
    pub fn receiver_dsp(&self) -> Option<(ReceiverDsp, ReceiverDsp)> {
        match self.intent {
            Intent::ReceiverDsp {
                expected, value, ..
            } => Some((expected, value)),
            _ => None,
        }
    }
    pub fn refuse(&self, reason: Reason) {
        self.completion.refuse(reason);
    }

    pub fn validate(&self, engine: &Engine) -> Result<(), Reason> {
        self.permission.check(Instant::now())?;
        engine.remote_radio_idle()?;
        let mode_matches = if let Intent::PhoneMode {
            expected_override,
            override_mode,
            expected_native_mode,
            ..
        } = &self.intent
        {
            // The requested override changes the physical mode. Its later
            // readback is checked at commit; keep the original native policy
            // and displayed override bound independently from that CAT value.
            engine
                .remote_phone_mode_target(expected_override.as_deref(), override_mode.as_deref())?
                == self.target_mode
                && engine.rig_mode_effective() == *expected_native_mode
        } else if let Intent::Level {
            mode,
            level,
            expected,
            value,
            expected_native,
        } = self.intent
        {
            engine.validate_remote_level(mode, level, expected, value, expected_native)?;
            engine.remote_filter_mode(self.connection)? == self.expected_mode
        } else if let Intent::ReceiverDsp {
            mode,
            expected,
            value,
        } = self.intent
        {
            engine.validate_remote_receiver_dsp(mode, expected, value)?;
            engine.remote_filter_mode(self.connection)? == self.expected_mode
        } else if let Some((expected, hz)) = self.filter_width() {
            if !matches!(self.intent, Intent::FilterWidth { mode, .. } if mode == engine.settings.operating_mode)
            {
                return Err(Reason::ContextChanged);
            }
            engine.validate_remote_filter_width(expected, hz)?;
            engine.remote_filter_mode(self.connection)? == self.expected_mode
        } else {
            engine.rig_mode_effective() == self.expected_mode
        };
        if engine.settings.active_radio != self.radio
            || engine.settings.dial_hz() != self.expected_hz
            || !mode_matches
            || (self.power_limit.is_some() && engine.rf_power != self.expected_power)
        {
            return Err(Reason::ContextChanged);
        }
        if let Intent::Band { mode } = &self.intent {
            let om = if mode == "cw" {
                OperatingMode::Cw
            } else {
                OperatingMode::Phone
            };
            let target = engine.prepare_band_pick(&self.band, om);
            if engine.settings.operating_mode != om
                || !target.is_some_and(|(dial, sideband)| {
                    (dial * 1e6).round() as u64 == self.target_hz && sideband == self.sideband
                })
            {
                return Err(Reason::ContextChanged);
            }
        }
        engine.remote_radio_link(self.connection)
    }

    /// Called under the same Engine mutex used by local controls, after the
    /// worker publishes its final readback. True means canonical state changed,
    /// even if saving failed (the receipt then remains uncertain). The worker
    /// must adopt that confirmed position without scheduling another CAT write.
    pub fn commit(self, engine: &mut Engine) -> bool {
        self.commit_readback(engine, None)
    }

    /// Power is the owning worker's later CAT readback, not a browser argument.
    /// Mode entry may lower it to the new native ceiling, never raise it.
    pub fn commit_readback(self, engine: &mut Engine, power: Option<f32>) -> bool {
        self.commit_readings(engine, power, None, None, None)
    }

    pub fn commit_filter_readback(self, engine: &mut Engine, width: Option<u32>) -> bool {
        self.commit_readings(engine, None, width, None, None)
    }

    pub fn commit_receiver_dsp_readback(
        self,
        engine: &mut Engine,
        value: Option<ReceiverDsp>,
    ) -> bool {
        self.commit_readings(engine, None, None, value, None)
    }

    pub fn commit_level_readback(self, engine: &mut Engine, value: Option<f32>) -> bool {
        self.commit_readings(engine, None, None, None, value)
    }

    fn commit_readings(
        self,
        engine: &mut Engine,
        power: Option<f32>,
        width: Option<u32>,
        dsp: Option<ReceiverDsp>,
        level_readback: Option<f32>,
    ) -> bool {
        let confirmed = (|| {
            self.validate(engine)?;
            if let Some((level, _, desired)) = self.level() {
                let actual = level_readback.ok_or(Reason::HardwareUnconfirmed)?;
                if !level.valid_target(actual)
                    || !level.same_display_value(desired, actual)
                    || (level == RadioLevel::Power
                        && actual > engine.active_power_ceiling() + 0.001)
                {
                    return Err(Reason::HardwareUnconfirmed);
                }
            }
            if self.filter_width().is_some_and(|(_, hz)| width != Some(hz)) {
                return Err(Reason::HardwareUnconfirmed);
            }
            if self
                .receiver_dsp()
                .is_some_and(|(_, value)| dsp != Some(value))
            {
                return Err(Reason::HardwareUnconfirmed);
            }
            if let Some(limit) = self.power_limit {
                if !power.is_some_and(|p| p.is_finite() && p >= 0.0 && p <= limit + 0.001) {
                    return Err(Reason::HardwareUnconfirmed);
                }
            }
            let o = engine.remote_monitor_observation();
            for read in [
                o.radio.readings.dial,
                o.radio.readings.mode,
                o.radio.readings.ptt,
            ] {
                let read = read.ok_or(Reason::HardwareUnconfirmed)?;
                if read.connection_generation != self.connection
                    || read.read_sequence <= self.prior_read
                    || read.age_ms >= 1000
                {
                    return Err(Reason::HardwareUnconfirmed);
                }
            }
            if o.radio.rig_dial_mhz.map(|m| (m * 1e6).round() as u64) != Some(self.target_hz)
                || o.radio.rig_mode.as_deref() != Some(self.target_mode.as_str())
            {
                return Err(Reason::HardwareUnconfirmed);
            }
            Ok(())
        })();
        if let Err(reason) = confirmed {
            self.refuse(reason);
            return false;
        }
        // Do not wait behind an in-flight decoder after the final permission
        // check. Keep the original serialization mutex, with native poison recovery.
        let source = engine.source.clone();
        let mut source_slot = if matches!(&self.intent, Intent::Tier(_) | Intent::Workspace { .. })
        {
            let slot = match source.try_lock() {
                Ok(slot) => slot,
                Err(TryLockError::Poisoned(error)) => error.into_inner(),
                Err(TryLockError::WouldBlock) => {
                    self.refuse(Reason::StationBusy);
                    return false;
                }
            };
            Some(slot)
        } else {
            None
        };
        if let Err(reason) = self.permission.check(Instant::now()) {
            self.refuse(reason);
            return false;
        }
        let receiver = self.filter_width().is_some()
            || self.receiver_dsp().is_some()
            || self.level().is_some();
        let persist = !matches!(
            &self.intent,
            Intent::Tier(_)
                | Intent::Level { .. }
                | Intent::FilterWidth { .. }
                | Intent::ReceiverDsp { .. }
                | Intent::PhoneMode { .. }
        );
        match self.intent {
            Intent::Level { level, value, .. } => {
                engine.commit_remote_level(level, value, level_readback.expect("validated level"))
            }
            Intent::PhoneMode { override_mode, .. } => {
                engine.request_sideband_override(override_mode.as_deref());
                if let Some(power) = power.filter(|_| self.power_limit.is_some()) {
                    engine.rf_power = Some(power);
                    engine.observe_rig_power(power);
                }
            }
            Intent::ReceiverDsp { value, .. } => engine.commit_remote_receiver_dsp(value),
            Intent::FilterWidth { hz, .. } => {
                // This is observed hardware state. Never create the native
                // pending/retry slot, a Settings mutation or a later CAT write.
                engine.remote_actuation.revoke();
                engine.observe_rig_passband(Some(hz));
            }
            Intent::Tier(tier) => engine.set_tier_with_installer(tier, |engine, decoder| {
                engine
                    .install_source_into(source_slot.as_mut().expect("tier decoder lock"), decoder);
            }),
            Intent::Frequency => {
                engine.set_frequency(self.target_hz as f64 / 1e6, &self.band, &self.sideband)
            }
            Intent::Spot { mode, call } => {
                engine.work_spot_split_with_arming(
                    &mode,
                    self.target_hz as f64 / 1e6,
                    &self.band,
                    None,
                    false,
                );
                engine.note_work_call(Some(call));
                if let Some(power) = power.filter(|_| self.power_limit.is_some()) {
                    engine.rf_power = Some(power);
                    engine.observe_rig_power(power);
                }
            }
            Intent::Band { mode } => engine.pick_band(&self.band, Some(&mode)),
            Intent::Workspace {
                workspace,
                follow_frequency,
                ..
            } => {
                engine.set_operating_mode_with_arming("digital", follow_frequency, false);
                let mut decoder = |engine: &mut Engine, mutation| match mutation {
                    DecoderMutation::Install(source) => engine.install_source_into(
                        source_slot.as_mut().expect("workspace decoder lock"),
                        source,
                    ),
                    DecoderMutation::ResetHarq => Engine::harq_reset_serialized(
                        source_slot.as_ref().expect("workspace decoder lock"),
                    ),
                };
                match workspace {
                    Workspace::Ft => {
                        engine.set_area_with_decoder("dx", &mut decoder);
                        if engine.tier() == Tier::Js8 {
                            engine.set_tier_with_installer(Tier::Ft8, |engine, source| {
                                decoder(engine, DecoderMutation::Install(source))
                            });
                        }
                    }
                    Workspace::Tempo => engine.set_area_with_decoder("msg", &mut decoder),
                    Workspace::Js8 => {
                        engine.js8_start_session();
                        engine.set_tier_with_installer(Tier::Js8, |engine, source| {
                            decoder(engine, DecoderMutation::Install(source))
                        });
                    }
                }
                if let Some(power) = power.filter(|_| self.power_limit.is_some()) {
                    engine.rf_power = Some(power);
                    engine.observe_rig_power(power);
                }
            }
            Intent::Mode {
                mode,
                follow_frequency,
            } => {
                engine.set_operating_mode_with_arming(&mode, follow_frequency, false);
                if let Some(power) = power.filter(|_| self.power_limit.is_some()) {
                    // Carry the actual lower level if the radio's mode register
                    // recalled one. Do not queue a later power increase to a cap.
                    engine.rf_power = Some(power);
                    engine.observe_rig_power(power);
                }
            }
        }
        // This QSY has ALREADY reached the radio. Consuming its one-shot under
        // the lock cannot consume a later local gesture's retune request.
        if !receiver {
            engine.take_immediate_retune();
        }
        // The native tier verb changes live tier/decoder state and does not
        // persist Settings. Frequency and section gestures retain their save.
        let saved = if persist {
            engine.settings.save(
                engine
                    .remote_settings_path
                    .as_ref()
                    .expect("validated store"),
            )
        } else {
            Ok(())
        };
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
