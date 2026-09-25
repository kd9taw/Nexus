//! Bounded, immutable collection snapshots. Runs on a blocking worker, never on
//! the socket/live-instrument loop. Only the scoped SSTV resource reader accesses
//! image files; no collection has vault or TX access.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BinaryHeap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
mod configuration;
mod confirmations;
mod dxpeditions;
mod field_day;
mod insights;
mod js8;
#[cfg(test)]
pub(super) mod log_tests;
pub(super) mod memories;
pub(crate) mod navigation;
mod ota;
mod parks;
pub(super) mod picture;
mod pounce;
mod recall;
mod rotator;

/// The settings a Remote browser may change, and under which grant (see `configuration`).
pub(super) use configuration::{WRITABLE_CONTROL_KEYS, WRITABLE_LOGGING_KEYS};

/// The same bounded public projection used by the Settings document. Neither
/// private settings fields nor client-supplied filesystem paths enter its digest.
pub(super) fn settings_revision(
    settings: &tempo_app::settings::Settings,
) -> Result<String, &'static str> {
    configuration::settings_revision(settings)
}

/// The `programming` document's revision for one `radioprog.json` — what a browser echoes back
/// when it curates the working channel list. It is read out of the SAME document builder the
/// browser was served, so a change can never be checked against a second, drifting computation.
pub(super) fn programming_revision(path: &std::path::Path) -> Result<String, &'static str> {
    configuration::programming_revision(path)
}

const PAGE_BYTES: usize = 256 * 1024;
const CACHE_BYTES: usize = 16 * 1024 * 1024;
const SNAPSHOT_BYTES: usize = 4 * 1024 * 1024;
const MAX_ROWS: usize = 3000;
const TTL: Duration = Duration::from_secs(60);
#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum Collection {
    Decodes,
    Needs,
    Spots,
    Log,
    Entities,
    Health,
    Recall,
    Awards,
    Statistics,
    Dxpeditions,
    Memories,
    Ota,
    FieldDay,
    Js8Context,
    SstvImage,
    Aprs,
    Settings,
    Programming,
    Connect,
    Path,
    Satellites,
    Satellite,
    Parks,
    Confirmations,
    Pounce,
    Rotator,
}
#[derive(Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Request {
    pub request_id: String,
    pub collection: Collection,
    pub cursor: Option<String>,
    pub search: String,
    pub unconfirmed: bool,
    pub after: Option<u64>,
}
impl Request {
    pub fn valid(&self) -> bool {
        super::transport::identifier(&self.request_id)
            && self.search.len() <= 96
            && !self.search.chars().any(char::is_control)
            && (if self.collection == Collection::Recall {
                (3..=32).contains(&self.search.len())
                    && self
                        .search
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'/')
                    && !self.unconfirmed
                    && self.cursor.is_none()
            } else if navigation::collection(self.collection) {
                navigation::valid_search(self.collection, &self.search) && !self.unconfirmed
            } else if self.collection == Collection::SstvImage {
                super::sstv::identifier(&self.search) && !self.unconfirmed
            } else if self.collection == Collection::Parks {
                parks::valid_search(&self.search) && !self.unconfirmed
            } else {
                self.collection == Collection::Log || (self.search.is_empty() && !self.unconfirmed)
            })
            && self.after.is_none_or(|n| {
                self.collection == Collection::Decodes && n <= 9_007_199_254_740_991
            })
            && (!matches!(
                self.collection,
                Collection::Awards
                    | Collection::Statistics
                    | Collection::Dxpeditions
                    | Collection::Memories
                    | Collection::Ota
                    | Collection::FieldDay
                    | Collection::Js8Context
                    | Collection::Parks
                    | Collection::Confirmations
                    | Collection::Pounce
                    | Collection::Rotator
            ) || self.cursor.is_none())
            && self
                .cursor
                .as_deref()
                .is_none_or(|c| parse_cursor(c).is_some())
    }
}
fn parse_cursor(cursor: &str) -> Option<(&str, usize)> {
    let (id, offset) = cursor.split_once(':')?;
    let n = offset.parse::<usize>().ok()?;
    (super::transport::identifier(id) && n > 0 && n < MAX_ROWS && n.to_string() == offset)
        .then_some((id, n))
}
pub(super) fn snapshot_id() -> Result<String, &'static str> {
    let s = super::transport::random_secret()?;
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &s[..8],
        &s[8..12],
        &s[12..16],
        &s[16..20],
        &s[20..32]
    ))
}
#[derive(Clone)]
pub struct Sources {
    /// The needs model the desktop's readers share ([`crate::NeedsKept`]), so a browser's
    /// Needed list folds the log no more often than the desktop's does.
    pub needs: crate::NeedsKept,
    pub spots: crate::SharedSpots,
    pub live_paths: crate::SharedLivePaths,
    pub region_paths: crate::SharedRegionPaths,
    pub ota: crate::SharedOtaSpots,
    pub health: crate::SharedHealth,
    pub propagation: crate::PropCache,
    pub memories: memories::Bank,
    pub parks: crate::SharedParks,
    pub pounces: crate::pouncer::SharedRecent,
    pub sstv: super::sstv::Source,
    pub navigation: navigation::Source,
}

