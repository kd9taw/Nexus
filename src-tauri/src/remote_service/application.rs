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
/// How old a cached sample may be and still be served while the Engine is busy. The relay drops
/// the station's socket over a sample whose age plus its credit's round trip reaches the 3 s
/// freshness window, and a batch can wait up to the longest topic interval (1 s) for a due
/// topic, so one second of age keeps a second of transit margin.
const BUSY_GRACE: Duration = Duration::from_millis(1000);
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
    #[serde(rename = "get_sstv_state")]
    Sstv,
    #[serde(rename = "get_remote_aprs_state")]
    Aprs,
    #[serde(rename = "get_remote_satellite_state")]
    Satellite,
}
impl Command {
    pub(super) fn interval(self) -> Duration {
        Duration::from_millis(match self {
            Self::Snapshot | Self::Js8 => 500,
            Self::Spectrum | Self::Scope => 100,
            Self::Meters | Self::Cw | Self::Rtty | Self::Psk => 200,
            Self::Settings | Self::BandPlan | Self::Sstv | Self::Aprs | Self::Satellite => 1000,
        })
    }
    pub(super) fn legacy(self) -> bool {
        !matches!(
            self,
            Self::Scope
                | Self::Cw
                | Self::Rtty
                | Self::Psk
                | Self::Js8
                | Self::Sstv
                | Self::Aprs
                | Self::Satellite
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
    pub(super) sstv_images: super::sstv::SharedImages,
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
    "activeRadio",
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
    // The geographic alert scope (#174), beside the band scopes it sits with in Settings.
    "alertContinents",
    "alertEntities",
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
    let all = serde_json::to_value(&settings).map_err(|_| "applicationUnavailable")?;
    let mut view = serde_json::Map::new();
    for key in SETTINGS_KEYS {
        if let Some(value) = all.get(*key) {
            view.insert((*key).into(), value.clone());
        }
    }
    view.insert("bandChoices".into(), serde_json::json!({
        "cw": tempo_app::bandplan::licensed_bands(settings.license_class, tempo_app::settings::OperatingMode::Cw),
        "phone": tempo_app::bandplan::licensed_bands(settings.license_class, tempo_app::settings::OperatingMode::Phone),
    }));
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
            let eng = match engine.try_lock() {
                Ok(eng) => eng,
                Err(_) => return self.cached(command, request_id, base, now),
            };
            // Clone the typed result while locked; encoding and diffing belong
            // outside the engine lock, independently of the radio loop.
            let value = match command {
                Command::Satellite => Ok(super::query::navigation::live(&eng)?),
                Command::Aprs => Ok(super::aprs::live(&eng)?),
                Command::Meters => return Err("applicationUnavailable"), // handled without the engine above
                Command::Sstv => {
                    super::sstv::preflight(&eng)?;
                    let mut state = crate::sstv_state_dto(&eng);
                    let class = eng.settings().license_class;
                    let captured_at_ms = super::now_ms();
                    drop(eng);
                    match self.sstv_images.try_lock() {
                        Ok(mut images) => images.project(&mut state.gallery)?,
                        Err(_) => return self.cached(command, request_id, base, now),
                    }
                    let plan: Vec<_> = tempo_app::bandplan::sstv_band_plan().into_iter().map(|mut c| {
                        c.tx = tempo_app::privileges::tx_allowed(class, c.dial_mhz, tempo_app::settings::OperatingMode::Phone);
                        c
                    }).collect();
                    let value = serde_json::json!({"state":state,"capturedAtMs":captured_at_ms,"plan":plan});
                    if value.to_string().len() > 256 * 1024 { return Err("applicationTooLarge"); }
                    Ok(value)
                }
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
                    let snapshot = eng.snapshot();
                    let ft_runtime = eng.remote_ft_runtime();
                    let ft_settings = eng.remote_ft_settings();
                    let current_key = eng.current_qso_log_key();
                    let pending_key = eng.pending_qso_log_key();
                    drop(eng);
                    serde_json::to_value(snapshot).map(|mut value| {
                        value["remoteFtRuntime"] = serde_json::json!(ft_runtime);
                        value["remoteFtSettings"] = serde_json::json!(ft_settings);
                        value["currentQsoLogKey"] = serde_json::json!(current_key);
                        value["pendingQsoLogKey"] = serde_json::json!(pending_key);
                        value
                    })
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
    /// The radio loop holds the Engine across blocking CAT, so a `try_lock` miss is routine, not
    /// a fault. The cached sample is what the browser is already showing: it is served again with
    /// its true age rather than an error, which made the browser drop a known-good value and
    /// disable every station control on it at once. Only a topic never yet read, or a sample too
    /// old for the wire, is refused as busy; the browser then keeps its value and ages it.
    fn cached(
        &self,
        command: Command,
        request_id: &str,
        base: Option<u64>,
        now: Instant,
    ) -> Result<String, &'static str> {
        match self.entries.get(&command) {
            Some(entry) if now.saturating_duration_since(entry.at) < BUSY_GRACE => {
                self.reply(command, request_id, base, now)
            }
            _ => Err("applicationBusy"),
        }
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
///
/// A credit that is slow to come back stalls the stream and nothing else: `next` publishes
/// nothing until it arrives, and it is honoured whenever it does. There is deliberately no
/// deadline here. The one there was (three seconds awaiting, then `serviceUnavailable`) was
/// propagated by the transport with `?`, which dropped the STATION'S socket - every connected
/// browser gone, transmit revoked, control lapsed - over a credit that was merely late, and a
/// large send blocking the same select loop was enough to make it late. Whether the station is
/// still answering is the relay's question: it keeps its own deadline on an unanswered credit
/// and closes the socket from its side, and the transport's ping/pong covers a dead service.
#[derive(Default)]
pub(super) struct Stream {
    watch: String,
    topics: Vec<Command>,
    credit: Option<String>,
    awaiting: Option<String>,
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
            || topics.len() > 13
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
        if self.awaiting.as_deref() != Some(previous) || self.credit.is_some() {
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
                    // A busy miss changes nothing at either end: the relay keeps its last value
                    // through an error, so the base it acknowledged still stands and the next good
                    // read is a delta, not a full resend. Any other error drops the base.
                    if error != "applicationBusy" {
                        self.offered.remove(&topic);
                    }
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
        self.awaiting = Some(self.credit.take().ok_or("invalidResponse")?);
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
    }
    /// A credit the relay is slow to return stalls the STREAM, never the station's socket. The
    /// old contract failed `next` with `serviceUnavailable` after three seconds awaiting, and the
    /// transport propagated that with `?` - one late credit dropped the station's link, which took
    /// every connected browser with it, revoked TX and lapsed control. The relay keeps its own
    /// deadline on an unanswered credit and is the one that decides whether the station is gone.
    #[test]
    fn a_late_credit_stalls_the_stream_and_is_honoured_when_it_arrives() {
        use std::sync::{Arc, Mutex};
        let engine = Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
            Default::default(),
        )));
        let mut publisher = Publisher::default();
        let mut stream = Stream::default();
        let now = Instant::now();
        let watch = "bbe7d95a-6fbd-47aa-95a7-ab1e4c083dd6";
        let next = "056c07c9-65c3-48fc-a188-957de3338a4a";
        stream
            .watch(watch.into(), vec![Command::Meters], Some(REQUEST.into()))
            .unwrap();
        assert!(stream.next(&mut publisher, &engine, now).unwrap().is_some());
        for seconds in [1, 3, 4, 60] {
            assert_eq!(
                stream.next(&mut publisher, &engine, now + Duration::from_secs(seconds)),
                Ok(None),
                "{seconds} s without a credit is a stalled stream, not a dead session"
            );
        }
        stream
            .credit(watch, REQUEST, next.into())
            .expect("a late credit is still the credit for the batch in flight");
        assert!(stream
            .next(&mut publisher, &engine, now + Duration::from_secs(61))
            .unwrap()
            .is_some());
        assert!(
            stream.credit(watch, REQUEST, next.into()).is_err(),
            "positive control: an ACK for a batch already credited is still refused"
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
    fn settings_projection_identifies_selected_profile_without_exporting_profiles() {
        let settings = tempo_app::settings::Settings {
            active_radio: 7,
            ..Default::default()
        };
        let view = settings_view(&settings).unwrap();
        assert_eq!(view["activeRadio"], 7);
        assert!(view.get("radios").is_none());
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
        for (mode, om) in [
            ("cw", tempo_app::settings::OperatingMode::Cw),
            ("phone", tempo_app::settings::OperatingMode::Phone),
        ] {
            assert_eq!(
                view["bandChoices"][mode],
                serde_json::to_value(tempo_app::bandplan::licensed_bands(
                    settings.license_class,
                    om
                ))
                .unwrap()
            );
        }
        assert!(view
            .as_object()
            .unwrap()
            .keys()
            .all(|key| key == "bandChoices" || SETTINGS_KEYS.contains(&key.as_str())));
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
            // #289's folder picker opens an OS dialog on the STATION's screen. A browser has no
            // filesystem to pick from, and a remote operator must never be able to raise a modal
            // nobody is sitting in front of. It is excluded by construction — not a variant here
            // — and this line is what keeps it that way if someone ever widens the vocabulary.
            "pick_data_folder",
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

    /// The radio loop holds the Engine across blocking CAT, so a `try_lock` miss is routine. It
    /// used to answer `applicationBusy`, which the browser turned into a deleted snapshot and
    /// every station control disabled at once. A miss now serves the cached sample with its true
    /// age while it is within BUSY_GRACE, and the stream keeps its delta base through the error
    /// that follows once the grace runs out.
    #[test]
    fn busy_engine_serves_the_cached_sample_aged_and_the_stream_keeps_its_base() {
        use std::sync::{Arc, Mutex};
        let engine = Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
            Default::default(),
        )));
        let mut publisher = Publisher::default();
        let now = Instant::now();
        let fresh: Value = serde_json::from_str(
            &publisher
                .read(&engine, Command::Snapshot, REQUEST, None, now)
                .unwrap(),
        )
        .unwrap();
        assert_eq!(fresh["revision"], 1);
        let busy = engine.lock().unwrap();
        // Past the interval, so the read reaches for the Engine and misses: the cached sample.
        let served: Value = serde_json::from_str(
            &publisher
                .read(
                    &engine,
                    Command::Snapshot,
                    REQUEST,
                    Some(1),
                    now + Duration::from_millis(600),
                )
                .expect("a busy Engine serves the cached sample"),
        )
        .unwrap();
        assert_eq!(served["revision"], 1);
        assert_eq!(served["ageMs"], 600, "served with its true age");
        assert_eq!(served["baseRevision"], 1);
        assert_eq!(
            served["data"],
            json!({}),
            "an unchanged sample is an empty delta"
        );
        assert_eq!(
            publisher
                .read(
                    &engine,
                    Command::Snapshot,
                    REQUEST,
                    Some(1),
                    now + Duration::from_millis(1000)
                )
                .unwrap_err(),
            "applicationBusy",
            "a sample at BUSY_GRACE is too old for the wire"
        );
        // Positive control: with nothing cached a busy Engine is still busy.
        assert_eq!(
            Publisher::default()
                .read(&engine, Command::Snapshot, REQUEST, None, now)
                .unwrap_err(),
            "applicationBusy"
        );
        drop(busy);

        let mut stream = Stream::default();
        let watch = "bbe7d95a-6fbd-47aa-95a7-ab1e4c083dd6";
        let credits = [
            "056c07c9-65c3-48fc-a188-957de3338a4a",
            "1b0d5a2e-2c6f-4d3e-9d4a-3f5c7e8b9a01",
            "2c1e6b3f-3d7a-4e4f-8e5b-4a6d8f9c0b12",
        ];
        stream
            .watch(watch.into(), vec![Command::Snapshot], Some(REQUEST.into()))
            .unwrap();
        let base = now + Duration::from_secs(10);
        let first: Value =
            serde_json::from_str(&stream.next(&mut publisher, &engine, base).unwrap().unwrap())
                .unwrap();
        assert_eq!(first["updates"][0]["revision"], 1);
        stream.credit(watch, REQUEST, credits[0].into()).unwrap();
        let busy = engine.lock().unwrap();
        let cached: Value = serde_json::from_str(
            &stream
                .next(&mut publisher, &engine, base + Duration::from_millis(600))
                .unwrap()
                .expect("a busy miss within the grace is a sample, not an error"),
        )
        .unwrap();
        assert_eq!(cached["updates"][0]["type"], "applicationResult");
        assert_eq!(cached["updates"][0]["baseRevision"], 1);
        assert_eq!(cached["updates"][0]["ageMs"], 600);
        stream.credit(watch, credits[0], credits[1].into()).unwrap();
        let refused: Value = serde_json::from_str(
            &stream
                .next(&mut publisher, &engine, base + Duration::from_millis(1200))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(refused["updates"][0]["type"], "applicationError");
        assert_eq!(refused["updates"][0]["error"], "applicationBusy");
        stream.credit(watch, credits[1], credits[2].into()).unwrap();
        drop(busy);
        // The browser still holds revision 1, so the read after the error is a delta against it.
        let resumed: Value = serde_json::from_str(
            &stream
                .next(&mut publisher, &engine, base + Duration::from_millis(1800))
                .unwrap()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(resumed["updates"][0]["type"], "applicationResult");
        assert_eq!(
            resumed["updates"][0]["baseRevision"], 1,
            "a busy error must not force a full resend"
        );
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
