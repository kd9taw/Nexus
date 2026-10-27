//! The Logbook's change history and its undo, as the UI asks for them: a contact's history, the
//! contacts deleted and not put back, and the undo of one change, of a run, or of a delete
//! ([`tempo_app::loghistory`]). A run is undone by the id the change that made it handed back
//! (an import's, a purge's, an undo's own for its redo) — never by "the latest of a kind", which
//! a contact logged a moment later would be.
//!
//! Every read runs with the Engine lock released. Every undo is planned with the lock released and
//! made under it, made again when the store turns it back ([`tempo_app::logwrite::until_written`]),
//! and answers only once it is on disk ([`crate::durable_command`]). None sends anything anywhere.

use serde::Serialize;
use std::sync::Arc;
use tauri::State;
use tempo_core::logbook::history::{CannotUndo, Entry, Kind, CONTEST_COLUMNS_TAG};
use tempo_core::logbook::RecordId;

use crate::{durable_command, SharedEngine};

/// One field a change changed.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldChangeDto {
    pub tag: String,
    pub old: Option<String>,
    pub new: Option<String>,
}

/// One entry of a contact's history.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntryDto {
    pub seq: i64,
    /// `create`, `edit`, `delete`, `merge` or `fill` — or `unknown`, a kind a later build
    /// recorded, which is never undone.
    pub kind: &'static str,
    /// Who made it: `operator`, `import`, `fill`, `lotw`, `eqsl`, `qrz`, `pota`, `contest`,
    /// `remote`, `file` or `undo` (or a word a later build wrote).
    pub source: String,
    pub run: String,
    pub at_ms: i64,
    /// The entry this one undid.
    pub undoes: Option<i64>,
    /// The latest entry that undid this one.
    pub undone_by: Option<i64>,
    /// The fields it changed. Empty for a new contact and a delete (a delete keeps the contact
    /// whole, for the restore).
    pub changes: Vec<FieldChangeDto>,
}

fn entries_dto(entries: Vec<Entry>) -> Vec<HistoryEntryDto> {
    let undone_by = |seq: i64| {
        entries
            .iter()
            .filter(|e| e.undoes == Some(seq))
            .map(|e| e.seq)
            .max()
    };
    entries
        .iter()
        .map(|e| HistoryEntryDto {
            seq: e.seq,
            kind: e.kind.word(),
            source: e.source.clone(),
            run: e.run.clone(),
            at_ms: e.at_ms,
            undoes: e.undoes,
            undone_by: undone_by(e.seq),
            changes: match e.kind {
                Kind::Delete | Kind::Create => Vec::new(),
                _ => e.changes().into_iter().flat_map(shown).collect(),
            },
        })
        .collect()
}

/// One field change as the history shows it. A merged contest contact's directed columns are one
/// history-only field ([`CONTEST_COLUMNS_TAG`]); shown, they are each column that changed, as
/// `TAG (contest)`, and none where they did not (an entry carries them unchanged as its mark).
fn shown(c: tempo_core::logbook::history::FieldChange) -> Vec<FieldChangeDto> {
    if c.tag != CONTEST_COLUMNS_TAG {
        return vec![FieldChangeDto {
            tag: c.tag,
            old: c.old,
            new: c.new,
        }];
    }
    let columns =
        |v: &Option<String>| tempo_core::contest::carrier::decode_pairs(v.as_deref().unwrap_or(""));
    let (old, new) = (columns(&c.old), columns(&c.new));
    let values = |pairs: &[(String, String)], tag: &str| {
        let v: Vec<&str> = pairs
            .iter()
            .filter(|(t, _)| t == tag)
            .map(|(_, v)| v.as_str())
            .collect();
        (!v.is_empty()).then(|| v.join(", "))
    };
    let mut tags: Vec<&str> = Vec::new();
    for (t, _) in old.iter().chain(&new) {
        if !tags.contains(&t.as_str()) {
            tags.push(t);
        }
    }
    tags.into_iter()
        .filter_map(|tag| {
            let (was, now) = (values(&old, tag), values(&new, tag));
            (was != now).then(|| FieldChangeDto {
                tag: tempo_core::logbook::history::contest_column_label(tag),
                old: was,
                new: now,
            })
        })
        .collect()
}

