//! Remote APRS tune: the desktop's [`Engine::aprs_tune`] (the APRS cockpit's channel pick — 2 m FM
//! simplex on a regional APRS channel) through the existing readback transaction.
//!
//! A browser may name only one of the regional APRS channels; this is the APRS pick, never a
//! general 2 m tune. The tune commits only after the owning CAT worker reads back the dial, FM and
//! the simplex, toneless configuration APRS needs. Nothing here arms, keys or queues an APRS
//! transmission, and the cockpit's entry auto-tune never reaches this path.
use super::*;

/// The regional 2 m APRS channels in Hz: the same seven the cockpit's channel picker offers.
pub(super) const APRS_CHANNELS_HZ: [u64; 7] = [
    144_390_000,
    144_800_000,
    145_175_000,
    144_575_000,
    144_660_000,
    144_930_000,
    145_570_000,
];

impl Engine {
    /// Whether the APRS context in force is exactly the one a committed remote APRS tune set.
    pub(super) fn remote_aprs_hold_matches(&self) -> bool {
        self.aprs_fm && !self.fm_channel && self.remote_aprs_hold == Some(self.settings.dial_hz())
    }

    pub fn queue_remote_aprs_tune(
        &mut self,
        dial_mhz: f64,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_retune_idle()?;
        if !dial_mhz.is_finite() {
            return Err(Reason::InvalidAction);
        }
        let hz = (dial_mhz * 1e6).round() as u64;
        if !APRS_CHANNELS_HZ.contains(&hz)
            || crate::bandplan::band_for_dial(hz as f64 / 1e6) != Some("2m")
        {
            return Err(Reason::InvalidAction);
        }
        if self.source_kind != crate::dto::SourceKind::Native {
            return Err(Reason::UnsupportedAction);
        }
        // The desktop verb hands the APRS tune to the radio routed for 2 m FM. That hand-off has no
        // remote transaction: refuse it rather than tune the active radio instead.
        if !self.settings.radio_pegged
            && self
                .settings
                .route_radio("2m", crate::settings::RouteMode::Fm)
                .is_some_and(|id| id != self.settings.active_radio)
        {
            return Err(Reason::UnsupportedAction);
        }
        if self.rig_covers_mhz(hz as f64 / 1e6) == Some(false) {
            return Err(Reason::HardwareUnavailable);
        }
        self.queue_remote_target(
            Target {
                hz,
                mode: self.fm_mode_word(),
                band: "2m".into(),
                sideband: "FM".into(),
                power_limit: None,
                intent: Intent::Aprs,
            },
            connection,
            permit,
        )
    }
}
