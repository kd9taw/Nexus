//! The log's changes as a command makes them, holding the Engine mutex — SPEC-2 v3 §4.6, C19
//! Part B: **planned with the Engine lock released, made under it.**
//!
//! A change to rows the log already holds reads those rows first. Since C19 that read is the
//! store's ([`LogPlan::rows`]), and a read of the store never runs under the Engine lock (the
//! radio loop takes that lock every 20 ms, and `io_fence` stops a debug build that tries). So a
//! command here holds the lock twice, briefly, and never across the read:
//!
//! 1. another window's commits and a `log.adi` something else wrote taken in with the lock
//!    released ([`crate::engine::sync_shared_log`]), then under the lock the plan's handles
//!    ([`StationCore::log_plan`] — pointers, no I/O): [`crate::engine::log_plan`];
//! 2. with it released: the rows, read from the store;
//! 3. under the lock again: the change, made only while the rows it read are still the rows the
//!    log holds ([`StationCore::unchanged_since`]) — otherwise planned again, and after
//!    [`PLANS`] plans refused as `LogBusy` ([`RowRefusal::Busy`]).
//!
//! Each step 3 is O(the rows changed): the decision about each row, the index following its
//! pairs, a channel send to the writer. The command then waits for its change to reach the disk
//! with every lock released ([`Durability::wait`]), as every operator command has since C9.
//!
//! Each function here has a twin on [`StationCore`] that makes the same plan and the same commit
//! in one breath, for an owner holding the log with no Engine guard (a test's own engine).

use std::sync::{Arc, Mutex};

use tempo_core::logbook::{
    LogOp, QsoEdit, QsoRecord, RecordId, RowAfter, UploadService, UploadStatus,
};

use crate::engine::{engine_lock, Engine};
use crate::logstore::Durability;
use crate::station::{
    self, Decided, LogFill, LotwSigned, LotwStamped, MadeRow, RowRefusal, StationCore, PLANS,
};

/// What a change to one row came to: made (`Some` of what the caller answered), found nothing
/// to change (`None`), or refused ([`RowRefusal`]) — or `Err`, the store could not be read.
pub type RowOutcome<T> = Result<Result<Option<T>, RowRefusal>, String>;

/// ★ Plan a change to the contact `id` with the Engine lock released and make it under the lock
/// — the by-id change every command makes (an edit, a QSL mark, a satellite tag, a delete).
///
/// `decide` is handed, under the lock, the row as the plan read it, and answers what to make of
/// it ([`Decided`]). When the change is made, `then` runs in the same hold of the lock with the
/// row as it was and as it now is — what the command answers, and anything that must happen
/// with the change (a corrected call queued to the connectors, the contest log's own row). The
/// durability of the change made, for the command to wait on once the lock is released.
pub fn change_row<T>(
    engine: &Mutex<Engine>,
    id: RecordId,
    context: &str,
    mut decide: impl FnMut(&Engine, &Arc<QsoRecord>) -> Decided,
    mut then: impl FnMut(&mut Engine, MadeRow) -> T,
) -> (RowOutcome<T>, Durability) {
    for _ in 0..PLANS {
        let plan = crate::engine::log_plan(engine);
        let before = match plan.row(id) {
            Ok(Some(row)) => row,
            Ok(None) => return (Ok(Err(RowRefusal::Gone)), Durability::default()),
            Err(e) => return (Err(e), Durability::default()),
        };
        #[cfg(test)]
        tests::race();
        let mut e = engine_lock(engine);
        let (made, durability) = e.with_log_tickets(|e| {
            let (class, after) = match decide(e, &before) {
                Ok(Some(made)) => made,
                Ok(None) => return Some(Ok(Ok(None))),
                Err(refusal) => return Some(Ok(Err(refusal))),
            };
            let after = after.map(Arc::new);
            let rows = vec![(Arc::clone(&before), after.clone())];
            e.station_mut()
                .commit_planned(&plan, class, rows, false, Vec::new(), context)
                .ok()?;
            Some(Ok(Ok(Some(then(e, (Arc::clone(&before), after))))))
        });
        if let Some(made) = made {
            return (made, durability);
        }
    }
    (Ok(Err(RowRefusal::Busy)), Durability::default())
}

