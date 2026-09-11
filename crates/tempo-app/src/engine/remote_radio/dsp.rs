//! Receive-only DSP choices. These closed types cannot name a keying function
//! such as VOX or TUNER, and never borrow a local pending/retry slot.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiverFunction {
    Nb,
    Nr,
    Notch,
    ManualNotch,
}

impl ReceiverFunction {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "nb" => Self::Nb,
            "nr" => Self::Nr,
            "notch" => Self::Notch,
            "manualNotch" => Self::ManualNotch,
            _ => return None,
        })
    }
    /// Same slots as Engine's observed funcs and RadioLoop's RIG_FUNCS.
    pub fn index(self) -> usize {
        match self {
            Self::Nb => 0,
            Self::Nr => 1,
            Self::Notch => 2,
            Self::ManualNotch => 5,
        }
    }
    pub fn token(self) -> &'static str {
        match self {
            Self::Nb => "NB",
            Self::Nr => "NR",
            Self::Notch => "ANF",
            Self::ManualNotch => "MN",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgcSpeed {
    Auto,
    Fast,
    Mid,
    Slow,
    Off,
}
impl AgcSpeed {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "auto" => Self::Auto,
            "fast" => Self::Fast,
            "mid" => Self::Mid,
            "slow" => Self::Slow,
            "off" => Self::Off,
            _ => return None,
        })
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Fast => "fast",
            Self::Mid => "mid",
            Self::Slow => "slow",
            Self::Off => "off",
        }
    }
    pub fn hamlib_value(self) -> u8 {
        match self {
            Self::Off => 0,
            Self::Fast => 2,
            Self::Slow => 3,
            Self::Mid => 5,
            Self::Auto => 6,
        }
    }
    /// Native display groups SUPERFAST with FAST and USER with MEDIUM.
    /// Unknown values cannot supply Remote's expected reading.
    pub fn from_hamlib(value: u8) -> Option<Self> {
        Some(match value {
            0 => Self::Off,
            1 | 2 => Self::Fast,
            3 => Self::Slow,
            4 | 5 => Self::Mid,
            6 => Self::Auto,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReceiverDsp {
    Function { func: ReceiverFunction, on: bool },
    Agc(AgcSpeed),
}

impl Engine {
    pub(super) fn validate_remote_receiver_dsp(
        &self,
        mode: OperatingMode,
        expected: ReceiverDsp,
        value: ReceiverDsp,
    ) -> Result<(), Reason> {
        if !matches!(mode, OperatingMode::Cw | OperatingMode::Phone)
            || mode != self.settings.operating_mode
        {
            return Err(Reason::ContextChanged);
        }
        if self.pending_func.iter().any(Option::is_some) || self.agc_picked {
            return Err(Reason::StationBusy);
        }
        match (expected, value) {
            (ReceiverDsp::Function { func, on }, ReceiverDsp::Function { func: target, .. })
                if func == target =>
            {
                match self.rig_funcs[func.index()] {
                    Some(actual) if actual == on => Ok(()),
                    Some(_) => Err(Reason::ContextChanged),
                    None => Err(Reason::ReadingUnavailable),
                }
            }
            (ReceiverDsp::Agc(speed), ReceiverDsp::Agc(_)) => {
                match self.rig_agc.as_deref().and_then(AgcSpeed::from_name) {
                    Some(actual) if actual == speed => Ok(()),
                    Some(_) => Err(Reason::ContextChanged),
                    None => Err(Reason::ReadingUnavailable),
                }
            }
            _ => Err(Reason::InvalidAction),
        }
    }

    pub fn queue_remote_receiver_dsp(
        &mut self,
        mode: &str,
        expected: ReceiverDsp,
        value: ReceiverDsp,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_idle()?;
        let operating_mode = match mode {
            "cw" => OperatingMode::Cw,
            "phone" => OperatingMode::Phone,
            _ => return Err(Reason::InvalidAction),
        };
        self.validate_remote_receiver_dsp(operating_mode, expected, value)?;
        let mode = self.remote_filter_mode(connection)?;
        self.queue_remote_target(
            Target {
                hz: self.settings.dial_hz(),
                mode,
                band: self.settings.band.clone(),
                sideband: self.settings.sideband.clone(),
                power_limit: None,
                intent: Intent::ReceiverDsp {
                    mode: operating_mode,
                    expected,
                    value,
                },
            },
            connection,
            permit,
        )
    }

    pub(super) fn commit_remote_receiver_dsp(&mut self, value: ReceiverDsp) {
        self.remote_actuation.revoke();
        match value {
            ReceiverDsp::Function { func, on } => self.rig_funcs[func.index()] = Some(on),
            ReceiverDsp::Agc(speed) => {
                // Adopt the confirmed desired value so ordinary reconciliation
                // does not reissue the old local pick after this transaction.
                self.agc = Some(speed.name().into());
                self.agc_picked = false;
                self.observe_rig_agc(speed.name().into());
                self.set_rig_refused_agc(None);
            }
        }
    }
}
