//! ⛔ THE LOG IN MEMORY IS GONE, AND STAYS GONE — SPEC-2 v3 C19's census of the cut.
//!
//! Until the cut the station kept a copy of the whole log in memory beside the store. The handles
//! on it were marked `#[deprecated]`, allowed in tests by a crate-wide allow, and counted down
//! (SPEC-2's census ratchet). The cut deleted the copy and every handle on it, the catch-up that
//! took a write made around the station into the hot index, and the store-less last resort
//! (C19L) — and, with nothing left to allow, the allow. A caller of a deleted item no longer
//! compiles; this scan keeps the DEFINITIONS out, so that a merge from a branch cut before the
//! cut cannot bring one back together with its callers.
//!
//! It reads every `.rs` file under tempo-core's and tempo-app's `src/`, comment lines aside, and
//! names each definition it finds. When it was written it found every one of them in the tree
//! the cut started from, and none after.

use std::path::{Path, PathBuf};

/// What the cut deleted, the file it may not come back to (`None`: any file), and the text that
/// defines it there.
const DELETED: &[(&str, Option<&str>, &str)] = &[
    (
        "the copy of the log in memory (StationCore::logbook)",
        Some("tempo-app/src/station.rs"),
        "logbook: Logbook",
    ),
    ("Engine::log_records", None, "fn log_records("),
    ("Engine::log_snapshot", None, "fn log_snapshot("),
    ("Engine::log_read_token", None, "fn log_read_token("),
    ("get_log", None, "fn get_log("),
    (
        "Engine::sync_shared_log_if_changed",
        None,
        "fn sync_shared_log_if_changed(",
    ),
    (
        "the store-less last resort (Engine::without_log_store)",
        None,
        "fn without_log_store(",
    ),
    (
        "a reader's arm over the log in memory",
        None,
        "LogRows::Memory",
    ),
    (
        "the log in memory's rows (LogRows::Memory)",
        Some("tempo-app/src/logstore.rs"),
        "Memory(Vec<Arc<QsoRecord>>)",
    ),
    (
        "the rows an open handed the copy (Opened::records)",
        Some("tempo-app/src/logstore.rs"),
        "records: Vec<QsoRecord>",
    ),
    (
        "StationCore::recover_external_appends",
        None,
        "fn recover_external_appends(",
    ),
    (
        "StationCore::append_to_log_file",
        None,
        "fn append_to_log_file(",
    ),
    ("StationCore::save_log", None, "fn save_log("),
    ("StationCore::last_log_mtime", None, "last_log_mtime"),
    ("log_file_stamp", None, "fn log_file_stamp("),
    ("DUAL_EXECUTION_ROWS", None, "DUAL_EXECUTION_ROWS"),
    ("StationCore::copy_is_the_log", None, "fn copy_is_the_log("),
    (
        "StationCore::check_plan_against_memory",
        None,
        "fn check_plan_against_memory(",
    ),
    (
        "StationCore::index_of_log_in_memory",
        None,
        "fn index_of_log_in_memory(",
    ),
    (
        "StationCore::reindex_log_in_memory",
        None,
        "fn reindex_log_in_memory(",
    ),
    (
        "StationCore::backfill_log_in_memory",
        None,
        "fn backfill_log_in_memory(",
    ),
    (
        "StationCore::backfill_after_bulk",
        None,
        "fn backfill_after_bulk(",
    ),
    (
        "HotIndex::catch_up",
        Some("tempo-core/src/logbook/hot.rs"),
        "fn catch_up(",
    ),
    (
        "HotIndex::rebuild",
        Some("tempo-core/src/logbook/hot.rs"),
        "fn rebuild(",
    ),
    ("HOT_CATCH_UPS", None, "HOT_CATCH_UPS"),
    ("Logbook::hand_over_minter", None, "fn hand_over_minter("),
    (
        "Logbook::follow",
        Some("tempo-core/src/logbook.rs"),
        "fn follow(",
    ),
    ("Records::write_following", None, "fn write_following("),
];

/// Each deleted item `text` — the file at `path`, relative to `crates/` — defines, as
/// `path:line: item`. A comment line is not a definition.
fn found(path: &str, text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("//") {
            continue;
        }
        for (item, file, needle) in DELETED {
            if file.is_none_or(|f| path.ends_with(f)) && line.contains(needle) {
                out.push(format!("{path}:{}: {item}", n + 1));
            }
        }
    }
    out
}

/// Every `.rs` file under `dir`.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("a source folder reads") {
        let path = entry.expect("a source folder reads").path();
        if path.is_dir() {
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn nothing_the_cut_deleted_is_defined_again() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tempo-app sits in crates/");
    let mut files = Vec::new();
    for krate in ["tempo-core", "tempo-app"] {
        sources(&crates.join(krate).join("src"), &mut files);
    }
    assert!(
        files.len() > 100,
        "premise: the sources were found ({} files)",
        files.len()
    );
    let mut hits = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).expect("a source file reads");
        let path = f
            .strip_prefix(crates)
            .expect("under crates/")
            .to_string_lossy()
            .replace('\\', "/");
        hits.extend(found(&path, &text));
    }
    assert!(
        hits.is_empty(),
        "the cut deleted these, and they are defined again:\n{}",
        hits.join("\n")
    );
}

/// ★ POSITIVE CONTROL for the scan above: each definition, planted in a line of a file it may not
/// come back to, is named; a comment quoting it is not; and one scoped to a file is not named in
/// another, where its name is another item's.
#[test]
fn a_planted_definition_of_each_is_named() {
    for (item, file, needle) in DELETED {
        let path = file.unwrap_or("tempo-app/src/engine.rs");
        let planted = format!("    pub(crate) {needle}\n");
        assert!(
            found(path, &planted).iter().any(|h| h.ends_with(item)),
            "{item}: planted in {path}, and not named"
        );
        let quoted = format!("    /// `{needle}`\n");
        assert!(
            found(path, &quoted).is_empty(),
            "{item}: a comment quoting it is named"
        );
        if file.is_some() {
            assert!(
                found("tempo-app/src/lib.rs", &planted).is_empty(),
                "{item}: named outside {path}"
            );
        }
    }
}
