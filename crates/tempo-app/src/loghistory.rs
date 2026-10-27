//! The log's change history as the app keeps and uses it: where each change comes from, and the
//! undo of one change, of a deleted contact, or of a whole run.
//!
//! The history itself is the store's ([`tempo_core::logbook::history`]): written with the rows,
//! in the same transaction, from what the store held. What this module adds is the half only the
//! app knows — **who asked** — and the undo, which is a change like any other:
//!
//! - **Where a change comes from** ([`note`]). Every change the station makes is handed to the
//!   writer with a [`Note`]: its source, read off the context its maker names (an import, a
//!   download, the fill job, an edit), or the [`Origin`] its maker set in the same hold of the
//!   lock (an undo, the contest merge, a job's run spanning several changes). An append from a
//!   Remote client is the remote's; every other append is the operator's.
//! - **An undo is planned with the Engine lock released and made under it**, as every by-id
//!   change is ([`crate::logwrite`]): the entries and the contacts read from the store, the
//!   contacts' new state decided off the lock ([`tempo_core::logbook::history::undo`]), and the
//!   change made only while every contact it read is still the one it read. Made through the
//!   station's one commit, so the hot index, the store, and the `log.adi` mirror all follow it,
//!   and recorded as [`Source::Undo`] with a link to what it undid — itself undoable: a redo.
//! - **A run's undo is ONE transaction** ([`tempo_core::logbook::writer::Change::whole`]): every
//!   contact the run touched goes back, or, if the store refuses any of it, none does. A contact
//!   that cannot go back (taken out since, or every field it changed changed again) is left as it
//!   is and named in the answer; the rest still go back together.
//!
//! **An undo never sends anything to a service.** It calls no connector and queues no upload. It
//! does not touch an upload stamp either — except as the edit rule already does: an undo that
//! changes a contact's callsign clears its stamps, as a corrected call does, so the contact is
//! owed to the services under the call it now carries and goes with the next upload, and the
//! answer names it ([`Undone::owed`]). A corrected call made by an edit is queued to the
//! connectors at once (`requeue_if_corrected`); an undo's is not.
//!
//! **The undo of a callsign correction** (operator rulings) is the one undo that puts stamps back,
//! and it decides service by service, in the same hold of the Engine lock as the change
//! ([`corrections_undone`]). A service the corrected call has not been sent to — its share of the
//! correction's re-send still on the queue, untried, or never to be tried (switched off), and no
//! stamp of it taking the call; for LoTW, no stamp and no batch running — gets back the marks the
//! correction took off (its stamp; LoTW's with `LOTW_QSL_SENT`). A service that has the corrected
//! call, or may have, stays marked not uploaded and is named in the answer. What is queued under
//! the call the undo replaces is taken off the queue: the log no longer holds that call. After a
//! restart nothing is known of what was sent, and the contact stays owed to every service. No
//! upload state is recorded that cannot be shown to be true.
//!
//! **What the undo of a run leaves alone**, and names in the answer ([`Undone`]):
//!
//! - a contact the run added that has been changed since by any other run (the operator, a
//!   download, a later import, a later build — not the fill job, nor an undo), or uploaded since:
//!   it stays ([`CannotUndo::ChangedSince`]);
//! - a contact it would put back that was logged again after it was deleted — a contact added
//!   since, not by an undo, with the same station, band and mode within the log's duplicate
//!   window, as `log_qso`'s guard asks — re-imported, downloaded or logged by hand: it is not put
//!   back twice ([`CannotUndo::Duplicate`], read off the lock from the history and asked of the
//!   hot index under it, [`crate::station::StationCore::commit_undo`]). A contact the log held
//!   beside it before the delete never blocks it: the log held both, as an import that keeps two
//!   contacts a second apart holds both. The operator can restore one anyway
//!   ([`restore_deleted_anyway`]);
//! - an entry of a kind a later build recorded ([`CannotUndo::UnknownKind`]);
//! - an entry the operator already undid on its own (an undo linked to it, not redone since,
//!   [`history::undone_in`]), in a whole run's undo: planned as any other, so what it can still
//!   take back it does (a single undo keeps a field changed since), but not named as refused.
//! - a blank station call it would put back where the call in Settings is another call than the
//!   one the contact names: the contact would be owed to every service under a call it was not
//!   made under, so the call stays, with its stamps ([`Undone::held`], [`history::undo`]).
//!
//! **A whole run's undo of more than [`tempo_core::logbook::writer::CHUNK_ROWS`] contacts waits
//! while the radio transmits** ([`MUST_WAIT_FOR_TX`]): it is made in one hold of the Engine lock
//! (one transaction, never chunked), and the radio loop needs that lock at every slot boundary to
//! key and to unkey.
//!
//! **A run is undone by its id**, which the change that made it hands back — an import's
//! ([`crate::logwrite::import_adif_in_run`]), a purge's
//! ([`crate::engine::Engine::clear_logbook_in_run`]), an undo's own for its redo — never by "the
//! latest run of a kind" ([`latest_run`], kept for display): a contact a companion program or the
//! FT sequencer logs a moment later is the latest import, or the latest of the operator's.
//!
//! **The history lives in the store.** On the fallback path (1.13's: the log kept in `log.adi`,
//! its rows in a store in memory, `crate::logstore`) the history is in that memory store too, and
//! it ends with the session: Recently deleted is empty after a restart and nothing from an earlier
//! session can be undone. No contact is lost by it — `log.adi` holds the log — only the means to
//! undo what was done before the restart.
//!
//! Not undone here: the contest log's own row (Field Day's journal) — an undo changes the general
//! log only.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use tempo_core::logbook::history::{self, CannotUndo, Entry, Kind, Note, RunSummary, Source, Undo};
use tempo_core::logbook::hot::RowPair;
use tempo_core::logbook::{OpClass, QsoRecord, RecordId};

