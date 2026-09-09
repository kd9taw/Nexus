//! Reviewed application reads for the managed browser transport. No command in
//! this module can mutate the engine, key a transmitter, access the vault or write
//! a logbook. A shared short cache bounds work across observers; exact revisions
//! allow unchanged top-level fields to stay off the wire.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::time::{Duration, Instant};

pub const MAX_BYTES: usize = 768 * 1024;
const MAX_REVISION: u64 = 9_007_199_254_740_991;
#[derive(Clone, Copy, Debug, Deserialize, Serialize, Hash, PartialEq, Eq)]
pub enum Command {
    #[serde(rename = "get_snapshot")]
    Snapshot,
    #[serde(rename = "get_settings")]
    Settings,
    #[serde(rename = "get_band_plan")]
    BandPlan,
    #[serde(rename = "get_spectrum_row")]
    Spectrum,
    #[serde(rename = "get_meters")]
    Meters,
}
impl Command {
    fn interval(self) -> Duration {
        Duration::from_millis(match self {
            Self::Snapshot => 500,
            Self::Spectrum => 100,
            Self::Meters => 200,
            Self::Settings | Self::BandPlan => 1000,
        })
    }
}
struct Entry {
    revision: u64,
    value: Value,
    at: Instant,
    previous: Option<(u64, Value)>,
}
#[derive(Default)]
pub struct Publisher {
    entries: HashMap<Command, Entry>,
    revision: u64,
    spectrum: Option<tempo_app::engine::SpectrumFeed>,
    meters: tempo_app::engine::MeterFeed,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Reply<'a> {
    r#type: &'static str,
    request_id: &'a str,
    command: Command,
    revision: u64,
    base_revision: Option<u64>,
    age_ms: u64,
    data: Value,
    removed: Vec<String>,
}