// Display-only journal from the existing Remote snapshot producer. The engine's
// FT sequencing ring is neither read nor modified. No browser demand starts DSP.
#[derive(Default)]
pub struct Journal {
    epoch: String,
    seen_tx: VecDeque<String>,
    band: String,
    tier: String,
    rows: VecDeque<(String, Value)>,
    sequence: u64,
    dropped: usize,
    bytes: usize,
    generation: u64,
}
impl Journal {
    pub fn observe(&mut self, snapshot: &Value) {
        if self.epoch.is_empty() {
            let Ok(epoch) = snapshot_id() else { return };
            self.epoch = epoch;
        }
        let band = snapshot["radio"]["band"].as_str().unwrap_or("");
        let tier = snapshot["link"]["tier"].as_str().unwrap_or("");
        // Match desktop history: an off-band excursion does not discard the last
        // named band's rows. A tier change always starts a fresh context.
        if (!band.is_empty() && band != self.band) || tier != self.tier {
            self.rows.clear();
            self.seen_tx.clear();
            self.bytes = 0;
            self.dropped = 0;
            self.generation += 1;
        }
        if !band.is_empty() {
            self.band = band.to_owned();
        }
        self.tier = tier.to_owned();
        let slot = snapshot["radio"]["slot"].as_u64().unwrap_or(0);
        let at = super::now_ms();
        for row in snapshot["recentDecodes"].as_array().into_iter().flatten() {
            let key = if row["mine"] == true {
                format!("mine|{}", row["txAt"])
            } else {
                format!(
                    "{slot}|{}|{}",
                    row["message"],
                    (row["freqHz"].as_f64().unwrap_or(0.0) / 5.0).round()
                )
            };
            if let Some((_, previous)) = self.rows.iter_mut().find(|(k, _)| k == &key) {
                if previous["row"] != *row {
                    let bytes = row.to_string().len();
                    if bytes > PAGE_BYTES / 2 {
                        continue;
                    }
                    self.bytes =
                        self.bytes.saturating_sub(previous["row"].to_string().len()) + bytes;
                    self.sequence += 1;
                    previous["sequence"] = json!(self.sequence);
                    previous["row"] = row.clone();
                }
                self.trim();
                continue;
            }
            if row["mine"] == true && self.seen_tx.contains(&key) {
                continue;
            }
            let bytes = row.to_string().len();
            if bytes > PAGE_BYTES / 2 {
                continue;
            }
            if row["mine"] == true {
                self.seen_tx.push_back(key.clone());
                while self.seen_tx.len() > 64 {
                    self.seen_tx.pop_front();
                }
            }
            self.sequence += 1;
            self.bytes += bytes;
            self.rows.push_back((
                key,
                json!({ "sequence": self.sequence, "firstSequence": self.sequence, "slot": slot, "at": at, "row": row }),
            ));
            self.trim();
        }
    }
    fn trim(&mut self) {
        while self.rows.len() > MAX_ROWS || self.bytes > SNAPSHOT_BYTES / 2 {
            if let Some((_, old)) = self.rows.pop_front() {
                self.bytes = self.bytes.saturating_sub(old["row"].to_string().len());
                self.dropped += 1;
            }
        }
    }
    fn snapshot(&self, after: Option<u64>) -> (Vec<Value>, usize, Value) {
        let mut rows: Vec<_> = self
            .rows
            .iter()
            .filter(|(_, r)| after.is_none_or(|n| r["sequence"].as_u64().unwrap_or(0) > n))
            .map(|(_, r)| r.clone())
            .collect();
        rows.sort_by_key(|r| r["sequence"].as_u64().unwrap_or(0));
        let count = rows.len();
        (
            rows,
            count,
            json!({ "band": self.band, "tier": self.tier, "generation": format!("{}:{}", self.epoch, self.generation), "dropped": self.dropped, "latestSequence": self.sequence }),
        )
    }
}