use crate::engine::{engine_lock, Engine};
use crate::logstore::{Durability, READ_WAIT};
use crate::station::PLANS;

/// Where a change comes from, when its maker says so: set on the station in the hold of the lock
/// that makes the change, and taken by it ([`crate::station::StationCore::origin`]).
#[derive(Debug, Clone)]
pub struct Origin {
    pub(crate) source: Source,
    /// The run it belongs to; a new one when `None`.
    pub(crate) run: Option<String>,
    /// For an undo: the entry it undoes, by contact.
    pub(crate) undoes: HashMap<RecordId, i64>,
    /// Written in one transaction ([`tempo_core::logbook::writer::Change::whole`]).
    pub(crate) whole: bool,
}

impl From<Source> for Origin {
    fn from(source: Source) -> Origin {
        Origin {
            source,
            run: None,
            undoes: HashMap::new(),
            whole: false,
        }
    }
}

impl Origin {
    /// A change of `source` in the run `run`: one of several changes a job makes, undone together.
    pub(crate) fn in_run(source: Source, run: &str) -> Origin {
        Origin {
            run: Some(run.to_string()),
            ..Origin::from(source)
        }
    }
}

/// The source a change's context names: the maker's own word for the change, which every commit
/// carries ([`crate::station::StationCore::commit`]). The operator's, unless it names a download,
/// an import, the fill job or another program's `log.adi`.
pub(crate) fn source_of(context: &str) -> Source {
    match context {
        "import_adif" => Source::Import,
        "merge_lotw_report" | "merge_lotw_own_echo" => Source::Lotw,
        "merge_eqsl_report" => Source::Eqsl,
        "merge_qrz_report" | "gain_qrz_confirmations" => Source::Qrz,
        "import_pota_log" => Source::Pota,
        "fill" => Source::Fill,
        "take in log.adi" => Source::File,
        _ => Source::Operator,
    }
}

/// What a change of `source` to a contact the log holds is called.
fn edit_kind(source: Source) -> Kind {
    match source {
        Source::Fill | Source::ContestColumns => Kind::Fill,
        Source::Import | Source::Lotw | Source::Eqsl | Source::Qrz | Source::Pota => Kind::Merge,
        _ => Kind::Edit,
    }
}

/// A new run id, unique across every window and session sharing the store: a number drawn once
/// per process, and a count.
pub fn new_run() -> String {
    static PROCESS: OnceLock<u64> = OnceLock::new();
    static NEXT: AtomicU64 = AtomicU64::new(1);
    let process = *PROCESS.get_or_init(|| {
        use std::hash::{BuildHasher, Hasher};
        let mut h = std::collections::hash_map::RandomState::new().build_hasher();
        h.write_u128(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos()),
        );
        h.write_u32(std::process::id());
        h.finish()
    });
    format!("{process:016x}-{}", NEXT.fetch_add(1, Ordering::Relaxed))
}

