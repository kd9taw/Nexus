//! A filter adjustment reissues the actual received mode once, then proves the
//! width, unchanged frequency/mode and idle state through the same CAT owner.
use super::*;

impl Rig {
    pub fn remote_filter_width(
        &mut self,
        expected: Position,
        expected_width: u32,
        width: u32,
        permission: &WritePermission,
    ) -> Result<Readback, Reason> {
        let result = (|| {
            permission.check(Instant::now())?;
            let target = i32::try_from(width).map_err(|_| Reason::InvalidAction)?;
            if target <= 0 || expected_width == 0 {
                return Err(Reason::InvalidAction);
            }
            if self.control.is_none() {
                return Err(Reason::HardwareUnavailable);
            }
            if self.keyed {
                return Err(Reason::StationBusy);
            }
            if self.remote_reported_position(permission)? != expected {
                return Err(Reason::ContextChanged);
            }
            let (mode, before_width) = self.read_mode_passband();
            permission.check(Instant::now())?;
            if mode.as_deref() != Some(expected.mode()) || before_width != Some(expected_width) {
                return Err(Reason::ContextChanged);
            }
            self.remote_require_idle(permission)?;
            if width != expected_width {
                self.remote_set_mode(expected.mode(), target, permission)
                    .map_err(|_| Reason::HardwareUnconfirmed)?;
            }
            let sampled_at = Instant::now();
            let (mode, after_width) = self.read_mode_passband();
            permission.check(Instant::now())?;
            if mode.as_deref() != Some(expected.mode()) || after_width != Some(width) {
                return Err(Reason::HardwareUnconfirmed);
            }
            let after = self.remote_reported_position(permission)?;
            if after != expected {
                return Err(Reason::HardwareUnconfirmed);
            }
            self.remote_require_idle(permission)?;
            Ok(Readback {
                position: after,
                sampled_at,
                power: None,
                passband: Some(width),
            })
        })();
        if let Err(reason) = result {
            permission.refuse(reason);
        }
        result
    }
}
