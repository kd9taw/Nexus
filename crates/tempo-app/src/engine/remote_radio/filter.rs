//! Receiver width uses the existing radio transaction owner without borrowing
//! the native filter's durable-in-memory retry slot or changing the dial.
use super::*;

impl Engine {
    pub(super) fn validate_remote_filter_width(
        &self,
        expected: u32,
        hz: u32,
    ) -> Result<(), Reason> {
        let range = match self.settings.operating_mode {
            OperatingMode::Cw => 50..=2000,
            OperatingMode::Phone => 300..=4000,
            _ => return Err(Reason::UnsupportedAction),
        };
        if expected == 0 || !range.contains(&hz) {
            return Err(Reason::InvalidAction);
        }
        if self.pending_passband.is_some() || self.rig_passband != Some(expected) {
            return Err(Reason::ContextChanged);
        }
        Ok(())
    }

    pub(super) fn remote_filter_mode(&self, connection: u64) -> Result<String, Reason> {
        let observation = self.remote_monitor_observation();
        let read = observation
            .radio
            .readings
            .mode
            .ok_or(Reason::ReadingUnavailable)?;
        if read.connection_generation != connection {
            return Err(Reason::ContextChanged);
        }
        if read.age_ms >= 1000 {
            return Err(Reason::ReadingUnavailable);
        }
        observation.radio.rig_mode.ok_or(Reason::ReadingUnavailable)
    }

    pub fn queue_remote_filter_width(
        &mut self,
        mode: &str,
        expected: u32,
        hz: u32,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_idle()?;
        let operating_mode = match mode {
            "cw" => OperatingMode::Cw,
            "phone" => OperatingMode::Phone,
            _ => return Err(Reason::InvalidAction),
        };
        if self.settings.operating_mode != operating_mode {
            return Err(Reason::ContextChanged);
        }
        self.validate_remote_filter_width(expected, hz)?;
        let mode = self.remote_filter_mode(connection)?;
        self.queue_remote_target(
            Target {
                hz: self.settings.dial_hz(),
                mode,
                band: self.settings.band.clone(),
                sideband: self.settings.sideband.clone(),
                power_limit: None,
                intent: Intent::FilterWidth {
                    mode: operating_mode,
                    expected,
                    hz,
                },
            },
            connection,
            permit,
        )
    }
}
