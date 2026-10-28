//! The Logbook's changes to one contact, addressed by `RowRef {id, editKey}` (SPEC-2 v2 §3):
//! the contact's id, and the edit key of the version the caller holds (`QsoEdit::key`).
//!
//! They sit beside the commands that take the row the view showed (`edit_qso`, `mark_qsl_sent`,
//! …), which find it again by its content. A view that reads the log by id sends these. Each is
//! refused, changing nothing, when no contact has the id (`gone`) or when the contact is no
//! longer the version the caller held (`changed`, with the contact as it now stands for the
//! caller to show and retry against). The key covers only what an edit can write, so an upload
//! stamp or a confirmation landing meanwhile never refuses one. A change that is made returns
//! only once it is on disk ([`crate::durable_command`]), with the contact as it now stands and
//! the key the next change to it must send.
//!
//! Each is planned with the Engine lock released and made under it (SPEC-2 v3 C19,
//! [`tempo_app::logwrite`]): the contact is read from the store, and changed only while it is
//! still the contact that was read. One that keeps changing under the change — another writer,
//! every time it is planned — is refused as `busy`, changing nothing (`LogBusy`).
//!
//! The Logbook form's whole edit is ONE change here ([`edit_qso_by_id`]): the fields, and the
//! QSL-sent and paper-card marks where the form changes them, in one commit — where the form
//! used to send three commands, each its own write.

use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt::Write as _;
use std::sync::Arc;
use tauri::State;
use tempo_app::dto::LoggedQso;
#[cfg(test)]
use tempo_app::engine::Engine;
use tempo_app::logstore::LogRows;
use tempo_app::station::{MadeRow, RowRefusal};
use tempo_core::logbook::sqlite::{call_norm_of, Narrow, Order, Scope};
use tempo_core::logbook::{
    qsl_card_queued, FieldEdit, LogOp, QsoEdit, QsoRecord, RecordId, QSL_QUEUE_FIELD,
};

mod picture;

use crate::{durable_command, qsl_via_arg, sat_name_arg, SharedEngine};

/// One contact, as the version whose edit key is `edit_key`.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RowRef {
    pub id: String,
    pub edit_key: String,
}

/// A contact as it now stands, with the key the next change to it must send.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyedRow {
    pub row: LoggedQso,
    pub edit_key: String,
}

/// What a change addressed by a [`RowRef`] did.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum RowAnswer {
    /// Made, and on disk: the contact as it now stands.
    Applied { current: KeyedRow },
    /// Deleted, and on disk.
    Deleted {},
    /// Refused, nothing changed: the contact changed since the caller read it.
    Changed { current: KeyedRow },
    /// Refused, nothing changed: no contact has that id — deleted, or never in this log.
    Gone {},
    /// Refused, nothing changed: the contact kept changing while the change was being made —
    /// another writer changed it every time it was planned (`LogBusy`, SPEC-2 v3 §4.6). The
    /// caller may try again.
    Busy {},
}

/// `r` as a page of the log shows it (its entity as a page gives it, [`crate::log_queries::shown`]),
/// with its key.
fn keyed(r: &QsoRecord) -> KeyedRow {
    let mut row = LoggedQso::from(r.clone());
    row.entity = crate::log_queries::shown(r);
    KeyedRow {
        row,
        edit_key: QsoEdit::project(r).key(),
    }
}

fn refused(refusal: RowRefusal) -> RowAnswer {
    match refusal {
        RowRefusal::Gone => RowAnswer::Gone {},
        RowRefusal::Changed(now) => RowAnswer::Changed {
            current: keyed(&now),
        },
        RowRefusal::Busy => RowAnswer::Busy {},
    }
}

/// The contact after a change to it was made, as the change left it.
fn applied(made: MadeRow) -> RowAnswer {
    match made.1 {
        Some(now) => RowAnswer::Applied {
            current: keyed(&now),
        },
        None => RowAnswer::Deleted {},
    }
}

/// A malformed id names no contact: an id is only ever one the station handed out.
fn id_of(target: &RowRef) -> Result<RecordId, String> {
    target
        .id
        .parse()
        .map_err(|()| format!("'{}' is not a contact id.", target.id))
}

/// Make `op` on the contact `target` names — only while it is still the version the caller
/// held — and wait for it to reach the disk. Planned with the Engine lock released, and checked
/// again, and made, under it ([`tempo_app::logwrite::change_ops`]).
async fn change_row(
    engine: SharedEngine,
    target: RowRef,
    op: impl FnOnce(RecordId) -> LogOp + Send + 'static,
) -> Result<RowAnswer, String> {
    let id = id_of(&target)?;
    durable_command(move || {
        let op = op(id);
        // Made again, on the version the caller held, when the store turns it back: another
        // window changed the contact first, and the edit key then says so.
        let (made, durability) = tempo_app::logwrite::until_written(|| {
            tempo_app::logwrite::change_ops(
                &engine,
                id,
                Some(&target.edit_key),
                std::slice::from_ref(&op),
                "log change",
            )
        });
        (
            made.map(|made| match made {
                Ok(made) => applied(made),
                Err(refusal) => refused(refusal),
            }),
            durability,
        )
    })
    .await
}

/// The Logbook form's edit of one contact, as ONE change: the fields, the QSL-sent mark and the
/// paper-card mark (`QsoEdit`'s rules). Refused as `gone` / `changed` like every change here;
/// an `Err` is an edit that must not be sent again as it is (a QSL-sent code the form does not
/// offer).
///
/// `fields`: the Logbook contact card's changes to the fields kept as written (`FieldEdit`), in
/// the same change; absent from the edit form.
#[tauri::command]
pub async fn edit_qso_by_id(
    state: State<'_, SharedEngine>,
    target: RowRef,
    edit: QsoEdit,
    fields: Option<Vec<FieldEdit>>,
) -> Result<RowAnswer, String> {
    edit_row_with_fields(Arc::clone(&state), target, edit, fields.unwrap_or_default()).await
}

/// [`edit_row_with_fields`] with no field changes — the edit form's save, as the tests make it.
#[cfg(test)]
pub(crate) async fn edit_row(
    engine: SharedEngine,
    target: RowRef,
    edit: QsoEdit,
) -> Result<RowAnswer, String> {
    edit_row_with_fields(engine, target, edit, Vec::new()).await
}

pub(crate) async fn edit_row_with_fields(
    engine: SharedEngine,
    target: RowRef,
    edit: QsoEdit,
    fields: Vec<FieldEdit>,
) -> Result<RowAnswer, String> {
    let id = id_of(&target)?;
    durable_command(move || {
        let (made, durability) = tempo_app::logwrite::until_written(|| {
            tempo_app::logwrite::edit_row_with_fields(&engine, id, &target.edit_key, &edit, &fields)
        });
        (
            made.map(|made| match made {
                Ok(now) => RowAnswer::Applied {
                    current: keyed(&now),
                },
                Err(refusal) => refused(refusal),
            }),
            durability,
        )
    })
    .await
}

/// [`crate::mark_qsl_sent`], by id: `via` "B" / "D" / "E" marks the contact QSL-sent, dated now;
/// `null`, and only `null`, withdraws the mark.
#[tauri::command]
pub async fn mark_qsl_sent_by_id(
    state: State<'_, SharedEngine>,
    target: RowRef,
    via: Option<String>,
) -> Result<RowAnswer, String> {
    qsl_sent(Arc::clone(&state), target, via).await
}

pub(crate) async fn qsl_sent(
    engine: SharedEngine,
    target: RowRef,
    via: Option<String>,
) -> Result<RowAnswer, String> {
    let via = qsl_via_arg(via.as_deref())?;
    change_row(engine, target, move |id| {
        tempo_app::logwrite::qsl_sent(id, via)
    })
    .await
}

/// [`crate::mark_qsl_card`], by id: whether a paper QSL card arrived.
#[tauri::command]
pub async fn mark_qsl_card_by_id(
    state: State<'_, SharedEngine>,
    target: RowRef,
    received: bool,
) -> Result<RowAnswer, String> {
    qsl_card(Arc::clone(&state), target, received).await
}

pub(crate) async fn qsl_card(
    engine: SharedEngine,
    target: RowRef,
    received: bool,
) -> Result<RowAnswer, String> {
    change_row(engine, target, move |id| LogOp::MarkQslCard {
        id,
        received,
    })
    .await
}

/// [`crate::set_sat_tag`], by id: `satName` tags the contact, `null` removes the tag.
#[tauri::command]
pub async fn set_sat_tag_by_id(
    state: State<'_, SharedEngine>,
    target: RowRef,
    sat_name: Option<String>,
) -> Result<RowAnswer, String> {
    sat_tag(Arc::clone(&state), target, sat_name).await
}

pub(crate) async fn sat_tag(
    engine: SharedEngine,
    target: RowRef,
    sat_name: Option<String>,
) -> Result<RowAnswer, String> {
    // Gated BEFORE the lock, as the command by row is.
    let name = sat_name_arg(sat_name.as_deref())?;
    change_row(engine, target, move |id| LogOp::SetSatTag {
        id,
        sat_name: name,
    })
    .await
}

