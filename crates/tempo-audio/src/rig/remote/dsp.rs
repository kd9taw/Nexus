//! One receive-only function/AGC change through the owned CAT socket. Native
//! line builders are reused; arbitrary tokens and keying functions are absent.
use super::*;

impl Rig {
    fn remote_read_receiver_dsp(
        &mut self,
        value: ReceiverDsp,
        permission: &WritePermission,
    ) -> Result<(ReceiverDsp, Option<u8>), Reason> {
        permission.check(Instant::now())?;
        let read = match value {
            ReceiverDsp::Function { func, .. } => self
                .read_func(func.token())
                .map(|on| (ReceiverDsp::Function { func, on }, None)),
            // AGC is an enum-valued level, not read_level's 0..1 fraction.
            ReceiverDsp::Agc(_) => self.read_meter_f32("AGC").and_then(|raw| {
                if !raw.is_finite() || raw.fract() != 0.0 || !(0.0..=6.0).contains(&raw) {
                    return None;
                }
                let raw = raw as u8;
                AgcSpeed::from_hamlib(raw).map(|speed| (ReceiverDsp::Agc(speed), Some(raw)))
            }),
        };
        permission.check(Instant::now())?;
        read.ok_or(Reason::ReadingUnavailable)
    }

    pub fn remote_receiver_dsp(
        &mut self,
        expected_position: Position,
        expected: ReceiverDsp,
        value: ReceiverDsp,
        permission: &WritePermission,
    ) -> Result<Readback, Reason> {
        let result = (|| {
            permission.check(Instant::now())?;
            let line = match (expected, value) {
                (
                    ReceiverDsp::Function { func, .. },
                    ReceiverDsp::Function { func: target, on },
                ) if func == target => super::super::func_line(func.token(), u8::from(on)),
                (ReceiverDsp::Agc(_), ReceiverDsp::Agc(speed)) => {
                    super::super::level_line("AGC", &speed.hamlib_value().to_string())
                }
                _ => return Err(Reason::InvalidAction),
            };
            if self.control.is_none() {
                return Err(Reason::HardwareUnavailable);
            }
            if self.keyed {
                return Err(Reason::StationBusy);
            }
            if self.remote_reported_position(permission)? != expected_position {
                return Err(Reason::ContextChanged);
            }
            if self.remote_read_receiver_dsp(expected, permission)?.0 != expected {
                return Err(Reason::ContextChanged);
            }
            self.remote_require_idle(permission)?;
            // Re-picking an AGC chip is a native explicit reassertion, even
            // when the displayed speed matches. It still writes only once.
            let reply = self
                .command_permitted(&line, None, Some(permission))
                .map_err(|_| Reason::HardwareUnconfirmed)?;
            if !super::super::reply_ok(&reply) {
                return Err(Reason::HardwareUnconfirmed);
            }
            let sampled_at = Instant::now();
            let (after_value, raw_agc) = self
                .remote_read_receiver_dsp(value, permission)
                .map_err(|_| Reason::HardwareUnconfirmed)?;
            if after_value != value
                || matches!(value, ReceiverDsp::Agc(speed) if raw_agc != Some(speed.hamlib_value()))
            {
                return Err(Reason::HardwareUnconfirmed);
            }
            let after_position = self.remote_reported_position(permission)?;
            if after_position != expected_position {
                return Err(Reason::HardwareUnconfirmed);
            }
            self.remote_require_idle(permission)?;
            Ok(Readback {
                position: after_position,
                sampled_at,
                power: None,
                passband: None,
                receiver_dsp: Some(after_value),
            })
        })();
        if let Err(reason) = result {
            permission.refuse(reason);
        }
        result
    }
}