/// The note a change carries to the store: `origin` where its maker set one, else the source its
/// `context` names, in a run of its own.
pub(crate) fn note(origin: Option<Origin>, context: &str) -> Arc<Note> {
    let origin = origin.unwrap_or_else(|| Origin::from(source_of(context)));
    let at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    Arc::new(Note {
        run: origin.run.unwrap_or_else(new_run),
        at_ms,
        source: origin.source,
        edit: edit_kind(origin.source),
        undoes: origin.undoes,
    })
}

// ── reads ─────────────────────────────────────────────────────────────────────────────────────

/// `f` against the store, once it holds every change this process made before it was asked (or
/// as it stands after [`READ_WAIT`]). ⚠️ Never under the Engine lock.
fn read<T>(
    engine: &Mutex<Engine>,
    f: impl FnOnce(&tempo_core::logbook::sqlite::LogDb) -> tempo_core::logbook::sqlite::Result<T>,
) -> Result<T, String> {
    let reads = engine_lock(engine).log_store_reads();
    reads
        .read(READ_WAIT, f)
        .map(|(t, _)| t)
        .map_err(|e| e.to_string())
}

/// One contact's history, oldest change first.
pub fn history_of(engine: &Mutex<Engine>, id: RecordId) -> Result<Vec<Entry>, String> {
    read(engine, |db| db.history_of(id))
}

/// The contacts deleted and not put back, newest first, at most `limit`.
pub fn recently_deleted(engine: &Mutex<Engine>, limit: usize) -> Result<Vec<Entry>, String> {
    read(engine, |db| db.history_deleted(limit))
}

/// The purges and imports whose whole-run undo can still be asked for, newest first, at most
/// `limit` ([`tempo_core::logbook::sqlite::LogDb::history_undoable_runs`]): the lasting way to undo
/// one once its toast has gone. Made within the days Settings keeps deleted contacts (all of them,
/// at 0) — a purge's own contacts leave the history then, and an import is held to the same days.
/// Undone through [`undo_run`], with its transmit check and its guards.
pub fn undoable_runs(engine: &Mutex<Engine>, limit: usize) -> Result<Vec<RunSummary>, String> {
    let days = engine_lock(engine).settings().history_snapshot_days;
    let since_ms = if days == 0 {
        i64::MIN
    } else {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
            .saturating_sub(i64::from(days) * 86_400_000)
    };
    read(engine, |db| db.history_undoable_runs(since_ms, limit))
}

/// The latest run `source` made that changed a contact (any source's when `None`).
pub fn latest_run(
    engine: &Mutex<Engine>,
    source: Option<Source>,
) -> Result<Option<RunSummary>, String> {
    read(engine, |db| db.history_latest_run(source.map(Source::code)))
}

// ── undo ──────────────────────────────────────────────────────────────────────────────────────

/// What an undo made.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Undone {
    /// How many contacts it changed.
    pub changed: usize,
    /// The undo's own run: undoing it is the redo. `None` when it changed nothing.
    pub run: Option<String>,
    /// Fields left as they are because a later change moved them, by contact.
    pub kept: Vec<(RecordId, String)>,
    /// Fields left as they are because putting them back would move the contact under a call it
    /// was not made under (a blank station call, [`history::undo`]), by contact.
    pub held: Vec<(RecordId, String)>,
    /// The call in Settings the undo read, when it held a field: the call a blank station call put
    /// back would have sent those contacts under. `None` when Settings names no call.
    pub held_under: Option<String>,
    /// Contacts it could not change, and why.
    pub refused: Vec<(RecordId, CannotUndo)>,
    /// The call of each contact it could not change that the log holds or would have held, so
    /// the answer can name it.
    pub calls: HashMap<RecordId, String>,
    /// The contacts whose callsign it changed while the call it replaced has gone, or may have
    /// gone, to a service: left marked not uploaded there, for the answer to say so. A contact
    /// whose every service had not been sent that call is not here: its undo cancelled the
    /// re-send and put the marks back.
    pub owed: Vec<Owed>,
}

/// A contact an undo left marked not uploaded at services that may hold the call it replaced
/// ([`Undone::owed`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Owed {
    pub id: RecordId,
    /// The call the contact carries now.
    pub call: String,
    /// The services by name; none when nothing is known of what was sent (the session that made
    /// the correction has ended), when it is owed to every one.
    pub services: Vec<&'static str>,
}

