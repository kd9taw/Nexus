//! The UI's log questions, answered by the engine — SPEC-2 v3's **C17a** (§4.3), the engine half
//! of C17b's `LogSource` (`ui/src/features/logAnswers.ts`; the window's half is
//! `askingLogSource.ts`, which calls [`ask_log`] once per question).
//!
//! The statistics are answered COUNTED, in first-seen order (`LogStatCounts`), and ordered in the
//! window (`finishLogStats`, in `askLog`): the dashboard breaks ties with `localeCompare`, in the
//! webview's own locale, which nothing here can reproduce.
//!
//! A window used to hold the whole log and compute what it showed from it. It now asks for what it
//! shows — a page of the Logbook, where a row sits in it, one call's history, an entity's slots, a
//! roster's summary, the band map's worked calls — and holds only the answers. Every answer is the
//! one the UI computed (`tempo_core::logbook::query`, held to C17b's goldens); what changed is
//! where the rows come from.
//!
//! # Where each answer comes from
//!
//! | question | source |
//! |---|---|
//! | a page, where a row sits | an **order vector** — the rows' rowids in display order — kept per `index_rev` and query (LRU of [`ORDERS_KEPT`]); the default order straight off the `qso_recent` index, any other from one narrow pass in log order; then the page's rows by rowid |
//! | one call's history, a roster's summary | the `qso_callhist` index for the call, then the UI's own test on each row (SPEC-2 v3 P2: SQL narrows, Rust decides) |
//! | the band map's worked calls | the hot index's worked-before calls (C13) — under the Engine lock, no read at all |
//! | an entity's slots | an index of every entity's slots, kept per `index_rev` and grown row by row after an append |
//! | the squares worked, the Logbook globe's dots and bands, the statistics, the LoTW backlog | one narrow pass over the log in log order ([`LogRows`], C14's door), kept against the watermark of what each reads |
//!
//! A call spelled with a character outside ASCII is folded one way by the store and the hot index
//! (ASCII only) and another by the UI (all of Unicode: `ſ` → `S`). The hot index names every such
//! call ([`Engine::log_odd_call_keys`]), and those calls' rows are read and decided one by one, so
//! the answer is still the UI's.
//!
//! # Three rules every answer keeps
//!
//! 1. **Never a read under the Engine lock.** Under it an answer takes only watermarks and the
//!    log's handles (on the 1.13 path, a copy of its pointers); it reads after releasing it. A
//!    debug build panics if a read runs with an Engine guard held
//!    (`tempo_core::logbook::io_fence`).
//! 2. **Read your writes (P4).** A read first waits — bounded, off every lock — for every change
//!    made before the question was asked, so a Logbook asked straight after a contact shows it.
//!    A read that ran out of wait answers from the store as it stands, and is not kept.
//! 3. **Revisions name what the answer was cut from.** A page carries `revision` (the log's),
//!    `orderRev` (the `index_rev` its order was built at) and `contentRev` (the `content_rev` its
//!    rows were read at). An upload stamp or a QSL-sent mark moves `content_rev` and not
//!    `index_rev`, so the order survives it and only the rows are read again.

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use tempo_app::dto::LoggedQso;
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::logstore::{Freshness, LogRows, StoreReads};
use tempo_core::logbook::query::{
    self, BandsInLog, CallSummary, EntityIndex, GridCount, GridCounts, LogFold, LogQuery,
    LogStatCounter, LogStatCounts, LotwBacklog, OrderBuilder, WorkedGrids,
};
use tempo_core::logbook::sqlite::{self, Narrow, Order, Scope, ENTITY_COLUMNS, ORDER_COLUMNS};
use tempo_core::logbook::{QsoEdit, QsoRecord, RecordId};

use crate::{SharedEngine, Tally};

/// Order vectors kept (SPEC-2 v2 §2: an LRU of four). One is a log-sized `Vec<u32>`.
pub(crate) const ORDERS_KEPT: usize = 4;

/// How long a read waits for the writer to take the changes made before it was asked (P4).
const READ_WAIT: Duration = Duration::from_secs(2);

