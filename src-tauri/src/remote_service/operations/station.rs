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
    #[serde(rename = "decoder.js8Speed")]
    Js8Speed {
        #[serde(rename = "expectedSpeed")]
        expected_speed: u8,
        speed: u8,
    },
    #[serde(rename = "decoder.msk144Period")]
    Msk144Period {
        #[serde(rename = "expectedPeriodSecs")]
        expected_period_secs: u16,
        #[serde(rename = "periodSecs")]
        period_secs: u16,
    },
    #[serde(rename = "decoder.depth")]
    DecodeDepth {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "expectedDepth")]
        expected_depth: u8,
        depth: u8,
    },
    #[serde(rename = "receiver.rxOffset")]
    RxOffset {
        #[serde(rename = "expectedTier")]
        expected_tier: tempo_app::dto::Tier,
        #[serde(rename = "expectedHz")]
        expected_hz: f32,
        hz: f32,
    },
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
    Tier { tier: tempo_app::dto::Tier },
    #[serde(rename = "radio.workspace")]
    Workspace {
        workspace: tempo_app::engine::remote_radio::Workspace,
    },
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
    #[serde(rename = "amplifier.followBand")]
    AmpFollowBand {
        #[serde(rename = "radioId")]
        radio_id: u32,
        #[serde(rename = "expectedSettingsRevision")]
        expected_settings_revision: String,
        #[serde(rename = "expectedFollow")]
        expected_follow: bool,
        follow: bool,
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
        #[cfg(feature = "radio")]
        Action::DecodeDepth {
            expected_tier,
            expected_depth,
            depth,
        } => {
            engine.save_remote_decode_depth(
                *expected_tier,
                *expected_depth,
                *depth,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
        }
        #[cfg(feature = "radio")]
        Action::RxOffset {
            expected_tier,
            expected_hz,
            hz,
        } => {
            engine.save_remote_rx_offset(
                *expected_tier,
                *expected_hz,
                *hz,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
        }
        #[cfg(feature = "radio")]
        Action::Js8Speed {
            expected_speed,
            speed,
        } => {
            engine.save_remote_js8_speed(
                *expected_speed,
                *speed,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
        }
        #[cfg(feature = "radio")]
        Action::Msk144Period {
            expected_period_secs,
            period_secs,
        } => {
            engine.save_remote_msk144_period(
                *expected_period_secs,
                *period_secs,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                &permit,
            )?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
        }
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
        Action::Workspace { workspace } => {
            return engine.queue_remote_workspace(
                *workspace,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::Mode {
            mode,
            follow_frequency,
        } => {
            return engine.queue_remote_mode(
                mode,
                *follow_frequency,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::Tier { tier } => {
            return engine.queue_remote_tier(
                *tier,
                context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                permit,
            );
        }
        #[cfg(feature = "radio")]
        Action::AmpFollowBand {
            radio_id,
            expected_settings_revision,
            expected_follow,
            follow,
        } => {
            if expected_settings_revision.len() != 64
                || !expected_settings_revision
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                || expected_follow == follow
            {
                return Err(Reason::InvalidAction);
            }
            if *radio_id != context.radio_id
                || super::super::query::settings_revision(engine.settings())
                    .map_err(|_| Reason::ReadingUnavailable)?
                    != *expected_settings_revision
            {
                return Err(Reason::ContextChanged);
            }
            let profile = engine
                .settings()
                .active_profile()
                .ok_or(Reason::ContextChanged)?;
            if !["spe", "kpa"].contains(&profile.amp_model.trim().to_lowercase().as_str())
                || profile.amp_port.trim().is_empty()
            {
                return Err(Reason::HardwareUnavailable);
            }
            if *follow {
                tempo_app::remote_control::amplifier::follow_ready(
                    engine,
                    context.radio_connection.ok_or(Reason::ReadingUnavailable)?,
                    context.amp_connection.ok_or(Reason::ReadingUnavailable)?,
                    context
                        .amp_read_sequence
                        .ok_or(Reason::ReadingUnavailable)?,
                )?;
            }
            engine.save_remote_amp_follow_band(*radio_id, *expected_follow, *follow, &permit)?;
            let result = Completion::default();
            result.finish(Outcome::Applied {
                evidence: Evidence::SettingsSaved,
            });
            return Ok(result);
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

impl Action {
    pub fn minimum_version(&self) -> u8 {
        match self {
            Self::Frequency { .. }
            | Self::Mode { .. }
            | Self::Tier { .. }
            | Self::Workspace { .. }
            | Self::Js8Speed { .. }
            | Self::Msk144Period { .. }
            | Self::DecodeDepth { .. }
            | Self::RxOffset { .. }
            | Self::Radio { .. }
            | Self::AmpFollowBand { .. } => 3,
            _ => 2,
        }
    }
}

pub fn capabilities(version: u8) -> Vec<&'static str> {
    #[cfg(feature = "radio")]
    {
        if version == 2 {
            vec!["decoder", "amplifier"]
        } else {
            vec![
                "decoder",
                "amplifier",
                "frequency",
                "mode",
                "tier",
                "ampFollowBand",
                "workspace",
                "decoderSettings",
                "receiverSettings",
            ]
        }
    }
    #[cfg(not(feature = "radio"))]
    {
        let _ = version;
        vec!["decoder"]
    }
}
