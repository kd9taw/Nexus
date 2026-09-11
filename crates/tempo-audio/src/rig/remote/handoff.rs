//! Configure a prepared incoming radio under one original permission. The pool
//! owner retains the connection until native selection commits; this module
//! neither switches Engine profiles nor installs a retry in the active loop.
use super::*;
use tempo_app::engine::remote_radio::RadioLevel;

/// Station-resolved settings the native loop would reapply after a handoff.
/// Unset levels stay unset: an observed value is never turned into a preference.
pub struct Handoff {
    retune: Retune,
    levels: Vec<(RadioLevel, f32)>,
    agc: Option<AgcSpeed>,
    repeater: Option<RepeaterConfig>,
}

impl Handoff {
    pub fn new(
        retune: Retune,
        levels: Vec<(RadioLevel, f32)>,
        agc: Option<AgcSpeed>,
        repeater: Option<RepeaterConfig>,
    ) -> Result<Self, Reason> {
        if levels.len() > 5
            || levels.iter().enumerate().any(|(i, (level, value))| {
                !level.valid_target(*value)
                    || levels[..i].iter().any(|(earlier, _)| earlier == level)
                    || (*level == RadioLevel::Power
                        && retune.power_limit.is_some_and(|limit| *value > limit))
            })
            || matches!(retune.target.mode(), "FM" | "PKTFM") != repeater.is_some()
        {
            return Err(Reason::InvalidAction);
        }
        Ok(Self {
            retune,
            levels,
            agc,
            repeater,
        })
    }
}

/// Final readings from the same incoming connection, after all requested
/// settings. The owner still needs current authority, native commit, persistence
/// and cache adoption; this value alone does not mean the station switched.
#[derive(Debug)]
pub struct HandoffReadback {
    radio: Readback,
    levels: Vec<(RadioLevel, f32)>,
}

impl HandoffReadback {
    pub fn radio(&self) -> &Readback {
        &self.radio
    }
    pub fn levels(&self) -> &[(RadioLevel, f32)] {
        &self.levels
    }
}

impl Rig {
    pub fn remote_handoff(
        &mut self,
        handoff: Handoff,
        permission: &WritePermission,
    ) -> Result<HandoffReadback, Reason> {
        let result = self.remote_handoff_inner(handoff, permission);
        if let Err(reason) = &result {
            permission.refuse(*reason);
        }
        result
    }

    fn remote_handoff_inner(
        &mut self,
        handoff: Handoff,
        permission: &WritePermission,
    ) -> Result<HandoffReadback, Reason> {
        let Handoff {
            retune,
            levels,
            agc,
            repeater,
        } = handoff;
        if self.remote_idle_position(permission)?.position != retune.expected {
            return Err(Reason::ContextChanged);
        }
        // Do not change mode/dial only to discover an unreadable requested
        // setting. These values are probes, not the post-mode expected values:
        // band/mode registers may legitimately recall different levels.
        for (level, _) in &levels {
            self.remote_read_level_value(*level, permission)?;
        }
        if let Some(speed) = agc {
            self.remote_read_receiver_dsp(ReceiverDsp::Agc(speed), permission)?;
        }
        if repeater.is_some() {
            self.remote_read_repeater(permission)?;
        }
        let target = retune.target.clone();
        let power_limit = retune.power_limit;
        self.remote_retune(retune, permission)?;
        // Match the active loop: FM configuration follows mode/dial, before
        // the desired levels and AGC are reapplied.
        let expected_repeater = repeater
            .as_ref()
            .map(|config| self.remote_fm_repeater(target.clone(), config, permission))
            .transpose()?
            .and_then(|read| read.repeater);
        for (level, value) in &levels {
            let before = self.remote_read_level_value(*level, permission)?;
            self.remote_level(target.clone(), *level, before, *value, permission)?;
        }
        if let Some(speed) = agc {
            let value = ReceiverDsp::Agc(speed);
            let before = self.remote_read_receiver_dsp(value, permission)?.0;
            self.remote_receiver_dsp(target.clone(), before, value, permission)?;
        }
        // Re-read the whole result: a later command or front-panel change may
        // invalidate an earlier individual success. Never certify a mixture of
        // old and new configuration or substitute desired values for readings.
        let sampled_at = Instant::now();
        let mut actual_levels = Vec::with_capacity(levels.len());
        for (level, desired) in levels {
            let actual = self.remote_read_level_value(level, permission)?;
            if !level.valid_target(actual) || !level.same_display_value(actual, desired) {
                return Err(Reason::HardwareUnconfirmed);
            }
            actual_levels.push((level, actual));
        }
        let actual_agc = if let Some(speed) = agc {
            let expected = ReceiverDsp::Agc(speed);
            let (actual, raw) = self.remote_read_receiver_dsp(expected, permission)?;
            if actual != expected || raw != Some(speed.hamlib_value()) {
                return Err(Reason::HardwareUnconfirmed);
            }
            Some(actual)
        } else {
            None
        };
        let actual_repeater = if let Some(expected) = expected_repeater {
            let actual = self.remote_read_repeater(permission)?;
            if actual != expected {
                return Err(Reason::HardwareUnconfirmed);
            }
            Some(actual)
        } else {
            None
        };
        let power = if let Some(limit) = power_limit {
            let actual = self.remote_read_power(permission)?;
            if actual > limit + 0.001 {
                return Err(Reason::HardwareUnconfirmed);
            }
            Some(actual)
        } else {
            None
        };
        let position = self.remote_reported_position(permission)?;
        if position != target {
            return Err(Reason::HardwareUnconfirmed);
        }
        self.remote_require_idle(permission)?;
        Ok(HandoffReadback {
            radio: Readback {
                position,
                sampled_at,
                power,
                passband: None,
                receiver_dsp: actual_agc,
                level: None,
                repeater: actual_repeater,
            },
            levels: actual_levels,
        })
    }
}
