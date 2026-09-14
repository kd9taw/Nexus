//! Remote FM repeater tune: the desktop's one-step [`Engine::repeater_tune`] (the Program
//! section's Tune) through the existing readback transaction.
//!
//! The repeater OFFSET moves the transmitter: the rig keys the machine's INPUT, not the output
//! the operator listens on. So a browser gets one refusal the local button does not have — the
//! input is judged against the licence with the station's own emission model before anything is
//! queued. Nothing here arms or keys; the tune commits only after the owning CAT worker reads
//! back the dial, FM mode, shift, offset and tone.
use super::*;

/// The widest shift the grammar admits (23 cm machines use 12 MHz; 20 MHz leaves headroom for
/// odd cross-band splits without admitting nonsense).
pub(super) const MAX_OFFSET_HZ: i64 = 20_000_000;

/// A CTCSS tone: 0 = none, otherwise the standard range, to a tenth of a hertz.
pub(super) fn valid_tone(tone: f32) -> bool {
    tone == 0.0
        || (tone.is_finite()
            && (60.0..=260.0).contains(&tone)
            && ((tone * 10.0).round() - tone * 10.0).abs() < 1e-3)
}

impl Engine {
    /// Whether the channel hold in force is exactly the one a committed remote repeater tune set.
    pub(super) fn remote_fm_hold_matches(&self) -> bool {
        self.fm_channel
            && !self.aprs_fm
            && self.remote_fm_hold.as_ref().is_some_and(|(hz, config)| {
                *hz == self.settings.dial_hz() && *config == self.fm_repeater_config()
            })
    }

    pub fn queue_remote_repeater(
        &mut self,
        output_mhz: f64,
        shift: &str,
        offset_hz: i64,
        tone_hz: f32,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_retune_idle()?;
        if !output_mhz.is_finite()
            || !matches!(shift, "simplex" | "plus" | "minus")
            || !(0..=MAX_OFFSET_HZ).contains(&offset_hz)
            || !valid_tone(tone_hz)
        {
            return Err(Reason::InvalidAction);
        }
        let hz = (output_mhz * 1e6).round() as u64;
        let output_mhz = hz as f64 / 1e6;
        // FM voice is band-gated at 29 MHz (`rig_mode_at`, `rig_mode_effective`): below it there
        // is no FM repeater to tune, and the rig mode would be the section's, not FM.
        let Some(band) = crate::bandplan::band_for_dial(output_mhz).filter(|_| output_mhz >= 29.0)
        else {
            return Err(Reason::InvalidAction);
        };
        if self.source_kind != crate::dto::SourceKind::Native {
            return Err(Reason::UnsupportedAction);
        }
        // The desktop verb hands an FM tune to the radio routed for it. That hand-off has no
        // remote transaction: refuse it rather than tune the active radio instead.
        if !self.settings.radio_pegged
            && self
                .settings
                .route_radio(band, crate::settings::RouteMode::Fm)
                .is_some_and(|id| id != self.settings.active_radio)
        {
            return Err(Reason::UnsupportedAction);
        }
        if self.rig_covers_mhz(output_mhz) == Some(false) {
            return Err(Reason::HardwareUnavailable);
        }
        // Project the FM plumbing the desktop verb writes, to know the offset the rig will key
        // (0 = the band convention) without touching live Settings.
        let mut projected = self.settings.clone();
        projected.dial_mhz = output_mhz;
        projected.rptr_shift = shift.into();
        projected.rptr_offset_override_hz = offset_hz;
        projected.ctcss_tone_hz = tone_hz;
        let offset = projected.rptr_offset_hz();
        let input_mhz = match shift {
            "plus" => output_mhz + offset as f64 / 1e6,
            "minus" => output_mhz - offset as f64 / 1e6,
            _ => output_mhz,
        };
        if !self.emission_allowed(OperatingMode::Phone, input_mhz, "FM") {
            return Err(Reason::OutsidePrivileges);
        }
        self.queue_remote_target(
            Target {
                hz,
                mode: self.fm_mode_word(),
                band: band.into(),
                sideband: "FM".into(),
                power_limit: None,
                intent: Intent::Repeater {
                    shift: shift.into(),
                    offset_override: offset_hz,
                    offset,
                    tone: tone_hz,
                    input_mhz,
                },
            },
            connection,
            permit,
        )
    }
}