/// A malformed id names no contact: an id is only ever one the station handed out.
fn id_of(id: &str) -> Result<RecordId, String> {
    id.parse()
        .map_err(|()| format!("'{id}' is not a contact id."))
}

/// One contact's history, oldest change first.
#[tauri::command]
pub async fn qso_history(
    state: State<'_, SharedEngine>,
    id: String,
) -> Result<Vec<HistoryEntryDto>, String> {
    let engine = Arc::clone(&state);
    let id = id_of(&id)?;
    tokio::task::spawn_blocking(move || tempo_app::loghistory::history_of(&engine, id))
        .await
        .map_err(|e| format!("logbook task failed: {e}"))?
        .map(entries_dto)
}

/// A contact deleted and not put back.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeletedQsoDto {
    pub id: String,
    pub call: String,
    pub band: String,
    pub mode: String,
    pub when_unix: u64,
    pub deleted_at_ms: i64,
    pub source: String,
}

/// The contacts deleted and not put back, newest delete first.
#[tauri::command]
pub async fn recently_deleted_qsos(
    state: State<'_, SharedEngine>,
    limit: Option<u32>,
) -> Result<Vec<DeletedQsoDto>, String> {
    let engine = Arc::clone(&state);
    let limit = limit.unwrap_or(200).clamp(1, 5_000) as usize;
    let entries = tokio::task::spawn_blocking(move || {
        tempo_app::loghistory::recently_deleted(&engine, limit)
    })
    .await
    .map_err(|e| format!("logbook task failed: {e}"))??;
    Ok(entries
        .iter()
        .filter_map(|e| {
            let rec = e.deleted_contact()?;
            Some(DeletedQsoDto {
                id: e.qso_id.to_string(),
                call: rec.call,
                band: rec.band,
                mode: rec.mode,
                when_unix: rec.when_unix,
                deleted_at_ms: e.at_ms,
                source: e.source.clone(),
            })
        })
        .collect())
}

/// A purge, an import, the one-time contest-column copy or a whole run's undo whose whole-run undo
/// can still be asked for.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryRunDto {
    /// What `undo_history_run` takes.
    pub run: String,
    /// `purge`, `import`, `contestColumns` (the received grid, name and QTH of merged contest
    /// contacts copied into their own fields) or `undo` (a whole run's undo: undone, it is the
    /// redo).
    pub kind: &'static str,
    pub at_ms: i64,
    /// How many contacts it changed.
    pub contacts: u64,
    /// Whether undoing it takes contacts out of the log; else it puts them back.
    pub removes: bool,
}

fn run_dto(r: tempo_core::logbook::history::RunSummary) -> HistoryRunDto {
    HistoryRunDto {
        kind: match r.source.as_str() {
            "import" => "import",
            "undo" => "undo",
            "contest-columns" => "contestColumns",
            _ => "purge",
        },
        run: r.run,
        at_ms: r.at_ms,
        contacts: r.contacts,
        removes: r.removes,
    }
}

/// The purges, imports and whole-run undos whose whole-run undo can still be asked for, newest
/// first: the lasting way to undo one after its toast has gone, and to redo an undo after its own.
/// Each is undone by `undo_history_run`, with its wait while the radio transmits. Readable from a
/// computer driving the station; the undo itself is the station's only.
#[tauri::command]
pub async fn undoable_history_runs(
    state: State<'_, SharedEngine>,
) -> Result<Vec<HistoryRunDto>, String> {
    let engine = Arc::clone(&state);
    let runs =
        tokio::task::spawn_blocking(move || tempo_app::loghistory::undoable_runs(&engine, 20))
            .await
            .map_err(|e| format!("logbook task failed: {e}"))??;
    Ok(runs.into_iter().map(run_dto).collect())
}