/// `ops` made on the contact `id` — a QSL mark, the satellite tag, a delete — only while it is
/// still the version whose edit key is `edit_key` when one is given (a `RowRef`'s check,
/// [`StationCore::fresh_row`]), planned with the Engine lock released ([`change_row`]). What it
/// made, or why not: a row that took none of the ops (a blank satellite name) is answered as
/// [`RowRefusal::Gone`], as the command by row always answered it.
pub fn change_ops(
    engine: &Mutex<Engine>,
    id: RecordId,
    edit_key: Option<&str>,
    ops: &[LogOp],
    context: &str,
) -> (Result<Result<MadeRow, RowRefusal>, String>, Durability) {
    let (made, durability) = change_row(
        engine,
        id,
        context,
        |_, row| {
            if let Some(key) = edit_key {
                StationCore::fresh_row(row, key)?;
            }
            Ok(station::ops_on(row, ops))
        },
        |_, made| made,
    );
    let made = made.map(|made| made.and_then(|made| made.ok_or(RowRefusal::Gone)));
    (made, durability)
}

/// The Logbook form's edit of the contact `id`, as ONE change ([`StationCore::edit_ops`]: the
/// fields, and the QSL-sent and paper-card marks where the form changes them), only while it is
/// still the version whose edit key is `edit_key` — planned with the Engine lock released. The
/// command-side twin of [`Engine::edit_qso`], with what goes with an edit made in the same hold
/// of the lock: a corrected call queued to the connectors as the edit left it, and the contest
/// log's own row corrected. The contact as the change left it, or why not; `Err` for an edit the
/// caller must not send again as it is, or a store that could not be read.
pub fn edit_row(
    engine: &Mutex<Engine>,
    id: RecordId,
    edit_key: &str,
    edit: &QsoEdit,
) -> (
    Result<Result<Arc<QsoRecord>, RowRefusal>, String>,
    Durability,
) {
    let mut bad = None;
    // The row as the field edit alone leaves it: what a corrected call goes back out to the
    // connectors as, as it did when the marks were commands of their own.
    let edited: std::cell::RefCell<Option<QsoRecord>> = std::cell::RefCell::new(None);
    let (made, durability) = change_row(
        engine,
        id,
        "edit_qso",
        |e, stored| match e.station().edit_ops(id, edit_key, edit, stored) {
            Ok(Ok(ops)) => {
                *edited.borrow_mut() = ops.first().and_then(|op| match op.apply_to(stored) {
                    RowAfter::Now(row) => Some(*row),
                    RowAfter::Gone | RowAfter::Unchanged => None,
                });
                Ok(station::ops_on(stored, &ops))
            }
            Ok(Err(refusal)) => Err(refusal),
            Err(e) => {
                bad = Some(e);
                Ok(None)
            }
        },
        |e, (before, after)| {
            if let Some(edited) = edited.borrow().as_ref() {
                e.station_mut().requeue_if_corrected(&before, edited);
            }
            if let Some(after) = &after {
                e.correct_contest_row(after);
            }
            after
        },
    );
    if let Some(e) = bad {
        return (Err(e), durability);
    }
    let made = made.map(|made| match made {
        Ok(Some(Some(after))) => Ok(after),
        Ok(Some(None)) | Ok(None) => Err(RowRefusal::Gone),
        Err(refusal) => Err(refusal),
    });
    (made, durability)
}

/// A correction of the contact `id` as a whole record — the desktop's edit of a row it found by
/// the content on screen, and a Remote browser's — as [`Engine::update_qso`] makes it: `edit`
/// builds the record from the row as it stands, its country is filled from the resolver when it
/// carries none, a corrected call goes back out to the connectors, and the contest log's own row
/// is corrected. Only while the row is still the version whose edit key is `edit_key`, planned
/// with the Engine lock released ([`change_row`]); `then` runs in the hold of the lock that made
/// it, with the row as it was and as it now is.
pub fn update_row<T>(
    engine: &Mutex<Engine>,
    id: RecordId,
    edit_key: &str,
    mut edit: impl FnMut(&QsoRecord) -> QsoRecord,
    mut then: impl FnMut(&mut Engine, &MadeRow) -> T,
) -> (RowOutcome<T>, Durability) {
    change_row(
        engine,
        id,
        "update_qso",
        |e, row| {
            StationCore::fresh_row(row, edit_key)?;
            Ok(station::ops_on(row, &[e.station().edit_op(id, edit(row))]))
        },
        |e, made| {
            if let (before, Some(after)) = &made {
                e.station_mut().requeue_if_corrected(before, after);
                e.correct_contest_row(after);
            }
            then(e, &made)
        },
    )
}