struct Snapshot {
    id: String,
    collection: Collection,
    search: String,
    unconfirmed: bool,
    after: Option<u64>,
    rows: Vec<Value>,
    total: usize,
    meta: Value,
    at: Instant,
    bytes: usize,
}
#[derive(Default)]
pub struct Publisher {
    snapshots: VecDeque<Snapshot>,
    pub journal: Arc<Mutex<Journal>>,
    unassisted: bool,
    js8: js8::Cache,
    pub(super) sstv_images: super::sstv::SharedImages,
}
impl Publisher {
    pub fn read(
        &mut self,
        request: &Request,
        engine: &crate::SharedEngine,
        sources: Option<&Sources>,
        now: Instant,
    ) -> Result<String, &'static str> {
        if !request.valid() {
            return Err("applicationUnsupported");
        }
        if request.collection == Collection::SstvImage {
            // Validate a retained cursor too. Removing an image locally ends its
            // read capability even if an immutable page snapshot still exists.
            super::sstv::check_current(&self.sstv_images, &request.search, engine)?;
        }
        let unassisted = crate::unassisted();
        if self.unassisted != unassisted {
            self.snapshots.retain(|s| {
                !matches!(
                    s.collection,
                    Collection::Needs
                        | Collection::Spots
                        | Collection::Dxpeditions
                        | Collection::Connect
                        | Collection::Path
                )
            });
            self.unassisted = unassisted;
        }
        self.snapshots
            .retain(|s| now.saturating_duration_since(s.at) < TTL);
        let (id, offset) = if let Some(cursor) = &request.cursor {
            let (id, offset) = parse_cursor(cursor).ok_or("applicationUnsupported")?;
            (id.to_owned(), offset)
        } else {
            // Share one recent capture across observers. A fresh page-zero request
            // eventually sees local log changes; an existing cursor stays sealed.
            let reuse = if matches!(
                request.collection,
                Collection::Recall
                    | Collection::Awards
                    | Collection::Statistics
                    | Collection::Dxpeditions
                    | Collection::Memories
                    | Collection::Ota
                    | Collection::FieldDay
                    | Collection::Js8Context
                    | Collection::SstvImage
                    | Collection::Connect
                    | Collection::Path
                    | Collection::Satellites
                    | Collection::Satellite
            ) {
                0 // Explicit selection/Refresh must see intervening local log changes.
            } else if request.collection == Collection::Confirmations {
                // Diagnosing holds the engine for the whole log, as on the desktop.
                // Refreshes share one capture so a browser cannot hold it repeatedly.
                10_000
            } else if request.collection == Collection::Rotator {
                // The desktop's own rotor poll interval. Two browsers watching the same mast cost
                // one rotctld exchange, not two, and neither asks faster than the strip does.
                2000
            } else if request.collection == Collection::Decodes {
                500
            } else if request.collection == Collection::Needs {
                15000
            } else {
                5000
            };
            if let Some(s) = self.snapshots.iter().rev().find(|s| {
                s.collection == request.collection
                    && s.search == request.search
                    && s.unconfirmed == request.unconfirmed
                    && s.after == request.after
                    && now.saturating_duration_since(s.at) < Duration::from_millis(reuse)
            }) {
                (s.id.clone(), 0)
            } else {
                let (mut rows, total, meta) = self.capture(request, engine, sources)?;
                if (matches!(request.collection, Collection::SstvImage | Collection::Aprs)
                    || navigation::collection(request.collection))
                    && (rows.len() != total
                        || rows.len() > MAX_ROWS
                        || meta.to_string().len()
                            + rows.iter().map(|r| r.to_string().len()).sum::<usize>()
                            > SNAPSHOT_BYTES)
                {
                    return Err("applicationTooLarge");
                }
                rows.truncate(MAX_ROWS);
                let mut bytes = meta.to_string().len();
                let mut retained = 0;
                for row in &rows {
                    let size = row.to_string().len();
                    if size > PAGE_BYTES / 2 {
                        return Err("applicationTooLarge");
                    }
                    if bytes + size > SNAPSHOT_BYTES {
                        break;
                    }
                    bytes += size;
                    retained += 1;
                }
                rows.truncate(retained);
                while self.snapshots.len() >= 8
                    || self.snapshots.iter().map(|s| s.bytes).sum::<usize>() + bytes > CACHE_BYTES
                {
                    self.snapshots.pop_front();
                }
                let id = snapshot_id().map_err(|_| "applicationUnavailable")?;
                self.snapshots.push_back(Snapshot {
                    id: id.clone(),
                    collection: request.collection,
                    search: request.search.clone(),
                    unconfirmed: request.unconfirmed,
                    after: request.after,
                    rows,
                    total,
                    meta,
                    at: now,
                    bytes,
                });
                (id, 0)
            }
        };
        let s = self
            .snapshots
            .iter()
            .find(|s| {
                s.id == id
                    && s.collection == request.collection
                    && s.search == request.search
                    && s.unconfirmed == request.unconfirmed
                    && s.after == request.after
            })
            .ok_or("queryExpired")?;
        if navigation::collection(request.collection) {
            sources
                .ok_or("applicationUnavailable")?
                .navigation
                .validate(engine, &s.meta)?;
        }
        if offset > 0 && offset >= s.rows.len() {
            return Err("queryExpired");
        }
        // Image chunks have a known 64-KiB encoded ceiling. Start at three so
        // constructing a page never repeatedly serializes the entire image.
        let page_rows = if request.collection == Collection::SstvImage {
            3
        } else if navigation::collection(request.collection) {
            // Seven 16-KiB UTF-8 chunks fit even at worst-case JSON escaping.
            // Do not serialize a whole satellite catalog repeatedly while
            // decrementing from the generic 128-row page limit.
            7
        } else {
            128
        };
        let mut end = (offset + page_rows).min(s.rows.len());
        loop {
            let reply = json!({ "type": "applicationPage", "requestId": request.request_id, "collection": request.collection,
                "snapshotId": s.id, "offset": offset, "total": s.total, "retained": s.rows.len(),
                "nextCursor": (end < s.rows.len()).then(|| format!("{}:{end}", s.id)), "ageMs": now.elapsed().as_millis() as u64,
                "rows": s.rows[offset..end], "meta": { "capturedAgeMs": now.saturating_duration_since(s.at).as_millis() as u64, "source": s.meta } }).to_string();
            if reply.len() <= PAGE_BYTES {
                return Ok(reply);
            }
            if request.collection == Collection::Recall || end <= offset + 1 {
                return Err("applicationTooLarge");
            }
            end -= 1;
        }
    }
    fn capture(
        &mut self,
        request: &Request,
        engine: &crate::SharedEngine,
        sources: Option<&Sources>,
    ) -> Result<(Vec<Value>, usize, Value), &'static str> {
        let to_rows = |v| match v {
            Value::Array(a) => Ok(a),
            _ => Err("applicationUnavailable"),
        };
        let rows = match request.collection {
            Collection::Settings
            | Collection::Programming
            | Collection::Connect
            | Collection::Path
            | Collection::Satellites
            | Collection::Satellite => {
                let sources = sources.ok_or("applicationUnavailable")?;
                return sources.navigation.read(request, engine, sources);
            }
            Collection::Aprs => return super::aprs::capture(engine),
            Collection::Confirmations => {
                return Ok((Vec::new(), 0, confirmations::read_engine(engine)?));
            }
            Collection::Parks => {
                return parks::read(
                    &sources.ok_or("applicationUnavailable")?.parks,
                    &request.search,
                );
            }
            Collection::Pounce => {
                return pounce::read(&sources.ok_or("applicationUnavailable")?.pounces, engine);
            }
            Collection::Rotator => return rotator::read(engine),
            Collection::SstvImage => {
                return super::sstv::capture(
                    &self.sstv_images,
                    &request.search,
                    engine,
                    &sources.ok_or("applicationUnavailable")?.sstv,
                );
            }
            Collection::Js8Context => {
                return Ok((Vec::new(), 0, self.js8.read(engine)?));
            }
            Collection::FieldDay => {
                return Ok((Vec::new(), 0, field_day::read_engine(engine)?));
            }
            Collection::Ota => {
                return Ok((
                    Vec::new(),
                    0,
                    ota::read_engine(engine, sources.ok_or("applicationUnavailable")?)?,
                ));
            }
            Collection::Memories => {
                return Ok((
                    Vec::new(),
                    0,
                    memories::read(&sources.ok_or("applicationUnavailable")?.memories)?,
                ));
            }
            Collection::Dxpeditions => {
                return Ok((
                    Vec::new(),
                    0,
                    dxpeditions::read_engine(
                        engine,
                        &sources.ok_or("applicationUnavailable")?.propagation,
                    )?,
                ));
            }
            Collection::Awards | Collection::Statistics => {
                return Ok((
                    Vec::new(),
                    0,
                    insights::read_engine(engine, request.collection)?,
                ));
            }
            Collection::Recall => {
                return recall::read_engine(engine, &request.search)?.encode();
            }
            Collection::Decodes => {
                return Ok(self
                    .journal
                    .try_lock()
                    .map_err(|_| "applicationBusy")?
                    .snapshot(request.after))
            }
            Collection::Log => {
                // The log's handles under the lock; the window (every contact tested, and with a
                // search five lowercased fields each) is read after it is released, from one
                // picture of the log.
                let rows = tempo_app::engine::engine_try_lock(engine)
                    .map_err(|_| "applicationBusy")?
                    .log_rows();
                let (rows, total) = picture::read(&rows, |log| {
                    log_window(log, &request.search, request.unconfirmed)
                })?;
                let rows = rows
                    .into_iter()
                    .map(|r| {
                        let mut q = tempo_app::dto::LoggedQso::from(r);
                        q.entity =
                            propagation::dxcc::resolve(&q.call).map(|i| i.entity.to_string());
                        serde_json::to_value(q).map_err(|_| "applicationUnavailable")
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                return Ok((
                    rows,
                    total,
                    json!({ "order": "newestFirst", "limit": 2000 }),
                ));
            }
            Collection::Entities => to_rows(
                serde_json::to_value(crate::dxcc_entity_locations())
                    .map_err(|_| "applicationUnavailable")?,
            )?,
            Collection::Health => {
                return Ok((
                    Vec::new(),
                    0,
                    serde_json::to_value(crate::read_feed_health(
                        &sources.ok_or("applicationUnavailable")?.health,
                    ))
                    .map_err(|_| "applicationUnavailable")?,
                ))
            }
            Collection::Spots => {
                let sources = sources.ok_or("applicationUnavailable")?;
                let rows = crate::read_all_spots(&sources.spots, engine, true)
                    .map_err(|_| "applicationBusy")?;
                let total = rows.len();
                let values = rows
                    .into_iter()
                    .take(MAX_ROWS)
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| "applicationUnavailable")?;
                return Ok((values, total, json!({ "unassisted": crate::unassisted() })));
            }
            Collection::Needs => {
                let sources = sources.ok_or("applicationUnavailable")?;
                let eng =
                    tempo_app::engine::engine_try_lock(engine).map_err(|_| "applicationBusy")?;
                let rows = crate::read_need_alerts(
                    eng,
                    &sources.needs,
                    &sources.live_paths,
                    &sources.region_paths,
                    &sources.spots,
                    &sources.ota,
                )
                .map_err(|_| "applicationUnavailable")?;
                let total = rows.len();
                let values = rows
                    .into_iter()
                    .take(MAX_ROWS)
                    .map(serde_json::to_value)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| "applicationUnavailable")?;
                return Ok((values, total, json!({ "unassisted": crate::unassisted() })));
            }
        };
        let total = rows.len();
        Ok((rows, total, json!({})))
    }
}

