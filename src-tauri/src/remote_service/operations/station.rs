//! Closed station actions beneath the existing native authority. Receiver
//! changes use the same Engine verbs as the local cockpit. Radio/amplifier
//! work additionally needs the owning hardware worker's completion receipt.

use serde::{Deserialize, Serialize};
use tempo_app::{
    engine::Engine,
    remote_control::{Completion, Evidence, Outcome, Permit, Reason},
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Context {
    radio_id: u32,
    radio_connection: Option<u64>,
    amp_connection: Option<u64>,
    amp_read_sequence: Option<u64>,
}

impl Context {
    pub fn capture(engine: &Engine) -> Self {
        let observation = engine.remote_monitor_observation();
        let amp = observation.amplifier.as_ref().and_then(|a| a.reading);
        Self {
            radio_id: observation.radio.id,
            radio_connection: observation
                .radio
                .readings
                .cat
                .map(|r| r.connection_generation),
            amp_connection: amp.map(|r| r.connection_generation),
            amp_read_sequence: amp.map(|r| r.read_sequence),
        }
    }

    fn matches_radio(&self, engine: &Engine) -> bool {
        self.radio_id == engine.settings().active_radio
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Receiver {
    Rtty,
    Psk,
    Sstv,
    Aprs,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TextReceiver {
    Cw,
    Rtty,
    Psk,
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum KeyboardReceiver {
    Rtty,
    Psk,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "action", deny_unknown_fields)]
pub enum Action {
    #[serde(rename = "decoder.arm")]
    ReceiverArm { receiver: Receiver, on: bool },
    #[serde(rename = "decoder.clear")]
    ReceiverClear { receiver: TextReceiver },
    #[serde(rename = "decoder.afcReset")]
    ReceiverAfcReset { receiver: KeyboardReceiver },
    #[serde(rename = "decoder.net")]
    ReceiverNet { receiver: KeyboardReceiver, hz: f32 },
    #[serde(rename = "decoder.pskMode")]
    PskMode { mode: String, reverse: bool },
    #[serde(rename = "radio.disarm")]
    Disarm {},
    #[serde(rename = "radio.frequency")]
    Frequency {
        #[serde(rename = "dialMhz")]
        dial_mhz: f64,
        band: String,
        sideband: String,
    },
    #[serde(rename = "radio.mode")]
    Mode {
        mode: String,
        #[serde(rename = "followFrequency")]
        follow_frequency: bool,
    },
    #[serde(rename = "radio.tier")]
    Tier { tier: String },
    #[serde(rename = "radio.select")]
    Radio {
        #[serde(rename = "radioId")]
        radio_id: u32,
    },
    #[serde(rename = "amplifier.operate")]
    AmpOperate {
        #[serde(rename = "expectedOperate")]
        expected_operate: bool,
        operate: bool,
    },
    #[serde(rename = "amplifier.band")]
    AmpBand {
        #[serde(rename = "expectedBand")]
        expected_band: String,
        direction: i8,
    },
}

pub fn execute(
    engine: &mut Engine,
    context: &Context,
    action: &Action,
    permit: Permit,
) -> Result<Completion, Reason> {
    if !permit.valid(std::time::Instant::now()) {
        return Err(Reason::AuthorityExpired);
    }
    if !context.matches_radio(engine) {
        return Err(Reason::ContextChanged);
    }
    if engine.remote_receiver_context_generation() == u64::MAX
        || engine.remote_actuation_context_generation() == u64::MAX
    {
        return Err(Reason::ContextChanged);
    }
    match action {
        Action::ReceiverArm { receiver, on } => match receiver {
            Receiver::Rtty => engine.set_rtty_armed(*on),
            Receiver::Psk => engine.set_psk_armed(*on),
            Receiver::Sstv => engine.set_sstv_armed(*on),
            Receiver::Aprs => engine.set_aprs_receive_only(*on),
        },
        Action::ReceiverClear { receiver } => match receiver {
            TextReceiver::Cw => engine.cw_clear(),
            TextReceiver::Rtty => engine.rtty_clear(),
            TextReceiver::Psk => engine.psk_clear(),
        },
        Action::ReceiverAfcReset { receiver } => match receiver {
            KeyboardReceiver::Rtty => engine.request_rtty_afc_reset(),
            KeyboardReceiver::Psk => engine.request_psk_afc_reset(),
        },
        Action::ReceiverNet { receiver, hz } => {
            if !hz.is_finite() || !(300.0..=3700.0).contains(hz) {
                return Err(Reason::InvalidAction);
            }
            match receiver {
                KeyboardReceiver::Rtty => engine.rtty_net(*hz),
                KeyboardReceiver::Psk => engine.psk_net(*hz),
            }
        }
        Action::PskMode { mode, reverse } => {
            use tempo_core::psk::PskModeKind;
            let mode = match mode.as_str() {
                "PSK31" => PskModeKind::Bpsk31,
                "QPSK31" => PskModeKind::Qpsk31,
                _ => return Err(Reason::InvalidAction),
            };
            engine
                .set_psk_mode(mode, *reverse)
                .map_err(|_| Reason::StationBusy)?;
        }
        Action::Disarm {} => {
            engine.set_tx_enabled(false);
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::StationState,
            });
            return Ok(result);
        }
        #[cfg(feature = "radio")]
        Action::Frequency {
            dial_mhz,
            band,
            sideband,
        } => {
            return engine.queue_remote_frequency(
                *dial_mhz,
                band,
                sideband,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::AmpOperate {
            expected_operate,
            operate,
        } => {
            use tempo_app::remote_control::amplifier::{Request, Target};
            if expected_operate == operate {
                return Err(Reason::InvalidAction);
            }
            let request = Request::new(
                engine,
                Target::Operate {
                    expected: *expected_operate,
                    desired: *operate,
                },
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                context.amp_connection.ok_or(Reason::ReadingUnavailable)?,
                context
                    .amp_read_sequence
                    .ok_or(Reason::ReadingUnavailable)?,
                permit,
            )?;
            return engine.queue_remote_amp(request);
        }
        #[cfg(feature = "radio")]
        Action::AmpBand {
            expected_band,
            direction,
        } => {
            use tempo_app::remote_control::amplifier::{Request, Target};
            use tempo_audio::amplifier::{band_index_for_label, kpa_band_label, spe_band_label};
            if ![-1, 1].contains(direction) {
                return Err(Reason::InvalidAction);
            }
            let next = i16::from(band_index_for_label(expected_band).ok_or(Reason::InvalidAction)?)
                + i16::from(*direction);
            let next = u8::try_from(next).map_err(|_| Reason::InvalidAction)?;
            let family = engine
                .settings()
                .active_profile()
                .map(|p| p.amp_model.trim().to_lowercase())
                .ok_or(Reason::HardwareUnavailable)?;
            // The shared SPE "15K" model token does not identify the hardware
            // series: early 1.5K manuals include 4 m, third-series rev. 3.2 does
            // not. Do not guess that a front-panel step beyond 6 m is supported
            // or that it wraps. 13K explicitly reports the 4 m-capable model.
            let model = engine
                .remote_monitor_observation()
                .amplifier
                .map(|a| a.model)
                .ok_or(Reason::ReadingUnavailable)?;
            if family == "spe" && next > if model == "13K" { 11 } else { 10 } {
                return Err(Reason::InvalidAction);
            }
            let desired = match family.as_str() {
                "spe" => spe_band_label(next),
                "kpa" => kpa_band_label(next),
                _ => None,
            }
            .ok_or(Reason::InvalidAction)?;
            let request = Request::new(
                engine,
                Target::Band {
                    expected: expected_band.clone(),
                    desired: desired.into(),
                    direction: *direction,
                },
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                context.amp_connection.ok_or(Reason::ReadingUnavailable)?,
                context
                    .amp_read_sequence
                    .ok_or(Reason::ReadingUnavailable)?,
                permit,
            )?;
            return engine.queue_remote_amp(request);
        }
        // Hardware actions are admitted only once their owner is connected.
        // They must never fall through to a generic engine/Tauri command.
        _ => return Err(Reason::UnsupportedAction),
    }
    let result = Completion::default();
    result.finish(Outcome::Applied {
        evidence: Evidence::ReceiverState,
    });
    Ok(result)
}

pub fn capabilities() -> Vec<&'static str> {
    #[cfg(feature = "radio")]
    {
        vec!["decoder", "amplifier", "frequency"]
    }
    #[cfg(not(feature = "radio"))]
    {
        vec!["decoder"]
    }
}