/// The operator's QSL-sent declaration on `id` — `Some(via)` marks it sent, dated now; `None`
/// withdraws it — as [`StationCore::mark_qsl_sent`] makes it.
pub fn qsl_sent(id: RecordId, via: Option<tempo_core::logbook::QslVia>) -> LogOp {
    LogOp::MarkQslSent {
        id,
        via,
        date_unix: crate::engine::now_unix_secs(),
    }
}

/// Stamp a connector's answer for the QSO it pushed (`pushed`) on the row that push names
/// ([`station::push_target`]: by its id while it is still that contact, or — a push that names
/// no row — the newest with its push key), planned with the Engine lock released. Whether a
/// record was stamped, and the durability of the stamp.
pub fn stamp_push(
    engine: &Mutex<Engine>,
    pushed: &QsoRecord,
    service: UploadService,
    status: UploadStatus,
) -> (bool, Durability) {
    for _ in 0..PLANS {
        let plan = crate::engine::log_plan(engine);
        let target = match station::push_target(&plan, pushed) {
            Ok(Some(row)) => row,
            Ok(None) => return (false, Durability::default()),
            Err(e) => {
                tempo_core::applog::error(
                    "logbook",
                    &format!("an upload stamp could not read the logbook: {e}"),
                );
                return (false, Durability::default());
            }
        };
        let Some(rows) = station::stamped_rows(&[target], service, &status) else {
            return (false, Durability::default());
        };
        let (made, durability) = engine_lock(engine).with_log_tickets(|e| {
            e.station_mut()
                .commit_planned(
                    &plan,
                    tempo_core::logbook::OpClass::Stamp,
                    rows,
                    false,
                    Vec::new(),
                    "upload stamp",
                )
                .is_ok()
        });
        if made {
            return (true, durability);
        }
    }
    tempo_core::applog::warn(
        "logbook",
        "an upload stamp's contact kept changing under it; not stamped",
    );
    (false, Durability::default())
}

/// Stamp `upload.lotw` on the contacts of a batch TQSL signed — by id, and only where each row
/// is still the one that was signed ([`station::stamp_lotw_batch_plan`]) — planned with the Engine
/// lock released. What it made of the batch, and the durability of the stamps.
pub fn stamp_lotw_batch(
    engine: &Mutex<Engine>,
    batch: &[LotwSigned],
    status: &UploadStatus,
) -> (LotwStamped, Durability) {
    let unrecorded = LotwStamped {
        unrecorded: batch.len(),
        ..LotwStamped::default()
    };
    for _ in 0..PLANS {
        let plan = crate::engine::log_plan(engine);
        let (report, pairs) = match station::stamp_lotw_batch_plan(&plan, batch, status) {
            Ok(planned) => planned,
            Err(e) => {
                tempo_core::applog::error(
                    "logbook",
                    &format!("a LoTW upload's stamps could not read the logbook: {e}"),
                );
                return (unrecorded, Durability::default());
            }
        };
        let Some(pairs) = pairs else {
            return (report, Durability::default());
        };
        let bulk = pairs.len() > tempo_core::logbook::writer::CHUNK_ROWS;
        let (made, durability) = engine_lock(engine).with_log_tickets(|e| {
            e.station_mut()
                .commit_planned(
                    &plan,
                    tempo_core::logbook::OpClass::Stamp,
                    pairs,
                    bulk,
                    Vec::new(),
                    "lotw upload stamp",
                )
                .is_ok()
        });
        if made {
            return (report, durability);
        }
    }
    tempo_core::applog::warn(
        "logbook",
        "a LoTW upload's contacts kept changing under its stamps; not stamped",
    );
    (unrecorded, Durability::default())
}

