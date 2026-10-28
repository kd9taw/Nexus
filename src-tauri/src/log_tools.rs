//! The Logbook's clean-up tools as the UI asks for them: the duplicate finder and its merge, the
//! bulk edit with its preview, the check with its fixes, and the blank grids
//! ([`tempo_app::logtools`]).
//!
//! Every read runs with the Engine lock released and changes nothing. Every change is planned
//! with the lock released and made under it, a chunk at a time, as ONE run of the log's history —
//! its `run` is what `undo_history_run` takes — and answers only once it is on disk
//! ([`crate::durable_command`]). None sends anything anywhere: no tool changes the worked call, so
//! nothing is queued to a service. The one change of stamps and confirmations a tool makes is a
//! bulk STATION_CALLSIGN change's — the call the contact is uploaded under — so the next upload
//! sends it under the new call, and the LoTW, eQSL and QRZ confirmations matched under the old
//! one are taken off, a paper card kept (`tempo_app::logtools`' header).
//!
//! A bulk edit is made on exactly the contacts its preview counted: the preview answers their ids,
//! and the edit takes those ids and the count the operator confirmed, and refuses a mismatch.
//!
//! The changes are the shack's own (`command_classes`): like a purge, a bulk change can reach
//! every contact in the log.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tauri::State;
use tempo_app::dto::LoggedQso;
use tempo_app::logtools::{self, DupeTier, Fix, MergeAsk, ProblemKind, ToolRun};
use tempo_core::logbook::cleanup::{Change, Field};
use tempo_core::logbook::history::content_key;
use tempo_core::logbook::query::LogQuery;
use tempo_core::logbook::RecordId;

use crate::log_queries::LogQueries;
use crate::{durable_command, SharedEngine};

/// A malformed id names no contact: an id is only ever one the station handed out.
pub(crate) fn id_of(id: &str) -> Result<RecordId, String> {
    id.parse()
        .map_err(|()| format!("'{id}' is not a contact id."))
}

async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("logbook task failed: {e}"))?
}

/// What a tool's change made.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolRunDto {
    /// How many contacts it changed or deleted.
    pub changed: usize,
    /// Its run: what `undo_history_run` takes to undo the whole change. `None` when it changed
    /// nothing.
    pub run: Option<String>,
    /// What it left alone: contacts gone since they were shown, changed since, and (a merge) groups
    /// with a conflict that must be chosen, and groups whose copies are two contacts.
    pub gone: usize,
    pub changed_since: usize,
    pub needs_choice: usize,
    pub distinct: usize,
}

pub(crate) fn run_dto(r: ToolRun) -> ToolRunDto {
    let count = |why: &str| r.refused.iter().filter(|(_, w)| *w == why).count();
    ToolRunDto {
        changed: r.changed,
        gone: count("gone"),
        changed_since: count("changed"),
        needs_choice: count("needsChoice"),
        distinct: count("distinct"),
        run: r.run,
    }
}

// ── duplicates ─────────────────────────────────────────────────────────────────────────────────

/// One value a copy holds for a conflicting field.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictValueDto {
    pub id: String,
    pub value: String,
}

/// A field the copies disagree on.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictDto {
    pub tag: String,
    pub values: Vec<ConflictValueDto>,
    /// Must be chosen before the merge: a confirmed copy's value, the QSL-sent mark, the contest
    /// exchange, a comment or notes, an identity field.
    pub guarded: bool,
    /// The copy whose value the suggested merge takes.
    pub chosen: String,
}

/// One contact logged more than once.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeGroupDto {
    /// The copies, in log order.
    pub copies: Vec<LoggedQso>,
    /// Each copy's revision, sent back with the merge: a copy changed since is not merged over.
    pub revisions: Vec<String>,
    /// The copy the suggested merge keeps.
    pub keep: usize,
    /// What the copies disagree on when that one is kept.
    pub conflicts: Vec<ConflictDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupeScanDto {
    pub scanned: usize,
    pub groups: Vec<DupeGroupDto>,
}

