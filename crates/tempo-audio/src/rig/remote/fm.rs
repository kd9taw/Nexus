//! Repeater configuration inside the caller's original CAT permission.
//! rigctld r/o/c report shift, offset Hz and tone tenths of Hz respectively.
//! https://hamlib.sourceforge.net/snapshots/man1/rigctld.1.html
use super::*;

/// Native repeater choices at the existing formatter's wire precision.
/// This is neither a persisted preference nor authorization to transmit.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepeaterConfig {
    shift: &'static str,
    offset_hz: i64,
    tone_tenths: u32,
}
impl RepeaterConfig {
    pub fn new(shift: &str, offset_hz: i64, tone_hz: f32) -> Result<Self, Reason> {
        let shift = match shift.trim().to_ascii_lowercase().as_str() {
            "simplex" | "none" => "simplex",
            "plus" | "+" => "plus",
            "minus" | "-" => "minus",
            _ => return Err(Reason::InvalidAction),
        };
        let tone = (tone_hz * 10.0).round();
        if offset_hz < 0
            || !tone_hz.is_finite()
            || tone_hz < 0.0
            || !tone.is_finite()
            || tone as f64 > u32::MAX as f64
        {
            return Err(Reason::InvalidAction);
        }
        Ok(Self {
            shift,
            offset_hz,
            tone_tenths: tone as u32,
        })
    }
    pub fn shift(&self) -> &'static str {
        self.shift
    }
    pub fn offset_hz(&self) -> i64 {
        self.offset_hz
    }
    pub fn tone_hz(&self) -> f32 {
        self.tone_tenths as f32 / 10.0
    }
}

impl Rig {
    pub(super) fn remote_read_repeater(
        &mut self,
        permission: &WritePermission,
    ) -> Result<RepeaterConfig, Reason> {
        let mut query = |line: &str| {
            permission.check(Instant::now())?;
            let reply = self.command(line).map_err(|_| Reason::ReadingUnavailable)?;
            permission.check(Instant::now())?;
            // A numeric value next to an explicit failure is not confirmation.
            if reply
                .lines()
                .any(|line| line.starts_with("RPRT ") && line.trim() != "RPRT 0")
            {
                return Err(Reason::ReadingUnavailable);
            }
            Ok(reply)
        };
        let shift_reply = query("r\n")?;
        let shift = match shift_reply.lines().next().map(str::trim) {
            Some("+") => "plus",
            Some("-") => "minus",
            Some("None") => "simplex",
            _ => return Err(Reason::ReadingUnavailable),
        };
        let offset_hz = query("o\n")?
            .lines()
            .find_map(|s| s.trim().parse::<i64>().ok())
            .filter(|v| *v >= 0)
            .ok_or(Reason::ReadingUnavailable)?;
        let tone_tenths = query("c\n")?
            .lines()
            .find_map(|s| s.trim().parse::<u32>().ok())
            .ok_or(Reason::ReadingUnavailable)?;
        Ok(RepeaterConfig {
            shift,
            offset_hz,
            tone_tenths,
        })
    }

    /// Configure the existing FM choices once and read them back. Zero offset
    /// preserves the radio's offset, exactly like native set_fm_repeater.
    /// The owner must adopt its requested-value cache only after commit; the
    /// returned values are the actual radio readings, including a retained offset.
    pub fn remote_fm_repeater(
        &mut self,
        expected_position: Position,
        target: &RepeaterConfig,
        permission: &WritePermission,
    ) -> Result<Readback, Reason> {
        let result = (|| {
            permission.check(Instant::now())?;
            if !matches!(expected_position.mode(), "FM" | "PKTFM") {
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
            self.remote_require_idle(permission)?;
            let before = self.remote_read_repeater(permission)?;
            let expected = RepeaterConfig {
                offset_hz: if target.offset_hz > 0 {
                    target.offset_hz
                } else {
                    before.offset_hz
                },
                ..target.clone()
            };
            let changes = [
                (
                    before.shift != target.shift,
                    super::super::rptr_shift_line(target.shift),
                ),
                (
                    target.offset_hz > 0 && before.offset_hz != target.offset_hz,
                    super::super::rptr_offset_line(target.offset_hz),
                ),
                (
                    before.tone_tenths != target.tone_tenths,
                    format!("C {}\n", target.tone_tenths),
                ),
            ];
            for (needed, line) in changes {
                if !needed {
                    continue;
                }
                if self.remote_reported_position(permission)? != expected_position {
                    return Err(Reason::ContextChanged);
                }
                self.remote_require_idle(permission)?;
                let reply = self
                    .command_permitted(&line, None, Some(permission))
                    .map_err(|_| Reason::HardwareUnconfirmed)?;
                if !super::super::reply_ok(&reply) {
                    return Err(Reason::HardwareUnconfirmed);
                }
            }
            let sampled_at = Instant::now();
            let actual = self
                .remote_read_repeater(permission)
                .map_err(|_| Reason::HardwareUnconfirmed)?;
            if actual != expected {
                return Err(Reason::HardwareUnconfirmed);
            }
            let position = self.remote_reported_position(permission)?;
            if position != expected_position {
                return Err(Reason::HardwareUnconfirmed);
            }
            self.remote_require_idle(permission)?;
            Ok(Readback {
                position,
                sampled_at,
                power: None,
                passband: None,
                receiver_dsp: None,
                level: None,
                repeater: Some(actual),
            })
        })();
        if let Err(reason) = result {
            permission.refuse(reason);
        }
        result
    }
}
