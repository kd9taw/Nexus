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
//!    A read that ran out of wait is REFUSED
//!    ([`Freshness::or_refuse`](tempo_app::logstore::Freshness::or_refuse)) and keeps nothing, and
//!    the window asks again. It never answers from the store as it stands: that answer left the
//!    contact just logged out of the Logbook until the next change.
//! 3. **Revisions name what the answer was cut from.** A page carries `revision` (the log's),
//!    `orderRev` (the `index_rev` its order was built at) and `contentRev` (the `content_rev` its
//!    rows were read at). An upload stamp or a QSL-sent mark moves `content_rev` and not
//!    `index_rev`, so the order survives it and only the rows are read again — except an order
//!    that reads what those marks change (a sent filter, an upload or QSL-sent-date sort), which
//!    is built again, and whose `orderRev` moves with it ([`Capture::order_rev`]).

use std::collections::{HashMap, HashSet};
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::State;
use tempo_app::dto::LoggedQso;
use tempo_app::engine::{engine_lock, Engine};
use tempo_app::logstore::{LogRows, StoreReads};
use tempo_core::logbook::query::{
    self, AwardNeed, BandsInLog, CallSummary, EntityIndex, GridCount, GridCounts, LogFilter,
    LogFold, LogQuery, LogStatCounter, LogStatCounts, LotwBacklog, OrderBuilder, WorkedGrids,
};
use tempo_core::logbook::sqlite::{self, Narrow, Order, Scope, ENTITY_COLUMNS, ORDER_COLUMNS};
use tempo_core::logbook::{QsoEdit, QsoRecord, RecordId, AWARD_EXTRAS};

use crate::{SharedEngine, Tally};

/// Order vectors kept (SPEC-2 v2 §2: an LRU of four). One is a log-sized `Vec<u32>`.
pub(crate) const ORDERS_KEPT: usize = 4;

/// How long a read waits for the writer to take the changes made before it was asked (P4), before
/// it is refused.
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

/// The statistics' counts (`statistics`) — with the record's DXCC number, and LoTW's beside it
/// (the passthrough fields [`AWARD_EXTRAS`] names), which decide the entity a contact counts
/// toward.
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
        "dxcc",
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
    /// The station's grid, for the path filter's grade of a contact that names none — the same
    /// source the Logbook's Path column reads (`log_path_grades`).
    my_grid: String,
}

impl Capture {
    fn take(eng: &Engine) -> Capture {
        Capture {
            rows: eng.log_rows(),
            revision: eng.log_revision(),
            index_rev: eng.log_index_rev(),
            content_rev: eng.log_content_rev(),
            my_grid: eng.settings().mygrid.clone(),
        }
    }

    /// The revision `query`'s order is built at, which a page and a place name it by (`orderRev`):
    /// the `index_rev`, or for an order that reads what a stamp changes
    /// ([`LogQuery::reads_content`]) the later of it and the `content_rev`. Every watermark is a
    /// revision from one counter, so this moves whenever such an order may have, and never back:
    /// the window, which keeps pages by (query, orderRev, offset), never files a page of a new
    /// order beside an old one's.
    fn order_rev(&self, query: &LogQuery) -> u64 {
        if query.reads_content() {
            self.index_rev.max(self.content_rev)
        } else {
            self.index_rev
        }
    }

    fn is_store(&self) -> bool {
        matches!(self.rows, LogRows::Store(_))
    }
}

/// A read of the store, waited for (P4) and fenced off the Engine lock by [`StoreReads`]; refused
/// ([`Freshness::or_refuse`](tempo_app::logstore::Freshness::or_refuse)) when the store did not
/// hold every change made before it was asked within the wait.
fn read<T>(
    reads: &StoreReads,
    f: impl FnOnce(&sqlite::LogDb) -> sqlite::Result<T>,
) -> Result<T, String> {
    let (answer, fresh) = reads.read(READ_WAIT, f).map_err(unreadable)?;
    fresh.or_refuse()?;
    Ok(answer)
}