/// Stamp `upload.lotw` on every contact still owed to LoTW — the operator's "already uploaded"
/// declaration (the Awards view's "Mark all as uploaded") — the pick and the stamps both read off
/// the store with the Engine lock released, [`STAMP_CHUNK`] contacts to a change. How many were
/// stamped, and their durability.
///
/// Made as one change, the declaration held the Engine lock for as long as the log is long: over
/// half a second on the SPEC-2 v3 §4.11 bench's 500,000-contact log, a third of a million of them
/// owed. A chunk's change is bounded by the chunk. So a declaration can stop partway — a chunk
/// the log keeps changing under, a store that cannot be read, the app closed between chunks — and
/// the chunks made before it stand. It says how far it got, and made again it picks only what is
/// still owed, so it finishes the rest and stamps nothing twice.
pub fn mark_lotw_uploaded_all(
    engine: &Mutex<Engine>,
    when_unix: i64,
) -> (Result<usize, String>, Durability) {
    mark_lotw_uploaded_in_chunks(engine, when_unix, STAMP_CHUNK)
}

/// How many contacts one change of the "already uploaded" declaration stamps: a bound on what one
/// plan reads whole and one commit holds the lock for, whatever the log's size. Set by SPEC-2 v3
/// §4.11's 5 ms bound on an Engine-lock hold, as the fill job's is ([`FILL_CHUNK`]): at 4,096 the
/// declaration's longest hold on the §4.11 bench was 3.07–3.91 ms at 500,000 contacts; at 1,024,
/// 0.86–0.98 ms (1.93 ms with the watcher's blind spot added), for 1–4% more time on the whole
/// declaration.
pub const STAMP_CHUNK: usize = 1_024;

/// [`mark_lotw_uploaded_all`], `chunk` contacts to a change: each chunk planned with the Engine
/// lock released and made under it, planned again while its contacts keep changing — [`PLANS`]
/// plans, then the declaration stops there — and the next chunk planned once it is in the store.
fn mark_lotw_uploaded_in_chunks(
    engine: &Mutex<Engine>,
    when_unix: i64,
    chunk: usize,
) -> (Result<usize, String>, Durability) {
    let rows = engine_lock(engine).log_rows();
    let ids = match station::lotw_unsent_ids(&rows) {
        Ok(ids) => ids,
        Err(e) => return (Err(e), Durability::default()),
    };
    let status = UploadStatus {
        outcome: tempo_core::logbook::UploadOutcome::Accepted,
        when_unix,
        detail: Some(tempo_core::logbook::UploadDetail::OperatorDeclared),
    };
    let (mut stamped, mut durability) = (0, Durability::default());
    for part in ids.chunks(chunk) {
        let mut made = None;
        for _ in 0..PLANS {
            let plan = crate::engine::log_plan(engine);
            let found = match plan.rows(part) {
                Ok(found) => found,
                Err(e) => return (Err(stopped(stamped, ids.len(), Some(e))), durability),
            };
            #[cfg(test)]
            tests::race();
            // Still owed as the plan reads them: a contact a change settled since the pick — an
            // upload's stamp, a confirmation — is left as that change made it.
            let rows: Vec<Arc<QsoRecord>> = part
                .iter()
                .filter_map(|id| found.get(id))
                .filter(|r| station::owed_to_lotw(r))
                .cloned()
                .collect();
            let Some(pairs) = station::stamped_rows(&rows, UploadService::Lotw, &status) else {
                made = Some((0, Durability::default()));
                break;
            };
            let n = pairs.len();
            let bulk = n > tempo_core::logbook::writer::CHUNK_ROWS;
            let (ok, d) = engine_lock(engine).with_log_tickets(|e| {
                e.station_mut()
                    .commit_planned(
                        &plan,
                        tempo_core::logbook::OpClass::Stamp,
                        pairs,
                        bulk,
                        Vec::new(),
                        "lotw upload stamp",
                    )
                    .is_ok()
            });
            if ok {
                made = Some((n, d));
                break;
            }
        }
        let Some((n, d)) = made else {
            return (Err(stopped(stamped, ids.len(), None)), durability);
        };
        stamped += n;
        // The next chunk is planned once this one is in the store. Ahead of the store's writer —
        // a few thousand rows a second — the chunks would pile up in flight, and each commit and
        // each plan's read looks through what is in flight row by row. One chunk, not two: with
        // the writer committing the one before while this one was made, the §4.11 bench read
        // holds over 5 ms here at 500,000 contacts.
        let stored = d.wait_stored(crate::logstore::DURABLE_WAIT);
        durability = durability.and(d);
        if let Err(why) = stored {
            return (Err(stopped(stamped, ids.len(), Some(why))), durability);
        }
    }
    (Ok(stamped), durability)
}