/// What an undo made.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoAnswer {
    /// How many contacts it changed.
    pub changed: usize,
    /// The undo's own run — undoing it is the redo. `None` when it changed nothing.
    pub run: Option<String>,
    /// The fields a later change moved, which the undo left as they are.
    pub kept: Vec<String>,
    /// The fields the undo left as they are because putting them back would move a contact under
    /// a call it was not made under: a blank station call, with another call in Settings.
    pub held: Vec<String>,
    /// The call in Settings a blank put back would have sent those contacts under, when a field
    /// was held.
    pub held_under: Option<String>,
    /// How many contacts it could not change, by why: `gone` (no longer in the log),
    /// `alreadyThere` (nothing to put back), `changedSince` (every field changed again since, or
    /// a contact it would take out changed or uploaded since, which stays), `duplicate` (logged
    /// again since it was deleted: the same station, band and mode within 5 minutes),
    /// `unknownKind` (a change a later build recorded).
    pub gone: usize,
    pub already_there: usize,
    pub changed_since: usize,
    pub duplicate: usize,
    pub unknown_kind: usize,
    /// The calls of the contacts left as they are because they changed since, and of those not
    /// put back because the log holds them already: sorted, each once.
    pub changed_since_calls: Vec<String>,
    pub duplicate_calls: Vec<String>,
    /// The calls of the contacts whose callsign it changed with nothing known of where the call
    /// it replaced was sent (the session that corrected it has ended): marked not uploaded to
    /// every service. Sorted, each once.
    pub owed_calls: Vec<String>,
    /// The contacts whose callsign it changed while the call it replaced has gone, or may have
    /// gone, to the services named: marked not uploaded there. Sorted by call. A correction no
    /// service was sent is not here: its undo cancelled the re-send and put the marks back.
    pub owed_at: Vec<OwedAt>,
}

/// A contact an undo left marked not uploaded at the services named ([`UndoAnswer::owed_at`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct OwedAt {
    pub call: String,
    /// The services' own names (LoTW, QRZ, Club Log, …).
    pub services: Vec<String>,
}

fn answer(undone: tempo_app::loghistory::Undone) -> UndoAnswer {
    let count = |why: CannotUndo| undone.refused.iter().filter(|(_, w)| *w == why).count();
    let mut kept: Vec<String> = undone.kept.iter().map(|(_, tag)| tag.clone()).collect();
    kept.sort();
    kept.dedup();
    let mut held: Vec<String> = undone.held.iter().map(|(_, tag)| tag.clone()).collect();
    held.sort();
    held.dedup();
    let calls = |why: CannotUndo| {
        let mut calls: Vec<String> = undone
            .refused
            .iter()
            .filter(|(_, w)| *w == why)
            .filter_map(|(id, _)| undone.calls.get(id).cloned())
            .collect();
        calls.sort();
        calls.dedup();
        calls
    };
    let mut owed_calls: Vec<String> = undone
        .owed
        .iter()
        .filter(|o| o.services.is_empty())
        .map(|o| o.call.clone())
        .collect();
    owed_calls.sort();
    owed_calls.dedup();
    let mut owed_at: Vec<OwedAt> = undone
        .owed
        .iter()
        .filter(|o| !o.services.is_empty())
        .map(|o| OwedAt {
            call: o.call.clone(),
            services: o.services.iter().map(|s| s.to_string()).collect(),
        })
        .collect();
    owed_at.sort_by(|a, b| a.call.cmp(&b.call));
    UndoAnswer {
        changed: undone.changed,
        run: undone.run.clone(),
        kept,
        held,
        held_under: undone.held_under.clone(),
        gone: count(CannotUndo::Gone),
        already_there: count(CannotUndo::AlreadyThere),
        changed_since: count(CannotUndo::ChangedSince),
        duplicate: count(CannotUndo::Duplicate),
        unknown_kind: count(CannotUndo::UnknownKind),
        changed_since_calls: calls(CannotUndo::ChangedSince),
        duplicate_calls: calls(CannotUndo::Duplicate),
        owed_calls,
        owed_at,
    }
}

/// Undo one change of a contact's history.
#[tauri::command]
pub async fn undo_qso_change(
    state: State<'_, SharedEngine>,
    seq: i64,
) -> Result<UndoAnswer, String> {
    let engine = Arc::clone(&state);
    durable_command(move || {
        let (made, durability) =
            tempo_app::logwrite::until_written(|| tempo_app::loghistory::undo_entry(&engine, seq));
        (made.map(answer), durability)
    })
    .await
}

/// Undo every change one run made, as one transaction. Only at the station: a whole run's undo can
/// delete as much of the log as a purge (`command_classes`).
#[tauri::command]
pub async fn undo_history_run(
    state: State<'_, SharedEngine>,
    run: String,
) -> Result<UndoAnswer, String> {
    let engine = Arc::clone(&state);
    durable_command(move || {
        let (made, durability) =
            tempo_app::logwrite::until_written(|| tempo_app::loghistory::undo_run(&engine, &run));
        (made.map(answer), durability)
    })
    .await
}

