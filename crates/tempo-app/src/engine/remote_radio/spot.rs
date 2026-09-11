//! Receive-only CW/Phone spot entry through the existing radio transaction.
//! Mode and exact spot frequency are one intent. Digital tier changes, split
//! and radio handoffs require their own complete contracts.
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
        self.remote_radio_idle()?;
        let operating_mode = match mode {
            "cw" => OperatingMode::Cw,
            "phone" => OperatingMode::Phone,
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
        if !projected.radio_pegged
            && projected
                .route_radio(
                    band,
                    self.route_mode_for(band, projected.dial_mhz, operating_mode),
                )
                .is_some_and(|id| id != projected.active_radio)
        {
            return Err(Reason::UnsupportedAction);
        }
        let mode_target = projected.rig_mode();
        let ceiling = if mode_target == "AM" {
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
}