/// Undo the entry `seq`.
pub fn undo_entry(engine: &Mutex<Engine>, seq: i64) -> (Result<Undone, String>, Durability) {
    match read(engine, |db| db.history_entry(seq)) {
        Ok(Some(entry)) => undo_entries(engine, &[entry], false, false),
        Ok(None) => (
            Err("That change is not in the logbook's history.".into()),
            Durability::default(),
        ),
        Err(e) => (Err(e), Durability::default()),
    }
}

/// Put back the contact `id` as its latest delete took it out — unless it was logged again since
/// ([`CannotUndo::Duplicate`]).
pub fn restore_deleted(
    engine: &Mutex<Engine>,
    id: RecordId,
) -> (Result<Undone, String>, Durability) {
    restore(engine, id, false)
}

/// [`restore_deleted`], put back even beside a contact logged again since: the operator's
/// "Restore anyway", when they know the two are different contacts.
pub fn restore_deleted_anyway(
    engine: &Mutex<Engine>,
    id: RecordId,
) -> (Result<Undone, String>, Durability) {
    restore(engine, id, true)
}

fn restore(
    engine: &Mutex<Engine>,
    id: RecordId,
    anyway: bool,
) -> (Result<Undone, String>, Durability) {
    match history_of(engine, id) {
        Ok(entries) => match entries.into_iter().rev().find(|e| e.kind == Kind::Delete) {
            Some(entry) => undo_entries(engine, &[entry], false, anyway),
            None => (
                Err("That contact was never deleted from this logbook.".into()),
                Durability::default(),
            ),
        },
        Err(e) => (Err(e), Durability::default()),
    }
}

/// Undo every change of the run `run`, in one transaction. One of more than
/// [`tempo_core::logbook::writer::CHUNK_ROWS`] contacts is refused while the radio transmits
/// ([`MUST_WAIT_FOR_TX`]).
pub fn undo_run(engine: &Mutex<Engine>, run: &str) -> (Result<Undone, String>, Durability) {
    match read(engine, |db| db.history_run(run)) {
        Ok(entries) if entries.is_empty() => (
            Err("That change is not in the logbook's history.".into()),
            Durability::default(),
        ),
        Ok(entries) => undo_entries(engine, &entries, true, false),
        Err(e) => (Err(e), Durability::default()),
    }
}

/// [`undo_run`] of a run that changed one contact: the redo of one change's undo or of one
/// restore. A run over more than one contact is refused ([`ONE_CONTACT_ONLY`]), so a whole run's
/// undo, which can delete as much of the log as a purge, stays [`undo_run`]'s, asked only at the
/// station (`command_classes`).
pub fn undo_contact_run(engine: &Mutex<Engine>, run: &str) -> (Result<Undone, String>, Durability) {
    match read(engine, |db| db.history_run(run)) {
        Ok(entries) if entries.is_empty() => (
            Err("That change is not in the logbook's history.".into()),
            Durability::default(),
        ),
        Ok(entries) if entries.iter().any(|e| e.qso_id != entries[0].qso_id) => {
            (Err(ONE_CONTACT_ONLY.into()), Durability::default())
        }
        Ok(entries) => undo_entries(engine, &entries, true, false),
        Err(e) => (Err(e), Durability::default()),
    }
}

/// What [`undo_contact_run`] answers for a run over more than one contact.
pub const ONE_CONTACT_ONLY: &str =
    "That changed more than one contact: undo it whole at the station.";

/// What a whole run's undo too large to make in a moment answers while the radio transmits or
/// the FT sequencer is armed ([`crate::engine::Engine::transmit_needs_the_lock`]).
pub const MUST_WAIT_FOR_TX: &str =
    "Stop transmitting first: undoing this many contacts holds up the radio while it is made.";

/// What a restore asks of the history, off the lock, for the station to check under it
/// ([`crate::station::StationCore::commit_undo`]).
#[derive(Debug, Default)]
pub(crate) struct Restores {
    /// For each contact the undo puts back, the delete it undoes.
    pub(crate) deleted_at: HashMap<RecordId, i64>,
    /// The contacts added since the earliest of those deletes, not by an undo, each with its
    /// latest such entry ([`tempo_core::logbook::sqlite::LogDb::history_created_after`]).
    pub(crate) logged_since: HashMap<RecordId, i64>,
    /// Put every one back, whatever the log holds: the operator's "Restore anyway".
    pub(crate) anyway: bool,
}