/// What a declaration that stopped at a chunk answers: `why`, the store's words — `None` for a
/// chunk the log kept changing under (LogBusy). Before any chunk is made nothing has changed, and
/// it says what any change says; after, the chunks made stand, and it says how far it got.
fn stopped(stamped: usize, total: usize, why: Option<String>) -> String {
    match (stamped, why) {
        (0, None) => station::LOG_BUSY.into(),
        (0, Some(why)) => why,
        (_, None) => format!(
            "{stamped} of {total} QSOs were marked as already on LoTW, but the logbook kept \
             changing while the rest were being marked, so they were not. Mark them again to \
             finish."
        ),
        (_, Some(why)) => format!(
            "{stamped} of {total} QSOs were marked as already on LoTW, but the rest could not be: \
             {why}. Mark them again to finish."
        ),
    }
}

/// ★ A bulk change — an import, a report merge, the POTA stamps — planned on the candidate sub-log
/// with the Engine lock released and made under it ([`station::plan_on_candidates`],
/// [`StationCore::commit_bulk`]), planned again while what it read keeps changing: [`PLANS`] plans
/// in all, then LogBusy. `plan_it` is handed each plan's handles and answers what the change makes
/// of them; `then` runs in the hold of the lock that made it (or, when it changed nothing, one of
/// its own) with that answer — what the command answers, and what goes with the change.
fn bulk<R, T>(
    engine: &Mutex<Engine>,
    context: &str,
    mut plan_it: impl FnMut(&station::LogPlan) -> Result<(R, station::Planned), String>,
    then: impl FnOnce(&mut Engine, R) -> T,
) -> (Result<T, String>, Durability) {
    let mut then = Some(then);
    for _ in 0..PLANS {
        let plan = crate::engine::log_plan(engine);
        let (out, planned) = match plan_it(&plan) {
            Ok(planned) => planned,
            Err(e) => return (Err(e), Durability::default()),
        };
        #[cfg(test)]
        tests::race();
        let mut out = Some(out);
        let (made, durability) = engine_lock(engine).with_log_tickets(|e| {
            if !planned.is_empty() {
                e.station_mut().commit_bulk(&plan, planned, context).ok()?;
            }
            let then = then.take()?;
            Some(then(e, out.take()?))
        });
        if let Some(made) = made {
            return (Ok(made), durability);
        }
    }
    (Err(station::LOG_BUSY.into()), Durability::default())
}

/// How many contacts the log holds, read with the Engine lock released — with every change this
/// process made before it counted.
fn log_len(engine: &Mutex<Engine>) -> Result<usize, String> {
    let rows = engine_lock(engine).log_rows();
    rows.count()
        .map(|(n, _)| n as usize)
        .map_err(|e| e.to_string())
}

/// An ADIF import ([`StationCore::import_adif`]), planned with the Engine lock released:
/// `(added, skipped, merged)`.
fn import(
    engine: &Mutex<Engine>,
    text: &str,
) -> (Result<(usize, usize, usize), String>, Durability) {
    bulk(
        engine,
        "import_adif",
        |plan| station::plan_import(plan, text),
        |_, counts| counts,
    )
}

/// What an import answers: `(added, skipped, merged, total)` — the contacts it added, the ones the
/// log already held, those of them it upgraded, and the log's size after it.
pub type ImportCounts = (usize, usize, usize, usize);

/// The Logbook's Import ([`StationCore::import_adif`]), planned with the Engine lock released:
/// its [`ImportCounts`], the total counted once the import is made.
pub fn import_adif(
    engine: &Mutex<Engine>,
    text: &str,
) -> (Result<ImportCounts, String>, Durability) {
    let (made, durability) = import(engine, text);
    let made =
        made.and_then(|(added, skipped, merged)| Ok((added, skipped, merged, log_len(engine)?)));
    (made, durability)
}