/// What the log window reads of every contact: the five fields a search looks in, the time the
/// window is ordered by, and the four confirmation channels `award_confirmed` is read from.
const WINDOW: tempo_core::logbook::sqlite::Narrow = tempo_core::logbook::sqlite::Narrow {
    columns: &[
        "call",
        "country",
        "grid",
        "band",
        "mode",
        "when_unix",
        "qsl_card_rcvd_raw",
        "lotw_rcvd_raw",
        "eqsl_rcvd_raw",
        "qrz_status_raw",
    ],
    uploads: false,
};

// Test the whole station log, retain only the newest matching window — newest by time, then
// later in the log — and read only those contacts whole, all from one picture of the log. No
// full log clone or serialization, and no positional write API.
fn log_window(
    log: &picture::Picture<'_>,
    search: &str,
    unconfirmed: bool,
) -> Result<(Vec<tempo_core::logbook::QsoRecord>, usize), &'static str> {
    let search = search.trim().to_lowercase();
    let mut selected = BinaryHeap::new();
    let mut total = 0;
    log.each(WINDOW, &mut |pick, q| {
        if unconfirmed && q.award_confirmed {
            return Ok(());
        }
        if !search.is_empty()
            && ![
                Some(q.call.as_str()),
                q.country.as_deref(),
                q.grid.as_deref(),
                Some(q.band.as_str()),
                Some(q.mode.as_str()),
            ]
            .into_iter()
            .flatten()
            .any(|s| s.to_lowercase().contains(&search))
        {
            return Ok(());
        }
        total += 1;
        selected.push(std::cmp::Reverse((q.when_unix, pick)));
        if selected.len() > 2000 {
            selected.pop();
        }
        Ok(())
    })?;
    let mut selected: Vec<_> = selected.into_iter().map(|v| v.0).collect();
    selected.sort_unstable_by(|a, b| b.cmp(a));
    let picks: Vec<_> = selected.into_iter().map(|(_, pick)| pick).collect();
    Ok((log.whole(&picks)?, total))
}