// This explicit projection is intentionally independent of Settings serialization:
// a newly added connector key, path or credential cannot silently enter Remote.
// These are actual station values, not default settings constructed in the browser.
const SETTINGS_KEYS: &[&str] = &[
    "mycall",
    "mygrid",
    "opName",
    "opState",
    "units",
    "licenseClass",
    "band",
    "dialMhz",
    "sideband",
    "phoneMode",
    "operatingMode",
    "b4MatchMode",
    "txLevel",
    "rxGain",
    "decodeDepth",
    "decodeFLowHz",
    "decodeFHighHz",
    "promptToLog",
    "clearDxAfterLog",
    "autoLog",
    "cwPitchHz",
    "cwWpm",
    "rttyShiftHz",
    "rttyBaud",
    "txEven",
    "holdTxFreq",
    "rxOffsetHz",
    "txOffsetHz",
    "q65PeriodS",
    "msk144PeriodS",
    "fst4PeriodS",
    "ampFollowBand",
    "ampModel",
    "alertDxccBands",
    "alertGridBands",
    "alertRareGridBands",
    "bandEdgeTones",
    "blockedCalls",
    "companionAddr",
    "fdActive",
    "fdEvent",
    "fdOperator",
    "macros",
    "potaNewActivationAlert",
    "preferRrr",
    "qsyCadence",
    "qsySet",
    "rotatorHost",
    "rotatorModel",
    "soundTxState",
    "sstvDefaultTxMode",
    "sstvTxPowerPct",
    "wheelTuneSensitivity",
    "specialOp",
];
fn settings_view(settings: &tempo_app::settings::Settings) -> Result<Value, &'static str> {
    let mut settings = settings.clone();
    settings.sync_flat_from_active();
    let all = serde_json::to_value(settings).map_err(|_| "applicationUnavailable")?;
    let mut view = serde_json::Map::new();
    for key in SETTINGS_KEYS {
        if let Some(value) = all.get(*key) {
            view.insert((*key).into(), value.clone());
        }
    }
    Ok(Value::Object(view))
}
fn changes(previous: &Value, current: &Value) -> Option<(Value, Vec<String>)> {
    let (previous, current) = (previous.as_object()?, current.as_object()?);
    let changed = current
        .iter()
        .filter(|(key, value)| previous.get(*key) != Some(*value))
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    let removed = previous
        .keys()
        .filter(|key| !current.contains_key(*key))
        .cloned()
        .collect();
    Some((Value::Object(changed), removed))
}
impl Publisher {
    pub fn with_feeds(
        spectrum: Option<tempo_app::engine::SpectrumFeed>,
        meters: tempo_app::engine::MeterFeed,
    ) -> Self {
        Self {
            spectrum,
            meters,
            ..Self::default()
        }
    }
    pub fn read(
        &mut self,
        engine: &crate::SharedEngine,
        command: Command,
        request_id: &str,
        base: Option<u64>,
        now: Instant,
    ) -> Result<String, &'static str> {
        if !super::transport::identifier(request_id)
            || base.is_some_and(|n| n == 0 || n > MAX_REVISION)
        {
            return Err("applicationUnsupported");
        }
        if self
            .entries
            .get(&command)
            .is_none_or(|entry| now.saturating_duration_since(entry.at) >= command.interval())
        {
            if command == Command::Meters {
                let meters = tempo_app::dto::MeterReadout {
                    rx_level: self.meters.rx_level(),
                    smeter_db: self.meters.smeter_db(),
                    cw_tone_hz: self.meters.cw_tone_hz(),
                };
                self.insert(
                    command,
                    serde_json::to_value(meters).map_err(|_| "applicationUnavailable")?,
                    now,
                )?;
                return self.reply(command, request_id, base, now);
            }
            if command == Command::Spectrum {
                if let Some(row) = self
                    .spectrum
                    .as_ref()
                    .and_then(|feed| feed.peek_audio_row())
                {
                    self.insert(
                        command,
                        serde_json::to_value(row).map_err(|_| "applicationUnavailable")?,
                        now,
                    )?;
                    return self.reply(command, request_id, base, now);
                }
            }
            // Never queue behind the radio loop. The browser retries a refused READ;
            // there is no deferred engine operation that can execute after disconnect.
            let eng = engine.try_lock().map_err(|_| "applicationBusy")?;
            // Clone the typed result while locked; encoding and diffing belong
            // outside the engine lock, independently of the radio loop.
            let value = match command {
                Command::Meters => return Err("applicationUnavailable"), // handled without the engine above
                Command::Snapshot => {
                    let value = eng.snapshot();
                    drop(eng);
                    serde_json::to_value(value)
                }
                Command::Settings => {
                    let value = eng.settings().clone();
                    drop(eng);
                    self.insert(command, settings_view(&value)?, now)?;
                    return self.reply(command, request_id, base, now);
                }
                Command::BandPlan => {
                    let value = eng.band_plan();
                    drop(eng);
                    serde_json::to_value(value)
                }
                Command::Spectrum => {
                    let value = eng.spectrum_row();
                    drop(eng);
                    serde_json::to_value(value)
                }
            }
            .map_err(|_| "applicationUnavailable")?;
            self.insert(command, value, now)?;
        }
        self.reply(command, request_id, base, now)
    }
    fn insert(&mut self, command: Command, value: Value, now: Instant) -> Result<(), &'static str> {
        if serde_json::to_vec(&value)
            .map_err(|_| "applicationUnavailable")?
            .len()
            > MAX_BYTES - 1024
        {
            return Err("applicationTooLarge");
        }
        if let Some(entry) = self.entries.get_mut(&command) {
            if entry.value == value {
                entry.at = now;
                return Ok(());
            }
        }
        self.revision = self
            .revision
            .checked_add(1)
            .filter(|n| *n <= MAX_REVISION)
            .ok_or("applicationUnavailable")?;
        let previous = self
            .entries
            .remove(&command)
            .map(|entry| (entry.revision, entry.value));
        self.entries.insert(
            command,
            Entry {
                revision: self.revision,
                value,
                at: now,
                previous,
            },
        );
        Ok(())
    }
    fn reply(
        &self,
        command: Command,
        request_id: &str,
        base: Option<u64>,
        now: Instant,
    ) -> Result<String, &'static str> {
        let entry = self.entries.get(&command).ok_or("applicationUnavailable")?;
        let prior = base.and_then(|revision| {
            if revision == entry.revision {
                Some(&entry.value)
            } else {
                entry
                    .previous
                    .as_ref()
                    .filter(|(n, _)| *n == revision)
                    .map(|(_, v)| v)
            }
        });
        let delta = prior.and_then(|previous| changes(previous, &entry.value));
        let base_revision = delta.as_ref().and(base);
        let (data, removed) = delta.unwrap_or_else(|| (entry.value.clone(), Vec::new()));
        let response = serde_json::to_string(&Reply {
            r#type: "applicationResult",
            request_id,
            command,
            revision: entry.revision,
            base_revision,
            age_ms: now.saturating_duration_since(entry.at).as_millis() as u64,
            data,
            removed,
        })
        .map_err(|_| "applicationUnavailable")?;
        if response.len() > MAX_BYTES {
            return Err("applicationTooLarge");
        }
        Ok(response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const REQUEST: &str = "8aa041cb-c642-459c-83f3-11a5b720647d";
    #[test]
    fn exact_revision_delta_and_missing_base_full_response() {
        let mut publisher = Publisher::default();
        let now = Instant::now();
        publisher
            .insert(
                Command::Snapshot,
                json!({"mycall":"TEST", "radio":{"dialMhz":14.074}}),
                now,
            )
            .unwrap();
        publisher
            .insert(
                Command::Snapshot,
                json!({"mycall":"TEST", "radio":{"dialMhz":7.074}}),
                now,
            )
            .unwrap();
        let delta: Value = serde_json::from_str(
            &publisher
                .reply(Command::Snapshot, REQUEST, Some(1), now)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(delta["baseRevision"], 1);
        assert_eq!(delta["data"], json!({"radio":{"dialMhz":7.074}}));
        let full: Value = serde_json::from_str(
            &publisher
                .reply(Command::Snapshot, REQUEST, Some(999), now)
                .unwrap(),
        )
        .unwrap();
        assert!(full["baseRevision"].is_null());
        assert_eq!(full["data"]["mycall"], "TEST");
    }
    #[test]
    fn settings_projection_is_closed_and_preserves_actual_operating_values() {
        let settings = tempo_app::settings::Settings {
            mycall: "TEST".into(),
            mygrid: "AA00".into(),
            ..Default::default()
        };
        let view = settings_view(&settings).unwrap();
        let actual = serde_json::to_value(&settings).unwrap();
        let missing: Vec<_> = SETTINGS_KEYS
            .iter()
            .filter(|key| actual.get(**key).is_none())
            .collect();
        assert!(
            missing.is_empty(),
            "Settings wire keys must exist: {missing:?}"
        );
        assert_eq!(view["mycall"], "TEST");
        assert_eq!(view["mygrid"], "AA00");
        assert!(view
            .as_object()
            .unwrap()
            .keys()
            .all(|key| SETTINGS_KEYS.contains(&key.as_str())));
        for key in [
            "cloudlogKey",
            "clublogApiKey",
            "qrzUsername",
            "radioProfiles",
        ] {
            assert!(view.get(key).is_none());
        }
    }
    #[test]
    fn wire_command_vocabulary_excludes_control_and_unreviewed_reads() {
        for command in [
            "get_snapshot",
            "get_settings",
            "get_band_plan",
            "get_spectrum_row",
            "get_meters",
        ] {
            assert!(serde_json::from_value::<Command>(json!(command)).is_ok());
        }
        for command in [
            "set_frequency",
            "halt_tx",
            "log_current_qso",
            "get_credentials_status",
        ] {
            assert!(serde_json::from_value::<Command>(json!(command)).is_err());
        }
    }

    #[test]
    fn busy_engine_refuses_uncached_work_but_live_spectrum_uses_its_feed() {
        use std::sync::{Arc, Mutex};
        let engine = Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
            Default::default(),
        )));
        let spectrum = tempo_app::engine::SpectrumFeed::default();
        spectrum.publish_audio(tempo_app::dto::Spectrum {
            row: vec![0.25; 8],
            lo_hz: 0.0,
            hi_hz: 4000.0,
            source: "audio".into(),
        });
        let meters = tempo_app::engine::MeterFeed::default();
        meters.set_smeter_db(Some(-12));
        let mut publisher = Publisher::with_feeds(Some(spectrum), meters);
        let _busy = engine.lock().unwrap();
        let now = Instant::now();
        assert_eq!(
            publisher
                .read(&engine, Command::Settings, REQUEST, None, now)
                .unwrap_err(),
            "applicationBusy"
        );
        let response: Value = serde_json::from_str(
            &publisher
                .read(&engine, Command::Spectrum, REQUEST, None, now)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(response["data"]["row"], json!(vec![0.25; 8]));
        assert_eq!(response["data"]["source"], "audio");
        assert_eq!(response["data"]["hiHz"], 4000.0);
        let meter: Value = serde_json::from_str(
            &publisher
                .read(&engine, Command::Meters, REQUEST, None, now)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(meter["data"]["smeterDb"], -12);
    }

    #[test]
    fn oversized_reads_do_not_replace_a_valid_cached_revision() {
        let mut publisher = Publisher::default();
        let now = Instant::now();
        publisher
            .insert(Command::Snapshot, json!({"mycall":"TEST"}), now)
            .unwrap();
        assert_eq!(
            publisher
                .insert(
                    Command::Snapshot,
                    json!({"conversations":"x".repeat(MAX_BYTES)}),
                    now
                )
                .unwrap_err(),
            "applicationTooLarge"
        );
        let reply: Value = serde_json::from_str(
            &publisher
                .reply(Command::Snapshot, REQUEST, None, now)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(reply["revision"], 1);
        assert_eq!(reply["data"]["mycall"], "TEST");
    }
}