/// The companion import of the contact WSJT-X logged (its `LoggedAdif` record) — made beside the
/// radio loop, never in its tick, by the same import as the Logbook's ([`import_adif`]), with no
/// count of the log after it: nobody is answered. How many contacts it added.
pub fn import_logged_contact(
    engine: &Mutex<Engine>,
    adif: &str,
) -> (Result<usize, String>, Durability) {
    let (made, durability) = import(engine, adif);
    (made.map(|(added, _, _)| added), durability)
}

/// A LoTW confirmation report merged ([`StationCore::merge_lotw_report`]), planned with the Engine
/// lock released. Its summary.
pub fn merge_lotw_report(
    engine: &Mutex<Engine>,
    text: &str,
) -> (
    Result<tempo_core::reconcile::ReconcileSummary, String>,
    Durability,
) {
    bulk(
        engine,
        "merge_lotw_report",
        |plan| station::plan_report(plan, text),
        |e, summary| {
            e.station_mut().last_lotw_reconcile = Some(summary.clone());
            summary
        },
    )
}

/// An eQSL confirmation report merged ([`StationCore::merge_eqsl_report`]), planned with the Engine
/// lock released. Its summary.
pub fn merge_eqsl_report(
    engine: &Mutex<Engine>,
    text: &str,
) -> (
    Result<tempo_core::reconcile::ReconcileSummary, String>,
    Durability,
) {
    bulk(
        engine,
        "merge_eqsl_report",
        |plan| station::plan_report(plan, text),
        |e, summary| {
            e.station_mut().last_eqsl_reconcile = Some(summary.clone());
            summary
        },
    )
}

/// QRZ's download merged ([`StationCore::merge_qrz_report`]), planned with the Engine lock
/// released. How many contacts it added, and its summary.
pub fn merge_qrz_report(
    engine: &Mutex<Engine>,
    text: &str,
) -> (
    Result<(usize, tempo_core::reconcile::ReconcileSummary), String>,
    Durability,
) {
    bulk(
        engine,
        "merge_qrz_report",
        |plan| station::plan_download(plan, text),
        |e, (added, summary)| {
            e.station_mut().last_qrz_reconcile = Some(summary.clone());
            (added, summary)
        },
    )
}

/// A pota.app export's park references stamped ([`StationCore::import_pota_log`]), planned with
/// the Engine lock released: `(stamped, already, unmatched)`.
pub fn import_pota_log(
    engine: &Mutex<Engine>,
    text: &str,
) -> (Result<(usize, usize, usize), String>, Durability) {
    bulk(
        engine,
        "import_pota_log",
        |plan| station::plan_ota_refs(plan, text),
        |_, counts| counts,
    )
}

/// LoTW's own-QSO report merged ([`StationCore::merge_lotw_own_echo`]), planned with the Engine
/// lock released: how many uploads were newly promoted.
pub fn merge_lotw_own_echo(
    engine: &Mutex<Engine>,
    text: &str,
    when_unix: i64,
) -> (Result<usize, String>, Durability) {
    bulk(
        engine,
        "merge_lotw_own_echo",
        |plan| station::plan_own_echo(plan, text, when_unix),
        |_, promoted| promoted,
    )
}

/// The Field Day merge ([`Engine::fd_merge_to_general`]), its "already there" — every merge
/// identity the general log holds — read with the Engine lock released, and the merge made under
/// it only while the log has not changed since ([`Engine::fd_merge_planned`]): planned again
/// otherwise, [`PLANS`] plans in all. What it merged; `Err` outside Field Day.
pub fn fd_merge_to_general(
    engine: &Mutex<Engine>,
) -> (Result<tempo_core::contest::MergeReport, String>, Durability) {
    for _ in 0..PLANS {
        crate::engine::sync_shared_log(engine);
        let plan = {
            let mut e = engine_lock(engine);
            if !e.in_field_day() {
                return (
                    Err("Field Day mode is not active".into()),
                    Durability::default(),
                );
            }
            e.station_mut().log_plan()
        };
        let seen = match plan.merge_identities() {
            Ok(seen) => seen,
            Err(e) => return (Err(e), Durability::default()),
        };
        #[cfg(test)]
        tests::race();
        let (made, durability) =
            engine_lock(engine).with_log_tickets(|e| e.fd_merge_planned(&plan, seen));
        match made {
            Ok(Some(report)) => return (Ok(report), durability),
            Ok(None) => {}
            Err(e) => return (Err(e), durability),
        }
    }
    (Err(station::LOG_BUSY.into()), Durability::default())
}

