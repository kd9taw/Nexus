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
    #[serde(rename = "get_scope_snapshot")]
    Scope,
    #[serde(rename = "get_cw_state")]
    Cw,
    #[serde(rename = "get_rtty_state")]
    Rtty,
    #[serde(rename = "get_psk_state")]
    Psk,
    #[serde(rename = "get_js8_state")]
    Js8,
}
impl Command {
    pub(super) fn interval(self) -> Duration {
        Duration::from_millis(match self {
            Self::Snapshot | Self::Js8 => 500,
            Self::Spectrum | Self::Scope => 100,
            Self::Meters | Self::Cw | Self::Rtty | Self::Psk => 200,
            Self::Settings | Self::BandPlan => 1000,
        })
    }
    pub(super) fn legacy(self) -> bool {
        !matches!(
            self,
            Self::Scope | Self::Cw | Self::Rtty | Self::Psk | Self::Js8
        )
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
    pub(super) journal: Option<std::sync::Arc<std::sync::Mutex<super::query::Journal>>>,
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
    "rigModel",
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
            if matches!(command, Command::Spectrum | Command::Scope) {
                if let Some(row) = self.spectrum.as_ref().and_then(|feed| {
                    if command == Command::Scope {
                        feed.peek_scope_row()
                    } else {
                        feed.peek_audio_row()
                    }
                }) {
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
                Command::Scope => return Err("applicationUnavailable"), // no active scope request for observers
                Command::Cw => {
                    let value = crate::read_cw_state(&eng);
                    drop(eng);
                    serde_json::to_value(value)
                }
                Command::Rtty => {
                    let value = crate::rtty_state_dto(&eng);
                    drop(eng);
                    serde_json::to_value(value)
                }
                Command::Psk => {
                    let value = crate::psk_state_dto(&eng);
                    drop(eng);
                    serde_json::to_value(value)
                }
                Command::Js8 => {
                    let value = eng.bounded_js8_state().ok_or("applicationTooLarge")?;
                    let captured_at_ms = super::now_ms();
                    drop(eng);
                    let value = serde_json::json!({"state": value, "capturedAtMs": captured_at_ms});
                    if serde_json::to_vec(&value)
                        .map_err(|_| "applicationUnavailable")?
                        .len()
                        > 384 * 1024
                    {
                        return Err("applicationTooLarge");
                    }
                    Ok(value)
                }
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
        if command == Command::Snapshot {
            if let Some(journal) = &self.journal {
                if let Ok(mut journal) = journal.try_lock() {
                    journal.observe(&value);
                }
            }
        }
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

/// One shared publisher for the room's union of topics. A batch spends exactly
/// one room-issued credit; the next credit acknowledges its revision bases.
#[derive(Default)]
pub(super) struct Stream {
    watch: String,
    topics: Vec<Command>,
    credit: Option<String>,
    awaiting: Option<(String, Instant)>,
    bases: HashMap<Command, u64>,
    offered: HashMap<Command, u64>,
    sent: HashMap<Command, Instant>,
}
impl Stream {
    pub fn watch(
        &mut self,
        watch: String,
        topics: Vec<Command>,
        request: Option<String>,
    ) -> Result<(), &'static str> {
        if !super::transport::identifier(&watch)
            || topics.len() > 10
            || topics
                .iter()
                .enumerate()
                .any(|(i, topic)| topics[..i].contains(topic))
            || (topics.is_empty() != request.is_none())
            || request
                .as_deref()
                .is_some_and(|id| !super::transport::identifier(id))
        {
            return Err("invalidResponse");
        }
        *self = Self {
            watch,
            topics,
            credit: request,
            ..Self::default()
        };
        Ok(())
    }
    pub fn credit(
        &mut self,
        watch: &str,
        previous: &str,
        request: String,
    ) -> Result<(), &'static str> {
        if !super::transport::identifier(watch)
            || !super::transport::identifier(previous)
            || !super::transport::identifier(&request)
            || previous == request
        {
            return Err("invalidResponse");
        }
        if watch != self.watch {
            return Ok(());
        } // an old watch cannot replenish this one
        if self.awaiting.as_ref().map(|(id, _)| id.as_str()) != Some(previous)
            || self.credit.is_some()
            || self
                .awaiting
                .as_ref()
                .is_some_and(|(_, at)| at.elapsed() >= Duration::from_secs(3))
        {
            return Err("invalidResponse");
        }
        self.awaiting = None;
        self.bases = std::mem::take(&mut self.offered);
        self.credit = Some(request);
        Ok(())
    }
    pub fn active(&self) -> bool {
        !self.topics.is_empty()
    }
    pub fn next(
        &mut self,
        publisher: &mut Publisher,
        engine: &crate::SharedEngine,
        now: Instant,
    ) -> Result<Option<String>, &'static str> {
        if self
            .awaiting
            .as_ref()
            .is_some_and(|(_, at)| now.saturating_duration_since(*at) >= Duration::from_secs(3))
        {
            return Err("serviceUnavailable");
        }
        let Some(request) = &self.credit else {
            return Ok(None);
        };
        let mut updates = Vec::new();
        let mut bytes = 256;
        self.offered = self.bases.clone();
        for &topic in &self.topics {
            if self
                .sent
                .get(&topic)
                .is_some_and(|at| now.saturating_duration_since(*at) < topic.interval())
            {
                continue;
            }
            let sample = publisher
                .read(engine, topic, request, self.bases.get(&topic).copied(), now)
                .and_then(|data| {
                    if bytes + data.len() > MAX_BYTES - 1024 {
                        return Err("applicationTooLarge");
                    }
                    serde_json::from_str::<Value>(&data).map_err(|_| "applicationUnavailable")
                });
            let update = match sample {
                Ok(value) => {
                    self.offered
                        .insert(topic, value["revision"].as_u64().ok_or("invalidResponse")?);
                    value
                }
                Err(error) => {
                    self.offered.remove(&topic);
                    serde_json::json!({"type":"applicationError", "requestId":request, "command":topic, "error":error})
                }
            };
            bytes += serde_json::to_vec(&update)
                .map_err(|_| "invalidResponse")?
                .len();
            updates.push(update);
            self.sent.insert(topic, now);
        }
        if updates.is_empty() {
            return Ok(None);
        }
        let data = serde_json::json!({"type":"applicationBatch", "watchId":self.watch, "requestId":request, "updates":updates}).to_string();
        if data.len() > MAX_BYTES {
            return Err("invalidResponse");
        }
        self.awaiting = Some((self.credit.take().ok_or("invalidResponse")?, now));
        Ok(Some(data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    const REQUEST: &str = "8aa041cb-c642-459c-83f3-11a5b720647d";
    #[test]
    fn js8_observation_uses_native_state_without_entering_or_arming() {
        use std::sync::{Arc, Mutex};
        let engine = tempo_app::engine::Engine::with_settings(Default::default());
        let expected = serde_json::to_value(engine.js8_state()).unwrap();
        let settings = serde_json::to_value(engine.settings()).unwrap();
        let shared = Arc::new(Mutex::new(engine));
        let command: Command = serde_json::from_value(json!("get_js8_state")).unwrap();
        assert!(!command.legacy());
        let data = Publisher::default()
            .read(&shared, command, REQUEST, None, Instant::now())
            .unwrap();
        let reply: Value = serde_json::from_str(&data).unwrap();
        assert_eq!(reply["data"]["state"], expected);
        assert!(reply["data"]["capturedAtMs"].as_u64().unwrap() > 0);
        let engine = shared.lock().unwrap();
        assert_eq!(serde_json::to_value(engine.settings()).unwrap(), settings);
        assert!(!engine.snapshot().radio.tx_enabled);
    }
    #[test]
    fn keyboard_observer_reads_preserve_native_state_and_refuse_busy_engine() {
        use std::sync::{Arc, Mutex};
        let mut engine = tempo_app::engine::Engine::with_settings(Default::default());
        engine.set_rtty_armed(true);
        engine.set_psk_armed(true);
        let chars = vec![
            tempo_core::textmode::DecodedChar {
                ch: 'C',
                confidence: 0.3
            };
            4500
        ];
        engine.push_rtty_decode(&chars, -12.5, true);
        engine.push_psk_decode(&chars, 7.5, true);
        let before_rtty = serde_json::to_value(crate::rtty_state_dto(&engine)).unwrap();
        let before_psk = serde_json::to_value(crate::psk_state_dto(&engine)).unwrap();
        let before_settings = serde_json::to_value(engine.settings()).unwrap();
        assert_eq!(before_rtty["text"].as_str().unwrap().len(), 4000);
        assert_eq!(before_psk["charConf"].as_array().unwrap().len(), 4000);
        assert_eq!(before_psk["charConf"][0], 30);
        let shared = Arc::new(Mutex::new(engine));
        let mut publisher = Publisher::default();
        let now = Instant::now();
        for (name, expected) in [
            ("get_rtty_state", &before_rtty),
            ("get_psk_state", &before_psk),
        ] {
            let command: Command = serde_json::from_value(json!(name)).unwrap();
            assert!(
                !command.legacy(),
                "keyboard reads require an explicit stream capability"
            );
            for offset in [0, 250] {
                let result = publisher
                    .read(
                        &shared,
                        command,
                        REQUEST,
                        None,
                        now + Duration::from_millis(offset),
                    )
                    .unwrap();
                assert!(result.len() < MAX_BYTES);
                let value: Value = serde_json::from_str(&result).unwrap();
                assert_eq!(value["data"], *expected);
            }
            let _held = shared.lock().unwrap();
            assert_eq!(
                Publisher::default()
                    .read(&shared, command, REQUEST, None, now)
                    .unwrap_err(),
                "applicationBusy"
            );
        }
        let engine = shared.lock().unwrap();
        assert_eq!(
            serde_json::to_value(crate::rtty_state_dto(&engine)).unwrap(),
            before_rtty
        );
        assert_eq!(
            serde_json::to_value(crate::psk_state_dto(&engine)).unwrap(),
            before_psk
        );
        assert_eq!(
            serde_json::to_value(engine.settings()).unwrap(),
            before_settings
        );
        assert!(!engine.snapshot().radio.tx_enabled);
        assert!(engine.get_log().is_empty());
    }
    #[test]
    fn stream_is_credited_paced_and_restarts_without_old_delta_bases() {
        use std::sync::{Arc, Mutex};
        let engine = Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
            Default::default(),
        )));
        let mut publisher = Publisher::default();
        let mut stream = Stream::default();
        let now = Instant::now();
        let watch = "bbe7d95a-6fbd-47aa-95a7-ab1e4c083dd6";
        let next = "056c07c9-65c3-48fc-a188-957de3338a4a";
        assert!(stream.next(&mut publisher, &engine, now).unwrap().is_none());
        stream
            .watch(
                watch.into(),
                vec![Command::Meters, Command::Cw, Command::Rtty, Command::Psk],
                Some(REQUEST.into()),
            )
            .unwrap();
        let first: Value =
            serde_json::from_str(&stream.next(&mut publisher, &engine, now).unwrap().unwrap())
                .unwrap();
        assert_eq!(first["updates"].as_array().unwrap().len(), 4);
        assert!(first["updates"][0]["baseRevision"].is_null());
        assert_eq!(first["updates"][1]["command"], "get_cw_state");
        assert!(first["updates"][1]["data"]["sent"]
            .as_array()
            .unwrap()
            .is_empty());
        assert_eq!(first["updates"][2]["command"], "get_rtty_state");
        assert_eq!(first["updates"][3]["command"], "get_psk_state");
        assert!(stream
            .next(&mut publisher, &engine, now + Duration::from_millis(200))
            .unwrap()
            .is_none());
        stream.credit(watch, REQUEST, next.into()).unwrap();
        assert!(
            stream.credit(watch, REQUEST, next.into()).is_err(),
            "a batch can be acknowledged only once"
        );
        assert!(stream
            .next(&mut publisher, &engine, now + Duration::from_millis(100))
            .unwrap()
            .is_none());
        let second: Value = serde_json::from_str(
            &stream
                .next(&mut publisher, &engine, now + Duration::from_millis(200))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert!(second["updates"][0]["baseRevision"].is_number());
        stream
            .watch(next.into(), vec![Command::Meters], Some(REQUEST.into()))
            .unwrap();
        let restarted: Value = serde_json::from_str(
            &stream
                .next(&mut publisher, &engine, now + Duration::from_millis(300))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert!(restarted["updates"][0]["baseRevision"].is_null());
        stream.watch(watch.into(), vec![], None).unwrap();
        assert!(!stream.active());
        assert!(stream
            .next(&mut publisher, &engine, now + Duration::from_secs(10))
            .unwrap()
            .is_none());
        stream
            .watch(watch.into(), vec![Command::Meters], Some(REQUEST.into()))
            .unwrap();
        stream
            .next(&mut publisher, &engine, Instant::now())
            .unwrap();
        stream.awaiting.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(4);
        assert!(
            stream.credit(watch, REQUEST, next.into()).is_err(),
            "a late ACK cannot replenish an expired credit before the next tick"
        );
    }
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