/// What [`plan_undo`] decided.
struct Planned {
    pairs: Vec<RowPair>,
    class: OpClass,
    /// The entry undone, by contact.
    undoes: HashMap<RecordId, i64>,
    /// For each contact put back, the delete it undoes.
    deleted_at: HashMap<RecordId, i64>,
    answer: Undone,
}

/// What undoing `entries` makes of the contacts they name, as `rows` holds them: a pair per
/// contact that changes, the widest class among them, and the answer. `later` holds, for each
/// contact an entry added, the latest change another run made to it since
/// ([`tempo_core::logbook::sqlite::LogDb::history_changed_after`]): such a contact is kept, not
/// taken out, as is one uploaded since ([`Entry::uploaded_since`]). `settings_call`: the call in
/// Settings, the one a contact naming no station call is uploaded under ([`history::undo`]).
/// `undone`: the entries already undone on their own (a whole run's undo only): planned as any
/// other, but one that cannot be undone again is not named as refused.
///
/// Linear in the entries: each contact's are found through one index of them, built once.
fn plan_undo(
    entries: &[Entry],
    ids: &[RecordId],
    rows: &HashMap<RecordId, Arc<QsoRecord>>,
    later: &HashMap<RecordId, i64>,
    undone: &HashSet<i64>,
    settings_call: &str,
) -> Planned {
    let mut by_id: HashMap<RecordId, Vec<&Entry>> = HashMap::with_capacity(ids.len());
    for e in entries {
        by_id.entry(e.qso_id).or_default().push(e);
    }
    let mut changed: Vec<RowPair> = Vec::new();
    let mut put_back: Vec<RowPair> = Vec::new();
    let mut undoes = HashMap::new();
    let mut deleted_at = HashMap::new();
    let mut answer = Undone::default();
    let mut class = OpClass::Key;
    for id in ids {
        let before = rows.get(id).cloned();
        let mut now: Option<QsoRecord> = before.as_deref().cloned();
        let mut moved = false;
        let mut restored_from = None;
        // Latest first: the run's changes to one contact come off in the order they went on.
        for e in by_id.get(id).into_iter().flatten().rev() {
            let undo = match (e.kind, now.as_ref()) {
                // Added by the entry and changed or uploaded since: it stays.
                (Kind::Create, Some(cur))
                    if later.get(id).is_some_and(|&seq| seq > e.seq) || e.uploaded_since(cur) =>
                {
                    Err(CannotUndo::ChangedSince)
                }
                _ => history::undo(e, now.as_ref(), Some(settings_call)),
            };
            match undo {
                Ok(Undo::Remove(_)) => now = None,
                Ok(Undo::Restore(row)) => {
                    restored_from.get_or_insert(e.seq);
                    now = Some(row);
                }
                Ok(Undo::Revert { row, kept, held }) => {
                    answer.kept.extend(kept.into_iter().map(|tag| (*id, tag)));
                    answer.held.extend(held.into_iter().map(|tag| (*id, tag)));
                    now = Some(row);
                }
                // Already undone on its own: the operator took it back, so no refusal to name.
                Err(_) if undone.contains(&e.seq) => continue,
                Err(why) => {
                    answer.refused.push((*id, why));
                    if let Some(row) = now.as_ref() {
                        answer.calls.insert(*id, row.call.clone());
                    }
                    continue;
                }
            }
            moved = true;
            undoes.insert(*id, e.seq);
        }
        if !moved {
            continue;
        }
        match (before, now) {
            // Every field it would put back held: the contact stays as it is.
            (Some(b), Some(a)) if *b == a => {}
            (Some(b), Some(a)) => changed.push((Some(b), Some(Arc::new(a)))),
            (Some(b), None) => {
                class = OpClass::Structural;
                changed.push((Some(b), None));
            }
            (None, Some(a)) => {
                if let Some(seq) = restored_from {
                    deleted_at.insert(*id, seq);
                }
                put_back.push((None, Some(Arc::new(a))));
            }
            (None, None) => {}
        }
    }
    if !answer.held.is_empty() {
        answer.held_under =
            Some(settings_call.trim().to_ascii_uppercase()).filter(|c| !c.is_empty());
    }
    // The contacts put back go in after the ones changed, as an append's rows go in last.
    changed.extend(put_back);
    answer.changed = changed.len();
    Planned {
        pairs: changed,
        class,
        undoes,
        deleted_at,
        answer,
    }
}

