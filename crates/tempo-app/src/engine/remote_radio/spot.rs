//! Receive-only CW/Phone/RTTY spot entry through the existing radio transaction.
//! Mode and exact spot frequency are one intent. Digital tier changes and split
//! require their own complete contracts. Routed spots share the station-owned
//! incoming-radio transaction and native atomic Work verb.
//!
//! RTTY rides this same path rather than a second one: the desktop's own Work for an RTTY spot
//! is `work_spot("rtty", …)`, the identical verb it uses for CW and Phone, and the only thing
//! that differs is the section the projection enters. A tier never changes here, so RTTY needs
//! none of the decoder-swap machinery `queue_remote_digital_spot` carries.
use super::*;

impl Engine {
    pub fn queue_remote_spot(
        &mut self,
        mode: &str,
        dial_mhz: f64,
        band: &str,
        call: &str,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_retune_idle()?;
        let operating_mode = match mode {
            "cw" => OperatingMode::Cw,
            "phone" => OperatingMode::Phone,
            "rtty" => OperatingMode::Rtty,
            _ => return Err(Reason::InvalidAction),
        };
        if self.source_kind != crate::dto::SourceKind::Native {
            return Err(Reason::UnsupportedAction);
        }
        if !dial_mhz.is_finite()
            || !(0.0..=250000.0).contains(&dial_mhz)
            || band.is_empty()
            || crate::bandplan::band_for_dial(dial_mhz) != Some(band)
            || call.is_empty()
            || call.len() > 32
            || !call.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'/')
        {
            return Err(Reason::InvalidAction);
        }
        // Only project native mode policy. Never replace live Settings with a
        // clone: the commit uses the local verb to bank the previous context.
        // That verb clears a temporary sideband override even on the same band.
        let mut projected = self.settings.clone();
        projected.operating_mode = operating_mode;
        projected.dial_mhz = (dial_mhz * 1e6).round() / 1e6;
        projected.band = band.into();
        projected.sideband = "USB".into();
        let mode_target = projected.rig_mode();
        // Native Work enters the section before clearing a temporary override.
        // Preserve its power reduction even when the final spot mode is SSB.
        let ceiling = if operating_mode == OperatingMode::Phone
            && self
                .sideband_override
                .as_deref()
                .is_some_and(|m| m.eq_ignore_ascii_case("AM"))
        {
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
        if !projected.radio_pegged {
            if let Some(id) = projected
                .route_radio(
                    band,
                    self.route_mode_for(band, projected.dial_mhz, operating_mode),
                )
                .filter(|id| *id != projected.active_radio)
            {
                return self.queue_remote_routed_spot(
                    id,
                    super::super::remote_selection::RoutedSpot {
                        mode: mode.into(),
                        dial_mhz: projected.dial_mhz,
                        band: band.into(),
                        call: call.to_ascii_uppercase(),
                        power_limit,
                    },
                    connection,
                    permit,
                );
            }
        }
        self.queue_remote_target(
            Target {
                hz: projected.dial_hz(),
                mode: mode_target,
                band: projected.band,
                sideband: projected.sideband,
                power_limit,
                intent: Intent::Spot {
                    mode: mode.into(),
                    call: call.to_ascii_uppercase(),
                },
            },
            connection,
            permit,
        )
    }

    /// FT8/FT4 Work from a browser: the desktop's tiered Work (Digital section, tier and the
    /// spot's exact dial under one commit) without its transmit authority. `remote_radio_idle`
    /// refuses it while TX is armed or owned, and it never starts a call: `note_work_call` at
    /// commit is only the log prefill the desktop Work leaves too.
    pub fn queue_remote_digital_spot(
        &mut self,
        tier: Tier,
        dial_mhz: f64,
        band: &str,
        call: &str,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_retune_idle()?;
        if !matches!(tier, Tier::Ft8 | Tier::Ft4)
            || !dial_mhz.is_finite()
            || !(0.0..=250000.0).contains(&dial_mhz)
            || band.is_empty()
            || crate::bandplan::band_for_dial(dial_mhz) != Some(band)
            || call.is_empty()
            || call.len() > 32
            || !call.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'/')
        {
            return Err(Reason::InvalidAction);
        }
        if self.source_kind != crate::dto::SourceKind::Native {
            return Err(Reason::UnsupportedAction);
        }
        let original = self.tier();
        // A tier change swaps the decoder under the source lock at commit. Decline a running
        // decode now instead of parking the Engine behind it, as the tier transaction does.
        if tier != original && matches!(self.source.try_lock(), Err(TryLockError::WouldBlock)) {
            return Err(Reason::StationBusy);
        }
        // Only project the native policy; the commit runs the local verbs under their lock.
        let mut projected = self.settings.clone();
        projected.operating_mode = OperatingMode::Digital;
        projected.dial_mhz = (dial_mhz * 1e6).round() / 1e6;
        projected.band = band.into();
        projected.sideband = "USB".into();
        let mode_target = projected.rig_mode();
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
        if !projected.radio_pegged {
            if let Some(id) = projected
                .route_radio(
                    band,
                    self.route_mode_for(band, projected.dial_mhz, OperatingMode::Digital),
                )
                .filter(|id| *id != projected.active_radio)
            {
                // The routed Work transaction carries no decoder swap: only a spot on the
                // current tier may ride it. A tier change there has no transaction yet.
                if tier != original {
                    return Err(Reason::UnsupportedAction);
                }
                return self.queue_remote_routed_spot(
                    id,
                    super::super::remote_selection::RoutedSpot {
                        mode: "digital".into(),
                        dial_mhz: projected.dial_mhz,
                        band: band.into(),
                        call: call.to_ascii_uppercase(),
                        power_limit,
                    },
                    connection,
                    permit,
                );
            }
        }
        self.queue_remote_target(
            Target {
                hz: projected.dial_hz(),
                mode: mode_target,
                band: projected.band,
                sideband: projected.sideband,
                power_limit,
                intent: Intent::DigitalSpot {
                    tier,
                    original,
                    call: call.to_ascii_uppercase(),
                },
            },
            connection,
            permit,
        )
    }
}