// ── what each fold reads ──────────────────────────────────────────────────────────────────────
//
// The four confirmation channels ride with any fold that reads `confirmed` or `award_confirmed`:
// the decoder derives both from them, exactly as it does for a whole record.

/// The squares worked (`workedGrids`).
const GRIDS: Narrow = Narrow {
    columns: &["grid"],
    uploads: false,
};

/// The Logbook globe's dots (`gridPoints`).
const GRID_POINTS: Narrow = Narrow {
    columns: &["band", "grid", "when_unix"],
    uploads: false,
};

/// The bands in the log (`bandsInLog`).
const BANDS: Narrow = Narrow {
    columns: &["band"],
    uploads: false,
};

/// The LoTW backlog (`lotwBacklog`): award confirmation, the LoTW stamp, the time of day.
const BACKLOG: Narrow = Narrow {
    columns: &[
        "time_known",
        "qsl_card_rcvd_raw",
        "lotw_rcvd_raw",
        "eqsl_rcvd_raw",
        "qrz_status_raw",
    ],
    uploads: true,
};

/// The statistics' counts (`statistics`).
const STATISTICS: Narrow = Narrow {
    columns: &[
        "call",
        "country",
        "state",
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

/// Everything the UI asks of the log (`LogQuestion`).
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum LogQuestion {
    CallHistory {
        call: String,
        band: String,
        mode: String,
        match_mode: bool,
    },
    Entity {
        entity: String,
    },
    CallsSummary {
        calls: Vec<String>,
    },
    WorkedCalls {
        calls: Vec<String>,
    },
    // Struct variants with no fields rather than unit variants: serde checks an internally tagged
    // unit variant for unknown fields not at all, and a question it did not understand must be
    // refused.
    WorkedGrids {},
    GridPoints {
        band: String,
    },
    BandsInLog {},
    Statistics {},
    LotwBacklog {},
    LogSize {},
    /// Rows by LOG POSITION — the Awards diagnosis's addressing, until each diagnosis carries its
    /// contact's id (it now does: `QsoDiagnosisDto::id`). JavaScript numbers.
    RowsAt {
        indices: Vec<f64>,
    },
    Row {
        id: String,
    },
    Page {
        query: LogQuery,
        offset: usize,
        limit: usize,
    },
    Locate {
        query: LogQuery,
        id: String,
    },
}

/// One page of the Logbook list (`LogPage`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PageAnswer {
    query: LogQuery,
    revision: u64,
    order_rev: u64,
    content_rev: u64,
    total: usize,
    log_size: u64,
    offset: usize,
    rows: Vec<LoggedQso>,
    keys: Vec<String>,
    /// Each row's edit key ([`QsoEdit::key`]): what a change to the row sends back with its id
    /// (`RowRef`, [`crate::log_by_id`]). Computed here and never in the UI.
    edit_keys: Vec<String>,
}

/// Where a row sits in an order (`LogLocate`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocateAnswer {
    query: LogQuery,
    order_rev: u64,
    index: Option<usize>,
}

/// One call's history (`CallHistory`), its contacts as rows.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CallHistoryAnswer {
    qsos: Vec<LoggedQso>,
    count: usize,
    worked_before: bool,
    dupe_this_band: bool,
    last_unix: Option<u64>,
    confirmed_count: usize,
    bands: Vec<String>,
    modes: Vec<String>,
}

/// What an answer takes under the Engine lock: the log's rows ([`LogRows`], C14's one door —
/// the store's handles, or on the 1.13 path a copy of the pointers of the log in memory) and the
/// watermarks they are read against. It reads nothing.
struct Capture {
    rows: LogRows,
    revision: u64,
    index_rev: u64,
    content_rev: u64,
}

impl Capture {
    fn take(eng: &Engine) -> Capture {
        Capture {
            rows: eng.log_rows(),
            revision: eng.log_revision(),
            index_rev: eng.log_index_rev(),
            content_rev: eng.log_content_rev(),
        }
    }

    fn is_store(&self) -> bool {
        matches!(self.rows, LogRows::Store(_))
    }
}