/// The undo's pairs that change a contact's callsign — the undo of a callsign correction, or the
/// redo of one — decided under the Engine lock against the upload queue, service by service
/// (operator rulings; [`crate::station::StationCore::correction_undone`]). Each service that has
/// not been sent the call the undo replaces gets its marks back from before the correction; each
/// that has, or may have, stays marked not uploaded, and the contact is returned with their names
/// for the answer to say so — with none when nothing is known of the correction (after a
/// restart), when it stays owed to every service. Returned too, with the call it carries now, is
/// every such contact: what is queued under that call is cancelled once the undo is made.
#[allow(clippy::type_complexity)]
fn corrections_undone(
    station: &crate::station::StationCore,
    on: u8,
    pairs: &mut [RowPair],
) -> (Vec<(RecordId, String)>, Vec<Owed>) {
    let mut cancel = Vec::new();
    let mut owed = Vec::new();
    for (before, after) in pairs.iter_mut() {
        let (Some(b), Some(a)) = (before.as_ref(), after.as_mut()) else {
            continue;
        };
        let Some(id) = b.id else { continue };
        if a.call.trim().eq_ignore_ascii_case(b.call.trim()) {
            continue;
        }
        cancel.push((id, b.call.clone()));
        let Some(undone) = station.correction_undone(b, a, on) else {
            owed.push(Owed {
                id,
                call: a.call.clone(),
                services: Vec::new(),
            });
            continue;
        };
        let row = Arc::make_mut(a);
        row.upload = undone.upload;
        let sent = row
            .extra
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case("LOTW_QSL_SENT"));
        if let (Some(v), false) = (undone.lotw_qsl_sent, sent) {
            row.extra.push(("LOTW_QSL_SENT".into(), v));
        }
        if !undone.reached.is_empty() {
            owed.push(Owed {
                id,
                call: row.call.clone(),
                services: undone.reached,
            });
        }
    }
    (cancel, owed)
}

