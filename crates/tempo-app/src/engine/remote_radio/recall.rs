//! Remote memory recall: the desktop's `recallMemory` at the station — the section, the memory's
//! exact dial, the phone mode, an FM machine's repeater plumbing and the memory's own sideband —
//! as ONE readback transaction instead of the desktop's Settings write followed by a Work.
//!
//! Two differences from the local recall, both deliberate: a browser recall never arms transmit
//! (the desktop's Work arms Phone and CW on entry), and an FM machine's INPUT is judged against the
//! licence before anything is queued, because the offset moves the transmitter. A routed recall
//! (another radio owns the band) has no transaction that carries the phone plumbing, so it is
//! refused rather than tuned onto the active radio.
use super::repeater::{valid_tone, MAX_OFFSET_HZ};
use super::*;

impl Engine {
    #[allow(clippy::too_many_arguments)]
    pub fn queue_remote_memory_recall(
        &mut self,
        section: &str,
        dial_mhz: f64,
        band: &str,
        sideband: Option<&str>,
        fm: Option<(&str, i64, f32)>,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_retune_idle()?;
        let operating_mode = match section {
            "cw" => OperatingMode::Cw,
            "phone" => OperatingMode::Phone,
            "digital" => OperatingMode::Digital,
            _ => return Err(Reason::InvalidAction),
        };
        let phone = operating_mode == OperatingMode::Phone;
        if !dial_mhz.is_finite()
            || !(0.0..=250000.0).contains(&dial_mhz)
            || band.is_empty()
            || crate::bandplan::band_for_dial(dial_mhz) != Some(band)
            // Only a Phone memory names a sideband, and never together with an FM machine.
            || sideband.is_some_and(|s| !phone || fm.is_some() || !matches!(s, "USB" | "LSB"))
            // An FM machine is Phone voice at or above 29 MHz, with a closed shift and tone.
            || fm.is_some_and(|(shift, offset, tone)| {
                !phone
                    || dial_mhz < 29.0
                    || !matches!(shift, "simplex" | "plus" | "minus")
                    || !(0..=MAX_OFFSET_HZ).contains(&offset)
                    || !valid_tone(tone)
            })
        {
            return Err(Reason::InvalidAction);
        }
        if self.source_kind != crate::dto::SourceKind::Native {
            return Err(Reason::UnsupportedAction);
        }
        // Project the desktop's Settings patch and Work entry without touching live state.
        let mut projected = self.settings.clone();
        projected.operating_mode = operating_mode;
        projected.dial_mhz = (dial_mhz * 1e6).round() / 1e6;
        projected.band = band.into();
        projected.sideband = "USB".into();
        if phone {
            projected.phone_mode = if fm.is_some() { "fm" } else { "ssb" }.into();
        }
        if let Some((shift, offset, tone)) = fm {
            projected.rptr_shift = shift.into();
            projected.rptr_offset_override_hz = offset;
            projected.ctcss_tone_hz = tone;
        }
        let mode_target = sideband
            .map(str::to_string)
            .unwrap_or_else(|| projected.rig_mode());
        let repeater = matches!(mode_target.as_str(), "FM" | "PKTFM").then(|| {
            (
                projected.rptr_shift.clone(),
                projected.rptr_offset_hz(),
                projected.ctcss_tone_hz,
            )
        });
        let input_mhz = fm.map(|(shift, ..)| {
            let offset = projected.rptr_offset_hz() as f64 / 1e6;
            match shift {
                "plus" => projected.dial_mhz + offset,
                "minus" => projected.dial_mhz - offset,
                _ => projected.dial_mhz,
            }
        });
        if input_mhz.is_some_and(|input| !self.emission_allowed(OperatingMode::Phone, input, "FM"))
        {
            return Err(Reason::OutsidePrivileges);
        }
        // The Work entry clears a temporary override only after entering the section: keep an AM
        // override's lower ceiling, exactly as the CW/Phone Work path does.
        let ceiling = if phone
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
        self.queue_remote_target(
            Target {
                hz: projected.dial_hz(),
                mode: mode_target,
                band: projected.band.clone(),
                sideband: projected.sideband.clone(),
                power_limit,
                intent: Intent::Recall {
                    section: section.into(),
                    phone_mode: phone.then(|| projected.phone_mode.clone()),
                    fm: fm.map(|(shift, offset, tone)| (shift.into(), offset, tone)),
                    sideband: sideband.map(str::to_string),
                    repeater,
                    input_mhz,
                },
            },
            connection,
            permit,
        )
    }
}
