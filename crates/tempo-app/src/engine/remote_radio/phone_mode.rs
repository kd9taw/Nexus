//! The existing Phone AUTO/sideband/AM picker is a transient radio choice,
//! not a Settings save or a complete FM/repeater transaction.
use super::*;

impl Engine {
    pub(super) fn remote_phone_mode_target(
        &self,
        expected: Option<&str>,
        mode: Option<&str>,
    ) -> Result<String, Reason> {
        if self.settings.operating_mode != OperatingMode::Phone
            || self.source_kind != crate::dto::SourceKind::Native
        {
            return Err(Reason::UnsupportedAction);
        }
        for value in [expected, mode].into_iter().flatten() {
            if !matches!(value, "USB" | "LSB" | "AM") {
                return Err(Reason::InvalidAction);
            }
        }
        if self.sideband_override.as_deref() != expected {
            return Err(Reason::ContextChanged);
        }
        // Same bands offered by the existing Phone picker. No new AM buttons
        // or mode policy are introduced in the browser.
        let dial = self.settings.dial_mhz;
        if mode == Some("AM") && !(dial > 0.0 && (dial < 10.0 || dial >= 28.0)) {
            return Err(Reason::InvalidAction);
        }
        // remote_radio_idle excludes APRS/FM/satellite holds and in-flight
        // SSTV. Under that contract this is the native effective-mode policy:
        // a Phone override wins, otherwise Settings owns the automatic mode.
        let target = mode
            .map(str::to_owned)
            .unwrap_or_else(|| self.settings.rig_mode());
        if !matches!(target.as_str(), "USB" | "LSB" | "AM") {
            return Err(Reason::UnsupportedAction);
        }
        Ok(target)
    }

    pub fn queue_remote_phone_mode(
        &mut self,
        expected: Option<&str>,
        mode: Option<&str>,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_idle()?;
        let target_mode = self.remote_phone_mode_target(expected, mode)?;
        let expected_cat_mode = self.remote_filter_mode(connection)?;
        if matches!(expected_cat_mode.as_str(), "FM" | "PKTFM") {
            return Err(Reason::UnsupportedAction);
        }
        let ceiling = if target_mode == "AM" {
            self.settings.rf_power_ceiling_am()
        } else {
            self.settings.rf_power_ceiling()
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
                hz: self.settings.dial_hz(),
                mode: target_mode,
                band: self.settings.band.clone(),
                sideband: self.settings.sideband.clone(),
                power_limit,
                intent: Intent::PhoneMode {
                    expected_override: expected.map(str::to_owned),
                    override_mode: mode.map(str::to_owned),
                    expected_native_mode: self.rig_mode_effective(),
                    expected_cat_mode,
                },
            },
            connection,
            permit,
        )
    }
}
