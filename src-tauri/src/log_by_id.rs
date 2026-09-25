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

use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;
use tempo_app::dto::LoggedQso;
use tempo_app::station::{MadeRow, RowRefusal};
use tempo_core::logbook::{LogOp, QsoEdit, QsoRecord, RecordId};

use crate::{durable_command, qsl_via_arg, sat_name_arg, SharedEngine};

/// One contact, as the version whose edit key is `edit_key`. The same shape a Remote page
/// names a row by (`remote_service::operations::logging::Target::Id`).
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

/// `r` as a page of the log shows it (the entity resolved, as the parent's `log_row` does), with
/// its key.
fn keyed(r: &QsoRecord) -> KeyedRow {
    let mut row = LoggedQso::from(r.clone());
    row.entity = propagation::dxcc::resolve(&row.call).map(|i| i.entity.to_string());
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
        let (made, durability) = tempo_app::logwrite::change_ops(
            &engine,
            id,
            Some(&target.edit_key),
            std::slice::from_ref(&op),
            "log change",
        );
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
#[tauri::command]
pub async fn edit_qso_by_id(
    state: State<'_, SharedEngine>,
    target: RowRef,
    edit: QsoEdit,
) -> Result<RowAnswer, String> {
    edit_row(Arc::clone(&state), target, edit).await
}

pub(crate) async fn edit_row(
    engine: SharedEngine,
    target: RowRef,
    edit: QsoEdit,
) -> Result<RowAnswer, String> {
    let id = id_of(&target)?;
    durable_command(move || {
        let (made, durability) =
            tempo_app::logwrite::edit_row(&engine, id, &target.edit_key, &edit);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::durable_command_tests::engine_on_store;
    use crate::remote_service::stored_log_tests::StoredLog;
    use std::path::Path;
    use tempo_app::engine::engine_lock;
    use tempo_core::logbook::{QslVia, UploadOutcome};

    const COMMANDS: [&str; 5] = [
        "edit_qso_by_id",
        "mark_qsl_sent_by_id",
        "mark_qsl_card_by_id",
        "set_sat_tag_by_id",
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
        let sources: [(&str, &str); 7] = [
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
            (
                "remote logging.rs",
                include_str!("remote_service/operations/logging.rs"),
            ),
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