/// What a `dxcc` argument MEANS on [`set_dxcc_by_id`]: `null` clears the number the contact
/// counts toward (its call counts again), and any other value must name a DXCC entity, current or
/// deleted ([`propagation::dxcc::entity_by_dxcc`]). ADIF's 0 ("no entity") names none, and a
/// record keeps whatever it is given, so a number the awards would pass over is refused here
/// rather than written.
fn dxcc_arg(dxcc: Option<u32>) -> Result<Option<u32>, String> {
    match dxcc {
        Some(code) if propagation::dxcc::entity_by_dxcc(code).is_none() => Err(format!(
            "DXCC {code} is not a DXCC entity — pick one from the list."
        )),
        _ => Ok(dxcc),
    }
}

/// Set — or clear — by hand the DXCC entity the contact counts toward (ADIF `DXCC`): `dxcc` is
/// the entity's number, `null` clears it and the contact counts by its call again. LoTW's number
/// for the contact, where it keeps one, still counts over it (`tempo_core`'s `dxcc_set`). The
/// contact's `COUNTRY` is written with it: the name the number it is left with gives it, or its
/// call's entity where it is left with none (`StationCore::dxcc_ops`).
#[tauri::command]
pub async fn set_dxcc_by_id(
    state: State<'_, SharedEngine>,
    target: RowRef,
    dxcc: Option<u32>,
) -> Result<RowAnswer, String> {
    set_dxcc(Arc::clone(&state), target, dxcc).await
}

pub(crate) async fn set_dxcc(
    engine: SharedEngine,
    target: RowRef,
    dxcc: Option<u32>,
) -> Result<RowAnswer, String> {
    // Gated BEFORE the lock, as the satellite tag is.
    let dxcc = dxcc_arg(dxcc)?;
    let id = id_of(&target)?;
    // Planned on the row as it stands, unlike `change_row`'s ops: the COUNTRY that goes with the
    // number is named from the row's own call, and from LoTW's number where a clear leaves it.
    durable_command(move || {
        let (made, durability) = tempo_app::logwrite::until_written(|| {
            tempo_app::logwrite::set_dxcc(&engine, id, &target.edit_key, dxcc)
        });
        (
            made.map(|made| match made {
                Ok(made) => applied(made),
                Err(refusal) => refused(refusal),
            }),
            durability,
        )
    })
    .await
}

/// [`crate::delete_qso`], by id.
#[tauri::command]
pub async fn delete_qso_by_id(
    state: State<'_, SharedEngine>,
    target: RowRef,
) -> Result<RowAnswer, String> {
    delete_row(Arc::clone(&state), target).await
}

pub(crate) async fn delete_row(engine: SharedEngine, target: RowRef) -> Result<RowAnswer, String> {
    change_row(engine, target, LogOp::Delete).await
}

/// Put the contact `target` in the cards-to-send queue (`queued`: ADIF `QSL_SENT=Q`) or take it
/// out. A field kept as written ([`LogOp::SetFields`]), so it is recorded in the contact's history
/// and undone as any other. Marking the card sent takes it out as well
/// ([`crate::mark_qsl_sent`]).
#[tauri::command]
pub async fn queue_qsl_card_by_id(
    state: State<'_, SharedEngine>,
    target: RowRef,
    queued: bool,
) -> Result<RowAnswer, String> {
    queue_card(Arc::clone(&state), target, queued).await
}

pub(crate) async fn queue_card(
    engine: SharedEngine,
    target: RowRef,
    queued: bool,
) -> Result<RowAnswer, String> {
    change_row(engine, target, move |id| LogOp::SetFields {
        id,
        fields: vec![(QSL_QUEUE_FIELD.to_string(), queued.then(|| "Q".to_string()))],
    })
    .await
}

/// The contacts whose paper QSL card is still to go out ([`qsl_card_queued`]: `QSL_SENT` `Q` or
/// `R`, not marked sent), in log order, each with the key a change to it must send.
#[tauri::command]
pub async fn qsl_card_queue(state: State<'_, SharedEngine>) -> Result<Vec<KeyedRow>, String> {
    let engine = Arc::clone(&state);
    tauri::async_runtime::spawn_blocking(move || card_queue(&engine))
        .await
        .map_err(|e| format!("the logbook read did not finish: {e}"))?
}

/// What the cards-to-send pass reads of each row: the sent mark, and the field the queue is.
const CARD_QUEUE: Narrow = Narrow {
    columns: &["qsl_sent"],
    uploads: false,
};

pub(crate) fn card_queue(engine: &SharedEngine) -> Result<Vec<KeyedRow>, String> {
    // Handles under the lock; the reads after it is released.
    let rows = tempo_app::engine::engine_lock(engine).log_rows();
    let mut ids = Vec::new();
    rows.each_with(
        CARD_QUEUE,
        &[QSL_QUEUE_FIELD],
        Scope::All,
        Order::Log,
        &mut |r| {
            if qsl_card_queued(r) {
                ids.extend(r.id);
            }
            std::ops::ControlFlow::Continue(())
        },
    )
    .map_err(|e| e.to_string())?
    .or_refuse()?;
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let (whole, fresh) = rows.rows_by_ids(&ids).map_err(|e| e.to_string())?;
    fresh.or_refuse()?;
    Ok(whole
        .iter()
        .filter(|r| qsl_card_queued(r))
        .map(keyed)
        .collect())
}

// ── A row the log view showed, found again by its content ───────────────────────────────────
//
// The view hands back the row it showed (`LoggedQso`), never a position; these find the contact
// whose own row that is, with the Engine lock released (one picture of the log, `picture::read`).

/// The contact a log command names: the row the log view showed, keyed ([`seen_target`]). It is
/// found as it stands now, or refused as stale.
#[derive(Clone, Debug)]
pub(crate) struct KeyTarget {
    call: String,
    when_unix: u64,
    key: String,
}

/// The bytes a row key is the SHA-256 of. Numbers are compared in millionths, so a float and the
/// same integer agree.
fn row_canonical(value: &Value) -> String {
    fn walk(value: &Value, out: &mut String) {
        match value {
            Value::Null => out.push('z'),
            Value::Bool(b) => out.push(if *b { 't' } else { 'f' }),
            Value::Number(n) => {
                let x = n.as_f64().unwrap_or(0.0);
                let micro = (x.abs() * 1e6).round();
                let sign = if x < 0.0 && micro != 0.0 { "-" } else { "" };
                let _ = write!(out, "n{sign}{micro:.0};");
            }
            Value::String(s) => {
                let _ = write!(out, "s{}:{s}", s.len());
            }
            Value::Array(items) => {
                let _ = write!(out, "a{}[", items.len());
                items.iter().for_each(|item| walk(item, out));
                out.push(']');
            }
            Value::Object(map) => {
                let mut keys: Vec<&String> = map.keys().collect();
                keys.sort();
                let _ = write!(out, "o{}{{", keys.len());
                for k in keys {
                    let _ = write!(out, "s{}:{k}", k.len());
                    walk(&map[k], out);
                }
                out.push('}');
            }
        }
    }
    let mut out = String::new();
    walk(value, &mut out);
    out
}

fn value_key(row: &Value) -> String {
    digest(&SHA256, row_canonical(row).as_bytes())
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The key of a stored record, built as the log view's row is built (the DTO plus its entity,
/// [`crate::log_queries::shown`]). If that row shape changes, keys stop matching and every change
/// is refused as stale — safe, and caught by the echoed-row test.
pub(crate) fn row_key(record: &QsoRecord) -> String {
    let mut q = tempo_app::dto::LoggedQso::from(record.clone());
    q.entity = crate::log_queries::shown(record);
    serde_json::to_value(q)
        .map(|v| value_key(&v))
        .unwrap_or_default()
}

/// The identity of a row the log view showed: the row itself, exactly as the view was handed it,
/// keyed by its canonical bytes' SHA-256. [`find`] then finds the record whose OWN key matches
/// (the change checks it is still that version where it is made), so the row is a claim to be
/// matched, never an address to trust. The view computes no hash (the desktop webview has no proven `crypto.subtle`), and
/// the view is handed no such keys: keying a whole log on every reload would cost a serialise and a
/// digest per record under the engine lock, where keying one row on one click costs nothing anyone
/// can notice.
pub(crate) fn seen_target(seen: &tempo_app::dto::LoggedQso) -> KeyTarget {
    KeyTarget {
        call: seen.call.clone(),
        when_unix: seen.when_unix,
        key: serde_json::to_value(seen)
            .map(|v| value_key(&v))
            .unwrap_or_default(),
    }
}

/// What finding a key target's contact reads of the contacts with its call: the call and the
/// time, the two things the target names outright.
const CANDIDATES: Narrow = Narrow {
    columns: &["call", "when_unix"],
    uploads: false,
};

/// The contact the log view saw as a [`RowRef`]: its id and the edit key of the version found,
/// or `None` when no contact is that row any more. A position is never the answer: one kept from
/// an earlier read is stale the moment another writer deletes above it, and acting on it deleted
/// or rewrote a different contact.
///
/// It is the FIRST contact in log order whose own row is
/// the row the view saw ([`row_key`]), as the search of the log in memory took it, found in ONE
/// picture of the log with the Engine lock released (SPEC-2 v3 C18). SQL narrows to the contacts
/// whose `call_norm` is the call's, which holds every contact with exactly that call; the call,
/// the time and the key are tested here, on whole records from that same picture.
///
/// ⚠️ A key target reads the store: never under the Engine lock (a debug build panics). Take
/// `rows` (`Engine::log_rows`) under it; the change checks what was found where it is made
/// ([`crate::change_found`]).
pub(crate) fn find(rows: &LogRows, t: &KeyTarget) -> Result<Option<RowRef>, &'static str> {
    let call = call_norm_of(&t.call);
    picture::read(rows, |log| {
        let mut picks = Vec::new();
        log.each_in(Scope::CallNorm(&call), CANDIDATES, &mut |pick, q| {
            if q.call == t.call && q.when_unix == t.when_unix {
                picks.push(pick);
            }
            Ok(())
        })?;
        let first = log.whole(&picks)?.into_iter().find(|r| row_key(r) == t.key);
        Ok(first.and_then(|r| {
            Some(RowRef {
                id: r.id?.to_string(),
                edit_key: QsoEdit::project(&r).key(),
            })
        }))
    })
}