/// The log's duplicates: `tier` `rule` — the log's own duplicate rule, the same station, band and
/// mode within 5 minutes — or `possible`, within an hour. Changes nothing.
#[tauri::command]
pub async fn find_log_duplicates(
    state: State<'_, SharedEngine>,
    tier: String,
) -> Result<DupeScanDto, String> {
    let tier = match tier.as_str() {
        "rule" => DupeTier::Rule,
        "possible" => DupeTier::Possible,
        other => return Err(format!("'{other}' is not a duplicate tier.")),
    };
    let engine = Arc::clone(&state);
    let scan = blocking(move || logtools::find_duplicates(&engine, tier)).await?;
    Ok(DupeScanDto {
        scanned: scan.scanned,
        groups: scan
            .groups
            .into_iter()
            .map(|g| DupeGroupDto {
                revisions: g.copies.iter().map(content_key).collect(),
                conflicts: g
                    .conflicts
                    .into_iter()
                    .map(|c| ConflictDto {
                        tag: c.tag,
                        values: c
                            .values
                            .into_iter()
                            .map(|(id, value)| ConflictValueDto {
                                id: id.to_string(),
                                value,
                            })
                            .collect(),
                        guarded: c.guarded,
                        chosen: c.chosen.to_string(),
                    })
                    .collect(),
                keep: g.keep,
                copies: g.copies.into_iter().map(LoggedQso::from).collect(),
            })
            .collect(),
    })
}

/// One copy of a merge, as it was shown.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergeCopyDto {
    pub id: String,
    pub revision: String,
}

/// One merge the operator asked for.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MergeDto {
    pub keep: String,
    pub copies: Vec<MergeCopyDto>,
    /// For a conflicting field, the copy whose value the merged contact takes.
    #[serde(default)]
    pub choices: HashMap<String, String>,
}

fn merge_ask(m: MergeDto) -> Result<MergeAsk, String> {
    Ok(MergeAsk {
        keep: id_of(&m.keep)?,
        copies: m
            .copies
            .into_iter()
            .map(|c| Ok((id_of(&c.id)?, c.revision)))
            .collect::<Result<_, String>>()?,
        choices: m
            .choices
            .into_iter()
            .map(|(tag, id)| Ok((tag, id_of(&id)?)))
            .collect::<Result<_, String>>()?,
    })
}

/// Merge each group into its kept copy, as ONE run. Sends nothing.
#[tauri::command]
pub async fn merge_log_duplicates(
    state: State<'_, SharedEngine>,
    merges: Vec<MergeDto>,
) -> Result<ToolRunDto, String> {
    let asks: Vec<MergeAsk> = merges
        .into_iter()
        .map(merge_ask)
        .collect::<Result<_, _>>()?;
    let engine = Arc::clone(&state);
    durable_command(move || {
        let (made, durability) = logtools::merge_duplicates(&engine, &asks);
        (made.map(run_dto), durability)
    })
    .await
}

// ── bulk edit ──────────────────────────────────────────────────────────────────────────────────

/// The contacts a bulk edit acts on: the ones ticked, or every one the Logbook's filter shows.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BulkTarget {
    #[serde(default)]
    pub ids: Option<Vec<String>>,
    #[serde(default)]
    pub query: Option<LogQuery>,
}

