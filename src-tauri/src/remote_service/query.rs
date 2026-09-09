//! Bounded, immutable collection snapshots. Runs on a blocking worker, never on
//! the socket/live-instrument loop. This module has no file, vault or TX access.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BinaryHeap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
mod recall;

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
            } else {
                self.collection == Collection::Log || (self.search.is_empty() && !self.unconfirmed)
            })
            && self.after.is_none_or(|n| {
                self.collection == Collection::Decodes && n <= 9_007_199_254_740_991
            })
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
fn snapshot_id() -> Result<String, &'static str> {
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
    pub spots: crate::SharedSpots,
    pub live_paths: crate::SharedLivePaths,
    pub region_paths: crate::SharedRegionPaths,
    pub ota: crate::SharedOtaSpots,
    pub health: crate::SharedHealth,
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
        let unassisted = crate::unassisted();
        if self.unassisted != unassisted {
            self.snapshots
                .retain(|s| !matches!(s.collection, Collection::Needs | Collection::Spots));
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
            let reuse = if request.collection == Collection::Recall {
                0 // Explicit selection/Refresh must see intervening local log changes.
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
        if offset > 0 && offset >= s.rows.len() {
            return Err("queryExpired");
        }
        let mut end = (offset + 128).min(s.rows.len());
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
        &self,
        request: &Request,
        engine: &crate::SharedEngine,
        sources: Option<&Sources>,
    ) -> Result<(Vec<Value>, usize, Value), &'static str> {
        let to_rows = |v| match v {
            Value::Array(a) => Ok(a),
            _ => Err("applicationUnavailable"),
        };
        let rows = match request.collection {
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
                let eng = engine.try_lock().map_err(|_| "applicationBusy")?;
                let (rows, total) =
                    log_window(eng.log_records(), &request.search, request.unconfirmed);
                drop(eng);
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
                let eng = engine.try_lock().map_err(|_| "applicationBusy")?;
                let rows = crate::read_need_alerts(
                    eng,
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

// Scan the whole station log, retain only the newest matching window. No full
// log clone or serialization under the engine lock, and no positional write API.
fn log_window(
    records: &[tempo_core::logbook::QsoRecord],
    search: &str,
    unconfirmed: bool,
) -> (Vec<tempo_core::logbook::QsoRecord>, usize) {
    let search = search.trim().to_lowercase();
    let mut selected = BinaryHeap::new();
    let mut total = 0;
    for (i, q) in records.iter().enumerate() {
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
            .map(|(_, i)| records[i].clone())
            .collect(),
        total,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn pages_remain_sealed_across_local_log_changes_and_expire_without_guessing() {
        let engine = engine();
        let data: String = (0..270)
            .map(|i| adif(&format!("K1T{i:03}"), "010000"))
            .collect();
        engine.lock().unwrap().import_adif(&data);
        let before = engine.lock().unwrap().get_log();
        let mut publisher = Publisher::default();
        let now = Instant::now();
        let mut req = request(Collection::Log);
        let first: Value =
            serde_json::from_str(&publisher.read(&req, &engine, None, now).unwrap()).unwrap();
        assert_eq!(first["rows"].as_array().unwrap().len(), 128);
        assert_eq!(first["total"], 270);
        assert_eq!(
            engine.lock().unwrap().get_log(),
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
        let eng = engine.lock().unwrap();
        let (rows, total) = log_window(eng.log_records(), "", false);
        assert_eq!(total, 2301);
        assert_eq!(rows.len(), 2000);
        assert!(!rows.iter().any(|q| q.call == "ZL1OLD"));
        let (rows, total) = log_window(eng.log_records(), "zl1old", false);
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