/// The check a change makes where it is made ([`crate::change_found`]: the version found is still the
/// contact's), asked of the store alone — for a test that holds what [`find`] found. The id, or
/// `None` when the contact is gone or another version.
///
/// ⚠️ It reads the store: a test holding the Engine lock holds it raw.
#[cfg(test)]
pub(crate) fn locate(engine: &Engine, row: &RowRef) -> Option<RecordId> {
    let id = row.id.parse().ok()?;
    engine.fresh_log_row(id, &row.edit_key).ok().map(|_| id)
}

#[cfg(test)]
mod find_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::durable_command_tests::engine_on_store;
    use crate::remote_service::stored_log_tests::StoredLog;
    use std::path::Path;
    use tempo_app::engine::engine_lock;
    use tempo_core::logbook::{QslVia, UploadOutcome};

    const COMMANDS: [&str; 8] = [
        "edit_qso_by_id",
        "mark_qsl_sent_by_id",
        "queue_qsl_card_by_id",
        "qsl_card_queue",
        "mark_qsl_card_by_id",
        "set_sat_tag_by_id",
        "set_dxcc_by_id",
        "delete_qso_by_id",
    ];

    /// A command the UI invokes is reachable only if it is defined here AND registered in
    /// `generate_handler!`; a name missing from the list fails at runtime with nothing at compile
    /// time to catch it. Each is an `async fn`, so it never waits for the engine on the UI
    /// thread, and waits for its change to reach the disk on the blocking pool.
    #[test]
    fn every_command_here_is_async_and_registered() {
        let here = include_str!("log_by_id.rs");
        let list = include_str!("lib.rs")
            .split_once("tauri::generate_handler![")
            .expect("the handler list")
            .1
            .split_once("])")
            .expect("the end of the handler list")
            .0;
        for name in COMMANDS {
            // Column zero, so this test's own strings cannot satisfy it.
            assert!(
                here.lines()
                    .any(|l| l.starts_with(&format!("pub async fn {name}("))),
                "{name} is not an async command here"
            );
            assert!(
                list.lines()
                    .any(|l| l.trim() == format!("log_by_id::{name},")),
                "{name} is not registered"
            );
        }
        let commands = here
            .lines()
            .filter(|l| l.trim() == "#[tauri::command]")
            .count();
        assert_eq!(
            commands,
            COMMANDS.len(),
            "a command here is missing from COMMANDS"
        );
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime")
    }

    /// The contact at `at`, as a caller that read it holds it.
    fn row_ref(engine: &SharedEngine, at: usize) -> (RowRef, QsoRecord) {
        let row = QsoRecord::clone(&engine.lock().unwrap().stored_log()[at]);
        let target = RowRef {
            id: row.id.expect("an id").to_string(),
            edit_key: QsoEdit::project(&row).key(),
        };
        (target, row)
    }

    /// The row `id` as the store holds it, read through a connection of the test's own.
    fn on_disk(dir: &Path, id: &str) -> Option<QsoRecord> {
        let db = tempo_core::logbook::migrate::database_path(&dir.join("log.adi"));
        tempo_core::logbook::sqlite::LogDb::open(&db)
            .and_then(|d| d.load_all())
            .expect("read the store")
            .into_iter()
            .find(|r| r.id.is_some_and(|i| i.to_string() == id))
    }

    fn current(answer: RowAnswer) -> KeyedRow {
        match answer {
            RowAnswer::Applied { current } => current,
            other => panic!("expected the change to be made: {other:?}"),
        }
    }

    /// ★ Each change by `RowRef` is made, on disk when it returns, and answers with the contact
    /// as it now stands and the key the NEXT change must send — so a follow-up chained on that
    /// key is made too. The form's edit carries both QSL marks in the one change.
    #[test]
    fn a_change_by_row_ref_is_made_and_answers_the_key_for_the_next() {
        let (dir, engine) = engine_on_store("by-id", 6);
        let rt = runtime();

        let (target, held) = row_ref(&engine, 2);
        let mut edit = QsoEdit::project(&held);
        edit.name = Some("Edited".into());
        edit.qsl_sent_via = Some("D".into());
        edit.qsl_card = true;
        let now = current(
            rt.block_on(edit_row(engine.clone(), target.clone(), edit))
                .expect("made"),
        );
        let stored = on_disk(&dir, &target.id).expect("on disk when the command returned");
        assert_eq!(
            (
                stored.name.as_deref(),
                stored.qsl_sent.via,
                stored.qsl_rcvd.card
            ),
            (Some("Edited"), Some(QslVia::Direct), true)
        );
        assert_eq!(now.row.id.as_deref(), Some(target.id.as_str()));
        assert_eq!(now.edit_key, QsoEdit::project(&stored).key());
        assert_ne!(now.edit_key, target.edit_key, "the edit moved the key");

        // Chained on the answer's key: the marks and the tag are made in turn.
        let next = |key: String| RowRef {
            id: target.id.clone(),
            edit_key: key,
        };
        let now = current(
            rt.block_on(qsl_sent(
                engine.clone(),
                next(now.edit_key),
                Some("B".into()),
            ))
            .expect("made"),
        );
        let now = current(
            rt.block_on(qsl_card(engine.clone(), next(now.edit_key), false))
                .expect("made"),
        );
        let now = current(
            rt.block_on(sat_tag(
                engine.clone(),
                next(now.edit_key),
                Some("AO-91".into()),
            ))
            .expect("made"),
        );
        let stored = on_disk(&dir, &target.id).expect("held");
        assert_eq!(
            (
                stored.qsl_sent.via,
                stored.qsl_rcvd.card,
                stored.sat_name.as_deref()
            ),
            (Some(QslVia::Bureau), false, Some("AO-91"))
        );
        assert_eq!(
            rt.block_on(delete_row(engine.clone(), next(now.edit_key))),
            Ok(RowAnswer::Deleted {})
        );
        assert!(on_disk(&dir, &target.id).is_none(), "deleted on disk");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ The cards-to-send queue end to end, on disk: a queued contact is listed, and only it;
    /// marking its card sent takes it off the list and writes `QSL_SENT=Y` in place of the `Q`,
    /// and nothing else about the contact moves. Taking a card out of the queue lists it no more.
    #[test]
    fn a_queued_card_is_listed_until_it_is_marked_sent_and_nothing_else_moves() {
        let (dir, engine) = engine_on_store("card-queue", 6);
        let rt = runtime();
        assert!(card_queue(&engine).expect("read").is_empty());

        let (target, before) = row_ref(&engine, 2);
        let now = current(
            rt.block_on(queue_card(engine.clone(), target.clone(), true))
                .expect("made"),
        );
        let listed = card_queue(&engine).expect("read");
        assert_eq!(
            listed.iter().map(|k| k.row.id.clone()).collect::<Vec<_>>(),
            vec![Some(target.id.clone())]
        );
        assert_eq!(
            listed[0].edit_key, now.edit_key,
            "listed with the key a change must send"
        );
        let queued = on_disk(&dir, &target.id).expect("held");
        assert!(qsl_card_queued(&queued));

        let next = RowRef {
            id: target.id.clone(),
            edit_key: listed[0].edit_key.clone(),
        };
        current(
            rt.block_on(qsl_sent(engine.clone(), next, Some("B".into())))
                .expect("made"),
        );
        assert!(
            card_queue(&engine).expect("read").is_empty(),
            "sent: off the list"
        );
        let sent = on_disk(&dir, &target.id).expect("held");
        let mut expect = before.clone();
        expect.qsl_sent = sent.qsl_sent;
        assert_eq!(sent, expect, "only the QSL-sent mark moved");
        assert!(sent.qsl_sent.sent);
        let adif = tempo_core::logbook::adif_record(&sent);
        assert_eq!(adif.matches("QSL_SENT:").count(), 1, "{adif}");

        // Queued and taken out again: listed no more, and the contact as it was.
        let (target, before) = row_ref(&engine, 4);
        let now = current(
            rt.block_on(queue_card(engine.clone(), target.clone(), true))
                .expect("made"),
        );
        let back = RowRef {
            id: target.id.clone(),
            edit_key: now.edit_key,
        };
        current(
            rt.block_on(queue_card(engine.clone(), back, false))
                .expect("made"),
        );
        assert!(card_queue(&engine).expect("read").is_empty());
        assert_eq!(on_disk(&dir, &target.id).expect("held").extra, before.extra);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A change to a contact that changed since the caller read it is refused with the contact
    /// as it now stands, and one to a contact that is gone is refused as gone; neither changes
    /// anything. A background stamp is not a change the key sees: it refuses nothing.
    #[test]
    fn a_stale_or_gone_row_ref_is_refused_and_changes_nothing() {
        let (dir, engine) = engine_on_store("by-id-stale", 6);
        let rt = runtime();
        let (target, held) = row_ref(&engine, 3);

        // An upload stamp lands after the read: the card is still made.
        let (stamped, _) = tempo_app::logwrite::stamp_push(
            &engine,
            &held,
            tempo_core::logbook::UploadService::Qrz,
            tempo_core::logbook::UploadStatus {
                outcome: UploadOutcome::Accepted,
                when_unix: 1,
                detail: None,
            },
        );
        assert!(stamped);
        let now = current(
            rt.block_on(qsl_card(engine.clone(), target.clone(), true))
                .expect("made"),
        );

        // Another writer's edit lands after the caller's read of the new version.
        let stale = RowRef {
            id: target.id.clone(),
            edit_key: now.edit_key.clone(),
        };
        {
            let id = held.id.expect("an id");
            let view = engine_lock(&engine).log_view();
            let mut theirs = QsoRecord::clone(&view.row(id).expect("read").expect("held"));
            theirs.grid = Some("FN42".into());
            let edit = LogOp::Edit {
                id,
                rec: Box::new(theirs),
            };
            let (made, _) = tempo_app::logwrite::change_ops(&engine, id, None, &[edit], "theirs");
            assert!(matches!(made, Ok(Ok(_))), "their edit is made");
        }
        let before = engine.lock().unwrap().stored_log();
        for answer in [
            rt.block_on(qsl_sent(engine.clone(), stale.clone(), Some("B".into()))),
            rt.block_on(qsl_card(engine.clone(), stale.clone(), false)),
            rt.block_on(sat_tag(engine.clone(), stale.clone(), None)),
            rt.block_on(set_dxcc(engine.clone(), stale.clone(), Some(182))),
            rt.block_on(delete_row(engine.clone(), stale.clone())),
            rt.block_on(edit_row(
                engine.clone(),
                stale.clone(),
                QsoEdit::project(&held),
            )),
        ] {
            match answer {
                Ok(RowAnswer::Changed { current }) => {
                    assert_eq!(
                        current.row.grid.as_deref(),
                        Some("FN42"),
                        "as it now stands"
                    )
                }
                other => panic!("a stale key must be refused as changed: {other:?}"),
            }
        }
        assert_eq!(
            engine.lock().unwrap().stored_log(),
            before,
            "nothing changed"
        );

        let gone = RowRef {
            id: tempo_core::logbook::RecordId::Provisional {
                hash: 7,
                ordinal: 0,
            }
            .to_string(),
            edit_key: now.edit_key,
        };
        assert_eq!(
            rt.block_on(qsl_card(engine.clone(), gone, true)),
            Ok(RowAnswer::Gone {})
        );
        let malformed = RowRef {
            id: "row 3".into(),
            edit_key: target.edit_key,
        };
        assert!(rt.block_on(delete_row(engine.clone(), malformed)).is_err());
        assert_eq!(engine.lock().unwrap().stored_log(), before);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── the DXCC entity set by hand ─────────────────────────────────────────

    /// A store-backed engine whose log holds `rows` (ADIF records, header added), on a folder of
    /// the test's own, with the station's country resolvers wired as the launch wires them.
    fn engine_with(tag: &str, rows: &str) -> (std::path::PathBuf, SharedEngine) {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("nexus-dxcc-{tag}-{nanos}"));
        std::fs::create_dir_all(&dir).expect("scratch");
        let adif = format!("{}{rows}", tempo_core::logbook::adif_header());
        std::fs::write(dir.join("log.adi"), adif).expect("log");
        let opened = tempo_app::logstore::open(
            &dir.join("log.adi"),
            Arc::new(|_| tempo_core::logbook::sqlite::Resolved::default()),
            None,
        )
        .expect("the store opens");
        let mut e = tempo_app::engine::Engine::new("K2DEF", "FN31", 0);
        e.attach_log_store(opened);
        crate::wire_country_resolvers(&mut e);
        (dir, Arc::new(std::sync::Mutex::new(e)))
    }

    /// The 2015 Navassa operation on its US 1×1 call, as an old log holds it: no DXCC number,
    /// so cty.dat's `=K1N` places it in the United States. Not confirmed.
    const K1N_2015: &str =
        "<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:6>123400<BAND:3>20m<MODE:2>CW<EOR>\n";

    /// [`K1N_2015`] as Nexus's own log holds it: the country its call names, filled when it was
    /// logged.
    const K1N_2015_LOGGED: &str = "<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:6>123400<BAND:3>20m\
        <MODE:2>CW<COUNTRY:13>United States<EOR>\n";

    /// [`K1N_2015`] as another logger exported it: 182 for Navassa, and a `COUNTRY` that the number
    /// contradicts — the call's entity.
    const K1N_2015_CONTRADICTED: &str = "<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:6>123400\
        <BAND:3>20m<MODE:2>CW<DXCC:3>182<COUNTRY:13>United States<EOR>\n";

    /// A contact's (DXCC, COUNTRY).
    fn pair(r: &QsoRecord) -> (Option<u32>, Option<String>) {
        (r.dxcc, r.country.clone())
    }

    /// Each contact's (DXCC, COUNTRY) in the Logbook's ADIF export, as the desktop writes it.
    fn exported(engine: &SharedEngine) -> Vec<(String, Option<u32>, Option<String>)> {
        let source = tempo_app::logexport::Source::of(&engine_lock(engine));
        let file = tempo_app::logexport::export_logbook(&source, "adif", None, None)
            .expect("exported")
            .text;
        tempo_core::logbook::parse_adif(&file)
            .iter()
            .map(|r| (r.call.clone(), r.dxcc, r.country.clone()))
            .collect()
    }

    /// The contact's (DXCC, COUNTRY) in the ADIF a logbook service is sent for the Logbook's `row`
    /// (`upload_record`, every service push's one conversion).
    fn uploaded(row: &LoggedQso) -> (Option<u32>, Option<String>) {
        let adif = tempo_core::logbook::adif_record(&crate::upload_record(row.clone()));
        pair(&tempo_core::logbook::parse_adif(&adif)[0])
    }

    /// The award summary as the Awards view reads it: from the store.
    fn awards(engine: &SharedEngine) -> Arc<propagation::AwardSummary> {
        crate::log_folds::awards_kept(engine, &crate::LogTallies::default()).expect("folded")
    }

    /// The entities worked and not yet confirmed — the Awards view's "new one" chase.
    fn chase(engine: &SharedEngine) -> Vec<String> {
        awards(engine)
            .needed
            .iter()
            .map(|n| n.entity.clone())
            .collect()
    }

    /// ★ SP9RCL's case: an old log's K1N contact carries no DXCC number, so it counts as the
    /// United States. Set to Navassa by hand, it counts as Navassa — stored as ADIF `DXCC`, read
    /// by the awards from the store — and the row the change answers with shows it. Cleared, it
    /// counts by its call again.
    #[test]
    fn a_k1n_contact_set_to_navassa_by_hand_counts_navassa_and_a_clear_falls_back() {
        let (dir, engine) = engine_with("k1n", K1N_2015);
        let rt = runtime();
        assert_eq!(chase(&engine), ["United States"], "premise: the call alone");

        let (target, _) = row_ref(&engine, 0);
        let now = current(
            rt.block_on(set_dxcc(engine.clone(), target.clone(), Some(182)))
                .expect("made"),
        );
        assert_eq!(chase(&engine), ["Navassa Island"]);
        let stored = on_disk(&dir, &target.id).expect("on disk when the command returned");
        assert_eq!(stored.dxcc, Some(182));
        assert_eq!(now.row.entity.as_deref(), Some("Navassa Island"));
        assert_eq!(now.row.dxcc, Some(182));

        let next = RowRef {
            id: target.id.clone(),
            edit_key: now.edit_key,
        };
        let now = current(
            rt.block_on(set_dxcc(engine.clone(), next, None))
                .expect("made"),
        );
        assert_eq!(on_disk(&dir, &target.id).expect("held").dxcc, None);
        assert_eq!(chase(&engine), ["United States"], "the call counts again");
        assert_eq!(now.row.entity.as_deref(), Some("United States"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The number set by hand is the contact's ADIF `DXCC`: the export writes it, and a log that
    /// imports the file counts the contact as Navassa.
    #[test]
    fn a_dxcc_set_by_hand_survives_an_export_and_an_import() {
        let (dir, engine) = engine_with("export", K1N_2015);
        let rt = runtime();
        let (target, _) = row_ref(&engine, 0);
        current(
            rt.block_on(set_dxcc(engine.clone(), target, Some(182)))
                .expect("made"),
        );
        let source = tempo_app::logexport::Source::of(&engine_lock(&engine));
        let file = tempo_app::logexport::export_logbook(&source, "adif", None, None)
            .expect("exported")
            .text;

        let (into, other) = engine_with("import", "");
        let (made, _) =
            tempo_app::logwrite::until_written(|| tempo_app::logwrite::import_adif(&other, &file));
        assert_eq!(made.expect("imported").0, 1, "one contact added");
        assert_eq!(chase(&other), ["Navassa Island"]);
        let held = other.lock().unwrap().stored_log();
        assert_eq!(held.iter().map(|r| r.dxcc).collect::<Vec<_>>(), [Some(182)]);
        assert!(file.contains("<DXCC:3>182"), "{file}");
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_dir_all(&into);
    }

    /// LoTW's number counts over one set by hand. The operator sets the K1N contact to the United
    /// States; LoTW then confirms it as Navassa. The awards count Navassa, confirmed, and list the
    /// two side by side, as they list any contact LoTW confirmed as another entity — the log's
    /// own number is not changed.
    #[test]
    fn lotws_number_counts_over_one_set_by_hand_and_the_two_are_listed() {
        let (dir, engine) = engine_with("lotw", K1N_2015);
        let rt = runtime();
        let (target, _) = row_ref(&engine, 0);
        current(
            rt.block_on(set_dxcc(engine.clone(), target.clone(), Some(291)))
                .expect("made"),
        );
        let report = format!(
            "{}<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:6>123400<BAND:3>20m<MODE:2>CW\
             <LOTW_QSL_RCVD:1>Y<DXCC:3>182<EOR>\n",
            tempo_core::logbook::adif_header()
        );
        let (summary, _) = tempo_app::logwrite::until_written(|| {
            tempo_app::logwrite::merge_lotw_report(&engine, &report)
        });
        assert_eq!(
            summary.expect("merged").matched,
            1,
            "premise: LoTW's row paired"
        );
        let summary = awards(&engine);
        assert_eq!(
            summary
                .dxcc_disagreements
                .iter()
                .map(|x| (
                    x.call.as_str(),
                    x.logged_dxcc,
                    x.logged_entity.as_deref(),
                    x.lotw_dxcc,
                    x.lotw_entity.as_deref()
                ))
                .collect::<Vec<_>>(),
            [(
                "K1N",
                291,
                Some("United States"),
                182,
                Some("Navassa Island")
            )]
        );
        assert_eq!(
            (summary.dxcc_confirmed, summary.needed.len()),
            (1, 0),
            "Navassa, confirmed"
        );
        let stored = on_disk(&dir, &target.id).expect("held");
        assert_eq!(
            (stored.dxcc, tempo_core::logbook::record_lotw_dxcc(&stored)),
            (Some(291), Some(182)),
            "the log keeps its own, LoTW's beside it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ The re-review's probe. The K1N contact, logged as the United States (291), is confirmed by
    /// LoTW as Navassa (182, kept beside the log's) with LoTW's zone. The operator then corrects
    /// the call to K1NX: LoTW's number described the old call's QSO, so the contact no longer
    /// counts as Navassa — not in the Awards, the Needed board's needs or the statistics, which all
    /// read `award_fields` — and LoTW's zone goes with it. By the form and by the whole-contact
    /// edit a Remote browser sends back with the old extras in it.
    #[test]
    fn a_corrected_call_no_longer_counts_under_lotws_number_for_the_old_one() {
        for remote in [false, true] {
            let (dir, engine) = engine_with("lotw-call-fix", K1N_2015);
            let rt = runtime();
            let (target, _) = row_ref(&engine, 0);
            current(
                rt.block_on(set_dxcc(engine.clone(), target.clone(), Some(291)))
                    .expect("made"),
            );
            let report = format!(
                "{}<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:6>123400<BAND:3>20m<MODE:2>CW\
                 <LOTW_QSL_RCVD:1>Y<DXCC:3>182<CQZ:1>8<EOR>\n",
                tempo_core::logbook::adif_header()
            );
            let (summary, _) = tempo_app::logwrite::until_written(|| {
                tempo_app::logwrite::merge_lotw_report(&engine, &report)
            });
            assert_eq!(summary.expect("merged").matched, 1, "premise: paired");
            let held = on_disk(&dir, &target.id).expect("held");
            let counted = |r: &QsoRecord| {
                propagation::awards::counted_entity(None, &crate::log_folds::award_fields(r))
                    .and_then(|c| c.code)
            };
            assert_eq!(
                (
                    counted(&held),
                    tempo_core::logbook::record_cq_zone(&held),
                    awards(&engine).dxcc_confirmed
                ),
                (Some(182), Some(8), 1),
                "premise: Navassa, LoTW's zone, confirmed"
            );

            let key = QsoEdit::project(&held).key();
            if remote {
                // As the `edit_qso` command takes it: the row the client was shown, sent back.
                let mut shown = LoggedQso::from(held.clone());
                shown.call = "K1NX".into();
                let sent: QsoRecord = shown.into();
                let (made, durability) = tempo_app::logwrite::update_row(
                    &engine,
                    target.id.parse().expect("an id"),
                    &key,
                    |stored| crate::edited_whole(&sent, stored),
                    |_, _| (),
                );
                made.expect("reads").expect("made");
                durability
                    .wait(tempo_app::logstore::DURABLE_WAIT)
                    .expect("durable");
            } else {
                let mut fix = QsoEdit::project(&held);
                fix.call = "K1NX".into();
                let next = RowRef {
                    id: target.id.clone(),
                    edit_key: key,
                };
                current(
                    rt.block_on(edit_row(engine.clone(), next, fix))
                        .expect("made"),
                );
            }
            let after = on_disk(&dir, &target.id).expect("held");
            assert_eq!(after.call, "K1NX", "remote: {remote}");
            assert_eq!(
                (
                    tempo_core::logbook::record_lotw_dxcc(&after),
                    tempo_core::logbook::record_cq_zone(&after)
                ),
                (None, None),
                "LoTW's number and zone for the old call are gone (remote: {remote})"
            );
            assert_ne!(counted(&after), Some(182), "remote: {remote}");
            let summary = awards(&engine);
            assert!(
                summary.dxcc_disagreements.is_empty() && summary.dxcc_confirmed == 0,
                "remote: {remote}"
            );
            assert!(
                !chase(&engine).iter().any(|e| e == "Navassa Island"),
                "remote: {remote}"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// The number set by hand survives the edit form, which carries no DXCC and leaves it alone as
    /// it leaves every field it does not carry; a callsign correction drops it with the call it
    /// described.
    #[test]
    fn a_dxcc_set_by_hand_survives_an_edit_and_goes_with_a_corrected_call() {
        let (dir, engine) = engine_with("edit", K1N_2015);
        let rt = runtime();
        let (target, _) = row_ref(&engine, 0);
        let next = |key: String| RowRef {
            id: target.id.clone(),
            edit_key: key,
        };
        let now = current(
            rt.block_on(set_dxcc(engine.clone(), target.clone(), Some(182)))
                .expect("made"),
        );
        let mut edit = QsoEdit::project(&on_disk(&dir, &target.id).expect("held"));
        edit.name = Some("Navassa".into());
        let now = current(
            rt.block_on(edit_row(engine.clone(), next(now.edit_key), edit))
                .expect("made"),
        );
        assert_eq!(
            chase(&engine),
            ["Navassa Island"],
            "an ordinary edit keeps it"
        );
        assert_eq!(on_disk(&dir, &target.id).expect("held").dxcc, Some(182));

        let mut fix = QsoEdit::project(&on_disk(&dir, &target.id).expect("held"));
        fix.call = "K1NA".into();
        current(
            rt.block_on(edit_row(engine.clone(), next(now.edit_key), fix))
                .expect("made"),
        );
        assert_eq!(
            on_disk(&dir, &target.id).expect("held").dxcc,
            None,
            "a corrected call drops it"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Only a number that names a DXCC entity is written — ADIF's 0 ("no entity") and a number the
    /// table does not hold are refused, changing nothing — and a deleted entity is one.
    #[test]
    fn only_a_number_that_names_an_entity_is_set() {
        let (dir, engine) = engine_with("refused", K1N_2015);
        let rt = runtime();
        let (target, _) = row_ref(&engine, 0);
        let before = engine.lock().unwrap().stored_log();
        for code in [0, 73, 523] {
            let refused = rt.block_on(set_dxcc(engine.clone(), target.clone(), Some(code)));
            assert!(refused.is_err(), "{code}: {refused:?}");
        }
        assert_eq!(
            engine.lock().unwrap().stored_log(),
            before,
            "nothing changed"
        );
        current(
            rt.block_on(set_dxcc(engine.clone(), target.clone(), Some(229)))
                .expect("made"),
        );
        assert_eq!(on_disk(&dir, &target.id).expect("held").dxcc, Some(229));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ COUNTRY follows the number set by hand. The K1N contact, logged as the United States, is
    /// set to Navassa: the record, the Logbook's row (the cell and its tooltip), the ADIF export and
    /// what an upload sends all say Navassa Island beside 182 — the two never disagree. Cleared,
    /// the contact is its call's again, COUNTRY with it.
    #[test]
    fn a_country_set_by_hand_and_its_dxcc_number_agree_everywhere_the_contact_goes() {
        let (dir, engine) = engine_with("agree", K1N_2015_LOGGED);
        let rt = runtime();
        let (target, held) = row_ref(&engine, 0);
        let us = (None, Some("United States".to_string()));
        assert_eq!(pair(&held), us, "premise: the call's entity, no number");

        let now = current(
            rt.block_on(set_dxcc(engine.clone(), target.clone(), Some(182)))
                .expect("made"),
        );
        let navassa = (Some(182), Some("Navassa Island".to_string()));
        let stored = on_disk(&dir, &target.id).expect("on disk when the command returned");
        assert_eq!(pair(&stored), navassa, "the record");
        assert_eq!((now.row.dxcc, now.row.country.clone()), navassa, "the row");
        assert_eq!(
            now.row.entity.as_deref(),
            Some("Navassa Island"),
            "the Country cell"
        );
        assert_eq!(
            exported(&engine),
            [("K1N".to_string(), navassa.0, navassa.1.clone())],
            "the export"
        );
        assert_eq!(uploaded(&now.row), navassa, "an upload");

        let next = RowRef {
            id: target.id.clone(),
            edit_key: now.edit_key,
        };
        let now = current(
            rt.block_on(set_dxcc(engine.clone(), next, None))
                .expect("made"),
        );
        assert_eq!(pair(&on_disk(&dir, &target.id).expect("held")), us);
        assert_eq!((now.row.dxcc, now.row.country.clone()), us);
        assert_eq!(now.row.entity.as_deref(), Some("United States"));
        assert_eq!(exported(&engine), [("K1N".to_string(), us.0, us.1.clone())]);
        assert_eq!(uploaded(&now.row), us);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The Logbook form corrects a stored COUNTRY that names another entity than the DXCC number
    /// the contact keeps: a K1N contact imported as 182 beside "United States" is Navassa Island
    /// once the operator saves any edit to it. With no number left, the form names the call's
    /// entity, as it always has.
    #[test]
    fn an_edit_names_the_country_by_the_dxcc_number_the_contact_keeps() {
        let (dir, engine) = engine_with("edit-country", K1N_2015_CONTRADICTED);
        let rt = runtime();
        let (target, held) = row_ref(&engine, 0);
        let next = |key: String| RowRef {
            id: target.id.clone(),
            edit_key: key,
        };
        let mut edit = QsoEdit::project(&held);
        edit.name = Some("Navassa".into());
        let now = current(
            rt.block_on(edit_row(engine.clone(), target.clone(), edit))
                .expect("made"),
        );
        let stored = on_disk(&dir, &target.id).expect("held");
        assert_eq!(
            (pair(&stored), stored.name.as_deref()),
            ((Some(182), Some("Navassa Island".into())), Some("Navassa")),
            "the edit, and the country its number gives"
        );
        assert_eq!(now.row.country.as_deref(), Some("Navassa Island"));

        // Control: the number cleared by hand, the next edit names the call's entity.
        let now = current(
            rt.block_on(set_dxcc(engine.clone(), next(now.edit_key), None))
                .expect("made"),
        );
        let mut edit = QsoEdit::project(&on_disk(&dir, &target.id).expect("held"));
        edit.name = Some("Again".into());
        current(
            rt.block_on(edit_row(engine.clone(), next(now.edit_key), edit))
                .expect("made"),
        );
        assert_eq!(
            pair(&on_disk(&dir, &target.id).expect("held")),
            (None, Some("United States".into()))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ Under the launch's own resolvers, an edit keeps a stored COUNTRY that names the contact's
    /// DXCC entity in another spelling ("fed. rep. of germany") or that the entity table cannot
    /// place ("Germany", QRZ's) exactly as stored; and a corrected callsign takes the new call's
    /// country — by the form, and by the whole-contact edit a Remote browser sends with the old
    /// call's country in it.
    #[test]
    fn an_edit_keeps_an_agreeing_country_and_a_corrected_call_takes_its_own() {
        let rows = [
            "<CALL:6>DL1ABC<QSO_DATE:8>20240101<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW\
             <DXCC:3>230<COUNTRY:7>Germany<EOR>\n",
            "<CALL:6>DL2ABC<QSO_DATE:8>20240102<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW\
             <DXCC:3>230<COUNTRY:20>fed. rep. of germany<EOR>\n",
            "<CALL:6>DL3ABC<QSO_DATE:8>20240103<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW\
             <DXCC:3>230<COUNTRY:20>Fed. Rep. of Germany<EOR>\n",
            "<CALL:6>DL4ABC<QSO_DATE:8>20240104<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW\
             <DXCC:3>230<COUNTRY:20>Fed. Rep. of Germany<EOR>\n",
        ]
        .concat();
        let (dir, engine) = engine_with("edit-country-kept", &rows);
        let rt = runtime();
        for (at, country) in [(0, "Germany"), (1, "fed. rep. of germany")] {
            let (target, held) = row_ref(&engine, at);
            let mut edit = QsoEdit::project(&held);
            edit.name = Some("Hans".into());
            current(
                rt.block_on(edit_row(engine.clone(), target.clone(), edit))
                    .expect("made"),
            );
            let stored = on_disk(&dir, &target.id).expect("held");
            assert_eq!(
                (pair(&stored), stored.name.as_deref()),
                ((Some(230), Some(country.into())), Some("Hans")),
                "kept exactly as stored"
            );
        }

        let (target, held) = row_ref(&engine, 2);
        let mut edit = QsoEdit::project(&held);
        edit.call = "K1ABC".into();
        let now = current(
            rt.block_on(edit_row(engine.clone(), target.clone(), edit))
                .expect("made"),
        );
        assert_eq!(
            pair(&on_disk(&dir, &target.id).expect("held")),
            (None, Some("United States".into())),
            "the form's corrected call"
        );
        assert_eq!(now.row.country.as_deref(), Some("United States"));

        let (target, held) = row_ref(&engine, 3);
        let id = target.id.parse().expect("an id");
        let mut sent = held.clone();
        sent.call = "K2ABC".into();
        assert_eq!(
            sent.country.as_deref(),
            Some("Fed. Rep. of Germany"),
            "premise"
        );
        let (made, durability) = tempo_app::logwrite::update_row(
            &engine,
            id,
            &target.edit_key,
            |stored| crate::edited_whole(&sent, stored),
            |_, _| (),
        );
        made.expect("reads").expect("made");
        durability
            .wait(tempo_app::logstore::DURABLE_WAIT)
            .expect("durable");
        assert_eq!(
            pair(&on_disk(&dir, &target.id).expect("held")),
            (None, Some("United States".into())),
            "the whole-contact edit's corrected call"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ A contact imported with a COUNTRY its DXCC number contradicts — K1N, 182, "United States"
    /// — shows, exports and uploads under the number's name, Navassa Island. Its record is NOT
    /// rewritten: nobody edited it, so the store still holds exactly what was imported. Controls:
    /// a contact with no number, one with DXCC 0 and one with a number the table does not hold
    /// keep their own COUNTRY, since no entity is named to follow.
    #[test]
    fn a_contact_imported_with_a_country_its_number_contradicts_goes_out_under_the_numbers_name() {
        let rows = [
            K1N_2015_CONTRADICTED,
            "<CALL:4>W1AW<QSO_DATE:8>20200101<TIME_ON:6>120000<BAND:3>40m<MODE:2>CW\
             <COUNTRY:24>UNITED STATES OF AMERICA<EOR>\n",
            "<CALL:7>W1AW/MM<QSO_DATE:8>20200102<TIME_ON:6>120000<BAND:3>20m<MODE:3>SSB\
             <DXCC:1>0<COUNTRY:15>MARITIME MOBILE<EOR>\n",
            "<CALL:4>W1AW<QSO_DATE:8>20200103<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW<DXCC:3>523\
             <COUNTRY:24>UNITED STATES OF AMERICA<EOR>\n",
        ]
        .concat();
        let (dir, engine) = engine_with("contradicted", &rows);
        let held = engine.lock().unwrap().stored_log();
        let id = |at: usize| held[at].id.expect("an id").to_string();
        let before: Vec<QsoRecord> = (0..4)
            .map(|at| on_disk(&dir, &id(at)).expect("held"))
            .collect();
        assert_eq!(
            pair(&before[0]),
            (Some(182), Some("United States".into())),
            "premise: imported as it came"
        );

        let row = keyed(&held[0]).row;
        assert_eq!(row.entity.as_deref(), Some("Navassa Island"), "shown");
        assert_eq!(
            exported(&engine),
            [
                ("K1N".to_string(), Some(182), Some("Navassa Island".into())),
                (
                    "W1AW".to_string(),
                    None,
                    Some("UNITED STATES OF AMERICA".into())
                ),
                (
                    "W1AW/MM".to_string(),
                    Some(0),
                    Some("MARITIME MOBILE".into())
                ),
                (
                    "W1AW".to_string(),
                    Some(523),
                    Some("UNITED STATES OF AMERICA".into())
                ),
            ],
            "exported"
        );
        assert_eq!(
            uploaded(&row),
            (Some(182), Some("Navassa Island".into())),
            "uploaded"
        );
        let after: Vec<QsoRecord> = (0..4)
            .map(|at| on_disk(&dir, &id(at)).expect("held"))
            .collect();
        assert_eq!(after, before, "no stored record was rewritten");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A clear that leaves the log LoTW's number — LoTW confirmed the contact as Navassa while the
    /// log held the United States — names COUNTRY by LoTW's number, which the contact now
    /// carries: the number the change LEAVES, not the one the operator sent (none).
    #[test]
    fn a_clear_that_leaves_lotws_number_takes_its_country() {
        let (dir, engine) = engine_with("lotw-country", K1N_2015_LOGGED);
        let rt = runtime();
        let (target, _) = row_ref(&engine, 0);
        current(
            rt.block_on(set_dxcc(engine.clone(), target.clone(), Some(291)))
                .expect("made"),
        );
        let report = format!(
            "{}<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:6>123400<BAND:3>20m<MODE:2>CW\
             <LOTW_QSL_RCVD:1>Y<DXCC:3>182<EOR>\n",
            tempo_core::logbook::adif_header()
        );
        let (summary, _) = tempo_app::logwrite::until_written(|| {
            tempo_app::logwrite::merge_lotw_report(&engine, &report)
        });
        assert_eq!(summary.expect("merged").matched, 1, "premise: paired");
        let (target, held) = row_ref(&engine, 0);
        assert_eq!(
            (pair(&held), tempo_core::logbook::record_lotw_dxcc(&held)),
            ((Some(291), Some("United States".into())), Some(182)),
            "premise: the log's own number, LoTW's beside it"
        );
        current(
            rt.block_on(set_dxcc(engine.clone(), target.clone(), None))
                .expect("made"),
        );
        assert_eq!(
            pair(&on_disk(&dir, &target.id).expect("held")),
            (Some(182), Some("Navassa Island".into()))
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ A WAE area follows its DXCC number strictly. A Sicily contact set to Italy's 248 by hand
    /// is Italy in the record, the row (its Country column and its tooltip), the export and an
    /// upload. One imported with 248 beside "Sicily" is shown, exported and uploaded as Italy, and
    /// its stored record keeps "Sicily": nobody edited it. Without a number a Sicily contact is
    /// still Sicily, as its call places it.
    #[test]
    fn a_wae_areas_contact_carrying_its_parents_number_is_its_parent_everywhere() {
        let rows = [
            "<CALL:6>IT9ABC<QSO_DATE:8>20200101<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW\
             <COUNTRY:6>Sicily<EOR>\n",
            "<CALL:6>IT9XYZ<QSO_DATE:8>20200102<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW\
             <DXCC:3>248<COUNTRY:6>Sicily<EOR>\n",
            "<CALL:6>IT9DEF<QSO_DATE:8>20200103<TIME_ON:6>120000<BAND:3>20m<MODE:2>CW\
             <COUNTRY:6>Sicily<EOR>\n",
        ]
        .concat();
        let (dir, engine) = engine_with("wae", &rows);
        let rt = runtime();
        let held = engine.lock().unwrap().stored_log();
        let italy = (Some(248), Some("Italy".to_string()));
        let sicily = (None, Some("Sicily".to_string()));

        let imported = keyed(&held[1]).row;
        assert_eq!(
            imported.entity.as_deref(),
            Some("Italy"),
            "an imported one's Country column"
        );
        assert_eq!(uploaded(&imported), italy, "an imported one's upload");
        let imported_before = on_disk(&dir, &imported.id.clone().expect("an id")).expect("held");

        let (target, _) = row_ref(&engine, 0);
        let now = current(
            rt.block_on(set_dxcc(engine.clone(), target.clone(), Some(248)))
                .expect("made"),
        );
        assert_eq!(
            pair(&on_disk(&dir, &target.id).expect("held")),
            italy,
            "the record"
        );
        assert_eq!((now.row.dxcc, now.row.country.clone()), italy, "the row");
        assert_eq!(
            now.row.entity.as_deref(),
            Some("Italy"),
            "the Country column"
        );
        assert_eq!(uploaded(&now.row), italy, "an upload");
        assert_eq!(
            exported(&engine),
            [
                ("IT9ABC".to_string(), italy.0, italy.1.clone()),
                ("IT9XYZ".to_string(), italy.0, italy.1.clone()),
                ("IT9DEF".to_string(), sicily.0, sicily.1.clone()),
            ],
            "the export"
        );
        let plain = keyed(&held[2]).row;
        assert_eq!(
            (plain.entity.clone(), (plain.dxcc, plain.country.clone())),
            (Some("Sicily".to_string()), sicily),
            "without a number: its call's WAE area"
        );
        assert_eq!(
            on_disk(&dir, &imported.id.expect("an id")).expect("held"),
            imported_before,
            "the imported record is not rewritten"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── COUNTRY filled from the DXCC number ─────────────────────────────────

    /// A contact's (call, DXCC, COUNTRY), each as the station holds it.
    fn held_countries(engine: &SharedEngine) -> Vec<(String, Option<u32>, Option<String>)> {
        engine
            .lock()
            .unwrap()
            .stored_log()
            .iter()
            .map(|r| (r.call.clone(), r.dxcc, r.country.clone()))
            .collect()
    }

    /// K1N's 2015 contact carrying 182 and no COUNTRY, as a logger that writes `DXCC` only exports
    /// it; and a W1AW contact carrying neither.
    const NUMBER_NO_COUNTRY: &str = "<CALL:3>K1N<QSO_DATE:8>20150210<TIME_ON:6>123400\
        <BAND:3>20m<MODE:2>CW<DXCC:3>182<EOR>\n\
        <CALL:4>W1AW<QSO_DATE:8>20200101<TIME_ON:6>120000<BAND:3>40m<MODE:2>CW<EOR>\n";

    /// What every fill below must leave: the number's name where a number names an entity, else
    /// the call's entity.
    fn filled_by_number() -> Vec<(String, Option<u32>, Option<String>)> {
        vec![
            ("K1N".into(), Some(182), Some("Navassa Island".into())),
            ("W1AW".into(), None, Some("United States".into())),
        ]
    }

    /// ★ An import fills a missing COUNTRY from the contact's DXCC number: K1N with 182 is stored
    /// as Navassa Island, a contact without a number takes its call's entity, and a contact that
    /// carries a COUNTRY keeps it — the fill writes only a missing field.
    #[test]
    fn an_import_fills_a_missing_country_from_the_dxcc_number() {
        let (dir, engine) = engine_with("fill-import", "");
        let text = format!(
            "{}{NUMBER_NO_COUNTRY}{}",
            tempo_core::logbook::adif_header(),
            K1N_2015_CONTRADICTED.replace("20150210", "20150211")
        );
        let (made, _) =
            tempo_app::logwrite::until_written(|| tempo_app::logwrite::import_adif(&engine, &text));
        assert_eq!(made.expect("imported").0, 3, "three contacts added");
        let mut want = filled_by_number();
        want.push(("K1N".into(), Some(182), Some("United States".into())));
        assert_eq!(held_countries(&engine), want);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A contact logged with a DXCC number and no COUNTRY (another program's, or a Remote log
    /// form's) is stored with the number's name, as an import is.
    #[test]
    fn a_contact_logged_with_a_number_and_no_country_takes_the_numbers_name() {
        let (dir, engine) = engine_with("fill-logged", "");
        for rec in tempo_core::logbook::parse_adif(NUMBER_NO_COUNTRY) {
            assert!(rec.country.is_none(), "premise: no COUNTRY");
            engine_lock(&engine).log_qso(rec);
        }
        assert_eq!(held_countries(&engine), filled_by_number());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The 1.13 path's load fills a missing COUNTRY from the DXCC number too, as an insert does.
    #[test]
    fn the_older_log_files_load_fills_a_missing_country_from_the_dxcc_number() {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let dir = std::env::temp_dir().join(format!("nexus-dxcc-fill-file-{nanos}"));
        std::fs::create_dir_all(&dir).expect("scratch");
        let log = dir.join("log.adi");
        std::fs::write(
            &log,
            format!("{}{NUMBER_NO_COUNTRY}", tempo_core::logbook::adif_header()),
        )
        .expect("log");
        let mut e = tempo_app::engine::Engine::new("K2DEF", "FN31", 0);
        crate::wire_country_resolvers(&mut e);
        e.set_log_path(log);
        let engine: SharedEngine = Arc::new(std::sync::Mutex::new(e));
        assert_eq!(held_countries(&engine), filled_by_number());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ The fill job fills a stored contact's missing COUNTRY from its DXCC number, as an insert
    /// does — K1N with 182 → Navassa Island — and from its call where it carries none.
    #[test]
    fn the_fill_job_fills_a_missing_country_from_the_dxcc_number() {
        let (dir, engine) = engine_with("fill-job", NUMBER_NO_COUNTRY);
        assert!(
            held_countries(&engine).iter().all(|(_, _, c)| c.is_none()),
            "premise: the store holds no COUNTRY"
        );
        let done = tempo_app::logfill::fill_log_store(
            &engine,
            1,
            &crate::country_of,
            &|_: &str, _: Option<&str>| None::<String>,
            &|_: &str| false,
        )
        .expect("the job runs");
        assert_eq!((done.lacking, done.filled), (2, 2));
        assert_eq!(held_countries(&engine), filled_by_number());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The fill rule's version moved with the rule, so a store the job filled under the older rule
    /// is filled again. Under that rule a contact its call cannot place, carrying 182, was left
    /// without a country, and it gains Navassa Island now; a contact the older rule filled from its
    /// call keeps that country — the job fills only what is missing.
    #[test]
    fn a_store_filled_under_the_older_fill_rule_is_filled_again() {
        let rows = "<CALL:1>Q<QSO_DATE:8>20150210<TIME_ON:6>123400<BAND:3>20m<MODE:2>CW\
            <DXCC:3>182<EOR>\n\
            <CALL:3>K1N<QSO_DATE:8>20150211<TIME_ON:6>123400<BAND:3>20m<MODE:2>CW<DXCC:3>182<EOR>\n";
        let (dir, engine) = engine_with("fill-rules", rows);
        let k1n = engine.lock().unwrap().stored_log()[1].id.expect("an id");
        // What the job wrote under the older rule: K1N's country from its call, none for Q (its call
        // is nowhere), and that rule's version.
        let older = [tempo_app::station::LogFill {
            id: k1n,
            country: Some("United States".into()),
            state: None,
        }];
        let wrote = tempo_app::logwrite::fill(&engine, &older, crate::fill_version(1))
            .expect("the older fill is written");
        assert_eq!(wrote, 1, "premise: the older rule's fill");
        let done = tempo_app::logfill::fill_log_store(
            &engine,
            crate::log_fill_version(),
            &crate::country_of,
            &|_: &str, _: Option<&str>| None::<String>,
            &|_: &str| false,
        )
        .expect("the job runs");
        assert!(
            !done.current,
            "this rule's version is another: the job runs again"
        );
        assert_eq!(
            held_countries(&engine),
            [
                (
                    "Q".to_string(),
                    Some(182),
                    Some("Navassa Island".to_string())
                ),
                (
                    "K1N".to_string(),
                    Some(182),
                    Some("United States".to_string())
                ),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── positions never name a contact ──────────────────────────────────────

    /// A parameter name that says "a place in the log".
    const POSITIONAL: [&str; 9] = [
        "index",
        "indices",
        "idx",
        "at",
        "pos",
        "position",
        "positions",
        "row",
        "rows",
    ];

    /// `src` with every test-only item left out: an item under `#[cfg(test)]` or
    /// `#[cfg(all(test, …))]` — its further attributes and doc comments, then through its `;`,
    /// or through the line that closes it (rustfmt closes an item alone on a line at its indent).
    fn production(src: &str) -> String {
        let lines: Vec<&str> = src.lines().collect();
        let mut out = String::new();
        let mut i = 0;
        while i < lines.len() {
            let t = lines[i].trim_start();
            if !(t.starts_with("#[cfg(test)]") || t.starts_with("#[cfg(all(test")) {
                out.push_str(lines[i]);
                out.push('\n');
                i += 1;
                continue;
            }
            let indent = &lines[i][..lines[i].len() - t.len()];
            i += 1;
            while i < lines.len() && {
                let t = lines[i].trim_start();
                t.starts_with("#[") || t.starts_with("//")
            } {
                i += 1;
            }
            if i < lines.len() && lines[i].trim_end().ends_with(';') {
                i += 1;
                continue;
            }
            let close = format!("{indent}}}");
            while i < lines.len() && lines[i] != close {
                i += 1;
            }
            i += 1;
        }
        out
    }

    /// Whether a function's visibility reaches outside its crate: `pub`, not `pub(crate)`.
    fn public(visibility: &str) -> bool {
        visibility == "pub" || visibility.starts_with("pub ")
    }

    /// Every production `fn` in `src` that names a place in the log: a parameter called like a
    /// position and typed `usize` (alone or in a slice, `Vec` or `Option`), or a `usize` handed
    /// back by a function whose name says it finds one. As `(visibility, name)`.
    fn positional_fns(src: &str) -> Vec<(String, String)> {
        let src = production(src);
        let mut found = Vec::new();
        for (at, _) in src.match_indices("fn ") {
            let before = &src[..at];
            if before
                .chars()
                .last()
                .is_some_and(|c| c.is_alphanumeric() || c == '_')
            {
                continue; // `…fn ` inside another word
            }
            let rest = &src[at + 3..];
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            let Some(open) = rest.find('(') else { continue };
            if name.is_empty() || rest[name.len()..open].contains(['{', ';', '\n']) {
                continue;
            }
            let mut depth = 0;
            let mut close = open;
            for (j, c) in rest[open..].char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' => {
                        depth -= 1;
                        if depth == 0 {
                            close = open + j;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            let params = &rest[open + 1..close];
            let mut nesting = 0;
            let mut parts = vec![String::new()];
            for c in params.chars() {
                match c {
                    '<' | '(' | '[' => nesting += 1,
                    '>' | ')' | ']' => nesting -= 1,
                    ',' if nesting == 0 => {
                        parts.push(String::new());
                        continue;
                    }
                    _ => {}
                }
                parts.last_mut().expect("one").push(c);
            }
            let by_param = parts.iter().any(|p| {
                p.split_once(':').is_some_and(|(n, ty)| {
                    let n = n.trim().trim_start_matches("mut ");
                    POSITIONAL.contains(&n) && ty.contains("usize")
                })
            });
            let returns = rest[close + 1..]
                .split(['{', ';'])
                .next()
                .unwrap_or_default();
            let by_return = ["locate", "position", "index", "indices", "unsent"]
                .iter()
                .any(|w| name.contains(w))
                && returns.contains("usize");
            if by_param || by_return {
                let line_start = before.rfind('\n').map_or(0, |n| n + 1);
                let visibility = before[line_start..].trim().to_string();
                found.push((visibility, name));
            }
        }
        found
    }

    /// `LogBusy` reaches the UI as a kind of its own with nothing else in it — the contact kept
    /// changing while the change was planned, so there is no one version of it to show.
    #[test]
    fn log_busy_answers_as_a_kind_of_its_own() {
        assert_eq!(
            serde_json::to_value(refused(RowRefusal::Busy)).expect("serialize"),
            serde_json::json!({ "kind": "busy" })
        );
    }

    /// The scan is only worth having if it can fail, and if it can pass: a position-addressed
    /// change and a position handed back are each named; a change by id, and a helper inside a
    /// test module, are not.
    #[test]
    fn the_positions_scan_names_a_position_and_nothing_else() {
        let planted = [
            "impl Engine {",
            "    pub fn delete_qso(&mut self, index: usize) -> bool {",
            "        true",
            "    }",
            "    pub(crate) fn mark_card(&mut self, at: usize) {}",
            "    pub fn stamp_all(&mut self, indices: &[usize]) {}",
            "    pub fn locate(&self, key: &str) -> Option<usize> {",
            "        None",
            "    }",
            "    pub fn delete_by_id(&mut self, id: RecordId) -> bool {",
            "        true",
            "    }",
            "    pub fn set_power(&mut self, watts: usize) {}",
            "}",
            "",
            "#[cfg(test)]",
            "mod tests {",
            "    fn id_at(e: &Engine, at: usize) -> RecordId {",
            "        todo!()",
            "    }",
            "}",
        ]
        .join("\n");
        let found: Vec<(String, bool)> = positional_fns(&planted)
            .into_iter()
            .map(|(visibility, name)| (name, public(&visibility)))
            .collect();
        let found: Vec<(&str, bool)> = found.iter().map(|(n, p)| (n.as_str(), *p)).collect();
        assert_eq!(
            found,
            [
                ("delete_qso", true),
                ("mark_card", false),
                ("stamp_all", true),
                ("locate", true)
            ]
        );
    }

    /// ⛔ SPEC-2 C16: NO LOG CHANGE IS ADDRESSED BY POSITION. Across the log's model, the station,
    /// the engine, the commands and the Remote, every function that takes a place in the log, or
    /// hands one back to be acted on, is on this list with its reason — and nothing else is. A
    /// position names a contact only until the other writer deletes above it.
    ///
    /// A ratchet both ways: a new positional function fails here, and so does an entry that no
    /// longer exists — take it off the list when its reason goes.
    #[test]
    fn no_log_change_is_addressed_by_position() {
        let sources: [(&str, &str); 6] = [
            (
                "tempo-core logbook.rs",
                include_str!("../../crates/tempo-core/src/logbook.rs"),
            ),
            (
                "tempo-core op.rs",
                include_str!("../../crates/tempo-core/src/logbook/op.rs"),
            ),
            (
                "tempo-app station.rs",
                include_str!("../../crates/tempo-app/src/station.rs"),
            ),
            (
                "tempo-app engine.rs",
                include_str!("../../crates/tempo-app/src/engine.rs"),
            ),
            ("lib.rs", include_str!("lib.rs")),
            ("log_by_id.rs", include_str!("log_by_id.rs")),
        ];
        // (file, function, why a position is right there)
        let allowed: [(&str, &str, &str); 10] = [
            (
                "tempo-core op.rs",
                "position_of",
                "the id's place, inside Logbook::apply",
            ),
            (
                "tempo-core logbook.rs",
                "newest_match_index",
                "an id-less push, in one call",
            ),
            (
                "tempo-app engine.rs",
                "func_index",
                "a DSP function's slot, not a contact",
            ),
            (
                "tempo-app engine.rs",
                "sat_row_pinned_against",
                "a satellite's transponder",
            ),
            ("lib.rs", "set_sat_transponder", "a satellite's transponder"),
            (
                "lib.rs",
                "pick_sat_transponder",
                "a satellite's transponder",
            ),
            ("lib.rs", "hold_sat_row", "a satellite's transponder"),
            (
                "lib.rs",
                "upload_lotw_report",
                "C19: a batch chosen by position is refused",
            ),
            (
                "lib.rs",
                "upload_lotw_report_impl",
                "C19: a batch chosen by position is refused",
            ),
            (
                "lib.rs",
                "chosen",
                "C19: a batch chosen by position is refused",
            ),
        ];
        let mut found = Vec::new();
        for (file, src) in sources {
            for (visibility, name) in positional_fns(src) {
                found.push((file, name.clone()));
                // The model's positional bodies stay behind `Logbook::apply`: nothing outside
                // tempo-core can reach them.
                if file.starts_with("tempo-core") {
                    assert!(
                        !public(&visibility),
                        "Logbook::{name} is public again: {visibility} fn {name}"
                    );
                }
            }
        }
        let mut found: Vec<(&str, String)> = found;
        found.sort();
        let mut expected: Vec<(&str, String)> = allowed
            .iter()
            .map(|(f, n, _)| (*f, n.to_string()))
            .collect();
        expected.sort();
        assert_eq!(
            found, expected,
            "a function names a place in the log that is not on the list, or one on the list \
             is gone — a change must name its contact by id"
        );
    }
}
