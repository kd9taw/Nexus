//! Closed native level choices, retaining the local limits and requested/observed
//! distinction. Only the radio owner can supply the later physical readback.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RadioLevel {
    Power,
    MicGain,
    NoiseReduction,
    Compression,
    NotchFrequency,
    /// AF gain — the rig's own volume control. ⚠️ ON SOME STATIONS THIS IS ALSO THE DECODER'S
    /// AUDIO. A soundcard fed from the rig's speaker or headphone jack (a SignaLink, a
    /// Rigblaster, a bare 3.5 mm lead) sits BEHIND this knob, so turning it down to silence
    /// the speaker silences FT8/RTTY/PSK too. A rig's own USB codec and a fixed-level
    /// ACC/DATA jack do not, and Nexus cannot tell which one an operator has — the capture
    /// device name is the same either way. So the control carries the warning rather than
    /// the app guessing; see the cockpit's `phone.analog.af.*` strings.
    AfGain,
    /// RF gain — RECEIVE front-end gain, not transmit power. Hamlib spells them `RF` and
    /// `RFPOWER` for exactly this reason and the two must never be transposed.
    RfGain,
    /// Squelch. The FM-relevant one, and the other decode hazard: on most rigs a closed
    /// squelch mutes the AF path the decoder listens to as well.
    Squelch,
}

impl RadioLevel {
    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "power" => Self::Power,
            "micGain" => Self::MicGain,
            "nr" => Self::NoiseReduction,
            "compression" => Self::Compression,
            "notch" => Self::NotchFrequency,
            "afGain" => Self::AfGain,
            "rfGain" => Self::RfGain,
            "squelch" => Self::Squelch,
            _ => return None,
        })
    }
    pub fn token(self) -> &'static str {
        match self {
            Self::Power => "RFPOWER",
            Self::MicGain => "MICGAIN",
            Self::NoiseReduction => "NR",
            Self::Compression => "COMP",
            Self::NotchFrequency => "NOTCHF",
            // Hamlib's own names, read off `rig.h` and a live `rigctl l ?`: `AF` is volume,
            // `RF` is RF GAIN (not TX power), `SQL` is squelch. All three are float 0..1.
            Self::AfGain => "AF",
            Self::RfGain => "RF",
            Self::Squelch => "SQL",
        }
    }
    pub fn valid_reading(self, value: f32) -> bool {
        value.is_finite()
            && match self {
                Self::NotchFrequency => value >= 0.0,
                _ => (0.0..=1.0).contains(&value),
            }
    }
    pub fn valid_target(self, value: f32) -> bool {
        self.valid_reading(value)
            && (self != Self::NotchFrequency
                || (super::super::NOTCH_MIN_HZ..=super::super::NOTCH_MAX_HZ).contains(&value))
    }
    /// The existing controls display whole percentages or integer hertz. Keep
    /// the actual readback separately; this comparison never fabricates a value.
    pub fn same_display_value(self, a: f32, b: f32) -> bool {
        let scale = if self == Self::NotchFrequency {
            1.0
        } else {
            100.0
        };
        self.valid_reading(a) && self.valid_reading(b) && (a * scale).round() == (b * scale).round()
    }
}

