//! One native level write with prior-state comparison and later physical readback.
//! The name is a closed enum and every write uses the original socket permission.
use super::*;
use tempo_app::engine::remote_radio::RadioLevel;

impl Rig {
    fn remote_read_level_value(
        &mut self,
        level: RadioLevel,
        permission: &WritePermission,
    ) -> Result<f32, Reason> {
        permission.check(Instant::now())?;
        // Use the shared raw meter reader so NOTCHF stays in hertz.
        // read_level accepts only unit-range fractions.
        let value = self.read_meter_f32(level.token());
        permission.check(Instant::now())?;
        value
            .filter(|value| level.valid_reading(*value))
            .ok_or(Reason::ReadingUnavailable)
    }
    pub fn remote_level(
        &mut self,
        expected_position: Position,
        level: RadioLevel,
        expected: f32,
        value: f32,
        permission: &WritePermission,
    ) -> Result<Readback, Reason> {
        let result = (|| {
            permission.check(Instant::now())?;
            if !level.valid_reading(expected) || !level.valid_target(value) {
                return Err(Reason::InvalidAction);
            }
            if self.control.is_none() {
                return Err(Reason::HardwareUnavailable);
            }
            if self.keyed {
                return Err(Reason::StationBusy);
            }
            if self.remote_reported_position(permission)? != expected_position {
                return Err(Reason::ContextChanged);
            }
            let before = self.remote_read_level_value(level, permission)?;
            if !level.same_display_value(before, expected) {
                return Err(Reason::ContextChanged);
            }
            self.remote_require_idle(permission)?;
            // Match the existing native setters' wire precision exactly.
            let encoded = if level == RadioLevel::NotchFrequency {
                format!("{}", value.max(0.0).round() as i32)
            } else {
                format!("{:.3}", value.clamp(0.0, 1.0))
            };
            let line = super::super::level_line(level.token(), &encoded);
            let reply = self
                .command_permitted(&line, None, Some(permission))
                .map_err(|_| Reason::HardwareUnconfirmed)?;
            if !super::super::reply_ok(&reply) {
                return Err(Reason::HardwareUnconfirmed);
            }
            let sampled_at = Instant::now();
            let actual = self
                .remote_read_level_value(level, permission)
                .map_err(|_| Reason::HardwareUnconfirmed)?;
            if !level.valid_target(actual) || !level.same_display_value(actual, value) {
                return Err(Reason::HardwareUnconfirmed);
            }
            let after = self.remote_reported_position(permission)?;
            if after != expected_position {
                return Err(Reason::HardwareUnconfirmed);
            }
            self.remote_require_idle(permission)?;
            Ok(Readback {
                position: after,
                sampled_at,
                power: None,
                passband: None,
                receiver_dsp: None,
                level: Some(actual),
                repeater: None,
            })
        })();
        if let Err(reason) = result {
            permission.refuse(reason);
        }
        result
    }
}