#[cfg(test)]
pub(super) fn configuration_probe(
    engine: &crate::SharedEngine,
) -> Result<serde_json::Value, &'static str> {
    Ok(
        serde_json::json!({"settings":configuration::build(Collection::Settings,engine)?,"programming":configuration::build(Collection::Programming,engine)?}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_service::stored_log_tests::StoredLog;
    const ID: &str = "10000000-0000-4000-8000-000000000001";
    fn request(collection: Collection) -> Request {
        Request {
            request_id: ID.into(),
            collection,
            cursor: None,
            search: String::new(),
            unconfirmed: false,
            after: None,
        }
    }
    fn engine() -> crate::SharedEngine {
        Arc::new(Mutex::new(tempo_app::engine::Engine::with_settings(
            Default::default(),
        )))
    }
    fn adif(call: &str, time: &str) -> String {
        format!(
            "<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260909<TIME_ON:6>{time}<EOR>\n",
            call.len()
        )
    }
    #[test]
    fn full_log_insights_are_separate_argument_free_reads() {
        for collection in ["awards", "statistics"] {
            let value = json!({ "requestId": ID, "collection": collection,
                "cursor": null, "search": "", "unconfirmed": false, "after": null });
            let query: Request =
                serde_json::from_value(value.clone()).expect("insight read supported");
            assert!(query.valid());
            for (field, bad) in [
                ("search", json!("W1AW")),
                ("unconfirmed", json!(true)),
                ("after", json!(1)),
                ("cursor", json!(format!("{ID}:1"))),
            ] {
                let mut invalid = value.clone();
                invalid[field] = bad;
                assert!(!serde_json::from_value::<Request>(invalid).unwrap().valid());
            }
            let result = Publisher::default()
                .read(&query, &engine(), None, Instant::now())
                .unwrap();
            let page: Value = serde_json::from_str(&result).unwrap();
            assert_eq!(page["collection"], collection);
            assert_eq!(page["total"], 0);
            assert_eq!(page["rows"], json!([]));
            assert_eq!(page["meta"]["source"]["logCount"], 0);
        }
    }
    #[test]
    fn pages_remain_sealed_across_local_log_changes_and_expire_without_guessing() {
        let engine = engine();
        let data: String = (0..270)
            .map(|i| adif(&format!("K1T{i:03}"), "010000"))
            .collect();
        engine.lock().unwrap().import_adif(&data);
        let before = engine.lock().unwrap().stored_records();
        let mut publisher = Publisher::default();
        let now = Instant::now();
        let mut req = request(Collection::Log);
        let first: Value =
            serde_json::from_str(&publisher.read(&req, &engine, None, now).unwrap()).unwrap();
        assert_eq!(first["rows"].as_array().unwrap().len(), 128);
        assert_eq!(first["total"], 270);
        assert_eq!(
            engine.lock().unwrap().stored_records(),
            before,
            "reading cannot change records, confirmations or connector state"
        );
        engine
            .lock()
            .unwrap()
            .import_adif(&adif("ZL1OLD", "000000"));
        req.cursor = first["nextCursor"].as_str().map(str::to_owned);
        let second: Value =
            serde_json::from_str(&publisher.read(&req, &engine, None, now).unwrap()).unwrap();
        assert_eq!(second["offset"], 128);
        assert_eq!(second["total"], 270);
        assert_ne!(first["rows"][0]["call"], second["rows"][0]["call"]);
        assert_eq!(
            publisher.read(&req, &engine, None, now + TTL),
            Err("queryExpired")
        );
        req.cursor = None;
        req.search = "zl1old".into();
        let found: Value =
            serde_json::from_str(&publisher.read(&req, &engine, None, now + TTL).unwrap()).unwrap();
        assert_eq!(found["total"], 1);
        assert_eq!(found["rows"][0]["call"], "ZL1OLD");
    }
    #[test]
    fn a_large_log_is_bounded_but_search_covers_contacts_outside_the_window() {
        let engine = engine();
        let data: String = (0..2300)
            .map(|i| adif(&format!("K1T{i:04}"), "010000"))
            .collect();
        engine
            .lock()
            .unwrap()
            .import_adif(&(adif("ZL1OLD", "000000") + &data));
        let log = engine.lock().unwrap().log_rows();
        let window = |search| picture::read(&log, |p| log_window(p, search, false)).unwrap();
        let (rows, total) = window("");
        assert_eq!(total, 2301);
        assert_eq!(rows.len(), 2000);
        assert!(!rows.iter().any(|q| q.call == "ZL1OLD"));
        let (rows, total) = window("zl1old");
        assert_eq!(total, 1);
        assert_eq!(rows[0].call, "ZL1OLD");
    }
    #[test]
    fn missing_feeds_and_a_busy_engine_are_not_empty_collections() {
        let engine = engine();
        let mut publisher = Publisher::default();
        assert_eq!(
            publisher.read(&request(Collection::Needs), &engine, None, Instant::now()),
            Err("applicationUnavailable")
        );
        let _held = engine.lock().unwrap();
        assert_eq!(
            publisher.read(&request(Collection::Log), &engine, None, Instant::now()),
            Err("applicationBusy")
        );
    }

    /// A REMOTE BROWSER IS SERVED THE STATION'S WATCHED ROWS (operator 2026-09-24: "watched
    /// counts as needed"). The desktop's watch list lives in the engine both boards read, so the
    /// browser's Needed list leads with the station's watched station — and stops the moment the
    /// desktop sends a list without it, whatever the browser's own watch list holds.
    #[test]
    fn a_remote_browser_is_served_the_stations_watched_rows() {
        use tempo_app::watchlist::{WatchEntry, WatchKind};
        use tempo_net::cluster::{ClusterSpot, SpotBuffer};
        let engine: crate::SharedEngine = Arc::new(Mutex::new(tempo_app::engine::Engine::new(
            "KD9TAW", "EN52", 0,
        )));
        let mut spots = SpotBuffer::new(8);
        for call in ["VK9XX", "W1AW"] {
            spots.push(ClusterSpot {
                spotter: "W3LPL".into(), // the operator's own continent — the locality gate
                dx_call: call.into(),
                freq_khz: 14_025.0,
                comment: "CW 18 dB".into(),
                time_utc: None,
                received_unix: crate::now_unix() as u64,
                corroborators: Vec::new(),
                rbn: false,
            });
        }
        let sources = Sources {
            needs: Default::default(),
            spots: Arc::new(Mutex::new(spots)),
            live_paths: Default::default(),
            region_paths: crate::SharedRegionPaths(Default::default()),
            ota: Default::default(),
            health: Default::default(),
            propagation: Default::default(),
            memories: Default::default(),
            parks: Default::default(),
            pounces: Default::default(),
            sstv: Default::default(),
            navigation: Default::default(),
        };
        let rows = |publisher: &mut Publisher| {
            let page: Value = serde_json::from_str(
                &publisher
                    .read(
                        &request(Collection::Needs),
                        &engine,
                        Some(&sources),
                        Instant::now(),
                    )
                    .unwrap(),
            )
            .unwrap();
            page["rows"].as_array().unwrap().clone()
        };
        engine.lock().unwrap().set_watch_list(vec![WatchEntry {
            kind: WatchKind::Call,
            value: "W1AW".into(),
        }]);
        let watched = rows(&mut Publisher::default());
        assert_eq!(watched[0]["call"], "W1AW", "{watched:?}");
        assert_eq!(watched[0]["tags"][0], "Wanted");
        assert!(watched.iter().any(|r| r["call"] == "VK9XX"), "{watched:?}");

        engine.lock().unwrap().set_watch_list(Vec::new());
        let unwatched = rows(&mut Publisher::default());
        assert!(
            unwatched
                .iter()
                .all(|r| !r["tags"].as_array().unwrap().contains(&json!("Wanted"))),
            "{unwatched:?}"
        );
    }

    // ── the Log collection from the store, held to the code before C18 ───────────────────
    //
    // SPEC-2 v3 C18: the window is read from one picture of the logbook store now, and must be
    // the window the log in memory gave, row for row and byte for byte. The oracle is the code
    // before C18, VERBATIM, reading the log in memory beside the store it mirrors.

    use super::log_tests::{launch, memory, settle, synthetic_log, Dir, Gen, CALLS};

    /// `log_window` as C12 left it, VERBATIM.
    fn old_log_window<R: std::borrow::Borrow<tempo_core::logbook::QsoRecord>>(
        records: &[R],
        search: &str,
        unconfirmed: bool,
    ) -> (Vec<tempo_core::logbook::QsoRecord>, usize) {
        let search = search.trim().to_lowercase();
        let mut selected = BinaryHeap::new();
        let mut total = 0;
        for (i, q) in records.iter().enumerate() {
            let q: &tempo_core::logbook::QsoRecord = std::borrow::Borrow::borrow(q);
            if unconfirmed && q.award_confirmed {
                continue;
            }
            if !search.is_empty()
                && ![
                    Some(q.call.as_str()),
                    q.country.as_deref(),
                    q.grid.as_deref(),
                    Some(q.band.as_str()),
                    Some(q.mode.as_str()),
                ]
                .into_iter()
                .flatten()
                .any(|s| s.to_lowercase().contains(&search))
            {
                continue;
            }
            total += 1;
            selected.push(std::cmp::Reverse((q.when_unix, i)));
            if selected.len() > 2000 {
                selected.pop();
            }
        }
        let mut selected: Vec<_> = selected.into_iter().map(|v| v.0).collect();
        selected.sort_unstable_by(|a, b| b.cmp(a));
        (
            selected
                .into_iter()
                .map(|(_, i)| {
                    std::borrow::Borrow::<tempo_core::logbook::QsoRecord>::borrow(&records[i])
                        .clone()
                })
                .collect(),
            total,
        )
    }

    /// The Log collection's arm of `Publisher::capture` as C12 left it, VERBATIM.
    fn old_log_capture(
        engine: &crate::SharedEngine,
        search: &str,
        unconfirmed: bool,
    ) -> Result<(Vec<Value>, usize, Value), &'static str> {
        // The log as the store holds it ([`StoredLog`]), in place of the copy in memory: these
        // tests hold the old window against the new over the same rows, and whether the store
        // holds what the old write path wrote (P6) is the Stage-1 lockstep suite's job.
        let records = engine
            .lock()
            .map_err(|_| "applicationUnavailable")?
            .stored_log();
        tempo_core::logbook::io_fence::whole_log_off_engine_lock("Remote's log window");
        let (rows, total) = old_log_window(&records, search, unconfirmed);
        let rows = rows
            .into_iter()
            .map(|r| {
                let mut q = tempo_app::dto::LoggedQso::from(r);
                q.entity = propagation::dxcc::resolve(&q.call).map(|i| i.entity.to_string());
                serde_json::to_value(q).map_err(|_| "applicationUnavailable")
            })
            .collect::<Result<Vec<_>, _>>()?;
        Ok((
            rows,
            total,
            json!({ "order": "newestFirst", "limit": 2000 }),
        ))
    }

    /// The Log collection's capture as the station makes it now.
    fn log_capture(
        engine: &crate::SharedEngine,
        search: &str,
        unconfirmed: bool,
    ) -> Result<(Vec<Value>, usize, Value), &'static str> {
        let mut req = request(Collection::Log);
        req.search = search.into();
        req.unconfirmed = unconfirmed;
        Publisher::default().capture(&req, engine, None)
    }

    /// A capture as the bytes a browser is sent of it.
    fn bytes(capture: Result<(Vec<Value>, usize, Value), &'static str>) -> String {
        match capture {
            Ok((rows, total, meta)) => {
                json!({ "rows": rows, "total": total, "meta": meta }).to_string()
            }
            Err(e) => format!("refused: {e}"),
        }
    }

    /// What the page searches for: empty, a call in several spellings, a country, text outside
    /// ASCII (`ſ` and `ı` fold INTO ASCII upper case; `ö` only lower-cases), a band, a mode, a
    /// grid, "m" (nearly every band: more than 2,000 matches), and "z", which a date's trailing Z
    /// makes match everything on the desktop's Logbook (not here: the window looks in five fields
    /// and never the date).
    const SEARCHES: &[&str] = &[
        "",
        "w1aw",
        " W1AW ",
        "k1",
        "germany",
        "Österreich",
        "ö",
        "ſ",
        "s",
        "ı",
        "i",
        "é",
        "curaçao",
        "m",
        "z",
        "20m",
        "2m",
        "odd",
        "ft8",
        "usb",
        "ssb",
        "jn",
        "ab",
        "q0zzz",
        "alaska",
    ];

    fn assert_the_window_is_the_old_window(e: &crate::SharedEngine, searches: &[&str], what: &str) {
        for &search in searches {
            for unconfirmed in [false, true] {
                let (old, new) = (
                    bytes(old_log_capture(e, search, unconfirmed)),
                    bytes(log_capture(e, search, unconfirmed)),
                );
                assert!(
                    new == old,
                    "{what}: the window for {search:?} (unconfirmed {unconfirmed}) differs\n\
                     store:  {:.400}\nmemory: {:.400}",
                    new,
                    old
                );
            }
        }
    }

    /// ★ PARITY ON A FIXTURE: 3,000 contacts, many to a second, calls, countries and notes
    /// outside ASCII, confirmations of every channel — every search the page makes, with and
    /// without "unconfirmed", read from the store and from the 1.13 path, byte for byte the
    /// window of the log in memory. Several searches match more than 2,000 contacts.
    #[test]
    fn the_log_window_read_from_the_store_is_the_window_of_the_log_in_memory() {
        let text = synthetic_log(3_000, 0xC18A_0001);
        let d = Dir::new("window");
        std::fs::write(d.log(), &text).unwrap();
        let store = launch(&d);
        let (_, total, _) = old_log_capture(&store, "", false).unwrap();
        assert_eq!(total, 3_000, "premise: every contact converted");
        let (rows, total, _) = old_log_capture(&store, "m", false).unwrap();
        assert!(
            total > 2_000 && rows.len() == 2_000,
            "premise: {total} match 'm'"
        );
        let (_, unconfirmed, _) = old_log_capture(&store, "", true).unwrap();
        assert!(
            (1..3_000).contains(&unconfirmed),
            "premise: {unconfirmed} unconfirmed"
        );
        let (rows, _, _) = old_log_capture(&store, "ö", false).unwrap();
        assert!(!rows.is_empty(), "premise: text outside ASCII is found");
        let (rows, _, _) = old_log_capture(&store, "ſ", false).unwrap();
        assert!(!rows.is_empty(), "premise: a call outside ASCII is found");
        assert!(
            rows.iter()
                .any(|r| !r["extra"].as_array().unwrap().is_empty()),
            "premise: rows carry what only a whole record holds"
        );
        assert_the_window_is_the_old_window(&store, SEARCHES, "the store");
        assert_the_window_is_the_old_window(&memory(&d, &text), SEARCHES, "the 1.13 path");
        settle(&store);
    }

    /// ★ TIES: 2,500 contacts in ONE second. The window is the newest 2,000 by time and then by
    /// place in the log — so the LAST 2,000 logged, the last one first — and the cut falls
    /// inside the tie, exactly where the log in memory put it.
    #[test]
    fn a_tie_across_the_cut_is_ordered_and_cut_by_place_in_the_log() {
        let text: String = (0..2_500)
            .map(|i| {
                let call = format!("K{}T{:04}", i % 10, i);
                format!(
                    "<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260909<TIME_ON:6>120000<EOR>\n",
                    call.len()
                )
            })
            .collect();
        let d = Dir::new("ties");
        std::fs::write(d.log(), &text).unwrap();
        let store = launch(&d);
        let (rows, total, _) = log_capture(&store, "", false).unwrap();
        assert_eq!((total, rows.len()), (2_500, 2_000));
        let calls: Vec<&str> = rows.iter().map(|r| r["call"].as_str().unwrap()).collect();
        assert_eq!(calls[0], "K9T2499", "the last logged first");
        assert_eq!(
            calls[1_999], "K0T0500",
            "and the cut inside the tie, by place in the log"
        );
        assert_the_window_is_the_old_window(&store, &["", "t0", "k1"], "ties");
        settle(&store);
    }

    /// ★ THE PROPERTY: over twelve seeded logs and 24 random changes each — logged contacts,
    /// imports, edits that move a contact in time, change its call, band, grid or country,
    /// deletes, QSL cards and marks, satellite tags, stamps, LoTW credit and the fill job —
    /// after EVERY change the window read from the store is the window of the log in memory.
    #[test]
    fn after_every_change_the_log_window_read_from_the_store_is_the_old_window() {
        for seed in 1..=12u64 {
            let d = Dir::new(&format!("window-prop-{seed}"));
            std::fs::write(d.log(), synthetic_log(240, seed * 7_919)).unwrap();
            let e = launch(&d);
            let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            for step in 0..24u64 {
                super::log_tests::random_change(&e, &mut g, step);
                let at = (step as usize * 5) % SEARCHES.len();
                let searches: Vec<&str> =
                    SEARCHES.iter().cycle().skip(at).take(5).copied().collect();
                assert_the_window_is_the_old_window(
                    &e,
                    &searches,
                    &format!("seed {seed}, step {step}"),
                );
            }
            settle(&e);
        }
    }

    /// ★ AT 150,000: the window of a lifetime log, read from the store, is the window of the log
    /// in memory.
    #[test]
    fn at_150k_the_log_window_read_from_the_store_is_the_old_window() {
        let d = Dir::new("window-150k");
        std::fs::write(d.log(), synthetic_log(150_000, 0xC18A_0150)).unwrap();
        let store = launch(&d);
        let (_, total, _) = old_log_capture(&store, "", false).unwrap();
        assert_eq!(total, 150_000, "premise: every contact converted");
        assert_the_window_is_the_old_window(&store, &["", "w1aw", "ö", "20m"], "150k");
        settle(&store);
    }

    /// ★ ONE PICTURE: the window's two newest contacts, picked by its pass, are edited out of
    /// the window and deleted — by the operator, committed to the store (on the 1.13 path, made:
    /// its store in memory commits them once the read ends) — before the pass's whole records
    /// are read. They are still the contacts the pass picked: the window is the log as the read
    /// found it, every row of it, on the store and on the 1.13 path. The next read has both
    /// changes.
    #[test]
    fn a_contact_edited_or_deleted_between_the_pass_and_the_whole_read_is_the_one_picked() {
        use super::picture::{at_seams, Seam};
        let text = synthetic_log(400, 0x00C1_8AB7);
        let d = Dir::new("between");
        std::fs::write(d.log(), &text).unwrap();
        for (arm, e) in [("store", launch(&d)), ("1.13 path", memory(&d, &text))] {
            let before = bytes(old_log_capture(&e, "", false));
            let (rows, _, _) = old_log_capture(&e, "", false).unwrap();
            let place = |id: &Value| -> tempo_core::logbook::RecordId {
                let id = id.as_str().expect("a row's id").parse().expect("an id");
                assert!(
                    e.lock().unwrap().logged_row(id).is_some(),
                    "the window's row is in the log"
                );
                id
            };
            let (edit, delete) = (place(&rows[0]["id"]), place(&rows[1]["id"]));
            let (hook, changes) = (e.clone(), std::rc::Rc::new(std::cell::Cell::new(0)));
            let counted = changes.clone();
            let during = bytes(at_seams(
                move |seam| {
                    if seam != Seam::Whole {
                        return;
                    }
                    let mut eng = hook.lock().unwrap();
                    let mut r =
                        tempo_core::logbook::QsoRecord::clone(&eng.logged_row(edit).expect("held"));
                    r.call = "ZZ9ZZZ".into();
                    r.when_unix = 1_000_000_000;
                    assert!(eng.update_qso(edit, r));
                    eng.delete_qso(delete);
                    // On the store they commit while the read runs, beside its snapshot. The 1.13
                    // path's store is in memory, where a read holds off every commit until it
                    // ends: there they commit after it, and the next read waits for them.
                    if arm == "store" {
                        eng.flush_log_store(std::time::Duration::from_secs(60))
                            .expect("committed to the store");
                    }
                    counted.set(counted.get() + 1);
                },
                || log_capture(&e, "", false),
            ));
            assert_eq!(
                changes.get(),
                1,
                "{arm}: premise: the changes landed mid-read"
            );
            assert!(
                during == before,
                "{arm}: the window is the log as the read found it\n{during:.300}\n{before:.300}"
            );
            let after = bytes(log_capture(&e, "", false));
            assert!(
                after != before,
                "{arm}: premise: the changes moved the window"
            );
            assert_eq!(
                after,
                bytes(old_log_capture(&e, "", false)),
                "{arm}: the next read"
            );
            settle(&e);
        }
    }

    /// ★ THE KEYS STILL MATCH. A browser names the row it changes by the bytes of the row it was
    /// served, and the change path finds the contact whose own row has those bytes — in the
    /// store, then checked under the Engine lock (`operations::logging::{seen_target, find,
    /// locate}`). Every row of a window read from the store is found, as the contact it was: a
    /// change made from a page served out of the store reaches the contact the page showed.
    #[test]
    fn every_row_served_from_the_store_is_found_by_the_change_path_at_its_own_place() {
        use crate::remote_service::operations::logging::{find, locate, seen_target};
        let d = Dir::new("keys");
        std::fs::write(d.log(), synthetic_log(2_500, 0x0C18_A4E7)).unwrap();
        let store = launch(&d);
        let (rows, _, _) = log_capture(&store, "", false).unwrap();
        assert_eq!(rows.len(), 2_000, "premise: a full window");
        let log = store.lock().unwrap().log_rows();
        for row in &rows {
            let seen: tempo_app::dto::LoggedQso = serde_json::from_value(row.clone()).unwrap();
            assert_eq!(
                serde_json::to_value(&seen).unwrap(),
                *row,
                "premise: the row a browser holds is the row it was sent"
            );
            let found = find(&log, &seen_target(&seen))
                .expect("the log reads")
                .unwrap_or_else(|| panic!("the change path finds the row it served: {row}"));
            let at = locate(&store.lock().unwrap(), &found)
                .unwrap_or_else(|| panic!("and it is still that contact: {row}"));
            assert_eq!(Some(at.to_string()), seen.id, "the contact it served");
        }
        settle(&store);
    }

    /// The window's calls outside ASCII are the log's own: every one the generator makes that
    /// the log holds is found by a search for itself, on the store as in memory.
    #[test]
    fn a_search_for_a_call_outside_ascii_finds_it_on_the_store() {
        let text = synthetic_log(600, 0x0C18_AA5C);
        let d = Dir::new("non-ascii");
        std::fs::write(d.log(), &text).unwrap();
        let store = launch(&d);
        let odd: Vec<&str> = CALLS.iter().copied().filter(|c| !c.is_ascii()).collect();
        assert!(odd.len() >= 3, "premise: calls outside ASCII");
        assert_the_window_is_the_old_window(&store, &odd, "calls outside ASCII");
        let found = odd
            .iter()
            .filter(|c| log_capture(&store, c, false).unwrap().1 > 0)
            .count();
        assert!(found >= 3, "premise: {found} of them are in the log");
        settle(&store);
    }

    #[test]
    fn journal_metadata_updates_keep_the_original_identity_and_time() {
        let mut journal = Journal::default();
        let mut snapshot = json!({ "radio": {"band":"20m", "slot":1}, "link":{"tier":"FT8"},
            "recentDecodes":[{"message":"CQ W1AW FN31", "freqHz":1000.0, "worked":false}] });
        journal.observe(&snapshot);
        let original = journal.snapshot(None).0[0].clone();
        snapshot["recentDecodes"][0]["worked"] = json!(true);
        journal.observe(&snapshot);
        let changed = journal.snapshot(Some(1)).0;
        assert_eq!(changed.len(), 1);
        assert_eq!(changed[0]["sequence"], 2);
        assert_eq!(changed[0]["firstSequence"], original["firstSequence"]);
        assert_eq!(changed[0]["at"], original["at"]);
        let mut next_session = Journal::default();
        next_session.observe(&snapshot);
        assert_ne!(
            journal.snapshot(None).2["generation"],
            next_session.snapshot(None).2["generation"]
        );
    }
    #[test]
    fn journal_dedupes_samples_orders_new_rows_and_resets_context_without_touching_ft() {
        let mut journal = Journal::default();
        let mut snapshot = json!({ "radio": {"band":"20m", "slot":1}, "link":{"tier":"FT8"},
            "recentDecodes":[{"message":"CQ W1AW FN31", "freqHz":1000.0, "mine":false}] });
        journal.observe(&snapshot);
        journal.observe(&snapshot);
        assert_eq!(journal.snapshot(None).0.len(), 1);
        assert!(journal.snapshot(Some(1)).0.is_empty());
        snapshot["radio"]["slot"] = json!(2);
        journal.observe(&snapshot);
        assert_eq!(journal.snapshot(Some(1)).0[0]["slot"], 2);
        snapshot["radio"]["band"] = json!("");
        journal.observe(&snapshot);
        assert_eq!(
            journal.snapshot(None).0.len(),
            2,
            "off-band excursions preserve the named band"
        );
        snapshot["radio"]["band"] = json!("40m");
        journal.observe(&snapshot);
        assert_eq!(journal.snapshot(None).0.len(), 1);
        assert_eq!(journal.snapshot(None).2["band"], "40m");
        for slot in 3..3010 {
            snapshot["radio"]["slot"] = json!(slot);
            journal.observe(&snapshot);
        }
        assert_eq!(journal.snapshot(None).0.len(), MAX_ROWS);
        assert!(journal.snapshot(None).2["dropped"].as_u64().unwrap() > 0);
    }
}
