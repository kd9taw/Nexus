//! One frequency intent owned by the existing radio worker. Preparation never
//! changes settings; only fresh, matching hardware readings permit the native
//! QSY and its atomic settings save. A failed/expired intent is never replayed.
use super::Engine;
use crate::remote_control::{Completion, Evidence, Outcome, Permit, Reason, WritePermission};
use crate::settings::OperatingMode;
use std::path::PathBuf;
use std::time::Instant;

pub struct Request {
    expected_hz: u64,
    expected_mode: String,
    target_hz: u64,
    target_mode: String,
    band: String,
    sideband: String,
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

    fn remote_frequency_idle(&self) -> Result<(), Reason> {
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
        if self
            .remote_radio_command
            .as_ref()
            .is_some_and(|r| matches!(r.completion.outcome(), Outcome::Pending))
        {
            return Err(Reason::StationBusy);
        }
        self.remote_frequency_idle()?;
        if !dial_mhz.is_finite()
            || !(0.0..=250000.0).contains(&dial_mhz)
            || crate::bandplan::band_for_dial(dial_mhz) != Some(band)
            || !matches!(sideband, "USB" | "LSB" | "FM" | "AM")
        {
            return Err(Reason::InvalidAction);
        }
        let target_hz = (dial_mhz * 1e6).round() as u64;
        let dial_mhz = target_hz as f64 / 1e6;
        if !self.settings.radio_pegged
            && self
                .settings
                .route_radio(band, self.route_mode(band, dial_mhz))
                .is_some_and(|id| id != self.settings.active_radio)
        {
            return Err(Reason::UnsupportedAction);
        }
        if target_hz == 0 || crate::bandplan::band_for_dial(target_hz as f64 / 1e6) != Some(band) {
            return Err(Reason::InvalidAction);
        }
        let target_mode =
            if self.settings.operating_mode == OperatingMode::Phone && band == self.settings.band {
                self.sideband_override
                    .clone()
                    .unwrap_or_else(|| self.settings.rig_mode_at(dial_mhz, sideband))
            } else {
                self.settings.rig_mode_at(dial_mhz, sideband)
            };
        // FM's repeater offset/tone reconciliation is a separate transaction.
        // Do not complete a QSY that leaves unguarded FM writes for the next tick.
        if matches!(target_mode.as_str(), "FM" | "PKTFM")
            || matches!(self.rig_mode_effective().as_str(), "FM" | "PKTFM")
        {
            return Err(Reason::UnsupportedAction);
        }
        let o = self.remote_monitor_observation();
        let cat = o.radio.readings.cat.ok_or(Reason::ReadingUnavailable)?;
        let completion = Completion::guarded(permit.clone());
        let request = Request {
            expected_hz: self.settings.dial_hz(),
            expected_mode: self.rig_mode_effective(),
            target_hz,
            target_mode,
            band: band.into(),
            sideband: sideband.into(),
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
    pub fn take_remote_frequency(&mut self) -> Option<Request> {
        self.remote_radio_command.take()
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
    pub fn refuse(&self, reason: Reason) {
        self.completion.refuse(reason);
    }

    pub fn validate(&self, engine: &Engine) -> Result<(), Reason> {
        self.permission.check(Instant::now())?;
        engine.remote_frequency_idle()?;
        if engine.settings.active_radio != self.radio
            || engine.settings.dial_hz() != self.expected_hz
            || engine.rig_mode_effective() != self.expected_mode
        {
            return Err(Reason::ContextChanged);
        }
        let o = engine.remote_monitor_observation();
        let cat = o.radio.readings.cat.ok_or(Reason::ReadingUnavailable)?;
        let ptt = o.radio.readings.ptt.ok_or(Reason::ReadingUnavailable)?;
        if cat.connection_generation != self.connection
            || ptt.connection_generation != self.connection
        {
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

    /// Called under the same Engine mutex used by local controls, after the
    /// worker publishes its final readback. True means canonical state changed,
    /// even if saving failed (the receipt then remains uncertain). The worker
    /// must adopt that confirmed position without scheduling another CAT write.
    pub fn commit(self, engine: &mut Engine) -> bool {
        let confirmed = (|| {
            self.validate(engine)?;
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
        engine.set_frequency(self.target_hz as f64 / 1e6, &self.band, &self.sideband);
        // This QSY has ALREADY reached the radio. Consuming its one-shot under
        // the lock cannot consume a later local gesture's retune request.
        engine.take_immediate_retune();
        let saved = engine.settings.save(
            engine
                .remote_settings_path
                .as_ref()
                .expect("validated store"),
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