/// A read of the store, waited for (P4) and fenced off the Engine lock by [`StoreReads`].
fn read<T>(
    reads: &StoreReads,
    f: impl FnOnce(&sqlite::LogDb) -> sqlite::Result<T>,
) -> Result<(T, Freshness), String> {
    reads.read(READ_WAIT, f).map_err(unreadable)
}

/// What an answer says when the store could not be read. Never an empty answer, which would
/// call every station on the air a new one.
fn unreadable(e: sqlite::Error) -> String {
    format!("the logbook could not be read: {e}")
}

/// A pass over the log in memory (the 1.13 path) runs with no Engine guard held.
fn off_lock(work: &str) {
    tempo_core::logbook::io_fence::whole_log_off_engine_lock(work);
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A row as the UI reads it: [`LoggedQso`] with its live cty.dat entity — the conversion every
/// row the UI is handed gets ([`crate::log_by_id`]'s answers too).
fn logged(r: QsoRecord, resolve: &dyn Fn(&str) -> Option<String>) -> LoggedQso {
    let mut q = LoggedQso::from(r);
    q.entity = resolve(&q.call);
    q
}

/// A row's key in a page: its id. A row the store holds always has one; a row that somehow
/// does not is named by its handle, which no other row can share.
fn key_of(r: &QsoRecord, handle: u32) -> String {
    r.id.map_or_else(|| format!("#{handle}"), |id| id.to_string())
}

/// A JavaScript array index: an integer in range, or nothing (`log[-1]`, `log[1.5]`).
fn position(i: f64, len: usize) -> Option<usize> {
    (i.fract() == 0.0 && i >= 0.0 && i < len as f64).then_some(i as usize)
}

fn json<T: Serialize>(answer: T) -> Result<Value, String> {
    serde_json::to_value(answer).map_err(|e| format!("the answer could not be sent: {e}"))
}

/// An order vector's name: the query, at the `index_rev` it holds for. Revisions come from one
/// counter for the whole process, so a log replaced by another never meets its predecessor's.
#[derive(Debug, Clone, PartialEq, Eq)]
struct OrderKey {
    index_rev: u64,
    query: LogQuery,
}

/// An order vector kept, or on its way: the first asker builds it and any other waits for that
/// build rather than making its own. `None` once built means the build was not kept (a stale
/// read, a failure), and the slot is dropped.
#[derive(Default)]
struct OrderSlot(OnceLock<Option<Arc<Vec<u32>>>>);

/// Every entity's slots, kept against `index_rev` — and against the log as it stood at
/// `revision`, so that when every change since was an append it is grown by the rows past
/// `after` rather than built again.
struct EntityCache {
    revision: u64,
    index_rev: u64,
    store: bool,
    /// The last rowid (store) or the count of rows (1.13 path) counted.
    after: u32,
    index: EntityIndex,
}

/// The worked-before calls with a character outside ASCII, kept against `key_rev`: their keys
/// as the hot index holds them, and each as the UI upper-cases it.
struct OddCalls {
    keys: Vec<String>,
    upper: HashSet<String>,
}

#[derive(Default)]
struct Inner {
    orders: Mutex<Vec<(OrderKey, Arc<OrderSlot>)>>,
    log_order: Mutex<Option<(u64, Arc<Vec<u32>>)>>,
    entities: Mutex<Option<Arc<EntityCache>>>,
    odd: Mutex<Option<(u64, Arc<OddCalls>)>>,
    grids: Tally<(), Vec<String>>,
    grid_points: Tally<String, Vec<GridCount>>,
    bands: Tally<(), Vec<String>>,
    backlog: Tally<(), LotwBacklog>,
    statistics: Tally<(), LogStatCounts>,
    #[cfg(test)]
    built: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    folded: std::sync::atomic::AtomicUsize,
}

/// The answers' kept state — the order vectors, the log-order vector, the entity index, the odd
/// calls — shared by every window's questions. Managed by the app; a test makes its own.
#[derive(Clone, Default)]
pub struct LogQueries(Arc<Inner>);

impl LogQueries {
    /// Answer `q`: take what it needs under the Engine lock, release it, read, answer. `resolve`
    /// is a call's live DXCC entity (cty.dat in the app).
    pub(crate) fn answer(
        &self,
        engine: &SharedEngine,
        q: &LogQuestion,
        resolve: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Value, String> {
        match q {
            LogQuestion::WorkedCalls { calls } => json(self.worked_calls(engine, calls)),
            LogQuestion::CallHistory {
                call,
                band,
                mode,
                match_mode,
            } => {
                let (c, keys) = self.call_capture(engine, std::slice::from_ref(call));
                let rows = self.call_rows(&c, &keys)?;
                let h = query::call_history(&rows, call, band, mode, *match_mode);
                json(CallHistoryAnswer {
                    qsos: h
                        .qsos
                        .iter()
                        .map(|&i| logged(rows[i].clone(), resolve))
                        .collect(),
                    count: h.count,
                    worked_before: h.worked_before,
                    dupe_this_band: h.dupe_this_band,
                    last_unix: h.last_unix,
                    confirmed_count: h.confirmed_count,
                    bands: h.bands,
                    modes: h.modes,
                })
            }
            LogQuestion::CallsSummary { calls } => {
                let (c, keys) = self.call_capture(engine, calls);
                let rows = self.call_rows(&c, &keys)?;
                let summary: serde_json::Map<String, Value> = query::calls_summary(&rows, calls)
                    .into_iter()
                    .map(|(call, s): (String, CallSummary)| {
                        serde_json::to_value(s).map(|v| (call, v))
                    })
                    .collect::<Result<_, _>>()
                    .map_err(|e| format!("the answer could not be sent: {e}"))?;
                Ok(Value::Object(summary))
            }
            LogQuestion::Entity { entity } => {
                let (c, cached, appended) = {
                    let eng = engine_lock(engine);
                    let cached = lock(&self.0.entities).clone();
                    let appended = cached
                        .as_ref()
                        .is_some_and(|k| eng.log_appended_only_since(k.revision));
                    (Capture::take(&eng), cached, appended)
                };
                let index = self.entity_index(&c, cached, appended, resolve)?;
                json(index.index.answer(entity))
            }
            // A grid, a band or a time moves `index_rev`; an upload stamp does not, and none of
            // these reads one.
            LogQuestion::WorkedGrids {} => json(&*self.fold(
                engine,
                &self.0.grids,
                (),
                Engine::log_index_rev,
                GRIDS,
                WorkedGrids::default(),
            )?),
            LogQuestion::GridPoints { band } => json(&*self.fold(
                engine,
                &self.0.grid_points,
                band.clone(),
                Engine::log_index_rev,
                GRID_POINTS,
                GridCounts::new(band),
            )?),
            LogQuestion::BandsInLog {} => json(&*self.fold(
                engine,
                &self.0.bands,
                (),
                Engine::log_index_rev,
                BANDS,
                BandsInLog::default(),
            )?),
            LogQuestion::Statistics {} => json(&*self.fold(
                engine,
                &self.0.statistics,
                (),
                Engine::log_index_rev,
                STATISTICS,
                LogStatCounter::new(resolve),
            )?),
            // The backlog reads the LoTW stamp — a Stamp moves `content_rev`, and a contact
            // imported with its stamp is an Append, which moves `index_rev` — so it is kept
            // against `revision`, which every change moves.
            LogQuestion::LotwBacklog {} => json(*self.fold(
                engine,
                &self.0.backlog,
                (),
                Engine::log_revision,
                BACKLOG,
                LotwBacklog::default(),
            )?),
            _ => {
                let c = Capture::take(&engine_lock(engine));
                match q {
                    LogQuestion::LogSize {} => json(self.log_size(&c)?),
                    LogQuestion::RowsAt { indices } => json(self.rows_at(&c, indices, resolve)?),
                    LogQuestion::Row { id } => json(self.row(&c, id, resolve)?),
                    LogQuestion::Page {
                        query,
                        offset,
                        limit,
                    } => self.page(&c, query, *offset, *limit, resolve),
                    LogQuestion::Locate { query, id } => self.locate(&c, query, id),
                    _ => unreachable!("answered above"),
                }
            }
        }
    }

    // ── the Logbook: order vectors, pages, places ────────────────────────────────────────────

    /// The order `query` gives at the capture's `index_rev`: kept, or built once however many
    /// ask at the same time.
    fn order_of(&self, c: &Capture, query: &LogQuery) -> Result<Arc<Vec<u32>>, String> {
        let key = OrderKey {
            index_rev: c.index_rev,
            query: query.clone(),
        };
        let slot = {
            let mut orders = lock(&self.0.orders);
            let slot = match orders.iter().position(|(k, _)| *k == key) {
                Some(at) => orders.remove(at).1,
                None => Arc::new(OrderSlot::default()),
            };
            orders.push((key, Arc::clone(&slot)));
            if orders.len() > ORDERS_KEPT {
                orders.remove(0);
            }
            slot
        };
        let mut not_kept = None;
        let kept = slot.0.get_or_init(|| match self.build_order(c, query) {
            Ok((order, Freshness::Current)) => Some(Arc::new(order)),
            Ok((order, Freshness::Stale(_))) => {
                not_kept = Some(Ok(Arc::new(order)));
                None
            }
            Err(e) => {
                not_kept = Some(Err(e));
                None
            }
        });
        match kept {
            Some(order) => Ok(Arc::clone(order)),
            None => {
                lock(&self.0.orders).retain(|(_, s)| !Arc::ptr_eq(s, &slot));
                match not_kept {
                    Some(answer) => answer,
                    // Another asker's build was not kept: this one reads for itself.
                    None => self.build_order(c, query).map(|(order, _)| Arc::new(order)),
                }
            }
        }
    }

    /// Build the order vector: off the index for the default order, else one narrow pass in log
    /// order through the UI's filter and comparator.
    fn build_order(&self, c: &Capture, query: &LogQuery) -> Result<(Vec<u32>, Freshness), String> {
        #[cfg(test)]
        self.0
            .built
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        match &c.rows {
            LogRows::Store(reads) => read(reads, |db| {
                if query.is_default() {
                    db.rowids_newest_first()
                } else {
                    let mut b = OrderBuilder::new(query);
                    db.each_narrow_after(ORDER_COLUMNS, 0, &mut |rowid, r| b.push(rowid, r))?;
                    Ok(b.finish())
                }
            }),
            LogRows::Memory(rows) => {
                off_lock("an order of the log in memory");
                let order = query::order(
                    rows.iter().enumerate().map(|(i, r)| (i as u32, r.as_ref())),
                    query,
                );
                Ok((order, Freshness::Current))
            }
        }
    }

    /// `limit` rows from `offset` of the order `query` gives, and the revisions they were cut at.
    fn page(
        &self,
        c: &Capture,
        query: &LogQuery,
        offset: usize,
        limit: usize,
        resolve: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Value, String> {
        let order = self.order_of(c, query)?;
        let slice: Vec<u32> = order.iter().skip(offset).take(limit).copied().collect();
        let (rows, log_size): (Vec<(u32, QsoRecord)>, u64) = match &c.rows {
            LogRows::Store(reads) => {
                let ((found, size), _) =
                    read(reads, |db| Ok((db.rows_at(&slice)?, db.row_count()?)))?;
                // A row another window deleted since the order was built is left out; the
                // window's next poll moves the order.
                let rows = slice
                    .iter()
                    .zip(found)
                    .filter_map(|(&h, r)| r.map(|r| (h, r)))
                    .collect();
                (rows, size)
            }
            LogRows::Memory(all) => {
                off_lock("a page of the log in memory");
                let rows = slice
                    .iter()
                    .filter_map(|&p| all.get(p as usize).map(|r| (p, QsoRecord::clone(r))))
                    .collect();
                (rows, all.len() as u64)
            }
        };
        let keys = rows.iter().map(|(h, r)| key_of(r, *h)).collect();
        let edit_keys = rows
            .iter()
            .map(|(_, r)| QsoEdit::project(r).key())
            .collect();
        json(PageAnswer {
            query: query.clone(),
            revision: c.revision,
            order_rev: c.index_rev,
            content_rev: c.content_rev,
            total: order.len(),
            log_size,
            offset,
            rows: rows.into_iter().map(|(_, r)| logged(r, resolve)).collect(),
            keys,
            edit_keys,
        })
    }

    /// Where the row `id` sits in the order `query` gives, or `null` when the order does not show
    /// it — filtered out, or gone.
    fn locate(&self, c: &Capture, query: &LogQuery, id: &str) -> Result<Value, String> {
        let order = self.order_of(c, query)?;
        let handle = match id.parse::<RecordId>() {
            Err(()) => None,
            Ok(id) => match &c.rows {
                LogRows::Store(reads) => read(reads, |db| db.rowid_of(&id))?.0,
                LogRows::Memory(rows) => {
                    off_lock("a place in the log in memory");
                    rows.iter().position(|r| r.id == Some(id)).map(|p| p as u32)
                }
            },
        };
        json(LocateAnswer {
            query: query.clone(),
            order_rev: c.index_rev,
            index: handle.and_then(|h| order.iter().position(|&x| x == h)),
        })
    }

    /// The row `id` names, or `null` when the log does not hold it.
    fn row(
        &self,
        c: &Capture,
        id: &str,
        resolve: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Option<LoggedQso>, String> {
        let Ok(id) = id.parse::<RecordId>() else {
            return Ok(None);
        };
        let row = match &c.rows {
            LogRows::Store(reads) => {
                read(reads, |db| match db.rowid_of(&id)? {
                    Some(rowid) => Ok(db.rows_at(&[rowid])?.pop().flatten()),
                    None => Ok(None),
                })?
                .0
            }
            LogRows::Memory(rows) => {
                off_lock("a row of the log in memory");
                rows.iter()
                    .find(|r| r.id == Some(id))
                    .map(|r| QsoRecord::clone(r))
            }
        };
        Ok(row.map(|r| logged(r, resolve)))
    }

    /// Rows by log position, `null` for a position the log does not have.
    fn rows_at(
        &self,
        c: &Capture,
        indices: &[f64],
        resolve: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Vec<Option<LoggedQso>>, String> {
        let rows: Vec<Option<QsoRecord>> = match &c.rows {
            LogRows::Store(reads) => {
                let log = self.log_order_of(c, reads)?;
                let rowids: Vec<Option<u32>> = indices
                    .iter()
                    .map(|&i| position(i, log.len()).map(|p| log[p]))
                    .collect();
                let asked: Vec<u32> = rowids.iter().flatten().copied().collect();
                let mut found = read(reads, |db| db.rows_at(&asked))?.0.into_iter();
                rowids
                    .iter()
                    .map(|r| r.and_then(|_| found.next().flatten()))
                    .collect()
            }
            LogRows::Memory(all) => {
                off_lock("rows of the log in memory");
                indices
                    .iter()
                    .map(|&i| position(i, all.len()).map(|p| QsoRecord::clone(&all[p])))
                    .collect()
            }
        };
        Ok(rows
            .into_iter()
            .map(|r| r.map(|r| logged(r, resolve)))
            .collect())
    }

    /// Every rowid in log order, kept against `index_rev`: what a log position names.
    fn log_order_of(&self, c: &Capture, reads: &StoreReads) -> Result<Arc<Vec<u32>>, String> {
        if let Some((at, order)) = lock(&self.0.log_order).as_ref() {
            if *at == c.index_rev {
                return Ok(Arc::clone(order));
            }
        }
        let (order, fresh) = read(reads, |db| db.rowids_in_log_order())?;
        let order = Arc::new(order);
        if fresh == Freshness::Current {
            *lock(&self.0.log_order) = Some((c.index_rev, Arc::clone(&order)));
        }
        Ok(order)
    }

    fn log_size(&self, c: &Capture) -> Result<u64, String> {
        match &c.rows {
            LogRows::Store(reads) => Ok(read(reads, |db| db.row_count())?.0),
            LogRows::Memory(rows) => Ok(rows.len() as u64),
        }
    }

    // ── calls ─────────────────────────────────────────────────────────────────────────────────

    /// The worked-before calls with a character outside ASCII, as of the log's `key_rev` — read
    /// off the hot index, UNDER the Engine lock the caller holds.
    fn odd_calls(&self, eng: &Engine) -> Arc<OddCalls> {
        let rev = eng.log_key_rev();
        let mut odd = lock(&self.0.odd);
        if let Some((at, calls)) = odd.as_ref() {
            if *at == rev {
                return Arc::clone(calls);
            }
        }
        let keys = eng.log_odd_call_keys();
        let upper = keys.iter().map(|k| query::worked_key(k)).collect();
        let calls = Arc::new(OddCalls { keys, upper });
        *odd = Some((rev, Arc::clone(&calls)));
        calls
    }

    /// The band map's question: which of `calls` the log holds, each upper-cased and untrimmed
    /// as the UI keys a call. Answered from the hot index under the Engine lock: an ASCII key
    /// is a worked-before lookup (the index keys ASCII calls exactly as the UI does), and the
    /// calls the index folds differently are compared as the UI folds them.
    fn worked_calls(&self, engine: &SharedEngine, calls: &[String]) -> Vec<String> {
        let wanted: Vec<String> = calls.iter().map(|c| query::worked_key(c)).collect();
        let ascii: Vec<String> = wanted.iter().filter(|k| k.is_ascii()).cloned().collect();
        let (odd, hits) = {
            let eng = engine_lock(engine);
            (self.odd_calls(&eng), eng.log_b4_worked(&ascii))
        };
        let hit: HashSet<&str> = ascii
            .iter()
            .zip(hits)
            .filter(|(_, h)| *h)
            .map(|(k, _)| k.as_str())
            .collect();
        calls
            .iter()
            .zip(&wanted)
            .filter(|(_, k)| hit.contains(k.as_str()) || odd.upper.contains(k.as_str()))
            .map(|(c, _)| c.clone())
            .collect()
    }

    /// What a question about `calls` takes under the Engine lock: the rows' handles, and the
    /// `call_norm` keys that name every row that could be one of them — each call as the UI
    /// trims and upper-cases it when that is ASCII, and the stored key of every call with a
    /// character outside ASCII that the UI's fold makes the same.
    fn call_capture(&self, engine: &SharedEngine, calls: &[String]) -> (Capture, Vec<String>) {
        let eng = engine_lock(engine);
        let odd = self.odd_calls(&eng);
        let mut keys: Vec<String> = Vec::new();
        for call in calls {
            let c = query::history_call(call);
            if c.is_empty() {
                continue;
            }
            if c.is_ascii() {
                keys.push(c.clone());
            }
            // A hot-index key is the call ASCII upper-cased; trimmed, it is the stored
            // `call_norm` (trimming and an ASCII case fold commute).
            keys.extend(
                odd.keys
                    .iter()
                    .filter(|k| query::history_call(k) == c)
                    .map(|k| k.trim().to_string()),
            );
        }
        keys.sort_unstable();
        keys.dedup();
        (Capture::take(&eng), keys)
    }

    /// Every row the stored `call_norm` keys name, whole, in log order. A candidate set: the
    /// question decides which of them are the call it is about.
    fn call_rows(&self, c: &Capture, keys: &[String]) -> Result<Vec<QsoRecord>, String> {
        if keys.is_empty() {
            return Ok(Vec::new());
        }
        match &c.rows {
            LogRows::Store(reads) => Ok(read(reads, |db| {
                let rowids = db.rowids_by_call_norm(keys)?;
                Ok(db.rows_at(&rowids)?.into_iter().flatten().collect())
            })?
            .0),
            LogRows::Memory(rows) => {
                off_lock("a call's rows in the log in memory");
                let wanted: HashSet<&str> = keys.iter().map(String::as_str).collect();
                Ok(rows
                    .iter()
                    .filter(|r| wanted.contains(r.call.trim().to_ascii_uppercase().as_str()))
                    .map(|r| QsoRecord::clone(r))
                    .collect())
            }
        }
    }

    // ── the folds ─────────────────────────────────────────────────────────────────────────────

    /// A fold question (C14's folds, for the UI's questions): kept against `mark` — the
    /// watermark of what it reads — and `key`, or else one narrow pass over the log in log
    /// order with the Engine lock released, kept unless the read ran out of wait.
    fn fold<K: PartialEq, F: LogFold>(
        &self,
        engine: &SharedEngine,
        kept: &Tally<K, F::Answer>,
        key: K,
        mark: fn(&Engine) -> u64,
        narrow: Narrow,
        mut fold: F,
    ) -> Result<Arc<F::Answer>, String> {
        let (at, rows) = {
            let eng = engine_lock(engine);
            let at = mark(&eng);
            if let Some(answer) = kept.get(at, &key) {
                return Ok(answer);
            }
            (at, eng.log_rows())
        };
        #[cfg(test)]
        self.0
            .folded
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let fresh = rows
            .each(narrow, Scope::All, Order::Log, &mut |r| {
                fold.add(r);
                ControlFlow::Continue(())
            })
            .map_err(unreadable)?;
        let answer = fold.finish();
        Ok(match fresh {
            Freshness::Current => kept.put(at, key, answer),
            Freshness::Stale(_) => Arc::new(answer),
        })
    }

    // ── entities ──────────────────────────────────────────────────────────────────────────────

    /// Every entity's slots as of the capture: kept when `index_rev` has not moved, grown by the
    /// rows appended since when every change was an append, built again otherwise.
    fn entity_index(
        &self,
        c: &Capture,
        cached: Option<Arc<EntityCache>>,
        appended: bool,
        resolve: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Arc<EntityCache>, String> {
        let store = c.is_store();
        if let Some(k) = &cached {
            if k.index_rev == c.index_rev && k.store == store {
                return Ok(Arc::clone(k));
            }
        }
        let (mut index, from) = match cached {
            Some(k) if appended && k.store == store => (k.index.clone(), k.after),
            _ => (EntityIndex::default(), 0),
        };
        // A call's entity once per distinct call in the pass: a lifetime log repeats them.
        let mut entities: HashMap<String, Option<String>> = HashMap::new();
        let mut count = |index: &mut EntityIndex, r: &QsoRecord| {
            let e = entities
                .entry(r.call.clone())
                .or_insert_with(|| resolve(&r.call));
            index.add(e.as_deref(), r);
        };
        let (after, fresh) = match &c.rows {
            LogRows::Store(reads) => {
                let mut last = from;
                let ((), fresh) = read(reads, |db| {
                    db.each_narrow_after(ENTITY_COLUMNS, from, &mut |rowid, r| {
                        count(&mut index, r);
                        last = rowid;
                    })
                })?;
                (last, fresh)
            }
            LogRows::Memory(rows) => {
                off_lock("the entities of the log in memory");
                for r in rows.iter().skip(from as usize) {
                    count(&mut index, r);
                }
                (rows.len() as u32, Freshness::Current)
            }
        };
        let built = Arc::new(EntityCache {
            revision: c.revision,
            index_rev: c.index_rev,
            store,
            after,
            index,
        });
        if fresh == Freshness::Current {
            let mut kept = lock(&self.0.entities);
            if kept.as_ref().is_none_or(|k| k.revision <= built.revision) {
                *kept = Some(Arc::clone(&built));
            }
        }
        Ok(built)
    }
}

/// A call's live DXCC entity (cty.dat) — what each row the UI is handed carries.
fn cty(call: &str) -> Option<String> {
    propagation::dxcc::resolve(call).map(|i| i.entity.to_string())
}

/// Answer one of the UI's log questions (`LogQuestion` → its answer, as `logAnswers.ts` types
/// them). On the blocking pool: it may wait for the writer, and it reads the disk.
#[tauri::command]
pub async fn ask_log(
    state: State<'_, SharedEngine>,
    queries: State<'_, LogQueries>,
    q: LogQuestion,
) -> Result<Value, String> {
    let (engine, queries) = (Arc::clone(&state), queries.inner().clone());
    tauri::async_runtime::spawn_blocking(move || queries.answer(&engine, &q, &cty))
        .await
        .map_err(|e| format!("the logbook read did not finish: {e}"))?
}

#[cfg(test)]
mod tests;
