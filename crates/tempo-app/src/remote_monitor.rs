//! Versioned, bounded station observations. This is a read contract, not a command API.
//! No settings object, logbook, decode history, port names or credentials belong here.

use crate::dto::AmpStatusDto;
use serde::{Deserialize, Serialize};

pub const VERSION: u8 = 1;
pub const POLL_MS: u64 = 500;
pub const STALE_MS: u64 = 3_000;
pub const MAX_FRAME_BYTES: usize = 16_384;
pub const MAX_TEXT_CHARS: usize = 64;
pub const MAX_SEQUENCE: u64 = 9_007_199_254_740_991;

pub fn bounded(value: &str) -> String {
    value.chars().take(MAX_TEXT_CHARS).collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Source {
    Native,
    Fixture,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Frame {
    pub version: u8,
    pub source: Source,
    /// Process/session identity, not an authentication token.
    pub epoch: String,
    pub sequence: u64,
    /// Observation publication time, NOT a hardware measurement timestamp.
    pub generated_at_ms: u64,
    pub station: Observation,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Observation {
    pub call: String,
    pub grid: String,
    pub radio: Radio,
    pub amplifier: Option<Amplifier>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Radio {
    pub id: u32,
    pub name: String,
    /// Nexus station dial; this is not confirmation from the physical radio.
    pub dial_mhz: Option<f64>,
    pub band: String,
    pub mode: String,
    pub rig_mode: Option<String>,
    pub cat_connected: Option<bool>,
    /// Absent when CAT is unavailable. Not proof that RF has stopped.
    pub rig_keyed: Option<bool>,
    /// The existing transmitter arbiter owns some activity, across operating modes.
    pub nexus_busy: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Amplifier {
    pub family: String,
    pub model: String,
    pub follow_band: bool,
    pub linked: bool,
    /// A protocol reason token. An early missed poll can have linked=true AND a reason.
    pub reason: String,
    pub operate: Option<bool>,
    pub transmitting: Option<bool>,
    pub output_watts: Option<u16>,
    pub band_label: Option<String>,
    pub swr: Option<f32>,
    pub swr_atu: Option<f32>,
    pub volts: Option<f32>,
    pub amps: Option<f32>,
    pub temp: Option<i16>,
    pub temp_celsius: bool,
    pub alarm: String,
    pub alarm_raised: bool,
    pub warning: String,
    pub warning_raised: bool,
    pub kpa_fault: Option<u8>,
}

impl Amplifier {
    pub fn from_status(status: &AmpStatusDto, follow_band: bool) -> Self {
        // Copy only the declared v1 fields. Adding a desktop DTO field cannot silently
        // grow this surface. Null readings survive the very first missed poll.
        Self {
            family: bounded(&status.family),
            model: bounded(&status.model),
            follow_band,
            linked: status.linked,
            reason: bounded(&status.reason),
            operate: status.operate,
            transmitting: status.transmitting,
            output_watts: status.output_watts,
            band_label: status.band_label.as_deref().map(bounded),
            swr: status.swr.filter(|n| n.is_finite()),
            swr_atu: status.swr_atu.filter(|n| n.is_finite()),
            volts: status.volts.filter(|n| n.is_finite()),
            amps: status.amps.filter(|n| n.is_finite()),
            temp: status.temp,
            temp_celsius: status.temp_celsius,
            alarm: bounded(&status.alarm),
            alarm_raised: status.alarm_raised,
            warning: bounded(&status.warning),
            warning_raised: status.warning_raised,
            kpa_fault: status.kpa_fault,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn committed_wire_fixtures_round_trip_through_rust() {
        // The browser preview and TS boundary tests consume these exact Rust-serialized
        // frames. Requiring byte-equivalent JSON values catches either side drifting.
        let fixtures: serde_json::Value = serde_json::from_str(include_str!(
            "../../../ui/src/remote-monitor/fixtures.v1.json"
        ))
        .unwrap();
        assert_eq!(fixtures.as_object().unwrap().len(), 9);
        for value in fixtures.as_object().unwrap().values() {
            let frame: Frame = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(frame.version, VERSION);
            assert_eq!(frame.source, Source::Fixture);
            assert_eq!(serde_json::to_value(&frame).unwrap(), *value);
            assert!(serde_json::to_vec(&frame).unwrap().len() < MAX_FRAME_BYTES);
        }
    }

    #[test]
    fn text_is_bounded_without_breaking_unicode_and_nonfinite_readings_are_absent() {
        let status = AmpStatusDto {
            model: "📻".repeat(1_000),
            swr: Some(f32::NAN),
            volts: Some(f32::INFINITY),
            ..Default::default()
        };
        let projected = Amplifier::from_status(&status, true);
        assert_eq!(projected.model.chars().count(), MAX_TEXT_CHARS);
        assert_eq!(projected.swr, None);
        assert_eq!(projected.volts, None);
        assert!(serde_json::to_vec(&projected).unwrap().len() < MAX_FRAME_BYTES);
    }
}