pub(crate) fn target_ids(
    engine: &SharedEngine,
    queries: &LogQueries,
    target: &BulkTarget,
) -> Result<Vec<RecordId>, String> {
    match (&target.ids, &target.query) {
        (Some(ids), None) => ids.iter().map(|id| id_of(id)).collect(),
        (None, Some(query)) => queries.ids_of(engine, query),
        _ => Err("Name the contacts to change: those ticked, or those the filter shows.".into()),
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BulkPreviewDto {
    /// The contacts previewed: what `bulk_edit_apply` is handed back, so it acts on exactly these.
    pub ids: Vec<String>,
    pub selected: usize,
    pub change: usize,
    pub unchanged: usize,
    pub missing: usize,
    /// Of those it changes, the ones whose station call it replaces with another.
    pub recall: usize,
    /// Of those, the ones uploaded under the station call it changes: sent again.
    pub resend: usize,
    /// The LoTW, eQSL and QRZ confirmations the changed station call takes off.
    pub unconfirm: usize,
    /// Of those it changes, the ones a filled or cleared station call sends to a service not yet
    /// sent them under another call than before; `move_to` that call, `move_from` the calls they
    /// went under, each as sent (a portable `/P` kept).
    pub moves: usize,
    pub move_to: Option<String>,
    pub move_from: Vec<String>,
    /// Of those it changes, the ones it moves into or out of an activation (a park or summit of
    /// yours, or the call it was worked under); of those, the ones already sent to a service
    /// (their stamps stay, nothing is sent again). Of every one it changes, moved or not, the ones
    /// in an activation file already exported, which `activation_files` names: those files no
    /// longer match the log.
    pub activation_changes: usize,
    pub activation_uploaded: usize,
    pub activation_exported: usize,
    pub activation_files: Vec<String>,
}

/// What a bulk edit would make of its contacts. Changes nothing.
#[tauri::command]
pub async fn bulk_edit_preview(
    state: State<'_, SharedEngine>,
    queries: State<'_, LogQueries>,
    target: BulkTarget,
    changes: Vec<Change>,
) -> Result<BulkPreviewDto, String> {
    let (engine, queries) = (Arc::clone(&state), queries.inner().clone());
    blocking(move || {
        let ids = target_ids(&engine, &queries, &target)?;
        let p = logtools::bulk_preview(&engine, &ids, &changes)?;
        Ok(BulkPreviewDto {
            ids: ids.iter().map(ToString::to_string).collect(),
            selected: p.selected,
            change: p.change,
            unchanged: p.unchanged,
            missing: p.missing,
            recall: p.recall,
            resend: p.resend,
            unconfirm: p.unconfirm,
            moves: p.moves,
            move_to: p.move_to,
            move_from: p.move_from,
            activation_changes: p.activation_changes,
            activation_uploaded: p.activation_uploaded,
            activation_exported: p.activation_exported,
            activation_files: p.activation_files,
        })
    })
    .await
}

/// Set or clear fields on the contacts the preview counted — `ids`, as the preview answered them —
/// as ONE run, only while they are the `expect_count` the operator confirmed, and the contacts it
/// sends under another call are still the `expect_moves` under `expect_move_to` the operator
/// acknowledged (none, unless the confirm's tick was given). Never the worked call; sends nothing.
#[tauri::command]
pub async fn bulk_edit_apply(
    state: State<'_, SharedEngine>,
    ids: Vec<String>,
    expect_count: usize,
    expect_moves: usize,
    expect_move_to: Option<String>,
    changes: Vec<Change>,
) -> Result<ToolRunDto, String> {
    let ids: Vec<RecordId> = ids.iter().map(|id| id_of(id)).collect::<Result<_, _>>()?;
    let engine = Arc::clone(&state);
    let acknowledged = logtools::MovesAck {
        moves: expect_moves,
        move_to: expect_move_to,
    };
    durable_command(move || {
        let (made, durability) =
            logtools::bulk_edit(&engine, &ids, expect_count, &acknowledged, &changes);
        (made.map(run_dto), durability)
    })
    .await
}

// ── the check ──────────────────────────────────────────────────────────────────────────────────

/// Whether the country file places `call` in another entity than the contact's own `logged`
/// number, a current entity's, and the number it would propose: `Some(None)` where the call's
/// prefix cannot decide it ([`propagation::dxcc::prefix_decides`]) — a mainland call logged as an
/// island of its own block (K1ABC as Hawaii, G4ABC as Scotland), a DXpedition's call, an exact-call
/// exception — which is listed for the operator and never fixed. A deleted entity's number is
/// never judged: the prefix it was logged under may name another entity today.
fn dxcc_mismatch(call: &str, logged: u32) -> Option<Option<u32>> {
    let entity = propagation::dxcc::entity_by_dxcc(logged)?;
    if entity.deleted {
        return None;
    }
    let by_call = propagation::dxcc::resolve(call)?.dxcc?;
    (by_call != logged).then(|| propagation::dxcc::prefix_decides(call, logged).then_some(by_call))
}

fn kind_word(k: ProblemKind) -> &'static str {
    match k {
        ProblemKind::Grid => "grid",
        ProblemKind::MyGrid => "myGrid",
        ProblemKind::Dxcc => "dxcc",
        ProblemKind::NoBand => "noBand",
        ProblemKind::BandFreq => "bandFreq",
        ProblemKind::Future => "future",
        ProblemKind::EndBeforeStart => "endBeforeStart",
        ProblemKind::Iota => "iota",
        ProblemKind::Pota => "pota",
        ProblemKind::Sota => "sota",
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FixValueDto {
    /// `null`: the fix clears the field.
    pub value: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProblemDto {
    pub id: String,
    pub call: String,
    pub when_unix: u64,
    pub kind: &'static str,
    pub field: Option<Field>,
    pub current: Option<String>,
    /// The proposed fix; `null` when none can be proposed.
    pub fix: Option<FixValueDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CheckDto {
    pub checked: usize,
    pub problems: Vec<ProblemDto>,
}

/// Check every contact for problems, each with its proposed fix. Changes nothing.
#[tauri::command]
pub async fn check_logbook(state: State<'_, SharedEngine>) -> Result<CheckDto, String> {
    let engine = Arc::clone(&state);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let (checked, problems) =
        blocking(move || logtools::check(&engine, &dxcc_mismatch, now)).await?;
    Ok(CheckDto {
        checked,
        problems: problems
            .into_iter()
            .map(|p| ProblemDto {
                id: p.id.to_string(),
                call: p.call,
                when_unix: p.when_unix,
                kind: kind_word(p.kind),
                field: p.field,
                current: p.current,
                fix: p.fix.map(|c| FixValueDto { value: c.value }),
            })
            .collect(),
    })
}

/// One fix the operator ticked, with what the check found in its field.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FixDto {
    pub id: String,
    pub field: Field,
    pub expect: Option<String>,
    pub value: Option<String>,
}

/// Make the fixes ticked, and only those, as ONE run. Sends nothing.
#[tauri::command]
pub async fn apply_log_fixes(
    state: State<'_, SharedEngine>,
    fixes: Vec<FixDto>,
) -> Result<ToolRunDto, String> {
    let fixes: Vec<Fix> = fixes
        .into_iter()
        .map(|f| {
            Ok(Fix {
                id: id_of(&f.id)?,
                field: f.field,
                expect: f.expect,
                value: f.value,
            })
        })
        .collect::<Result<_, String>>()?;
    let engine = Arc::clone(&state);
    durable_command(move || {
        let (made, durability) = logtools::apply_fixes(&engine, &fixes);
        (made.map(run_dto), durability)
    })
    .await
}

// ── blank grids ─────────────────────────────────────────────────────────────────────────────────

/// One contact with no grid, and the grid known for its station ([`logtools::GridOffer`]).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridOfferDto {
    pub id: String,
    pub call: String,
    pub when_unix: u64,
    pub band: String,
    pub mode: String,
    pub grid: String,
    /// `heard` (sent on the air) or `callbook` (home QTH, for the distance and the map only).
    pub source: &'static str,
    pub uploaded: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GridOffersDto {
    /// How many contacts have no grid.
    pub blank: usize,
    pub offers: Vec<GridOfferDto>,
}

fn source_word(s: logtools::GridFrom) -> &'static str {
    match s {
        logtools::GridFrom::Heard => "heard",
        logtools::GridFrom::Callbook => "callbook",
    }
}

/// The contacts with no grid for which a grid is known: heard on the air (ALL.TXT, then this
/// session), else a callbook lookup already made (the lookup cache, while fresh; nothing is looked
/// up). Changes nothing.
#[tauri::command]
pub async fn log_grid_offers(
    app: tauri::AppHandle,
    state: State<'_, SharedEngine>,
) -> Result<GridOffersDto, String> {
    use tauri::Manager;
    let engine = Arc::clone(&state);
    blocking(move || {
        let max_age = tempo_app::engine::engine_lock(&engine)
            .settings()
            .callbook_cache_max_age_secs();
        let now = u64::try_from(crate::now_unix()).unwrap_or(0);
        let cache = app.state::<crate::SharedCallbookCache>();
        let mut heard = |calls: &std::collections::HashSet<String>| {
            std::fs::File::open(crate::all_txt_path())
                .map(|f| logtools::heard_grids_in(std::io::BufReader::new(f), calls))
                .unwrap_or_default()
        };
        let session = |call: &str| tempo_app::engine::engine_lock(&engine).sent_grid(call);
        let mut kept = |call: &str| crate::callbook_update::kept(&cache, call, max_age);
        let (blank, offers) = logtools::grid_offers(&engine, &mut heard, &session, &mut kept, now)?;
        Ok(GridOffersDto {
            blank,
            offers: offers
                .into_iter()
                .map(|o| GridOfferDto {
                    id: o.id.to_string(),
                    call: o.call,
                    when_unix: o.when_unix,
                    band: o.band,
                    mode: o.mode,
                    grid: o.grid,
                    source: source_word(o.source),
                    uploaded: o.uploaded,
                })
                .collect(),
        })
    })
    .await
}

/// One contact the operator ticked, with the grid offered and where it came from.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GridPickDto {
    pub id: String,
    pub grid: String,
    pub source: String,
}

/// Fill the ticked contacts' grids, and only those, as ONE run: a callbook grid with the
/// callbook's mark. Sends nothing.
#[tauri::command]
pub async fn log_grid_fill(
    state: State<'_, SharedEngine>,
    picks: Vec<GridPickDto>,
) -> Result<ToolRunDto, String> {
    let picks: Vec<logtools::GridPick> = picks
        .into_iter()
        .map(|p| {
            Ok(logtools::GridPick {
                id: id_of(&p.id)?,
                grid: p.grid,
                source: match p.source.as_str() {
                    "heard" => logtools::GridFrom::Heard,
                    "callbook" => logtools::GridFrom::Callbook,
                    other => return Err(format!("'{other}' is not where a grid comes from.")),
                },
            })
        })
        .collect::<Result<_, String>>()?;
    let engine = Arc::clone(&state);
    durable_command(move || {
        let (made, durability) = logtools::fill_grids(&engine, &picks);
        (made.map(run_dto), durability)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The country file's answer is asked only of a current entity's number: a deleted one's is
    /// never called a mismatch, and a call its own number agrees with is not either.
    #[test]
    fn a_dxcc_mismatch_is_asked_only_of_a_current_entity() {
        assert_eq!(dxcc_mismatch("W1AW", 291), None, "agrees");
        assert_eq!(
            dxcc_mismatch("W1AW", 1),
            Some(Some(291)),
            "Canada's number on a US call"
        );
        let deleted = propagation::dxcc::entities()
            .into_iter()
            .find(|e| e.deleted)
            .expect("a deleted entity");
        assert_eq!(dxcc_mismatch("W1AW", deleted.code), None);
    }

    /// ★ A DXpedition is never "fixed". `3Y0PI` (Peter I, 1994) and `3Y5X` (Bouvet, 1989) have no
    /// exact entry in the country file and fall to Antarctica's bare `3Y`; logged as Peter I (199)
    /// or Bouvet (24) — which they were — they are listed for the operator with no number
    /// proposed. Logged as an entity with no `3Y` call at all, the file's number is proposed. An
    /// exact-call exception logged as another entity is listed, never fixed.
    #[test]
    fn a_dxpedition_call_is_never_fixed() {
        let antarctica = propagation::dxcc::resolve("3Y0PI").and_then(|i| i.dxcc);
        assert_eq!(antarctica, Some(13), "premise: the bare prefix places it");
        assert_eq!(dxcc_mismatch("3Y0PI", 199), Some(None));
        assert_eq!(dxcc_mismatch("3Y5X", 24), Some(None));
        assert_eq!(
            dxcc_mismatch("3Y0PI", 291),
            Some(Some(13)),
            "a US number is still wrong"
        );
        // =3Y0J is Bouvet's exact entry.
        assert_eq!(dxcc_mismatch("3Y0J", 199), Some(None));
        assert_eq!(dxcc_mismatch("3Y0J", 24), None);
    }

    /// ★ The classic DXCC slips are listed: a mainland call logged as an island or district of its
    /// own block, and the reverse. The prefix cannot say which the operator meant, so none is
    /// fixed; but none is hidden.
    #[test]
    fn a_mainland_call_logged_as_its_blocks_island_is_listed_unfixed() {
        const USA: u32 = 291;
        const HAWAII: u32 = 110;
        const ALASKA: u32 = 6;
        const SCOTLAND: u32 = 279;
        for (call, logged) in [
            ("K1ABC", HAWAII),
            ("W1AW", HAWAII),
            ("K1ABC", ALASKA),
            ("KH6ABC", USA),
            ("G4ABC", SCOTLAND),
        ] {
            assert_eq!(
                dxcc_mismatch(call, logged),
                Some(None),
                "{call} as {logged}"
            );
        }
        assert_eq!(dxcc_mismatch("KH6ABC", HAWAII), None, "agrees");
    }

    /// A merge's ids that are not ids are refused before anything is read.
    #[test]
    fn a_merge_naming_no_contact_is_refused() {
        let m = MergeDto {
            keep: "nope".into(),
            copies: Vec::new(),
            choices: HashMap::new(),
        };
        assert!(merge_ask(m).is_err());
    }
}