/// `f` against the store once it holds every change this process made before it was asked —
/// `None` when it still does not after [`READ_WAIT`]: what a plan must not decide on, since the
/// change it has not taken may be the one that matters. ⚠️ Never under the Engine lock.
fn read_current<T>(
    engine: &Mutex<Engine>,
    f: impl FnOnce(&tempo_core::logbook::sqlite::LogDb) -> tempo_core::logbook::sqlite::Result<T>,
) -> Result<Option<T>, String> {
    let reads = engine_lock(engine).log_store_reads();
    match reads.read(READ_WAIT, f) {
        Ok((t, crate::logstore::Freshness::Current)) => Ok(Some(t)),
        Ok(_) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// Undo `entries` — one change's, or a run's, `whole` — planned with the Engine lock released and
/// made under it, planned again while a contact it read keeps changing ([`PLANS`] plans).
/// `anyway`: put back every contact it would put back, whatever the log holds.
fn undo_entries(
    engine: &Mutex<Engine>,
    entries: &[Entry],
    whole: bool,
    anyway: bool,
) -> (Result<Undone, String>, Durability) {
    let mut seen = HashSet::with_capacity(entries.len());
    let ids: Vec<RecordId> = entries
        .iter()
        .map(|e| e.qso_id)
        .filter(|id| seen.insert(*id))
        .collect();
    let runs: BTreeSet<&str> = entries
        .iter()
        .filter(|e| e.kind == Kind::Create)
        .map(|e| e.run.as_str())
        .collect();
    // A whole run's own: what the operator undid of it one entry at a time is not named refused.
    let whole_runs: BTreeSet<&str> = if whole {
        entries.iter().map(|e| e.run.as_str()).collect()
    } else {
        BTreeSet::new()
    };
    let run = new_run();
    let settings_call = crate::logwrite::settings_call(engine);
    for _ in 0..PLANS {
        let plan = crate::engine::log_plan(engine);
        let rows = match plan.rows(&ids) {
            Ok(rows) => rows,
            Err(e) => return (Err(e), Durability::default()),
        };
        // The history is read after the plan, each time it is taken: a change made before the
        // plan is in what is read, and one made after it is the commit's precondition's to catch.
        // First, the contacts an entry added that another run changed since. A store still behind
        // this process's changes is not decided on: the plan is taken again.
        let mut later = HashMap::new();
        let mut behind = false;
        for run in &runs {
            match read_current(engine, |db| db.history_changed_after(run)) {
                Ok(Some(found)) => later.extend(found),
                Ok(None) => behind = true,
                Err(e) => return (Err(e), Durability::default()),
            }
        }
        // Then, for a whole run, the entries the operator already undid one at a time.
        let mut undone = HashSet::new();
        for run in &whole_runs {
            match read_current(engine, |db| db.history_undone_in(run)) {
                Ok(Some(found)) => undone.extend(found),
                Ok(None) => behind = true,
                Err(e) => return (Err(e), Durability::default()),
            }
        }
        if behind {
            continue;
        }
        let Planned {
            pairs,
            class,
            undoes,
            deleted_at,
            mut answer,
        } = plan_undo(entries, &ids, &rows, &later, &undone, &settings_call);
        if pairs.is_empty() {
            return (Ok(answer), Durability::default());
        }
        // Then the contacts logged since the first delete it undoes: what may block a restore.
        let logged_since = match deleted_at.values().min() {
            Some(&first) if !anyway => {
                match read_current(engine, |db| db.history_created_after(first)) {
                    Ok(Some(found)) => found,
                    Ok(None) => continue,
                    Err(e) => return (Err(e), Durability::default()),
                }
            }
            _ => HashMap::new(),
        };
        let restores = Restores {
            deleted_at,
            logged_since,
            anyway,
        };
        // The call of each contact it would put back, to name one it leaves out.
        let names: HashMap<RecordId, String> = pairs
            .iter()
            .filter(|(b, _)| b.is_none())
            .filter_map(|(_, a)| a.as_ref())
            .filter_map(|a| Some((a.id?, a.call.clone())))
            .collect();
        let large = whole && pairs.len() > tempo_core::logbook::writer::CHUNK_ROWS;
        #[cfg(test)]
        tests::race();
        let mut origin = Some(Origin {
            source: Source::Undo,
            run: Some(run.clone()),
            undoes,
            whole,
        });
        let mut pairs = pairs;
        // `pairs` and `origin` stay out here unless the change takes them, and are dropped with
        // the lock released: a whole run's are as big as the run.
        let (made, durability) = engine_lock(engine).with_log_tickets(|e| {
            // Before any of the work, in the same hold: the radio loop must not wait it out.
            if large && e.transmit_needs_the_lock() {
                return Err(MUST_WAIT_FOR_TX);
            }
            let on = crate::engine::upload_legs::on(e.settings());
            let (cancel, owed) = corrections_undone(e.station(), on, &mut pairs);
            let made =
                e.station_mut()
                    .commit_undo(&plan, class, &mut pairs, &mut origin, &restores);
            // Taken by the change it was set for — or by none, when the plan went stale.
            e.station_mut().origin = None;
            let made = made
                .ok()
                .map(|(ticket, duplicates)| (ticket.is_some(), duplicates));
            // What is queued under the calls it undid goes only with the change, in the same
            // hold: the upload worker never sees it in between.
            if matches!(made, Some((true, _))) {
                for (id, call) in cancel {
                    e.station_mut().cancel_queued_uploads(id, &call);
                }
            }
            Ok(made.map(|(committed, duplicates)| (committed, duplicates, owed)))
        });
        match made {
            Err(why) => return (Err(why.into()), Durability::default()),
            Ok(None) => {}
            Ok(Some((committed, duplicates, owed))) => {
                if committed {
                    answer.owed = owed;
                }
                // The contacts logged again since they were deleted were left out of it.
                answer.changed -= duplicates.len();
                for id in &duplicates {
                    answer.refused.push((*id, CannotUndo::Duplicate));
                    if let Some(call) = names.get(id) {
                        answer.calls.insert(*id, call.clone());
                    }
                }
                answer.run = committed.then_some(run);
                return (Ok(answer), durability);
            }
        }
    }
    (Err(crate::station::LOG_BUSY.into()), Durability::default())
}

#[cfg(test)]
mod tests;