impl Engine {
    pub(super) fn remote_level_desired(&self, level: RadioLevel) -> Option<f32> {
        match level {
            RadioLevel::Power => self.rf_power,
            RadioLevel::MicGain => self.mic_gain,
            RadioLevel::NoiseReduction => self.nr_level,
            RadioLevel::Compression => self.comp_level,
            RadioLevel::NotchFrequency => self.notch_freq_hz,
            RadioLevel::AfGain => self.af_gain,
            RadioLevel::RfGain => self.rf_gain,
            RadioLevel::Squelch => self.squelch,
        }
    }
    fn remote_level_displayed(&self, level: RadioLevel) -> Option<f32> {
        let observed = match level {
            RadioLevel::Power => self.rig_rf_power,
            RadioLevel::MicGain => self.rig_mic_gain,
            RadioLevel::NoiseReduction => self.rig_nr_level,
            RadioLevel::Compression => self.rig_comp_level,
            RadioLevel::NotchFrequency => self.rig_notch_freq_hz,
            RadioLevel::AfGain => self.rig_af_gain,
            RadioLevel::RfGain => self.rig_rf_gain,
            RadioLevel::Squelch => self.rig_squelch,
        };
        observed.or_else(|| self.remote_level_desired(level))
    }
    pub(super) fn validate_remote_level(
        &self,
        mode: OperatingMode,
        level: RadioLevel,
        expected: f32,
        value: f32,
        expected_native: Option<f32>,
    ) -> Result<(), Reason> {
        if mode != self.settings.operating_mode
            || self.remote_level_desired(level) != expected_native
        {
            return Err(Reason::ContextChanged);
        }
        if !level.valid_reading(expected) || !level.valid_reading(value) {
            return Err(Reason::InvalidAction);
        }
        if level == RadioLevel::NotchFrequency
            && !(super::super::NOTCH_MIN_HZ..=super::super::NOTCH_MAX_HZ).contains(&value)
        {
            return Err(Reason::InvalidAction);
        }
        if level == RadioLevel::Power && value > self.active_power_ceiling() {
            return Err(Reason::ContextChanged);
        }
        match self.remote_level_displayed(level) {
            Some(actual) if actual == expected => Ok(()),
            Some(_) => Err(Reason::ContextChanged),
            None => Err(Reason::ReadingUnavailable),
        }
    }
    pub fn queue_remote_level(
        &mut self,
        mode: &str,
        level: RadioLevel,
        expected: f32,
        value: f32,
        connection: u64,
        permit: Permit,
    ) -> Result<Completion, Reason> {
        self.remote_radio_idle()?;
        let operating_mode = match mode {
            "digital" => OperatingMode::Digital,
            "phone" => OperatingMode::Phone,
            "cw" => OperatingMode::Cw,
            "rtty" => OperatingMode::Rtty,
            "keyboard" => OperatingMode::Keyboard,
            _ => return Err(Reason::InvalidAction),
        };
        if !value.is_finite() {
            return Err(Reason::InvalidAction);
        }
        // Same limits as the native setters. The browser cannot raise the
        // active power ceiling or persist a different station preference.
        let value = match level {
            RadioLevel::Power => value.clamp(0.0, self.active_power_ceiling()),
            RadioLevel::NotchFrequency => {
                value.clamp(super::super::NOTCH_MIN_HZ, super::super::NOTCH_MAX_HZ)
            }
            _ => value.clamp(0.0, 1.0),
        };
        let expected_native = self.remote_level_desired(level);
        self.validate_remote_level(operating_mode, level, expected, value, expected_native)?;
        let cat_mode = self.remote_filter_mode(connection)?;
        self.queue_remote_target(
            Target {
                hz: self.settings.dial_hz(),
                mode: cat_mode,
                band: self.settings.band.clone(),
                sideband: self.settings.sideband.clone(),
                power_limit: None,
                intent: Intent::Level {
                    mode: operating_mode,
                    level,
                    expected,
                    value,
                    expected_native,
                },
            },
            connection,
            permit,
        )
    }
    pub(super) fn commit_remote_level(&mut self, level: RadioLevel, desired: f32, observed: f32) {
        match level {
            RadioLevel::Power => {
                self.set_rf_power(desired);
                self.observe_rig_power(observed);
            }
            RadioLevel::MicGain => {
                self.set_mic_gain(desired);
                self.observe_rig_mic_gain(observed);
            }
            RadioLevel::NoiseReduction => {
                self.set_nr_level(desired);
                self.observe_rig_nr_level(observed);
            }
            RadioLevel::Compression => {
                self.set_comp_level(desired);
                self.observe_rig_comp_level(observed);
            }
            RadioLevel::NotchFrequency => {
                self.set_notch_freq_hz(desired);
                self.observe_rig_notch_freq_hz(observed);
            }
            RadioLevel::AfGain => {
                self.set_af_gain(desired);
                self.observe_rig_af_gain(observed);
            }
            RadioLevel::RfGain => {
                self.set_rf_gain(desired);
                self.observe_rig_rf_gain(observed);
            }
            RadioLevel::Squelch => {
                self.set_squelch(desired);
                self.observe_rig_squelch(observed);
            }
        }
    }
}