/// What an answer says when the store could not be read. Never an empty answer, which would
/// call every station on the air a new one.
fn unreadable(e: sqlite::Error) -> String {
    format!("the logbook could not be read: {e}")
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A row as the UI reads it: [`LoggedQso`] with its entity (in the app [`shown`]) — the
/// conversion every row the UI is handed gets ([`crate::log_by_id`]'s answers too).
fn logged(r: QsoRecord, resolve: &dyn Fn(&QsoRecord) -> Option<String>) -> LoggedQso {
    let entity = resolve(&r);
    let mut q = LoggedQso::from(r);
    q.entity = entity;
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
    /// The `content_rev` too, for an order that reads what was SENT ([`LogQuery::reads_content`]):
    /// an upload stamp and a card marked sent move only `content_rev`, and the rows a sent filter
    /// lets through, and the order of an upload or QSL-sent-date sort, move with them.
    content_rev: Option<u64>,
    /// The path grades' revision too, for an order a path filter decides: the display-only grades
    /// grow as the log is graded, and move neither of the log's revisions (`path_grades`).
    grades_rev: Option<u64>,
    query: LogQuery,
}

/// An order vector kept, or on its way: the first asker builds it and any other waits for that
/// build rather than making its own. `None` once built means the build was not kept (a refused
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
    /// The app's whole-log tallies, for the award fold the award-need filter reads — the one the
    /// Awards view keeps, not a second.
    tallies: crate::LogTallies,
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
    /// The answers, reading the award fold through the app's own `tallies`.
    pub(crate) fn with_tallies(tallies: crate::LogTallies) -> Self {
        Self(Arc::new(Inner {
            tallies,
            ..Inner::default()
        }))
    }

    /// Answer `q`: take what it needs under the Engine lock, release it, read, answer. `resolve`
    /// is a row's DXCC entity — in the app [`shown`]: its call's, or the one its record's DXCC
    /// number names — what each row the UI is handed carries and what the country column sorts
    /// on. `card` is what an entity's slots are keyed by: in the app the entity a contact is
    /// under on the callsign card ([`on_card`]), which a typed call's card asks for. `counted` is
    /// the entity the statistics count a contact under: in the app, the one the awards count it
    /// toward ([`counted`]), by its record's DXCC number first.
    pub(crate) fn answer(
        &self,
        engine: &SharedEngine,
        q: &LogQuestion,
        resolve: &dyn Fn(&QsoRecord) -> Option<String>,
        card: &dyn Fn(&QsoRecord) -> Option<String>,
        counted: &dyn Fn(&QsoRecord) -> Option<String>,
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
                let index = self.entity_index(&c, cached, appended, card)?;
                json(index.index.answer(entity))
            }
            // A grid, a band or a time moves `index_rev`; an upload stamp does not, and none of
            // these reads one.
            LogQuestion::WorkedGrids {} => json(&*self.fold(
                engine,
                &self.0.grids,
                (),
                Engine::log_index_rev,
                (GRIDS, &[]),
                WorkedGrids::default(),
            )?),
            LogQuestion::GridPoints { band } => json(&*self.fold(
                engine,
                &self.0.grid_points,
                band.clone(),
                Engine::log_index_rev,
                (GRID_POINTS, &[]),
                GridCounts::new(band),
            )?),
            LogQuestion::BandsInLog {} => json(&*self.fold(
                engine,
                &self.0.bands,
                (),
                Engine::log_index_rev,
                (BANDS, &[]),
                BandsInLog::default(),
            )?),
            LogQuestion::Statistics {} => json(&*self.fold(
                engine,
                &self.0.statistics,
                (),
                Engine::log_index_rev,
                (STATISTICS, AWARD_EXTRAS),
                LogStatCounter::new(counted),
            )?),
            // The backlog reads the LoTW stamp — a Stamp moves `content_rev`, and a contact
            // imported with its stamp is an Append, which moves `index_rev` — so it is kept
            // against `revision`, which every change moves.
            LogQuestion::LotwBacklog {} => json(*self.fold(
                engine,
                &self.0.backlog,
                (),
                Engine::log_revision,
                (BACKLOG, &[]),
                LotwBacklog::default(),
            )?),
            _ => {
                // An award-need filter reads the award fold, which reads the log itself: asked
                // before the capture, so the order is cut no earlier than the fold it reads.
                let need = match q {
                    LogQuestion::Page { query, .. } | LogQuestion::Locate { query, .. } => {
                        match query.filter.need {
                            Some(award) => Some(self.need_slots(engine, award)?),
                            None => None,
                        }
                    }
                    _ => None,
                };
                let c = Capture::take(&engine_lock(engine));
                let order = OrderInputs {
                    resolve,
                    need: need.as_ref(),
                };
                match q {
                    LogQuestion::LogSize {} => json(self.log_size(&c)?),
                    LogQuestion::RowsAt { indices } => json(self.rows_at(&c, indices, resolve)?),
                    LogQuestion::Row { id } => json(self.row(&c, id, resolve)?),
                    LogQuestion::Page {
                        query,
                        offset,
                        limit,
                    } => self.page(&c, query, *offset, *limit, &order),
                    LogQuestion::Locate { query, id } => self.locate(&c, query, id, &order),
                    _ => unreachable!("answered above"),
                }
            }
        }
    }

    /// The ids of every row the order `query` gives, in that order: what a bulk change of "the
    /// contacts the filter shows" acts on (`crate::log_tools`). Read with the Engine lock
    /// released, as a page is.
    pub(crate) fn ids_of(
        &self,
        engine: &SharedEngine,
        query: &LogQuery,
    ) -> Result<Vec<RecordId>, String> {
        let need = match query.filter.need {
            Some(award) => Some(self.need_slots(engine, award)?),
            None => None,
        };
        let c = Capture::take(&engine_lock(engine));
        let inputs = OrderInputs {
            resolve: &shown,
            need: need.as_ref(),
        };
        let order = self.order_of(&c, query, &inputs)?;
        let found = match &c.rows {
            LogRows::Store(reads) => read(reads, |db| db.rows_at(&order))?,
        };
        Ok(found.into_iter().flatten().filter_map(|r| r.id).collect())
    }

    // ── the Logbook: order vectors, pages, places ────────────────────────────────────────────

    /// The order `query` gives at the capture's `index_rev`: kept, or built once however many
    /// ask at the same time. `resolve` is a row's DXCC entity, which the country column sorts on.
    fn order_of(
        &self,
        c: &Capture,
        query: &LogQuery,
        inputs: &OrderInputs<'_>,
    ) -> Result<Arc<Vec<u32>>, String> {
        let key = OrderKey {
            index_rev: c.index_rev,
            content_rev: query.reads_content().then_some(c.content_rev),
            grades_rev: query.filter.path.map(|_| crate::path_grades::rev()),
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
        let kept = slot
            .0
            .get_or_init(|| match self.build_order(c, query, inputs) {
                Ok(order) => Some(Arc::new(order)),
                Err(e) => {
                    not_kept = Some(e);
                    None
                }
            });
        match kept {
            Some(order) => Ok(Arc::clone(order)),
            None => {
                lock(&self.0.orders).retain(|(_, s)| !Arc::ptr_eq(s, &slot));
                match not_kept {
                    Some(e) => Err(e),
                    // Another asker's build was not kept: this one reads for itself.
                    None => self.build_order(c, query, inputs).map(Arc::new),
                }
            }
        }
    }

    /// Build the order vector: off the index for the default order, else one narrow pass in log
    /// order through the UI's filter and comparator — with each row's SUBMODE when the query
    /// searches, since the search looks there too and nothing else reads it. The field filters
    /// ride in the pass's own `WHERE` (`LogDb::each_filtered`), so the store hands back only the
    /// rows they let through; the fields SQL cannot decide are tested here ([`decide`]).
    fn build_order(
        &self,
        c: &Capture,
        query: &LogQuery,
        inputs: &OrderInputs<'_>,
    ) -> Result<Vec<u32>, String> {
        let resolve = inputs.resolve;
        #[cfg(test)]
        self.0
            .built
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        match &c.rows {
            LogRows::Store(reads) => read(reads, |db| {
                if query.is_default() {
                    db.rowids_newest_first()
                } else {
                    let decider = Decider::new(&query.filter, resolve, inputs.need, &c.my_grid);
                    let keep = |r: &QsoRecord| decider.keeps(r);
                    let mut b = OrderBuilder::new(query, resolve);
                    if query.filter.decided_by_caller() {
                        b = b.keep(&keep);
                    }
                    let (mut columns, mut extras) = b.reads(ORDER_COLUMNS);
                    // What the caller's test reads, and only that: each passthrough field is one
                    // more lookup per contact. The entity needs the call and the DXCC number,
                    // which every order reads.
                    let f = &query.filter;
                    if !js_field(&f.continent).is_empty() {
                        extras.push("CONT");
                    }
                    if f.cq_zone.is_some() {
                        extras.push("CQZ");
                    }
                    if f.need.is_some() {
                        columns
                            .extend(DECIDE_COLUMNS.iter().filter(|c| !ORDER_COLUMNS.contains(c)));
                        extras.extend(AWARD_EXTRAS);
                    }
                    if f.path.is_some() {
                        for c in crate::path_grades::COLUMNS {
                            if !columns.contains(c) {
                                columns.push(c);
                            }
                        }
                        extras.extend(crate::path_grades::READS);
                    }
                    extras.sort_unstable();
                    extras.dedup();
                    let children = b.children();
                    if query.filter.is_empty()
                        && extras.is_empty()
                        && children.is_empty()
                        && columns == ORDER_COLUMNS
                    {
                        // Exactly the pass the Logbook always made.
                        if b.searches() {
                            db.each_narrow_after_with_submode(
                                ORDER_COLUMNS,
                                0,
                                &mut |rowid, r| b.push(rowid, r),
                            )?;
                        } else {
                            db.each_narrow_after(ORDER_COLUMNS, 0, &mut |rowid, r| {
                                b.push(rowid, r)
                            })?;
                        }
                    } else {
                        db.each_filtered_with(
                            &columns,
                            &extras,
                            children,
                            b.searches(),
                            &query.filter,
                            &mut |rowid, r| b.push(rowid, r),
                        )?;
                    }
                    Ok(b.finish())
                }
            }),
        }
    }

    /// `limit` rows from `offset` of the order `query` gives, and the revisions they were cut at.
    fn page(
        &self,
        c: &Capture,
        query: &LogQuery,
        offset: usize,
        limit: usize,
        inputs: &OrderInputs<'_>,
    ) -> Result<Value, String> {
        let resolve = inputs.resolve;
        let order = self.order_of(c, query, inputs)?;
        let slice: Vec<u32> = order.iter().skip(offset).take(limit).copied().collect();
        let (rows, log_size): (Vec<(u32, QsoRecord)>, u64) = match &c.rows {
            LogRows::Store(reads) => {
                let (found, size) = read(reads, |db| Ok((db.rows_at(&slice)?, db.row_count()?)))?;
                // A row another window deleted since the order was built is left out; the
                // window's next poll moves the order.
                let rows = slice
                    .iter()
                    .zip(found)
                    .filter_map(|(&h, r)| r.map(|r| (h, r)))
                    .collect();
                (rows, size)
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
            order_rev: c.order_rev(query),
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
    fn locate(
        &self,
        c: &Capture,
        query: &LogQuery,
        id: &str,
        inputs: &OrderInputs<'_>,
    ) -> Result<Value, String> {
        let order = self.order_of(c, query, inputs)?;
        let handle = match id.parse::<RecordId>() {
            Err(()) => None,
            Ok(id) => match &c.rows {
                LogRows::Store(reads) => read(reads, |db| db.rowid_of(&id))?,
            },
        };
        json(LocateAnswer {
            query: query.clone(),
            order_rev: c.order_rev(query),
            index: handle.and_then(|h| order.iter().position(|&x| x == h)),
        })
    }

    /// The row `id` names, or `null` when the log does not hold it.
    fn row(
        &self,
        c: &Capture,
        id: &str,
        resolve: &dyn Fn(&QsoRecord) -> Option<String>,
    ) -> Result<Option<LoggedQso>, String> {
        let Ok(id) = id.parse::<RecordId>() else {
            return Ok(None);
        };
        let row = match &c.rows {
            LogRows::Store(reads) => read(reads, |db| match db.rowid_of(&id)? {
                Some(rowid) => Ok(db.rows_at(&[rowid])?.pop().flatten()),
                None => Ok(None),
            })?,
        };
        Ok(row.map(|r| logged(r, resolve)))
    }

    /// Rows by log position, `null` for a position the log does not have.
    fn rows_at(
        &self,
        c: &Capture,
        indices: &[f64],
        resolve: &dyn Fn(&QsoRecord) -> Option<String>,
    ) -> Result<Vec<Option<LoggedQso>>, String> {
        let rows: Vec<Option<QsoRecord>> = match &c.rows {
            LogRows::Store(reads) => {
                let log = self.log_order_of(c, reads)?;
                let rowids: Vec<Option<u32>> = indices
                    .iter()
                    .map(|&i| position(i, log.len()).map(|p| log[p]))
                    .collect();
                let asked: Vec<u32> = rowids.iter().flatten().copied().collect();
                let mut found = read(reads, |db| db.rows_at(&asked))?.into_iter();
                rowids
                    .iter()
                    .map(|r| r.and_then(|_| found.next().flatten()))
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
        let order = Arc::new(read(reads, |db| db.rowids_in_log_order())?);
        *lock(&self.0.log_order) = Some((c.index_rev, Arc::clone(&order)));
        Ok(order)
    }

    fn log_size(&self, c: &Capture) -> Result<u64, String> {
        match &c.rows {
            LogRows::Store(reads) => read(reads, |db| db.row_count()),
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
            LogRows::Store(reads) => read(reads, |db| {
                let rowids = db.rowids_by_call_norm(keys)?;
                Ok(db.rows_at(&rowids)?.into_iter().flatten().collect())
            }),
        }
    }

    // ── the folds ─────────────────────────────────────────────────────────────────────────────

    /// A fold question (C14's folds, for the UI's questions): kept against `mark` — the
    /// watermark of what it reads — and `key`, or else one narrow pass over the log in log
    /// order with the Engine lock released, kept — or refused, keeping nothing, if the read ran
    /// out of wait. What it reads is a narrow, and the passthrough fields read beside it.
    fn fold<K: PartialEq, F: LogFold>(
        &self,
        engine: &SharedEngine,
        kept: &Tally<K, F::Answer>,
        key: K,
        mark: fn(&Engine) -> u64,
        (narrow, extras): (Narrow, &[&str]),
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
        rows.each_with(narrow, extras, Scope::All, Order::Log, &mut |r| {
            fold.add(r);
            ControlFlow::Continue(())
        })
        .map_err(unreadable)?
        .or_refuse()?;
        Ok(kept.put(at, key, fold.finish()))
    }

    // ── entities ──────────────────────────────────────────────────────────────────────────────

    /// Every entity's slots as of the capture: kept when `index_rev` has not moved, grown by the
    /// rows appended since when every change was an append, built again otherwise.
    fn entity_index(
        &self,
        c: &Capture,
        cached: Option<Arc<EntityCache>>,
        appended: bool,
        resolve: &dyn Fn(&QsoRecord) -> Option<String>,
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
        // A row's entity once per distinct call and DXCC number in the pass: a lifetime log
        // repeats them.
        let mut entities: HashMap<(String, Option<u32>), Option<String>> = HashMap::new();
        let mut count = |index: &mut EntityIndex, r: &QsoRecord| {
            let e = entities
                .entry((r.call.clone(), r.dxcc))
                .or_insert_with(|| resolve(r));
            index.add(e.as_deref(), r);
        };
        let after = match &c.rows {
            LogRows::Store(reads) => {
                let mut last = from;
                read(reads, |db| {
                    db.each_narrow_after(ENTITY_COLUMNS, from, &mut |rowid, r| {
                        count(&mut index, r);
                        last = rowid;
                    })
                })?;
                last
            }
        };
        let built = Arc::new(EntityCache {
            revision: c.revision,
            index_rev: c.index_rev,
            store,
            after,
            index,
        });
        {
            let mut kept = lock(&self.0.entities);
            if kept.as_ref().is_none_or(|k| k.revision <= built.revision) {
                *kept = Some(Arc::clone(&built));
            }
        }
        Ok(built)
    }
}

// ── the fields a filter leaves to the caller ───────────────────────────────────────────────────

/// What an order is built with beside its query: a row's DXCC entity (`q.entity`), and for an
/// award-need filter the slots it asks about.
struct OrderInputs<'a> {
    resolve: &'a dyn Fn(&QsoRecord) -> Option<String>,
    need: Option<&'a NeedSlots>,
}

/// The columns [`Decider`] reads beyond the Logbook's own: what the award fold reads of a contact.
const DECIDE_COLUMNS: &[&str] = &[
    "call",
    "dxcc",
    "band",
    "grid",
    "state",
    "prop_mode",
    "qsl_card_rcvd_raw",
    "lotw_rcvd_raw",
    "eqsl_rcvd_raw",
    "qrz_status_raw",
];

/// An award's band slots that are worked and not confirmed, as the award fold counts them —
/// keyed `(band label, entity / state / zone / square)` — and, for VUCC, which the fold does not
/// list square by square, the squares confirmed on each VUCC band.
pub(crate) struct NeedSlots {
    award: AwardNeed,
    /// By band label (lower case), the keys open on it.
    open: HashMap<String, HashSet<String>>,
    vucc_bands: HashSet<String>,
}

/// A grid SQUARE as VUCC counts it (the award fold's rule): a valid four-character locator,
/// upper-cased; `None` for anything else.
fn vucc_square(grid: &str) -> Option<String> {
    let g = grid.trim().to_ascii_uppercase();
    let b = g.as_bytes();
    let ok = b.len() >= 4
        && (b'A'..=b'R').contains(&b[0])
        && (b'A'..=b'R').contains(&b[1])
        && b[2].is_ascii_digit()
        && b[3].is_ascii_digit();
    ok.then(|| g[..4].to_string())
}

impl LogQueries {
    /// The open slots of `award`, read off the award fold the Awards view keeps
    /// ([`crate::log_folds::awards_kept`]). VUCC's open squares are not in the fold's answer, so
    /// they are counted here by its rule — a terrestrial contact's valid square on a VUCC band,
    /// confirmed by LoTW or a card — and held to the fold's own per-band counts by a test.
    fn need_slots(&self, engine: &SharedEngine, award: AwardNeed) -> Result<NeedSlots, String> {
        let summary = crate::log_folds::awards_kept(engine, &self.0.tallies)?;
        let mut open: HashMap<String, HashSet<String>> = HashMap::new();
        let mut put = |needs: &[propagation::awards::BandNeed]| {
            for n in needs {
                for key in &n.unconfirmed {
                    open.entry(n.band.to_ascii_lowercase())
                        .or_default()
                        .insert(key.clone());
                }
            }
        };
        let mut vucc_bands = HashSet::new();
        match award {
            AwardNeed::Dxcc => put(&summary.band_needs.dxcc),
            AwardNeed::Was => put(&summary.band_needs.was),
            AwardNeed::Waz => put(&summary.band_needs.waz),
            AwardNeed::Ffma => {
                for g in &summary.ffma.unconfirmed {
                    open.entry("6m".to_string()).or_default().insert(g.clone());
                }
            }
            AwardNeed::Vucc => {
                vucc_bands = summary
                    .vucc
                    .awards
                    .iter()
                    .map(|a| a.band.to_ascii_lowercase())
                    .collect();
                // Every terrestrial square worked on a VUCC band, less the confirmed ones.
                let rows = engine_lock(engine).log_rows();
                tempo_core::logbook::io_fence::whole_log_off_engine_lock("the VUCC need");
                let mut worked = HashSet::new();
                let mut confirmed = HashSet::new();
                const VUCC: Narrow = Narrow {
                    columns: &[
                        "band",
                        "grid",
                        "prop_mode",
                        "qsl_card_rcvd_raw",
                        "lotw_rcvd_raw",
                        "eqsl_rcvd_raw",
                        "qrz_status_raw",
                    ],
                    uploads: false,
                };
                rows.each_with(
                    VUCC,
                    &[tempo_core::logbook::callbook::MAP_ONLY_GRID_FIELD],
                    Scope::All,
                    Order::Log,
                    &mut |r| {
                        let band = r.band.trim().to_ascii_lowercase();
                        if !crate::qso_is_sat(r.prop_mode.as_deref()) && vucc_bands.contains(&band)
                        {
                            // Never a grid for the map only (`counted_grid`).
                            if let Some(sq) =
                                tempo_core::logbook::counted_grid(r).and_then(vucc_square)
                            {
                                if r.award_confirmed {
                                    confirmed.insert((band.clone(), sq.clone()));
                                }
                                worked.insert((band, sq));
                            }
                        }
                        ControlFlow::Continue(())
                    },
                )
                .map_err(unreadable)?;
                for (band, sq) in worked.difference(&confirmed) {
                    open.entry(band.clone()).or_default().insert(sq.clone());
                }
            }
        }
        Ok(NeedSlots {
            award,
            open,
            vucc_bands,
        })
    }
}

/// A call and the DXCC number its record carries: what the entity a row is shown under turns on.
type ShownKey = (String, Option<u32>);

/// The test of the fields of a filter SQL cannot decide: the entity the Logbook shows
/// (`resolve`), the continent and CQ zone the contact counts in (its record's `CONT` / `CQZ`
/// first, else cty.dat's), and whether its band slot is still open for the award asked (`need`).
///
/// cty.dat's answer and the shown entity are asked once per call (and DXCC number) and kept for
/// the pass: a lifetime log works most of its calls many times, and the lookups are the cost.
struct Decider<'a> {
    f: &'a LogFilter,
    resolve: &'a dyn Fn(&QsoRecord) -> Option<String>,
    need: Option<&'a NeedSlots>,
    my_grid: &'a str,
    cty: std::cell::RefCell<HashMap<String, Option<propagation::dxcc::DxccInfo>>>,
    shown: std::cell::RefCell<HashMap<ShownKey, Option<String>>>,
}

impl<'a> Decider<'a> {
    fn new(
        f: &'a LogFilter,
        resolve: &'a dyn Fn(&QsoRecord) -> Option<String>,
        need: Option<&'a NeedSlots>,
        my_grid: &'a str,
    ) -> Self {
        Self {
            f,
            resolve,
            need,
            my_grid,
            cty: Default::default(),
            shown: Default::default(),
        }
    }

    fn cty(&self, call: &str) -> Option<propagation::dxcc::DxccInfo> {
        if let Some(found) = self.cty.borrow().get(call) {
            return *found;
        }
        let found = propagation::dxcc::resolve(call);
        self.cty.borrow_mut().insert(call.to_string(), found);
        found
    }

    fn keeps(&self, r: &QsoRecord) -> bool {
        let f = self.f;
        let entity = js_field(&f.entity);
        if !entity.is_empty() {
            let key = (r.call.clone(), r.dxcc);
            let cached = self.shown.borrow().get(&key).cloned();
            let shown = cached.unwrap_or_else(|| {
                let shown = (self.resolve)(r);
                self.shown.borrow_mut().insert(key, shown.clone());
                shown
            });
            if !shown.is_some_and(|e| e.trim().eq_ignore_ascii_case(entity)) {
                return false;
            }
        }
        if let Some(want) = f.path {
            if !crate::path_grades::filter_keeps(r, want, self.my_grid) {
                return false;
            }
        }
        let continent = js_field(&f.continent);
        if continent.is_empty() && f.cq_zone.is_none() && self.need.is_none() {
            return f.need.is_none();
        }
        let resolved = self.cty(&r.call);
        if !continent.is_empty() {
            let own = r
                .extra
                .iter()
                .find(|(k, _)| k == "CONT")
                .map(|(_, c)| c.trim().to_ascii_uppercase())
                .filter(|c| !c.is_empty());
            let cont = own.or_else(|| resolved.as_ref().map(|i| i.cont.to_string()));
            if !cont.is_some_and(|c| c.eq_ignore_ascii_case(continent)) {
                return false;
            }
        }
        let fields = crate::log_folds::award_fields(r);
        if let Some(zone) = f.cq_zone {
            if propagation::awards::counted_zone(resolved.as_ref(), &fields) != Some(zone) {
                return false;
            }
        }
        match self.need {
            None => f.need.is_none(),
            Some(n) => needed(r, n, resolved.as_ref(), &fields),
        }
    }
}

/// `s` trimmed — a filter field as it is read.
fn js_field(s: &str) -> &str {
    query::js_trim(s)
}

/// Whether `r` fills a slot of `n`'s award that is worked and not confirmed: confirming it would
/// add credit. The same gates the award fold counts the contact through.
fn needed(
    r: &QsoRecord,
    n: &NeedSlots,
    resolved: Option<&propagation::dxcc::DxccInfo>,
    fields: &propagation::RecordFields,
) -> bool {
    use propagation::model::Band;
    let sat = crate::qso_is_sat(r.prop_mode.as_deref());
    let on = |label: &str| n.open.get(label);
    let slots = Band::from_label(&r.band).and_then(|b| on(b.label()));
    let slot = |key: &str| slots.is_some_and(|open| open.contains(key));
    let counted = propagation::awards::counted_entity(resolved, fields);
    match n.award {
        AwardNeed::Dxcc => !sat && counted.is_some_and(|c| !c.deleted && slot(c.name)),
        AwardNeed::Was => {
            counted.is_some_and(|c| c.is_us_state())
                && tempo_core::logbook::record_field(r, "STATE")
                    .and_then(propagation::awards::valid_state)
                    .is_some_and(slot)
        }
        AwardNeed::Waz => propagation::awards::counted_zone(resolved, fields)
            .is_some_and(|z| slot(&z.to_string())),
        AwardNeed::Ffma | AwardNeed::Vucc => {
            let label = r.band.trim().to_ascii_lowercase();
            let on_band = match n.award {
                AwardNeed::Ffma => label == "6m",
                _ => n.vucc_bands.contains(&label),
            };
            !sat && on_band
                && tempo_core::logbook::counted_grid(r)
                    .and_then(vucc_square)
                    .is_some_and(|sq| on(&label).is_some_and(|open| open.contains(&sq)))
        }
    }
}

/// The DXCC entity a logged contact is shown under (`propagation::dxcc::shown_entity`): its
/// call's through cty.dat, or the one its record's own DXCC number names where that is another —
/// a 1×1 call logged as the DXpedition it was, a deleted entity, a number the operator set by
/// hand. What each row the UI is handed carries (the Logbook's country column, wherever a row is
/// made: here, the answers to a change, Remote's pages) and what the column sorts on.
pub(crate) fn shown(r: &QsoRecord) -> Option<String> {
    propagation::dxcc::shown_entity(propagation::dxcc::resolve(&r.call).as_ref(), r.dxcc)
        .map(str::to_string)
}

/// The DXCC entity a logged contact is under on the callsign card
/// (`propagation::dxcc::card_entity`), what an entity's worked slots are keyed by: [`shown`], with
/// the number its call carries standing in for a record that carries none. A typed call's card
/// asks for the same entity ([`crate::resolve_entity`]), so a WAE area's contact logged in Nexus
/// (Sicily, no number) is on the card of IT9XYZ and of I1ABC, under Italy, as one carrying 248 is.
pub(crate) fn on_card(r: &QsoRecord) -> Option<String> {
    propagation::dxcc::card_entity(propagation::dxcc::resolve(&r.call).as_ref(), r.dxcc)
        .map(str::to_string)
}

/// The country (ADIF `COUNTRY`) a contact takes from its record's DXCC number `code`
/// (`propagation::dxcc::country_by_dxcc`): the name of the DXCC entity the number names, the name
/// the contact is [`shown`] under; `None` for a number that names none. The station's resolver
/// for it — what the operator's changes and every fill write beside the number, and what an
/// export names.
pub(crate) fn dxcc_country(code: u32) -> Option<String> {
    propagation::dxcc::country_by_dxcc(code).map(str::to_string)
}

/// A contact as it leaves the station — a logbook service's upload, the operator's own logging
/// program, a Remote browser's file: its `COUNTRY` the name its own DXCC number gives it
/// ([`dxcc_country`]) where the number names one, so no other logger is handed a `DXCC` and a
/// `COUNTRY` that disagree. The stored record is not changed.
pub(crate) fn sent(mut r: QsoRecord) -> QsoRecord {
    if let Some(country) = r.dxcc.and_then(dxcc_country) {
        r.country = Some(country);
    }
    r
}

/// The DXCC entity a contact counts toward, as the awards count it
/// (`propagation::awards::counted_entity`): its record's DXCC number, LoTW's first, else its
/// call through cty.dat, a WAE area as the entity it belongs to — what the statistics count it
/// under.
fn counted(r: &QsoRecord) -> Option<String> {
    propagation::awards::counted_entity(
        propagation::dxcc::resolve(&r.call).as_ref(),
        &crate::log_folds::award_fields(r),
    )
    .map(|c| c.name.to_string())
}

/// `q` answered with the app's own resolvers ([`shown`], [`on_card`], [`counted`]): what
/// [`ask_log`] answers.
fn with_app_resolvers(
    queries: &LogQueries,
    engine: &SharedEngine,
    q: &LogQuestion,
) -> Result<Value, String> {
    queries.answer(engine, q, &shown, &on_card, &counted)
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
    tauri::async_runtime::spawn_blocking(move || with_app_resolvers(&queries, &engine, &q))
        .await
        .map_err(|e| format!("the logbook read did not finish: {e}"))?
}

#[cfg(test)]
mod tests;