/// Undo a run that changed one contact: the Redo of one change's undo or of one restore, which a
/// computer driving the station may ask for as it may the undo. A run over more than one contact
/// is refused; that is `undo_history_run`'s, at the station.
#[tauri::command]
pub async fn redo_qso_change(
    state: State<'_, SharedEngine>,
    run: String,
) -> Result<UndoAnswer, String> {
    let engine = Arc::clone(&state);
    durable_command(move || {
        let (made, durability) = tempo_app::logwrite::until_written(|| {
            tempo_app::loghistory::undo_contact_run(&engine, &run)
        });
        (made.map(answer), durability)
    })
    .await
}

/// Put a deleted contact back, as its latest delete took it out — unless it was logged again
/// since (`duplicate`). `anyway`: put it back all the same, the operator's "Restore anyway".
#[tauri::command]
pub async fn restore_deleted_qso(
    state: State<'_, SharedEngine>,
    id: String,
    anyway: Option<bool>,
) -> Result<UndoAnswer, String> {
    let engine = Arc::clone(&state);
    let id = id_of(&id)?;
    let anyway = anyway.unwrap_or(false);
    durable_command(move || {
        let (made, durability) = tempo_app::logwrite::until_written(|| {
            if anyway {
                tempo_app::loghistory::restore_deleted_anyway(&engine, id)
            } else {
                tempo_app::loghistory::restore_deleted(&engine, id)
            }
        });
        (made.map(answer), durability)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempo_core::logbook::history::RunSummary;

    /// A merged contest contact's columns show as each column that changed, never as the
    /// history's own tag, and not at all where an entry carries them unchanged.
    #[test]
    fn a_contest_columns_change_shows_each_column_that_changed() {
        let entry = |before: &str, after: &str| Entry {
            seq: 1,
            qso_id: "~0000000000000001".parse().unwrap(),
            kind: Kind::Fill,
            run: "r".into(),
            at_ms: 1,
            source: "fill".into(),
            undoes: None,
            before: Some(before.into()),
            after: Some(after.into()),
        };
        let cols = |v: &str| format!("<{CONTEST_COLUMNS_TAG}:{}>{v}", v.len());
        let moved = entry(
            &cols("MY_GRIDSQUARE::EN53;GRIDSQUARE::FN31"),
            &format!("<GRIDSQUARE:4>FN31{}", cols("MY_GRIDSQUARE::EN53")),
        );
        let shown: Vec<(String, Option<String>, Option<String>)> = entries_dto(vec![moved])[0]
            .changes
            .iter()
            .map(|c| (c.tag.clone(), c.old.clone(), c.new.clone()))
            .collect();
        assert_eq!(
            shown,
            [
                (
                    "GRIDSQUARE (contest)".to_string(),
                    Some("FN31".to_string()),
                    None
                ),
                ("GRIDSQUARE".to_string(), None, Some("FN31".to_string())),
            ]
        );
        let unchanged = entry(
            &format!("<NAME:3>BOB{}", cols("CQZ::3")),
            &format!("<NAME:5>HIRAM{}", cols("CQZ::3")),
        );
        let dto = entries_dto(vec![unchanged]);
        assert_eq!(dto[0].changes.len(), 1);
        assert_eq!(dto[0].changes[0].tag, "NAME");
    }

    /// Each listed run reaches the page as what it is, and which way its undo moves contacts.
    #[test]
    fn a_listed_run_says_what_it_is_and_which_way_its_undo_goes() {
        let run = |source: &str, removes: bool| {
            let dto = run_dto(RunSummary {
                run: "r".into(),
                source: source.into(),
                at_ms: 5,
                contacts: 41,
                removes,
            });
            (dto.kind, dto.removes, dto.contacts, dto.at_ms)
        };
        assert_eq!(run("operator", false), ("purge", false, 41, 5));
        assert_eq!(run("import", true), ("import", true, 41, 5));
        assert_eq!(run("undo", true), ("undo", true, 41, 5));
        assert_eq!(run("undo", false), ("undo", false, 41, 5));
        assert_eq!(
            run("contest-columns", false),
            ("contestColumns", false, 41, 5)
        );
    }
}