/// How many fills one change of the fill job carries: a bound on what one plan reads whole and
/// one commit holds the lock for, whatever the log's size. Its own value, set by SPEC-2 v3 §4.11's
/// 5 ms bound on an Engine-lock hold, not the store's read chunk (`RECORD_CHUNK`). A chunk's commit
/// costs the lock about 0.7 µs a row at 500,000 contacts — the check that every row is as the
/// plan read it, then the change — so 4,096 rows held it 2.5–3.2 ms, and a transient that slowed
/// a whole hold twofold took one to 6.4 ms. At 1,024 the §4.11 bench's longest fill hold was
/// 1.37–1.40 ms at 500,000 contacts (2.63 ms with the watcher's blind spot added), for 8–10% more
/// time on the whole job.
pub const FILL_CHUNK: usize = 1_024;

/// Write the fill job's fills (SPEC-2 v3 D2-A) — each planned with the Engine lock released, a
/// chunk of [`FILL_CHUNK`] at a time, and made under the lock only where the field is still empty
/// in the row as it now stands ([`station::fill_pairs`]), and planned only once every chunk but
/// the one made last is in the store. `fill_ver` goes with the LAST chunk, and only once every
/// earlier chunk is on disk: the store names the resolver data its fills come from only when it
/// holds every one of them. How many contacts gained a field.
///
/// ⚠️ It reads the store and waits for it: call it on the job's own thread, with no lock held.
pub fn fill(engine: &Mutex<Engine>, fills: &[LogFill], fill_ver: i64) -> Result<usize, String> {
    fill_in_chunks(engine, fills, fill_ver, FILL_CHUNK)
}

/// [`fill`], `chunk` fills to a change.
fn fill_in_chunks(
    engine: &Mutex<Engine>,
    fills: &[LogFill],
    fill_ver: i64,
    chunk: usize,
) -> Result<usize, String> {
    let chunks: Vec<&[LogFill]> = if fills.is_empty() {
        vec![&[]]
    } else {
        fills.chunks(chunk).collect()
    };
    let last = chunks.len() - 1;
    let mut filled = 0;
    // The chunk made before the latest: in the store before the next is planned.
    let mut previous = Durability::default();
    for (k, chunk) in chunks.into_iter().enumerate() {
        if k == last {
            // `fill_ver` only once every earlier fill is on disk.
            previous.wait_stored(crate::logstore::DURABLE_WAIT)?;
        }
        let meta = if k == last {
            vec![(crate::logfill::FILL_VER, fill_ver)]
        } else {
            Vec::new()
        };
        let ids: Vec<RecordId> = chunk.iter().map(|f| f.id).collect();
        let mut made = None;
        for _ in 0..PLANS {
            let plan = crate::engine::log_plan(engine);
            let rows = plan.rows(&ids)?;
            #[cfg(test)]
            tests::race();
            let pairs = station::fill_pairs(&rows, chunk);
            let n = pairs.len();
            let (ok, durability) = engine_lock(engine).with_log_tickets(|e| {
                e.station_mut()
                    .commit_planned(
                        &plan,
                        station::fill_class(n),
                        pairs,
                        true,
                        meta.clone(),
                        "fill",
                    )
                    .is_ok()
            });
            if ok {
                made = Some((n, durability));
                break;
            }
        }
        let Some((n, durability)) = made else {
            return Err(station::LOG_BUSY.into());
        };
        filled += n;
        // The store's writer takes this chunk while the next is planned, and the one before it
        // is in the store first. Ahead of the writer — a few thousand rows a second — the chunks
        // would pile up in flight, and each commit and each plan's read looks through what is in
        // flight row by row: two chunks, never the whole job.
        previous.wait_stored(crate::logstore::DURABLE_WAIT)?;
        previous = durability;
    }
    Ok(filled)
}

#[cfg(test)]
mod tests;
