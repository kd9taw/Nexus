//! The import preview's counts and "Import new only", through the engine and the real store: the
//! preview is the import's own plan, let go; new only adds what the log lacks and leaves what it
//! holds; and either import is one run its Undo takes back.

use super::*;
use crate::loghistory::{new_run, undo_run};
use crate::logstore::tests::{engine_on_store, flush, legacy_log, Dir};
use crate::logstore::DURABLE_WAIT;
use crate::test_util::StoredLog;

/// A shared engine whose log holds `K0ABC`… `K2ABC` (20m FT8, 2026-01-01 00:00:00, :01, :02).
fn shared(d: &Dir) -> Arc<Mutex<Engine>> {
    std::fs::write(d.log(), legacy_log(3)).unwrap();
    let e = engine_on_store(d);
    flush(&e);
    Arc::new(Mutex::new(e))
}

fn row(call: &str, secs: u32, extra: &str) -> String {
    format!(
        "<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260101<TIME_ON:6>0000{secs:02}{extra}<EOR>\n",
        call.len()
    )
}

/// Two contacts the log holds (one restated with a card the log lacks) and two it does not.
fn file() -> String {
    format!(
        "<EOH>\n{}{}{}{}",
        row("K0ABC", 0, "<QSL_RCVD:1>Y"),
        row("K1ABC", 1, ""),
        row("W1NEW", 30, ""),
        row("W2NEW", 31, ""),
    )
}

fn card(engine: &Mutex<Engine>, call: &str) -> bool {
    engine
        .stored_log()
        .iter()
        .find(|r| r.call == call)
        .is_some_and(|r| r.qsl_rcvd.card)
}

#[test]
fn the_preview_counts_by_the_logs_own_rule_and_writes_nothing() {
    let d = Dir::new("import-preview");
    let engine = shared(&d);
    let (new, dupes, updates) = preview_import(&engine, &file()).expect("the preview");
    assert_eq!((new, dupes, updates), (2, 2, 1));
    assert_eq!(engine.stored_log().len(), 3, "nothing was written");
    assert!(!card(&engine, "K0ABC"));
    // The import it previewed makes those very counts.
    let run = new_run();
    let (made, dur) = until_written(|| import_adif_in_run(&engine, &file(), &run));
    assert_eq!(made.expect("the import"), (2, 2, 1, 5));
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(card(&engine, "K0ABC"));
}

#[test]
fn new_only_adds_what_the_log_lacks_and_leaves_what_it_holds() {
    let d = Dir::new("import-new-only");
    let engine = shared(&d);
    let run = new_run();
    let (made, dur) = until_written(|| import_adif_new_only_in_run(&engine, &file(), &run));
    assert_eq!(made.expect("the import"), (2, 2, 0, 5));
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(!card(&engine, "K0ABC"), "the held contact is left as it is");
    // One run, and its Undo takes the two contacts out.
    let (undone, dur) = undo_run(&engine, &run);
    assert_eq!(undone.expect("undone").changed, 2);
    dur.wait(DURABLE_WAIT).unwrap();
    let calls: Vec<String> = engine.stored_log().iter().map(|r| r.call.clone()).collect();
    assert_eq!(calls, ["K0ABC", "K1ABC", "K2ABC"]);
}

#[test]
fn an_import_is_undone_as_one_run_its_upgrade_included() {
    let d = Dir::new("import-one-run");
    let engine = shared(&d);
    let run = new_run();
    let (made, dur) = until_written(|| import_adif_in_run(&engine, &file(), &run));
    made.expect("the import");
    dur.wait(DURABLE_WAIT).unwrap();
    let (undone, dur) = undo_run(&engine, &run);
    assert_eq!(
        undone.expect("undone").changed,
        3,
        "two added and one upgraded"
    );
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(engine.stored_log().len(), 3);
    assert!(!card(&engine, "K0ABC"), "the upgrade is undone too");
}
