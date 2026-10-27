//! The history's proofs through the engine and the real store: each path a contact changes by is
//! recorded under its own source; an undo, a restore and a run's undo put the log back, through
//! the same write path, so the `log.adi` mirror follows; a run's undo is all or nothing; and no
//! undo ever queues an upload.

use super::*;
use crate::engine::with_session_rows_at_launch;
use crate::logstore::tests::{engine_on_store, flush, id_at, legacy_log, open_fast, qso, Dir};
use crate::logstore::DURABLE_WAIT;
use crate::logwrite::{self, until_written};
use crate::station::LogFill;
use crate::test_util::StoredLog;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use tempo_core::logbook::{
    adif_header, adif_record, adif_record_own_log, LogOp, QsoEdit, UploadOutcome, UploadService,
    UploadStatus,
};

thread_local! {
    // A writer racing the undo planned on this thread: run after its plan's read, with no lock
    // held, before the undo is made. (A plain comment: a doc comment cannot attach through
    // `thread_local!`.)
    static RACE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// [`undo_entries`]'s seam: the racing writer, if a test set one.
pub(super) fn race() {
    let hook = RACE.with(|r| r.borrow_mut().take());
    if let Some(mut hook) = hook {
        hook();
        RACE.with(|r| *r.borrow_mut() = Some(hook));
    }
}

fn racing<T>(hook: impl FnMut() + 'static, f: impl FnOnce() -> T) -> T {
    RACE.with(|r| *r.borrow_mut() = Some(Box::new(hook)));
    let out = f();
    RACE.with(|r| *r.borrow_mut() = None);
    out
}

/// A shared engine on the store of `d`, holding `n` contacts.
fn shared(d: &Dir, n: usize) -> Arc<Mutex<Engine>> {
    std::fs::write(d.log(), legacy_log(n)).unwrap();
    let e = engine_on_store(d);
    flush(&e);
    Arc::new(Mutex::new(e))
}

fn row_of(engine: &Mutex<Engine>, id: RecordId) -> Option<Arc<QsoRecord>> {
    engine.stored_log().into_iter().find(|r| r.id == Some(id))
}

fn id_of(engine: &Mutex<Engine>, call: &str) -> RecordId {
    engine
        .stored_log()
        .iter()
        .find(|r| r.call == call)
        .and_then(|r| r.id)
        .unwrap_or_else(|| panic!("{call} is in the log"))
}

/// Each entry of a contact's history, as `(kind, source)`.
fn kinds(engine: &Mutex<Engine>, id: RecordId) -> Vec<(Kind, String)> {
    history_of(engine, id)
        .unwrap()
        .into_iter()
        .map(|e| (e.kind, e.source))
        .collect()
}

fn k(kind: Kind, source: &str) -> (Kind, String) {
    (kind, source.to_string())
}

fn mirror(d: &Dir) -> String {
    std::fs::read_to_string(d.log()).unwrap()
}

/// The store and `log.adi` both hold every change made so far.
fn settle(engine: &Mutex<Engine>) {
    flush(&engine_lock(engine));
}

fn edit_comment(engine: &Mutex<Engine>, id: RecordId, comment: &str) {
    let row = row_of(engine, id).expect("the contact");
    let (key, mut edit) = (QsoEdit::project(&row).key(), QsoEdit::project(&row));
    edit.comment = Some(comment.into());
    let (made, d) = logwrite::edit_row(engine, id, &key, &edit);
    made.expect("reads").expect("made");
    d.wait(DURABLE_WAIT).expect("on disk");
}

/// ★ Every path a contact changes by is recorded, under the source that made it: the operator's
/// log and edit and delete, a Remote client's logged contact, an import (a new contact, and a
/// merge into one the log holds), a LoTW download, and the fill job. A stamp is not recorded.
#[test]
fn every_change_is_recorded_with_the_source_that_made_it() {
    let d = Dir::new("history-sources");
    let engine = shared(&d, 2);
    engine_lock(&engine).log_qso(qso("W1AW", 1_789_000_000));
    engine_lock(&engine).log_qso_for_sync(qso("W2AW", 1_789_000_100));
    settle(&engine);
    let (w1, w2) = (id_of(&engine, "W1AW"), id_of(&engine, "W2AW"));
    assert_eq!(kinds(&engine, w1), vec![k(Kind::Create, "operator")]);
    assert_eq!(kinds(&engine, w2), vec![k(Kind::Create, "remote")]);

    edit_comment(&engine, w1, "edited");
    let h = history_of(&engine, w1).unwrap();
    assert_eq!((h[1].kind, h[1].source.as_str()), (Kind::Edit, "operator"));
    assert_eq!(
        h[1].changes()
            .into_iter()
            .map(|c| (c.tag, c.old, c.new))
            .collect::<Vec<_>>(),
        vec![("COMMENT".to_string(), None, Some("edited".to_string()))]
    );

    let w1_row = row_of(&engine, w1).unwrap();
    let mut text = adif_header();
    text.push_str(&adif_record(&qso("W3AW", 1_789_000_200)));
    text.push_str(&adif_record(&w1_row).replace("<EOR>", "<QSL_RCVD:1>Y<EOR>"));
    let (made, dur) = logwrite::import_adif(&engine, &text);
    made.expect("the import");
    dur.wait(DURABLE_WAIT).unwrap();
    let w3 = id_of(&engine, "W3AW");
    assert_eq!(kinds(&engine, w3), vec![k(Kind::Create, "import")]);
    assert_eq!(kinds(&engine, w1)[2], k(Kind::Merge, "import"));

    let report = adif_header()
        + &adif_record(&row_of(&engine, w1).unwrap()).replace("<EOR>", "<LOTW_QSL_RCVD:1>Y<EOR>");
    let (made, dur) = logwrite::merge_lotw_report(&engine, &report);
    made.expect("the report");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(kinds(&engine, w1)[3], k(Kind::Merge, "lotw"));

    logwrite::fill(
        &engine,
        &[LogFill {
            id: w2,
            country: Some("Filled".into()),
            state: None,
        }],
        1,
    )
    .expect("the fill");
    settle(&engine);
    assert_eq!(kinds(&engine, w2)[1], k(Kind::Fill, "fill"));

    // A connector's stamp: delivery bookkeeping, not history.
    let before = kinds(&engine, w1).len();
    let (_, dur) = logwrite::stamp_push(
        &engine,
        &row_of(&engine, w1).unwrap(),
        UploadService::Qrz,
        UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_900_000_000,
            detail: None,
        },
    );
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(row_of(&engine, w1).unwrap().upload.qrz.is_some(), "stamped");
    assert_eq!(kinds(&engine, w1).len(), before, "and nothing recorded");

    let (made, dur) = logwrite::change_ops(&engine, w3, None, &[LogOp::Delete(w3)], "delete");
    made.expect("reads").expect("deleted");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(kinds(&engine, w3)[1], k(Kind::Delete, "operator"));
}

/// The contest log's merge into the Logbook is recorded as the contest's.
#[test]
fn the_contest_merge_is_recorded_as_the_contests() {
    let d = Dir::new("history-contest");
    let mut s = Engine::new("W9XYZ", "EN51", 0).settings().clone();
    s.mycall = "W9XYZ".into();
    s.fd_active = true;
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "KANE".into();
    s.fd_position_id = "a1b2c3d4".into();
    let mut e = Engine::with_settings(s);
    e.attach_log_store(open_fast(&d));
    e.set_fd_log_path(d.0.join("fieldday.adi"));
    let shared = Mutex::new(e);
    with_session_rows_at_launch(
        &shared,
        |e| e.restore_opens_session(),
        |e, rows| {
            e.restore_field_day_if_enabled();
            if let Some(rows) = rows {
                e.open_session_from(rows);
            }
        },
    )
    .expect("the session's rows are read");
    {
        let mut e = engine_lock(&shared);
        let rx = [("RST", "599"), ("QTH", "COOK")].map(|(k, v)| (k.to_string(), v.to_string()));
        assert!(e.contest_log_manual("W9AAA", &rx, "CW", None).unwrap());
    }
    let (merged, dur) = logwrite::fd_merge_to_general(&shared);
    assert_eq!(merged.expect("the merge").added(), 1);
    dur.wait(DURABLE_WAIT).unwrap();
    settle(&shared);
    let id = id_of(&shared, "W9AAA");
    assert_eq!(kinds(&shared, id), vec![k(Kind::Create, "contest")]);
}

/// ★ An undo puts an edit back — through the same write path, so `log.adi` follows — and is
/// itself recorded, linked to what it undid, so undoing it is the redo.
#[test]
fn an_undo_puts_an_edit_back_and_undoing_the_undo_makes_it_again() {
    let d = Dir::new("history-undo");
    let engine = shared(&d, 3);
    let id = id_at(&*engine, 1);
    edit_comment(&engine, id, "edited by hand");
    settle(&engine);
    assert!(mirror(&d).contains("edited by hand"));
    let seq = history_of(&engine, id).unwrap().last().unwrap().seq;

    let (undone, dur) = undo_entry(&engine, seq);
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).expect("on disk");
    assert_eq!(undone.changed, 1);
    assert_eq!(
        row_of(&engine, id).unwrap().comment,
        None,
        "the edit is gone"
    );
    let h = history_of(&engine, id).unwrap();
    let last = h.last().unwrap();
    assert_eq!((last.kind, last.source.as_str()), (Kind::Edit, "undo"));
    assert_eq!(last.undoes, Some(seq), "linked to the edit it undid");
    settle(&engine);
    assert!(
        !mirror(&d).contains("edited by hand"),
        "log.adi follows the undo"
    );

    let (redone, dur) = undo_run(&engine, undone.run.as_deref().expect("the undo's run"));
    redone.expect("redone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        row_of(&engine, id).unwrap().comment.as_deref(),
        Some("edited by hand"),
        "the redo makes the edit again"
    );
    settle(&engine);
    assert!(mirror(&d).contains("edited by hand"));
}

/// ★ The Redo of one change's undo goes through [`undo_contact_run`], which a computer driving
/// the station may ask for: it makes the change again, as [`undo_run`] does.
#[test]
fn a_one_contact_runs_redo_makes_the_change_again() {
    let d = Dir::new("history-contact-redo");
    let engine = shared(&d, 3);
    let id = id_at(&*engine, 1);
    edit_comment(&engine, id, "edited by hand");
    settle(&engine);
    let seq = history_of(&engine, id).unwrap().last().unwrap().seq;
    let (undone, dur) = undo_entry(&engine, seq);
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(row_of(&engine, id).unwrap().comment, None);

    let (redone, dur) = undo_contact_run(&engine, undone.run.as_deref().expect("the undo's run"));
    assert_eq!(redone.expect("redone").changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        row_of(&engine, id).unwrap().comment.as_deref(),
        Some("edited by hand"),
        "the redo makes the edit again"
    );
    settle(&engine);
    assert!(mirror(&d).contains("edited by hand"), "log.adi follows it");
}

/// A run over more than one contact is refused by [`undo_contact_run`], changing nothing: it stays
/// [`undo_run`]'s, at the station.
#[test]
fn a_run_over_two_contacts_is_refused_as_one_contacts() {
    let d = Dir::new("history-contact-refused");
    let engine = shared(&d, 3);
    let mut text = adif_header();
    for i in 0..2u64 {
        text.push_str(&adif_record(&qso(&format!("N{i}TWO"), 1_789_000_000 + i)));
    }
    let (made, dur) = logwrite::import_adif(&engine, &text);
    made.expect("the import");
    dur.wait(DURABLE_WAIT).unwrap();
    let run = latest_run(&engine, Some(Source::Import))
        .unwrap()
        .expect("the run");
    assert_eq!(run.contacts, 2);

    let (refused, _) = undo_contact_run(&engine, &run.run);
    assert_eq!(refused, Err(ONE_CONTACT_ONLY.to_string()));
    assert_eq!(engine.stored_log().len(), 5, "nothing taken out");

    // The whole run's undo still takes both out.
    let (undone, dur) = undo_run(&engine, &run.run);
    assert_eq!(undone.unwrap().changed, 2);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(engine.stored_log().len(), 3);
}

/// ★ A deleted contact is listed under Recently deleted, survives a restart there, and comes
/// back whole — every field, its id, the foreign field another logger left — and `log.adi`
/// holds it again.
#[test]
fn a_deleted_contact_comes_back_whole_after_a_restart() {
    let d = Dir::new("history-restore");
    let engine = shared(&d, 3);
    let id = id_at(&*engine, 2);
    let was = row_of(&engine, id).unwrap();
    let (made, dur) = logwrite::change_ops(&engine, id, None, &[LogOp::Delete(id)], "delete");
    made.expect("reads").expect("deleted");
    dur.wait(DURABLE_WAIT).unwrap();
    settle(&engine);
    assert!(!mirror(&d).contains(&was.call));
    drop(engine);

    // The restart.
    let engine = Arc::new(Mutex::new(engine_on_store(&d)));
    let gone = recently_deleted(&engine, 50).unwrap();
    assert_eq!(gone.iter().map(|e| e.qso_id).collect::<Vec<_>>(), vec![id]);
    let (back, dur) = restore_deleted(&engine, id);
    assert_eq!(back.expect("restored").changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let back = row_of(&engine, id).expect("back in the log");
    assert_eq!(*back, *was, "the contact as it was, field for field");
    assert_eq!(
        adif_record_own_log(&back),
        adif_record_own_log(&was),
        "and as the log writes it"
    );
    assert!(recently_deleted(&engine, 50).unwrap().is_empty());
    settle(&engine);
    assert!(mirror(&d).contains(&was.call), "log.adi holds it again");
    // Restoring what is already there is refused, changing nothing.
    let (again, _) = restore_deleted(&engine, id);
    assert_eq!(again.unwrap().refused, vec![(id, CannotUndo::AlreadyThere)]);
}

/// ★ A run is undone whole, as one change: every contact an import added goes, the log it found
/// stays; and the fill job's chunks are one run, undone together.
#[test]
fn a_run_is_undone_whole() {
    let d = Dir::new("history-run");
    let engine = shared(&d, 3);
    let mut text = adif_header();
    for i in 0..300u64 {
        text.push_str(&adif_record(&qso(&format!("N{i}RUN"), 1_789_000_000 + i)));
    }
    let (made, dur) = logwrite::import_adif(&engine, &text);
    made.expect("the import");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(engine.stored_log().len(), 303);
    let run = latest_run(&engine, Some(Source::Import))
        .unwrap()
        .expect("the run");
    assert_eq!(run.contacts, 300);
    let (undone, dur) = undo_run(&engine, &run.run);
    assert_eq!(undone.unwrap().changed, 300);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        engine.stored_log().len(),
        3,
        "the log as the import found it"
    );
    settle(&engine);
    assert!(!mirror(&d).contains("N7RUN"));
}

/// The fill job writes in chunks of [`logwrite::FILL_CHUNK`], each its own change: they are one
/// run, undone together.
#[test]
fn the_fill_jobs_chunks_are_one_run_undone_together() {
    let d = Dir::new("history-fill-run");
    let n = logwrite::FILL_CHUNK + 6;
    let engine = shared(&d, n);
    let fills: Vec<LogFill> = engine
        .stored_log()
        .iter()
        .map(|r| LogFill {
            id: r.id.unwrap(),
            country: Some("Filled".into()),
            state: None,
        })
        .collect();
    assert_eq!(logwrite::fill(&engine, &fills, 1).expect("the fill"), n);
    settle(&engine);
    let run = latest_run(&engine, Some(Source::Fill))
        .unwrap()
        .expect("the run");
    assert_eq!(run.contacts as usize, n, "every chunk in one run");
    let (undone, dur) = undo_run(&engine, &run.run);
    assert_eq!(undone.unwrap().changed, n);
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(engine.stored_log().iter().all(|r| r.country.is_none()));
}

/// ⛔ A run's undo is ONE transaction: when the store turns any of it back — another window
/// changed one of its contacts after the undo read them — none of it lands, not the first chunk's
/// worth. Made again, it is made whole.
#[test]
fn a_run_undo_the_store_turns_back_changes_nothing() {
    let d = Dir::new("history-run-atomic");
    let a = shared(&d, 3);
    let mut text = adif_header();
    for i in 0..300u64 {
        text.push_str(&adif_record(&qso(&format!("N{i}RUN"), 1_789_000_000 + i)));
    }
    let (made, dur) = logwrite::import_adif(&a, &text);
    made.expect("the import");
    dur.wait(DURABLE_WAIT).unwrap();
    let run = latest_run(&a, Some(Source::Import)).unwrap().unwrap().run;
    let last = id_of(&a, "N299RUN");
    let b = Arc::new(Mutex::new(engine_on_store(&d)));
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let (b, plans) = (Arc::clone(&b), Rc::clone(&plans));
        move || {
            plans.set(plans.get() + 1);
            if plans.get() == 1 {
                // Window B marks the run's last contact, committed, inside A's plan-to-commit gap.
                let (made, _) = logwrite::change_ops(
                    &b,
                    last,
                    None,
                    &[LogOp::MarkQslCard {
                        id: last,
                        received: true,
                    }],
                    "window B",
                );
                assert!(matches!(made, Ok(Ok(_))), "B's card is made");
                engine_lock(&b)
                    .log_store_writer()
                    .flush(DURABLE_WAIT)
                    .unwrap();
            }
        }
    };
    let (undone, dur) = racing(hook, || undo_run(&a, &run));
    undone.expect("made");
    assert!(dur.turned_back(DURABLE_WAIT), "the store turned it back");
    let stored = tempo_core::logbook::sqlite::LogDb::open(&d.db())
        .unwrap()
        .load_all()
        .unwrap();
    assert_eq!(
        stored.len(),
        303,
        "not one contact of the run is gone: the undo was one transaction"
    );
    let (undone, dur) = until_written(|| undo_run(&a, &run));
    let undone = undone.expect("made again");
    dur.wait(DURABLE_WAIT).expect("and this time stored");
    // Made again, whole — but for the contact B marked meanwhile: a card received since the
    // import is a change of the operator's, and that contact stays, named.
    assert_eq!(undone.changed, 299);
    assert_eq!(undone.refused, vec![(last, CannotUndo::ChangedSince)]);
    let left: Vec<String> = a.stored_log().iter().map(|r| r.call.clone()).collect();
    assert_eq!(left.len(), 4);
    assert!(left.contains(&"N299RUN".to_string()));
}

/// ⛔ AN UNDO NEVER QUEUES AN UPLOAD. A corrected call queues the contact to every connector
/// (the positive control: the queue is watched, and moves); its undo puts the call back and
/// queues nothing — the contact is only owed again, its stamps cleared as a corrected call's are.
/// An undo that leaves the call alone keeps the stamps the contact has.
#[test]
fn an_undo_never_queues_an_upload() {
    let d = Dir::new("history-no-upload");
    let engine = shared(&d, 3);
    let id = id_at(&*engine, 0);
    let original = row_of(&engine, id).unwrap();
    let (_, dur) = logwrite::stamp_push(
        &engine,
        &original,
        UploadService::Qrz,
        UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_900_000_000,
            detail: None,
        },
    );
    dur.wait(DURABLE_WAIT).unwrap();
    engine_lock(&engine).take_pending_uploads();

    // An ordinary edit's undo keeps the stamp, and queues nothing.
    edit_comment(&engine, id, "note");
    let seq = history_of(&engine, id).unwrap().last().unwrap().seq;
    let (undone, dur) = undo_entry(&engine, seq);
    undone.unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(
        row_of(&engine, id).unwrap().upload.qrz.is_some(),
        "the stamp stays"
    );
    assert!(engine_lock(&engine).take_pending_uploads().is_empty());

    // A corrected call queues the contact (the control) …
    let row = row_of(&engine, id).unwrap();
    let key = QsoEdit::project(&row).key();
    let (made, dur) = logwrite::update_row(
        &engine,
        id,
        &key,
        |r| QsoRecord {
            call: "K9FIX".into(),
            ..r.clone()
        },
        |_, _| (),
    );
    made.unwrap().unwrap();
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(
        !engine_lock(&engine).take_pending_uploads().is_empty(),
        "the control: a corrected call is queued"
    );
    // The corrected contact reaches QRZ under its new call, and is stamped so.
    let (_, dur) = logwrite::stamp_push(
        &engine,
        &row_of(&engine, id).unwrap(),
        UploadService::Qrz,
        UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_900_000_100,
            detail: None,
        },
    );
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(
        row_of(&engine, id).unwrap().upload.qrz.is_some(),
        "premise: the contact carries a stamp the undo must clear"
    );
    engine_lock(&engine).take_pending_uploads();
    // … and its undo puts the call back and queues nothing.
    let seq = history_of(&engine, id).unwrap().last().unwrap().seq;
    let (undone, dur) = undo_entry(&engine, seq);
    assert_eq!(undone.unwrap().changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let now = row_of(&engine, id).unwrap();
    assert_eq!(now.call, original.call, "the call is back");
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "and nothing was queued"
    );
    assert!(
        now.upload.qrz.is_none(),
        "the stamp the corrected call earned is cleared: owed again under the call it carries"
    );
}

/// Contact `call` as another logger's ADIF file holds it: no id of ours.
fn record(call: &str, when: u64) -> String {
    adif_record(&qso(call, when))
}

/// ★ An import's Undo undoes THAT import — named by the run the import handed back — and nothing
/// made after it: a contact a companion program logs a moment later (the companion import, the
/// same import path) and one the operator logs both stay, and every contact the import added goes.
#[test]
fn an_imports_undo_takes_out_that_import_and_nothing_logged_since() {
    let d = Dir::new("history-import-undo");
    let engine = shared(&d, 3);
    let mut text = adif_header();
    for i in 0..20u64 {
        text.push_str(&record(&format!("N{i}IMP"), 1_789_000_000 + i * 600));
    }
    let run = new_run();
    let (made, dur) = until_written(|| logwrite::import_adif_in_run(&engine, &text, &run));
    assert_eq!(made.expect("the import").0, 20);
    dur.wait(DURABLE_WAIT).unwrap();

    // Logged after the import, before its Undo is clicked.
    let (added, dur) = logwrite::import_logged_contact(
        &engine,
        &(adif_header() + &record("W1WSJ", 1_789_900_000)),
    );
    assert_eq!(added.expect("the companion import"), 1);
    dur.wait(DURABLE_WAIT).unwrap();
    engine_lock(&engine).log_qso(qso("W2OPR", 1_789_950_000));
    settle(&engine);
    assert_eq!(
        latest_run(&engine, Some(Source::Import))
            .unwrap()
            .unwrap()
            .contacts,
        1,
        "the latest import is the companion's one contact, not the file: never what Undo should name"
    );

    let (undone, dur) = undo_run(&engine, &run);
    assert_eq!(undone.expect("undone").changed, 20);
    dur.wait(DURABLE_WAIT).unwrap();
    let calls: Vec<String> = engine.stored_log().iter().map(|r| r.call.clone()).collect();
    assert_eq!(
        calls.len(),
        5,
        "the three it found and the two logged since: {calls:?}"
    );
    assert!(
        calls.iter().all(|c| !c.ends_with("IMP")),
        "every contact it added is gone"
    );
    assert!(calls.contains(&"W1WSJ".to_string()) && calls.contains(&"W2OPR".to_string()));
}

/// ★ A purge's Undo puts back every contact the purge took out — the run the purge handed back —
/// and keeps a contact logged after it.
#[test]
fn a_purges_undo_puts_the_log_back_and_keeps_a_contact_logged_since() {
    let d = Dir::new("history-purge-undo");
    let engine = shared(&d, 30);
    let before: Vec<Arc<QsoRecord>> = engine.stored_log();
    let run = new_run();
    let (removed, dur) = engine_lock(&engine).with_log_tickets(|e| e.clear_logbook_in_run(&run));
    assert_eq!(removed, 30);
    dur.wait(DURABLE_WAIT).unwrap();
    engine_lock(&engine).log_qso(qso("W2OPR", 1_789_950_000));
    settle(&engine);
    assert_eq!(engine.stored_log().len(), 1);

    let (undone, dur) = undo_run(&engine, &run);
    let undone = undone.expect("undone");
    assert_eq!((undone.changed, undone.refused.len()), (30, 0));
    dur.wait(DURABLE_WAIT).unwrap();
    let after = engine.stored_log();
    assert_eq!(
        after.len(),
        31,
        "the log it took, and the contact logged since"
    );
    for was in &before {
        let back = after
            .iter()
            .find(|r| r.id == was.id)
            .expect("put back under its own id");
        assert_eq!(**back, **was, "whole, field for field");
    }
    settle(&engine);
    assert!(mirror(&d).contains("W2OPR") && mirror(&d).contains("K29ABC"));
}

/// ⛔ A whole run's undo too large to make in a moment is refused while the FT sequencer is armed
/// — it holds the Engine lock, which the radio loop needs at every slot boundary to key and to
/// unkey — and changes nothing; a restore of one contact is not. Disarmed, the undo is made.
#[test]
fn a_large_undo_waits_until_transmit_is_off() {
    let n = tempo_core::logbook::writer::CHUNK_ROWS + 20;
    let d = Dir::new("history-large-undo-tx");
    let engine = shared(&d, n);
    let run = new_run();
    let (_, dur) = engine_lock(&engine).with_log_tickets(|e| e.clear_logbook_in_run(&run));
    dur.wait(DURABLE_WAIT).unwrap();
    engine_lock(&engine).set_tx_enabled(true);
    assert!(
        engine_lock(&engine).transmit_needs_the_lock(),
        "premise: armed for FT"
    );

    let (refused, _) = undo_run(&engine, &run);
    assert_eq!(refused, Err(MUST_WAIT_FOR_TX.to_string()));
    assert!(engine.stored_log().is_empty(), "nothing was made");
    let one = recently_deleted(&engine, 1).unwrap().remove(0).qso_id;
    let (back, dur) = restore_deleted(&engine, one);
    assert_eq!(back.expect("a single restore is made").changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();

    engine_lock(&engine).set_tx_enabled(false);
    let (undone, dur) = undo_run(&engine, &run);
    let undone = undone.expect("made once transmit is off");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(undone.changed, n - 1);
    assert_eq!(engine.stored_log().len(), n);
}

/// ⛔ The same wait while the radio loop plays a voice-keyer message or an APRS frame on its own
/// deadline: the engine handed it over and keeps nothing of it, so only the loop's word says the
/// over is on the air ([`Engine::observe_loop_tx_hold`]). Once the over ends the undo is made.
#[test]
fn a_large_undo_waits_while_the_radio_loop_plays_a_message() {
    let n = tempo_core::logbook::writer::CHUNK_ROWS + 20;
    let d = Dir::new("history-large-undo-voice");
    let engine = shared(&d, n);
    let run = new_run();
    let (_, dur) = engine_lock(&engine).with_log_tickets(|e| e.clear_logbook_in_run(&run));
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(
        !engine_lock(&engine).transmit_needs_the_lock(),
        "premise: nothing armed or keyed"
    );
    engine_lock(&engine).observe_loop_tx_hold(Some(60_000.0));

    let (refused, _) = undo_run(&engine, &run);
    assert_eq!(refused, Err(MUST_WAIT_FOR_TX.to_string()));
    assert!(engine.stored_log().is_empty(), "nothing was made");

    engine_lock(&engine).observe_loop_tx_hold(None);
    let (undone, dur) = undo_run(&engine, &run);
    assert_eq!(undone.expect("made once the over ends").changed, n);
    dur.wait(DURABLE_WAIT).unwrap();
}

/// ★ A purge whose Undo was refused while transmitting can still be undone whole after its toast
/// has gone: it is listed with the imports still to undo ([`undoable_runs`]), and its undo goes
/// through the same run undo, with the same wait. Undone, it leaves the list; a single delete is
/// never on it.
#[test]
fn a_refused_purge_undo_stays_reachable_after_the_toast() {
    let n = tempo_core::logbook::writer::CHUNK_ROWS + 20;
    let d = Dir::new("history-purge-listed");
    let engine = shared(&d, n);
    delete(&engine, id_at(&*engine, 0));
    let run = new_run();
    let (_, dur) = engine_lock(&engine).with_log_tickets(|e| e.clear_logbook_in_run(&run));
    dur.wait(DURABLE_WAIT).unwrap();
    engine_lock(&engine).set_tx_enabled(true);
    let (refused, _) = undo_run(&engine, &run);
    assert_eq!(refused, Err(MUST_WAIT_FOR_TX.to_string()));

    let listed = undoable_runs(&engine, 20).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|r| (r.run.as_str(), r.source.as_str(), r.contacts))
            .collect::<Vec<_>>(),
        vec![(run.as_str(), "operator", (n - 1) as u64)],
        "the purge, and not the single delete before it"
    );
    let (again, _) = undo_run(&engine, &listed[0].run);
    assert_eq!(again, Err(MUST_WAIT_FOR_TX.to_string()), "the same wait");

    engine_lock(&engine).set_tx_enabled(false);
    let (undone, dur) = undo_run(&engine, &listed[0].run);
    let undone = undone.expect("made");
    assert_eq!(undone.changed, n - 1);
    dur.wait(DURABLE_WAIT).unwrap();

    // Undone, the purge is off the list and its undo is on it: the lasting Redo once the undo's
    // own toast has gone, through the same run undo and the same wait. Redone, the purge is back.
    let redo = undone.run.expect("the undo's own run");
    let listed = undoable_runs(&engine, 20).unwrap();
    assert_eq!(
        listed
            .iter()
            .map(|r| (r.run.as_str(), r.source.as_str(), r.contacts, r.removes))
            .collect::<Vec<_>>(),
        vec![(redo.as_str(), "undo", (n - 1) as u64, true)],
        "the undo, whose redo takes the contacts out again"
    );
    engine_lock(&engine).set_tx_enabled(true);
    let (refused, _) = undo_run(&engine, &redo);
    assert_eq!(
        refused,
        Err(MUST_WAIT_FOR_TX.to_string()),
        "the redo waits too"
    );
    assert_eq!(engine.stored_log().len(), n - 1, "nothing was made");
    engine_lock(&engine).set_tx_enabled(false);
    let (redone, dur) = undo_run(&engine, &redo);
    assert_eq!(redone.expect("redone").changed, n - 1);
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(engine.stored_log().is_empty());
    assert_eq!(
        undoable_runs(&engine, 20)
            .unwrap()
            .iter()
            .map(|r| r.source.as_str())
            .collect::<Vec<_>>(),
        vec!["undo", "operator"],
        "the redo (to undo again) and the purge"
    );
}

/// ★ The runs listed to undo whole are held to the days Settings keeps deleted contacts: an import
/// made a year ago is listed when the history is kept for good (0) and not at 30 days, beside one
/// made a week ago, which is listed at both.
#[test]
fn the_runs_listed_to_undo_are_held_to_the_days_settings_keeps() {
    use tempo_core::logbook::history::{Kind, Note};
    use tempo_core::logbook::sqlite::{Batch, LogDb, RowWrite};
    const DAY: i64 = 86_400_000;
    let d = Dir::new("history-runs-retention");
    let rows: Vec<Arc<QsoRecord>> = {
        let engine = shared(&d, 4);
        let rows = engine.stored_log();
        drop(engine);
        rows
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    {
        // Each pair taken out with no history, then added back as an import made `ago` days back.
        let mut db = LogDb::open(&d.db()).unwrap();
        for (pair, run, ago) in [
            (&rows[..2], "old import", 365),
            (&rows[2..], "recent import", 7),
        ] {
            let ids: Vec<RecordId> = pair.iter().filter_map(|r| r.id).collect();
            db.apply(Batch {
                remove: &ids,
                ..Default::default()
            })
            .unwrap();
            let upsert: Vec<RowWrite> = pair.iter().map(|r| RowWrite::new(Arc::clone(r))).collect();
            let note = Note {
                run: run.into(),
                at_ms: now - ago * DAY,
                source: Source::Import,
                edit: Kind::Merge,
                undoes: HashMap::new(),
            };
            db.apply(Batch {
                upsert: &upsert,
                history: Some(&note),
                ..Default::default()
            })
            .unwrap();
        }
    }
    let listed = |days: u32| -> Vec<String> {
        let mut s = Engine::new("K2DEF", "FN31", 0).settings().clone();
        s.history_snapshot_days = days;
        let mut e = Engine::with_settings(s);
        e.attach_log_store(open_fast(&d));
        let engine = Arc::new(Mutex::new(e));
        settle(&engine);
        undoable_runs(&engine, 20)
            .unwrap()
            .into_iter()
            .map(|r| r.run)
            .collect()
    };
    assert_eq!(
        listed(0),
        vec!["recent import", "old import"],
        "the control: kept for good"
    );
    assert_eq!(
        listed(30),
        vec!["recent import"],
        "a year is older than 30 days"
    );
}

/// ⛔ The engine's copy of the loop's hold ends by itself, a margin after the deadline it was told
/// of, whether or not the loop's end arrives: a loop that stops telling never leaves a large undo
/// refused for good.
#[test]
fn the_loops_hold_ends_by_itself_when_its_end_is_never_told() {
    let engine = Engine::new("K2DEF", "FN31", 0);
    let engine = Mutex::new(engine);
    engine_lock(&engine).observe_loop_tx_hold(Some(1.0));
    assert!(
        engine_lock(&engine).transmit_needs_the_lock(),
        "the control: held"
    );
    std::thread::sleep(std::time::Duration::from_millis(400));
    assert!(!engine_lock(&engine).transmit_needs_the_lock());
}

/// ★ A run's undo asks each contact it puts back about the contacts logged after ITS OWN delete.
/// One run deletes X, then — after the same contact was logged again as Y — Z, a contact the log
/// held beside X and Y all along. X is not put back beside Y; Z is, since Y was in the log beside
/// it before its delete.
#[test]
fn a_runs_undo_judges_each_put_back_by_its_own_delete() {
    let d = Dir::new("history-own-delete");
    let engine = shared(&d, 0);
    let t = 1_789_000_000;
    let file = adif_header() + &record("W1ROV", t) + &record("W1ROV", t + 400);
    let (made, dur) = logwrite::import_adif(&engine, &file);
    assert_eq!(made.expect("the import").0, 2, "premise: two contacts");
    dur.wait(DURABLE_WAIT).unwrap();
    let ids: Vec<RecordId> = engine.stored_log().iter().filter_map(|r| r.id).collect();
    let (x, z) = (ids[0], ids[1]);
    let run = new_run();
    let in_run = || {
        engine_lock(&engine).station_mut().origin = Some(Origin::in_run(Source::Operator, &run));
    };
    in_run();
    delete(&engine, x);
    // Y: the same contact again, 200 s from each — logged as SSB, so the log takes it, then
    // corrected to CW by hand.
    engine_lock(&engine).log_qso(QsoRecord {
        mode: "SSB".into(),
        ..qso("W1ROV", t + 200)
    });
    settle(&engine);
    let y = engine
        .stored_log()
        .iter()
        .find(|r| r.mode == "SSB")
        .and_then(|r| r.id)
        .expect("Y is logged");
    let row = row_of(&engine, y).unwrap();
    let (key, mut edit) = (QsoEdit::project(&row).key(), QsoEdit::project(&row));
    edit.mode = "CW".into();
    let (made, dur) = logwrite::edit_row(&engine, y, &key, &edit);
    made.expect("reads").expect("made");
    dur.wait(DURABLE_WAIT).unwrap();
    in_run();
    delete(&engine, z);
    assert_eq!(
        read(&engine, |db| db.history_run(&run)).unwrap().len(),
        2,
        "premise: one run"
    );

    let (undone, dur) = undo_run(&engine, &run);
    let undone = undone.expect("answered");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(undone.refused, vec![(x, CannotUndo::Duplicate)]);
    assert_eq!(undone.changed, 1);
    assert!(row_of(&engine, z).is_some(), "Z is back beside Y");
    assert!(row_of(&engine, x).is_none());
}

/// ★ The same contact logged again while a restore is being made — after the history was read,
/// before the change — is seen: the restore is planned again, and refused as a duplicate.
#[test]
fn a_contact_logged_again_during_the_restore_blocks_it() {
    let d = Dir::new("history-logged-during-restore");
    let engine = shared(&d, 2);
    engine_lock(&engine).log_qso(qso("W3HND", 1_789_000_000));
    settle(&engine);
    let id = id_of(&engine, "W3HND");
    delete(&engine, id);

    let racer = Arc::clone(&engine);
    let once = Cell::new(false);
    let (back, _) = racing(
        move || {
            if !once.replace(true) {
                engine_lock(&racer).log_qso(qso("W3HND", 1_789_000_030));
            }
        },
        || restore_deleted(&engine, id),
    );
    let back = back.expect("answered");
    assert_eq!(back.refused, vec![(id, CannotUndo::Duplicate)]);
    assert_eq!(engine.stored_log().len(), 3, "once: not put back beside it");
}

/// ★ Nothing is put back twice. A purge undone after the same contacts were imported again (new
/// ids, the log's duplicate rule: same station, band and mode within its window) puts none of
/// them back, and says so; a deleted contact logged again is not restored over it either.
#[test]
fn an_undo_never_puts_back_a_contact_the_log_holds_again() {
    let d = Dir::new("history-no-duplicate");
    let engine = shared(&d, 4);
    // Another logger's export of the same contacts: no id of ours.
    let file = engine.stored_log().iter().fold(adif_header(), |s, r| {
        s + &adif_record(&QsoRecord {
            id: None,
            ..QsoRecord::clone(r)
        })
    });
    let run = new_run();
    let (_, dur) = engine_lock(&engine).with_log_tickets(|e| e.clear_logbook_in_run(&run));
    dur.wait(DURABLE_WAIT).unwrap();
    let (made, dur) = logwrite::import_adif(&engine, &file);
    assert_eq!(made.expect("imported again").0, 4);
    dur.wait(DURABLE_WAIT).unwrap();

    let (undone, _) = undo_run(&engine, &run);
    let undone = undone.expect("answered");
    assert_eq!(undone.changed, 0);
    assert_eq!(undone.run, None, "nothing made, so nothing to redo");
    assert_eq!(undone.refused.len(), 4);
    assert!(undone
        .refused
        .iter()
        .all(|(_, why)| *why == CannotUndo::Duplicate));
    let mut named: Vec<&String> = undone.calls.values().collect();
    named.sort();
    assert_eq!(named, ["K0ABC", "K1ABC", "K2ABC", "K3ABC"]);
    assert_eq!(
        engine.stored_log().len(),
        4,
        "the log holds each contact once"
    );

    // A deleted contact logged again is not restored beside its new self.
    let id = id_of(&engine, "K2ABC");
    let (made, dur) = logwrite::change_ops(&engine, id, None, &[LogOp::Delete(id)], "delete");
    made.expect("reads").expect("deleted");
    dur.wait(DURABLE_WAIT).unwrap();
    let k2 = engine.stored_log().len();
    let again = file
        .split("<EOR>")
        .find(|r| r.contains("K2ABC"))
        .expect("K2ABC in the file")
        .to_string()
        + "<EOR>\n";
    let (made, dur) = logwrite::import_adif(&engine, &(adif_header() + &again));
    assert_eq!(made.expect("logged again").0, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    let (back, _) = restore_deleted(&engine, id);
    let back = back.expect("answered");
    assert_eq!(back.refused, vec![(id, CannotUndo::Duplicate)]);
    assert_eq!(engine.stored_log().len(), k2 + 1, "once");
}

/// Contact `call` on `grid` as another logger's ADIF file holds it.
fn record_on(call: &str, grid: &str, when: u64) -> String {
    adif_record(&QsoRecord {
        grid: Some(grid.into()),
        ..qso(call, when)
    })
}

/// Delete the contact `id`, as the Logbook's Delete does.
fn delete(engine: &Mutex<Engine>, id: RecordId) {
    let (made, dur) = logwrite::change_ops(engine, id, None, &[LogOp::Delete(id)], "delete");
    made.expect("reads").expect("deleted");
    dur.wait(DURABLE_WAIT).unwrap();
}

/// ★ A contact the log held beside the deleted one BEFORE the delete never blocks its restore. A
/// rover worked twice minutes apart, from two grids — two contacts the import kept, the same
/// station, band and mode within the duplicate window — and one deleted by mistake: Restore puts
/// it back.
#[test]
fn a_rover_pair_contact_deleted_by_mistake_comes_back() {
    let d = Dir::new("history-rover-pair");
    let engine = shared(&d, 2);
    let t = 1_789_000_000;
    let file =
        adif_header() + &record_on("W1ROV", "FN31", t) + &record_on("W1ROV/R", "FN32", t + 120);
    let (made, dur) = logwrite::import_adif(&engine, &file);
    assert_eq!(
        made.expect("the import").0,
        2,
        "premise: the import keeps both"
    );
    dur.wait(DURABLE_WAIT).unwrap();
    let rover = id_of(&engine, "W1ROV/R");
    delete(&engine, rover);
    // Another station logged since the delete: the restore asks the log, and W1ROV is not one of
    // the contacts logged since.
    engine_lock(&engine).log_qso(qso("K9OTH", t + 600));
    settle(&engine);

    let (back, dur) = restore_deleted(&engine, rover);
    let back = back.expect("answered");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!((back.changed, back.refused), (1, vec![]), "put back");
    let calls: Vec<String> = engine.stored_log().iter().map(|r| r.call.clone()).collect();
    assert!(calls.contains(&"W1ROV".to_string()) && calls.contains(&"W1ROV/R".to_string()));
    assert_eq!(calls.len(), 5);
}

/// ★ After a purge, Recently deleted puts the log back a contact at a time — the only way once the
/// purge's toast has gone — and the second contact of a pair the log held (the same station, band
/// and mode 200 s apart) comes back after the first: the first was in the log before the purge,
/// and is back by an undo.
#[test]
fn a_purge_restored_one_contact_at_a_time_loses_none() {
    let d = Dir::new("history-purge-one-by-one");
    let engine = shared(&d, 0);
    let t = 1_789_000_000;
    let file = adif_header() + &record("W1ROV", t) + &record("W1ROV", t + 200);
    let (made, dur) = logwrite::import_adif(&engine, &file);
    assert_eq!(
        made.expect("the import").0,
        2,
        "premise: the import keeps both"
    );
    dur.wait(DURABLE_WAIT).unwrap();
    let run = new_run();
    let (_, dur) = engine_lock(&engine).with_log_tickets(|e| e.clear_logbook_in_run(&run));
    dur.wait(DURABLE_WAIT).unwrap();
    // Another station logged since the purge: each restore asks the log.
    engine_lock(&engine).log_qso(qso("K9OTH", t + 600));
    settle(&engine);

    let gone = recently_deleted(&engine, 10).unwrap();
    assert_eq!(gone.len(), 2);
    for e in gone {
        let (back, dur) = restore_deleted(&engine, e.qso_id);
        let back = back.expect("answered");
        dur.wait(DURABLE_WAIT).unwrap();
        assert_eq!((back.changed, back.refused), (1, vec![]), "put back");
    }
    assert_eq!(engine.stored_log().len(), 3, "both contacts are back");
    assert!(recently_deleted(&engine, 10).unwrap().is_empty());
}

/// ★ A deleted contact logged again by hand afterwards is not restored beside its new self; the
/// operator's "Restore anyway" puts it back all the same, as a second contact.
#[test]
fn a_contact_logged_again_by_hand_blocks_its_restore_but_not_restore_anyway() {
    let d = Dir::new("history-logged-again");
    let engine = shared(&d, 2);
    engine_lock(&engine).log_qso(qso("W3HND", 1_789_000_000));
    settle(&engine);
    let id = id_of(&engine, "W3HND");
    delete(&engine, id);
    engine_lock(&engine).log_qso(qso("W3HND", 1_789_000_030));
    settle(&engine);
    assert_eq!(engine.stored_log().len(), 3, "premise: logged again");

    let (back, _) = restore_deleted(&engine, id);
    let back = back.expect("answered");
    assert_eq!(back.refused, vec![(id, CannotUndo::Duplicate)]);
    assert_eq!(back.calls.get(&id).map(String::as_str), Some("W3HND"));
    assert_eq!(engine.stored_log().len(), 3, "not put back");

    let (back, dur) = restore_deleted_anyway(&engine, id);
    let back = back.expect("answered");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!((back.changed, back.refused), (1, vec![]));
    assert_eq!(
        engine.stored_log().len(),
        4,
        "put back beside it, as the operator asked"
    );
    assert!(row_of(&engine, id).is_some(), "under its own id");
}

/// ★ An undo that takes one contact out and puts another back near it — the same station, band
/// and mode, within the duplicate window, the one taken out added after the other was deleted —
/// puts it back: the contact the same undo takes out is not counted against it. Without that the
/// removal would be made and the put-back refused, and the contact would leave the log. Undoing
/// the undo makes the change again.
#[test]
fn an_undo_that_swaps_two_near_contacts_puts_the_deleted_one_back() {
    use tempo_core::logbook::history::Note;
    use tempo_core::logbook::sqlite::{Batch, LogDb, RowWrite};
    let d = Dir::new("history-swap");
    let (b, a) = {
        let engine = shared(&d, 3);
        let b = id_at(&*engine, 0);
        let a = id_at(&*engine, 1);
        // A's id, free for the swap's new contact.
        delete(&engine, a);
        (b, a)
    };
    // One change: B taken out, and in its place a contact under A's id with B's station, band
    // and mode a minute later — added after B's delete, in the same change.
    let run = "swap";
    {
        let mut db = LogDb::open(&d.db()).unwrap();
        let was = db.rows_by_ids(&[b]).unwrap().remove(0);
        let swapped = QsoRecord {
            id: Some(a),
            when_unix: was.when_unix + 60,
            ..was.clone()
        };
        // Made now: a delete older than Settings keeps its contact is taken out of the history by
        // the trim the next engine's launch starts in the background, and B could not come back.
        let note = Note {
            run: run.into(),
            at_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as i64,
            source: Source::Operator,
            edit: Kind::Edit,
            undoes: HashMap::new(),
        };
        db.apply(Batch {
            remove: &[b],
            upsert: &[RowWrite::new(Arc::new(swapped))],
            history: Some(&note),
            ..Default::default()
        })
        .unwrap();
    }
    let engine = Arc::new(Mutex::new(engine_on_store(&d)));
    settle(&engine);
    assert!(row_of(&engine, b).is_none() && row_of(&engine, a).is_some());

    let (undone, dur) = undo_run(&engine, run);
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!((undone.changed, undone.refused), (2, vec![]));
    assert!(row_of(&engine, b).is_some(), "B is back");
    assert!(
        row_of(&engine, a).is_none(),
        "the contact that replaced it is gone"
    );

    let (redone, dur) = undo_run(&engine, &undone.run.expect("the undo's run"));
    assert_eq!(redone.expect("redone").changed, 2);
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(
        row_of(&engine, b).is_none() && row_of(&engine, a).is_some(),
        "made again"
    );
}

/// ★ An import's undo keeps a contact edited or confirmed since — by the operator, a download, a
/// later import — names it, and takes out the rest. The fill job's country is not such a change.
#[test]
fn an_imports_undo_keeps_a_contact_changed_since_and_names_it() {
    let d = Dir::new("history-import-changed");
    let engine = shared(&d, 2);
    let mut text = adif_header();
    for i in 0..5u64 {
        text.push_str(&record(&format!("N{i}IMP"), 1_789_000_000 + i * 600));
    }
    let run = new_run();
    let (made, dur) = until_written(|| logwrite::import_adif_in_run(&engine, &text, &run));
    assert_eq!(made.expect("the import").0, 5);
    dur.wait(DURABLE_WAIT).unwrap();
    let (edited, confirmed, filled) = (
        id_of(&engine, "N0IMP"),
        id_of(&engine, "N1IMP"),
        id_of(&engine, "N2IMP"),
    );
    edit_comment(&engine, edited, "worked him again on 40");
    let report = adif_header()
        + &adif_record(&row_of(&engine, confirmed).unwrap())
            .replace("<EOR>", "<LOTW_QSL_RCVD:1>Y<EOR>");
    let (made, dur) = logwrite::merge_lotw_report(&engine, &report);
    made.expect("the report");
    dur.wait(DURABLE_WAIT).unwrap();
    logwrite::fill(
        &engine,
        &[LogFill {
            id: filled,
            country: Some("Filled".into()),
            state: None,
        }],
        1,
    )
    .expect("the fill");
    settle(&engine);

    let (undone, dur) = undo_run(&engine, &run);
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        undone.changed, 3,
        "the three no one touched since, the filled one among them"
    );
    let mut kept = undone.refused.clone();
    kept.sort_by_key(|(id, _)| id.to_string());
    let mut want = vec![
        (edited, CannotUndo::ChangedSince),
        (confirmed, CannotUndo::ChangedSince),
    ];
    want.sort_by_key(|(id, _)| id.to_string());
    assert_eq!(kept, want);
    assert_eq!(undone.calls.get(&edited).map(String::as_str), Some("N0IMP"));
    assert_eq!(
        undone.calls.get(&confirmed).map(String::as_str),
        Some("N1IMP")
    );
    let mut calls: Vec<String> = engine.stored_log().iter().map(|r| r.call.clone()).collect();
    calls.sort();
    assert_eq!(calls, ["K0ABC", "K1ABC", "N0IMP", "N1IMP"]);
}

/// ★ A confirmation that lands while an import's undo is being planned — after the plan read the
/// contacts — keeps the contact it confirmed: the plan is taken again, and what changed since is
/// read again with it.
#[test]
fn a_confirmation_landing_during_the_plan_keeps_its_contact() {
    let d = Dir::new("history-import-race");
    let engine = shared(&d, 1);
    let mut text = adif_header();
    for i in 0..3u64 {
        text.push_str(&record(&format!("N{i}IMP"), 1_789_000_000 + i * 600));
    }
    let run = new_run();
    let (made, dur) = until_written(|| logwrite::import_adif_in_run(&engine, &text, &run));
    assert_eq!(made.expect("the import").0, 3);
    dur.wait(DURABLE_WAIT).unwrap();
    let confirmed = id_of(&engine, "N1IMP");
    let report = adif_header()
        + &adif_record(&row_of(&engine, confirmed).unwrap())
            .replace("<EOR>", "<LOTW_QSL_RCVD:1>Y<EOR>");

    let racer = Arc::clone(&engine);
    let once = Cell::new(false);
    let (undone, dur) = racing(
        move || {
            if !once.replace(true) {
                let (made, dur) = logwrite::merge_lotw_report(&racer, &report);
                made.expect("the report");
                dur.wait(DURABLE_WAIT).unwrap();
            }
        },
        || undo_run(&engine, &run),
    );
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(undone.refused, vec![(confirmed, CannotUndo::ChangedSince)]);
    assert_eq!(undone.changed, 2);
    assert!(
        row_of(&engine, confirmed).is_some(),
        "the confirmed contact stays"
    );
}

/// ★ An import's undo keeps a contact uploaded since, and one added again since by another
/// import under the same id (deleted, then imported again from a Nexus export).
#[test]
fn an_imports_undo_keeps_a_contact_uploaded_or_added_again_since() {
    let d = Dir::new("history-import-uploaded");
    let engine = shared(&d, 1);
    // A contact from a Nexus export: it carries its id.
    let own = row_of(&engine, id_at(&*engine, 0)).unwrap();
    let again = own.id.unwrap();
    delete(&engine, again);
    let mut text = adif_header() + &adif_record_own_log(&own);
    for i in 0..2u64 {
        text.push_str(&record(&format!("N{i}IMP"), 1_789_000_000 + i * 600));
    }
    let run = new_run();
    let (made, dur) = until_written(|| logwrite::import_adif_in_run(&engine, &text, &run));
    assert_eq!(made.expect("the import").0, 3);
    dur.wait(DURABLE_WAIT).unwrap();
    assert!(
        row_of(&engine, again).is_some(),
        "premise: the export's id is kept"
    );
    // Uploaded since.
    let uploaded = id_of(&engine, "N0IMP");
    let (_, dur) = logwrite::stamp_push(
        &engine,
        &row_of(&engine, uploaded).unwrap(),
        UploadService::Qrz,
        UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_900_000_000,
            detail: None,
        },
    );
    dur.wait(DURABLE_WAIT).unwrap();
    // Deleted and imported again by another import.
    delete(&engine, again);
    let (made, dur) = logwrite::import_adif(&engine, &(adif_header() + &adif_record_own_log(&own)));
    assert_eq!(made.expect("imported again").0, 1);
    dur.wait(DURABLE_WAIT).unwrap();

    let (undone, dur) = undo_run(&engine, &run);
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    let mut kept = undone.refused.clone();
    kept.sort_by_key(|(id, _)| id.to_string());
    let mut want = vec![
        (uploaded, CannotUndo::ChangedSince),
        (again, CannotUndo::ChangedSince),
    ];
    want.sort_by_key(|(id, _)| id.to_string());
    assert_eq!(kept, want);
    assert_eq!(undone.changed, 1, "the contact no one touched since goes");
    assert!(row_of(&engine, uploaded).is_some() && row_of(&engine, again).is_some());
}

/// ★ At each launch the writer takes out of the history the whole contacts deletes kept longer
/// ago than Settings keeps them (90 days by default) — and never an edit's fields. With 0 days it
/// keeps them (the positive control: the same store, the same launch, nothing taken).
#[test]
fn a_launch_trims_old_deleted_contacts_and_keeps_every_edit() {
    use tempo_core::logbook::history::{Kind, Note};
    use tempo_core::logbook::sqlite::LogDb;
    const DAY: i64 = 86_400_000;
    let d = Dir::new("history-trim");
    let ids: Vec<RecordId> = {
        let engine = shared(&d, 3);
        let ids = engine.stored_log().iter().filter_map(|r| r.id).collect();
        drop(engine);
        ids
    };
    // A year ago: the first contact edited, the second deleted; a week ago, the third deleted.
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let at = |run: &str, at_ms: i64| Note {
        run: run.into(),
        at_ms,
        source: Source::Operator,
        edit: Kind::Edit,
        undoes: HashMap::new(),
    };
    {
        let mut db = LogDb::open(&d.db()).unwrap();
        let mut first = db.rows_by_ids(&ids[..1]).unwrap().remove(0);
        first.comment = Some("a year ago".into());
        let upsert = [tempo_core::logbook::sqlite::RowWrite::new(Arc::new(first))];
        let old_edit = at("old edit", now - 365 * DAY);
        db.apply(tempo_core::logbook::sqlite::Batch {
            upsert: &upsert,
            history: Some(&old_edit),
            ..Default::default()
        })
        .unwrap();
        for (id, run, ago) in [(ids[1], "old delete", 365), (ids[2], "recent delete", 7)] {
            let note = at(run, now - ago * DAY);
            db.apply(tempo_core::logbook::sqlite::Batch {
                remove: &[id],
                history: Some(&note),
                ..Default::default()
            })
            .unwrap();
        }
    }
    let gone = |engine: &Mutex<Engine>| -> Vec<RecordId> {
        recently_deleted(engine, 10)
            .unwrap()
            .iter()
            .map(|e| e.qso_id)
            .collect()
    };

    // Kept for good: nothing taken.
    let mut s = Engine::new("K2DEF", "FN31", 0).settings().clone();
    s.history_snapshot_days = 0;
    let mut e = Engine::with_settings(s);
    e.attach_log_store(open_fast(&d));
    let engine = Arc::new(Mutex::new(e));
    settle(&engine);
    std::thread::sleep(std::time::Duration::from_millis(300));
    assert_eq!(
        gone(&engine),
        vec![ids[2], ids[1]],
        "the control: both kept"
    );
    drop(engine);

    // The default: the year-old delete goes, in the background; the week-old one stays.
    let engine = Arc::new(Mutex::new(engine_on_store(&d)));
    assert_eq!(engine_lock(&engine).settings().history_snapshot_days, 90);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while gone(&engine).len() > 1 && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(
        gone(&engine),
        vec![ids[2]],
        "only the recent delete is left"
    );
    let edit = history_of(&engine, ids[0]).unwrap();
    assert_eq!(
        edit.iter().map(|e| e.kind).collect::<Vec<_>>(),
        vec![Kind::Edit],
        "the year-old edit is kept"
    );
    assert_eq!(edit[0].changes().len(), 1);
}

/// The log, by id, to compare whole.
fn log_by_id(engine: &Mutex<Engine>) -> Vec<QsoRecord> {
    let mut log: Vec<QsoRecord> = engine.stored_log().iter().map(|r| (**r).clone()).collect();
    log.sort_by_key(|r| r.id.map(|id| id.to_string()));
    log
}

/// The fill job's run over the three contacts of a fresh log: three edits in one run.
fn fill_three(engine: &Mutex<Engine>) -> String {
    let fills: Vec<LogFill> = engine
        .stored_log()
        .iter()
        .map(|r| LogFill {
            id: r.id.unwrap(),
            country: Some("Filled".into()),
            state: None,
        })
        .collect();
    assert_eq!(logwrite::fill(engine, &fills, 1).expect("the fill"), 3);
    settle(engine);
    latest_run(engine, Some(Source::Fill))
        .unwrap()
        .expect("the run")
        .run
}

/// Undo `id`'s latest change on its own, as its History panel does: the undo's run.
fn undo_latest_of(engine: &Mutex<Engine>, id: RecordId) -> String {
    let seq = history_of(engine, id).unwrap().last().unwrap().seq;
    let (undone, dur) = undo_entry(engine, seq);
    let undone = undone.expect("undone");
    assert_eq!((undone.changed, undone.refused), (1, vec![]));
    dur.wait(DURABLE_WAIT).unwrap();
    undone.run.expect("the undo's run")
}

/// ★ A whole run's undo after one of its contacts was undone on its own reverts the rest and does
/// not name that contact as changed since: the operator already took it back.
#[test]
fn a_runs_undo_skips_a_contact_already_undone_on_its_own() {
    let d = Dir::new("history-run-after-single");
    let engine = shared(&d, 3);
    let before = log_by_id(&engine);
    let run = fill_three(&engine);
    undo_latest_of(&engine, id_at(&*engine, 1));

    let (undone, dur) = undo_run(&engine, &run);
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!((undone.changed, undone.refused), (2, vec![]));
    assert_eq!(log_by_id(&engine), before, "the log as the run found it");
}

/// ★ Redone since, the contact's change stands again: the whole run's undo reverts all three.
#[test]
fn a_runs_undo_reverts_a_contact_whose_own_undo_was_redone() {
    let d = Dir::new("history-run-after-redo");
    let engine = shared(&d, 3);
    let before = log_by_id(&engine);
    let run = fill_three(&engine);
    let id = id_at(&*engine, 1);
    let undo = undo_latest_of(&engine, id);
    let (redone, dur) = undo_contact_run(&engine, &undo);
    assert_eq!(redone.expect("redone").changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        row_of(&engine, id).unwrap().country.as_deref(),
        Some("Filled")
    );

    let (undone, dur) = undo_run(&engine, &run);
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!((undone.changed, undone.refused), (3, vec![]));
    assert_eq!(log_by_id(&engine), before, "the log as the run found it");
}

/// ★ An import's undo after one contact it added was taken out on its own takes out the rest, and
/// does not name the one already gone.
#[test]
fn an_imports_undo_skips_a_contact_already_taken_out_on_its_own() {
    let d = Dir::new("history-import-after-single");
    let engine = shared(&d, 3);
    let before = log_by_id(&engine);
    let mut text = adif_header();
    for i in 0..3u64 {
        text.push_str(&record(&format!("N{i}IMP"), 1_789_000_000 + i * 600));
    }
    let run = new_run();
    let (made, dur) = until_written(|| logwrite::import_adif_in_run(&engine, &text, &run));
    assert_eq!(made.expect("the import").0, 3);
    dur.wait(DURABLE_WAIT).unwrap();
    undo_latest_of(&engine, id_of(&engine, "N1IMP"));

    let (undone, dur) = undo_run(&engine, &run);
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!((undone.changed, undone.refused), (2, vec![]));
    assert_eq!(log_by_id(&engine), before, "the log as the import found it");
}

/// ★ A purge's undo after one contact was restored on its own puts back the rest, and does not
/// name the one already back.
#[test]
fn a_purges_undo_skips_a_contact_already_restored_on_its_own() {
    let d = Dir::new("history-purge-after-single");
    let engine = shared(&d, 3);
    let before = log_by_id(&engine);
    let run = new_run();
    let (removed, dur) = engine_lock(&engine).with_log_tickets(|e| e.clear_logbook_in_run(&run));
    assert_eq!(removed, 3);
    dur.wait(DURABLE_WAIT).unwrap();
    let id = before[0].id.unwrap();
    let (back, dur) = restore_deleted(&engine, id);
    assert_eq!(back.expect("restored").changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();

    let (undone, dur) = undo_run(&engine, &run);
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!((undone.changed, undone.refused), (2, vec![]));
    assert_eq!(log_by_id(&engine), before, "the log as the purge found it");
}

/// ★ A single undo that kept a field changed since does not take the entry out of the whole run's
/// undo: once the operator's own edit is undone and the field holds the run's value again, the
/// whole run's undo reverts it with the rest, and names nothing refused.
#[test]
fn a_runs_undo_reverts_a_field_a_single_undo_kept() {
    let d = Dir::new("history-run-after-partial");
    let engine = shared(&d, 3);
    let before = log_by_id(&engine);
    let fills: Vec<LogFill> = engine
        .stored_log()
        .iter()
        .map(|r| LogFill {
            id: r.id.unwrap(),
            country: Some("Filled".into()),
            state: Some("FS".into()),
        })
        .collect();
    assert_eq!(logwrite::fill(&engine, &fills, 1).expect("the fill"), 3);
    settle(&engine);
    let run = latest_run(&engine, Some(Source::Fill))
        .unwrap()
        .expect("the run")
        .run;
    let id = id_at(&*engine, 1);
    let state_before = before
        .iter()
        .find(|r| r.id == Some(id))
        .unwrap()
        .state
        .clone();
    assert_ne!(state_before.as_deref(), Some("FS"));
    // The operator edits the state.
    let row = row_of(&engine, id).unwrap();
    let (key, mut edit) = (QsoEdit::project(&row).key(), QsoEdit::project(&row));
    edit.state = Some("XX".into());
    let (made, dur) = logwrite::edit_row(&engine, id, &key, &edit);
    made.expect("reads").expect("made");
    dur.wait(DURABLE_WAIT).unwrap();
    let edit_seq = history_of(&engine, id).unwrap().last().unwrap().seq;
    // The fill's entry for the contact undone on its own: the country goes back, the state stays.
    let fill_seq = history_of(&engine, id)
        .unwrap()
        .into_iter()
        .find(|e| e.run == run)
        .unwrap()
        .seq;
    let (single, dur) = undo_entry(&engine, fill_seq);
    assert_eq!(single.expect("undone").changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    // The operator's edit undone: the state holds the fill's value again.
    let (own, dur) = undo_entry(&engine, edit_seq);
    assert_eq!(own.expect("the edit undone").changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(row_of(&engine, id).unwrap().state.as_deref(), Some("FS"));

    let (undone, dur) = undo_run(&engine, &run);
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!((undone.changed, undone.refused), (3, vec![]));
    assert_eq!(row_of(&engine, id).unwrap().state, state_before);
    assert_eq!(log_by_id(&engine), before, "the log as the run found it");
}

/// ★ A purge's contact restored on its own and then deleted again by the operator comes back with
/// the purge's undo: the purge's delete is planned as any other.
#[test]
fn a_purges_undo_puts_back_a_contact_restored_then_deleted_again() {
    let d = Dir::new("history-purge-restored-deleted");
    let engine = shared(&d, 3);
    let before = log_by_id(&engine);
    let run = new_run();
    let (removed, dur) = engine_lock(&engine).with_log_tickets(|e| e.clear_logbook_in_run(&run));
    assert_eq!(removed, 3);
    dur.wait(DURABLE_WAIT).unwrap();
    let id = before[0].id.unwrap();
    let (back, dur) = restore_deleted(&engine, id);
    assert_eq!(back.expect("restored").changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    delete(&engine, id);

    let (undone, dur) = undo_run(&engine, &run);
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!((undone.changed, undone.refused), (3, vec![]));
    assert_eq!(log_by_id(&engine), before, "the log as the purge found it");
}

/// ★ THE CONTACT CARD'S SAVE: one change, one history entry, one Undo. A save that corrects a
/// field the form has a box for AND fields kept as written (set one, correct one, remove one)
/// changes exactly those and nothing else on the row, is recorded once, and its undo puts the
/// whole row back as it was. A change made against a value the contact no longer holds is
/// refused, writing nothing; one naming a field the card may not write is an error.
#[test]
fn a_card_save_with_passthrough_fields_is_one_change_and_one_undo() {
    use tempo_core::logbook::FieldEdit;
    let d = Dir::new("history-card-fields");
    let engine = shared(&d, 3);
    let id = id_at(&*engine, 1);
    let change = |old: Option<&str>, new: Option<&str>, tag: &str| FieldEdit {
        tag: tag.into(),
        old: old.map(str::to_string),
        new: new.map(str::to_string),
    };
    // Seed two passthrough fields, as an import would have left them.
    let (seeded, dur) = logwrite::change_ops(
        &engine,
        id,
        None,
        &[LogOp::SetFields {
            id,
            fields: vec![
                ("CNTY".into(), Some("MA,MIDDLESEX".into())),
                ("CQZ".into(), Some("5".into())),
            ],
        }],
        "seed",
    );
    seeded.expect("reads").expect("made");
    dur.wait(DURABLE_WAIT).unwrap();
    let before = QsoRecord::clone(&row_of(&engine, id).unwrap());
    let entries = history_of(&engine, id).unwrap().len();

    let mut edit = QsoEdit::project(&before);
    edit.name = Some("Hans".into());
    let fields = [
        change(Some("MA,MIDDLESEX"), Some("MA,ESSEX"), "CNTY"),
        change(Some("5"), None, "CQZ"),
        change(None, Some("8"), "ITUZ"),
    ];
    let key = QsoEdit::project(&before).key();

    // Made against a value the contact no longer holds: refused, nothing written.
    let stale = [change(Some("MA,SUFFOLK"), Some("MA,ESSEX"), "CNTY")];
    let (made, _) = logwrite::edit_row_with_fields(&engine, id, &key, &edit, &stale);
    assert!(
        matches!(made, Ok(Err(crate::station::RowRefusal::Changed(_)))),
        "{made:?}"
    );
    // A field the card may not write: an error, nothing written.
    let modeled = [change(None, Some("WWFF"), "MY_SIG")];
    let (made, _) = logwrite::edit_row_with_fields(&engine, id, &key, &edit, &modeled);
    assert!(made.is_err(), "{made:?}");
    settle(&engine);
    assert_eq!(
        *row_of(&engine, id).unwrap(),
        before,
        "neither wrote anything"
    );

    let (made, dur) = logwrite::edit_row_with_fields(&engine, id, &key, &edit, &fields);
    made.expect("reads").expect("made");
    dur.wait(DURABLE_WAIT).unwrap();
    let mut want = before.clone();
    want.name = Some("Hans".into());
    want.extra.retain(|(k, _)| k != "CNTY" && k != "CQZ");
    want.extra.push(("CNTY".into(), "MA,ESSEX".into()));
    want.extra.push(("ITUZ".into(), "8".into()));
    want.extra.sort();
    assert_eq!(
        *row_of(&engine, id).unwrap(),
        want,
        "exactly the fields changed"
    );
    let h = history_of(&engine, id).unwrap();
    assert_eq!(h.len(), entries + 1, "one save, one entry");

    let (undone, dur) = undo_entry(&engine, h.last().unwrap().seq);
    assert_eq!(undone.expect("undone").changed, 1);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        *row_of(&engine, id).unwrap(),
        before,
        "the undo puts the whole row back"
    );
}

/// A contact as an uploaded one stands: the old call's extras (LoTW's DXCC, zones, continent,
/// county, received dates, eQSL's AG mark), another logger's `LOTW_QSL_SENT=Y`, and the QRZ and
/// LoTW stamps of real uploads. Returns its id and the row as read back.
fn uploaded_contact(engine: &Arc<Mutex<Engine>>) -> (RecordId, QsoRecord) {
    let id = id_at(&**engine, 1);
    let (seeded, dur) = logwrite::change_ops(
        engine,
        id,
        None,
        &[LogOp::SetFields {
            id,
            fields: [
                ("APP_EQSL_AG", "Y"),
                ("APP_NEXUS_LOTW_DXCC", "182"),
                ("CNTY", "MA,ESSEX"),
                ("CONT", "NA"),
                ("CQZ", "8"),
                ("EQSL_QSLRDATE", "20150302"),
                ("ITUZ", "11"),
                ("LOTW_QSLRDATE", "20150301"),
                ("LOTW_QSL_SENT", "Y"),
            ]
            .map(|(k, v)| (k.to_string(), Some(v.to_string())))
            .to_vec(),
        }],
        "seed",
    );
    seeded.expect("reads").expect("made");
    dur.wait(DURABLE_WAIT).unwrap();
    for (service, when) in [
        (UploadService::Qrz, 1_900_000_000),
        (UploadService::Clublog, 1_900_000_010),
        (UploadService::Eqsl, 1_900_000_020),
        (UploadService::Lotw, 1_900_000_050),
    ] {
        let (stamped, dur) = logwrite::stamp_push(
            engine,
            &row_of(engine, id).unwrap(),
            service,
            UploadStatus {
                outcome: UploadOutcome::Accepted,
                when_unix: when,
                detail: None,
            },
        );
        assert!(stamped, "seeded the {service:?} stamp");
        dur.wait(DURABLE_WAIT).unwrap();
    }
    engine_lock(engine).take_pending_uploads();
    let before = QsoRecord::clone(&row_of(engine, id).unwrap());
    assert!(
        before.upload.qrz.is_some()
            && before.upload.clublog.is_some()
            && before.upload.eqsl.is_some()
            && before.upload.lotw.is_some()
            && before
                .extra
                .iter()
                .any(|(k, v)| k == "LOTW_QSL_SENT" && v == "Y"),
        "premise: real upload marks on the row: {before:?}"
    );
    (id, before)
}

/// The edit form's correction of `before`'s call to the same call with an X, made.
fn correct_call(engine: &Arc<Mutex<Engine>>, id: RecordId, before: &QsoRecord) -> String {
    let mut edit = QsoEdit::project(before);
    edit.call = format!("{}X", before.call);
    let (made, dur) = logwrite::edit_row(engine, id, &QsoEdit::project(before).key(), &edit);
    made.expect("reads").expect("made");
    dur.wait(DURABLE_WAIT).unwrap();
    edit.call
}

/// ★ A CORRECTED CALL'S UNDO puts back everything the correction took off with the old call
/// (operator ruling): LoTW's DXCC, the zones, continent and county, LoTW's and eQSL's received
/// dates and eQSL's Authenticity-Guaranteed mark, the confirmations and the DXCC number — one save,
/// one history entry. On an UPLOADED contact whose re-send under the corrected call has not gone
/// out (still queued, untried), the undo also cancels that re-send and puts back the upload stamps
/// and `LOTW_QSL_SENT`: the row comes back exactly, and nothing is left queued.
#[test]
fn a_corrected_calls_undo_puts_back_what_the_old_call_carried() {
    let d = Dir::new("history-call-fix");
    let engine = shared(&d, 3);
    let (id, before) = uploaded_contact(&engine);
    let entries = history_of(&engine, id).unwrap().len();

    let call = correct_call(&engine, id, &before);
    let fixed = row_of(&engine, id).unwrap();
    assert_eq!(fixed.call, call);
    assert!(
        fixed.extra.iter().all(|(k, _)| !matches!(
            k.as_str(),
            "APP_EQSL_AG"
                | "APP_NEXUS_LOTW_DXCC"
                | "CNTY"
                | "CONT"
                | "CQZ"
                | "EQSL_QSLRDATE"
                | "ITUZ"
                | "LOTW_QSLRDATE"
                | "LOTW_QSL_SENT"
        )),
        "the old call's taken off: {:?}",
        fixed.extra
    );
    assert_eq!(
        (fixed.upload.qrz.as_ref(), fixed.upload.lotw.as_ref()),
        (None, None),
        "the correction clears the stamps"
    );
    let h = history_of(&engine, id).unwrap();
    assert_eq!(h.len(), entries + 1, "one save, one entry");
    assert_eq!(
        engine_lock(&engine)
            .station()
            .pending_uploads
            .iter()
            .filter(|p| p.rec.id == Some(id) && p.rec.call == call)
            .count(),
        1,
        "premise: the re-send under the corrected call is queued, untried"
    );

    let (undone, dur) = undo_entry(&engine, h.last().unwrap().seq);
    let undone = undone.expect("undone");
    assert_eq!(undone.changed, 1);
    assert!(undone.owed.is_empty(), "nothing owed: {:?}", undone.owed);
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        *row_of(&engine, id).unwrap(),
        before,
        "the undo puts the whole row back, its upload marks with it"
    );
    assert!(
        engine_lock(&engine).take_pending_uploads().is_empty(),
        "the re-send under the corrected call is cancelled"
    );
}

/// `service`'s stamp `outcome` on the contact `rec` names, made as a push's is (by its id, only
/// while the row still carries the call it was sent under).
fn stamp(
    engine: &Arc<Mutex<Engine>>,
    rec: &QsoRecord,
    service: UploadService,
    outcome: UploadOutcome,
) -> bool {
    let (stamped, dur) = logwrite::stamp_push(
        engine,
        rec,
        service,
        UploadStatus {
            outcome,
            when_unix: 1_900_000_200,
            detail: None,
        },
    );
    dur.wait(DURABLE_WAIT).unwrap();
    stamped
}

/// The callsign correction's entry in `id`'s history.
fn correction_seq(engine: &Arc<Mutex<Engine>>, id: RecordId) -> i64 {
    history_of(engine, id)
        .unwrap()
        .iter()
        .rev()
        .find(|e| e.changes().iter().any(|c| c.tag == "CALL"))
        .unwrap()
        .seq
}

/// The connector toggles: QRZ, Club Log, eQSL and HRDLog.net.
fn toggles(engine: &Arc<Mutex<Engine>>, qrz: bool, clublog: bool, eqsl: bool, hrdlog: bool) {
    let mut e = engine_lock(engine);
    e.set_upload_toggles(Some(qrz), Some(clublog), Some(eqsl));
    let mut s = e.settings().clone();
    s.hrdlog_upload = hrdlog;
    e.apply_settings(s);
}

/// What is queued for the contact `id`, by call.
fn queued_calls(engine: &Arc<Mutex<Engine>>, id: RecordId) -> Vec<String> {
    engine_lock(engine)
        .station()
        .pending_uploads
        .iter()
        .filter(|p| p.rec.id == Some(id))
        .map(|p| p.rec.call.clone())
        .collect()
}

/// `before` with the stamps of `services` taken off — and `LOTW_QSL_SENT` with LoTW's.
fn without(before: &QsoRecord, services: &[UploadService]) -> QsoRecord {
    let mut want = before.clone();
    for s in services {
        match s {
            UploadService::Qrz => want.upload.qrz = None,
            UploadService::Clublog => want.upload.clublog = None,
            UploadService::Eqsl => want.upload.eqsl = None,
            UploadService::Lotw => {
                want.upload.lotw = None;
                want.extra.retain(|(k, _)| k != "LOTW_QSL_SENT");
            }
        }
    }
    want
}

/// ★ PER SERVICE, PART SENT (operator ruling): a LoTW batch took the corrected call, while the
/// re-send to QRZ, Club Log, eQSL and HRDLog.net still waits on the queue, untried. The undo puts
/// back QRZ's, Club Log's and eQSL's stamps, leaves LoTW's off (`LOTW_QSL_SENT` with it), cancels
/// the waiting re-send, and names LoTW alone. A push of the corrected call still in flight then
/// stamps nothing on the restored contact, and neither does the LoTW batch's late stamp.
#[test]
fn a_corrected_calls_undo_puts_back_each_service_that_was_not_sent_it() {
    let d = Dir::new("history-call-fix-part");
    let engine = shared(&d, 3);
    toggles(&engine, true, true, true, true);
    let (id, before) = uploaded_contact(&engine);
    let call = correct_call(&engine, id, &before);
    let fixed = QsoRecord::clone(&row_of(&engine, id).unwrap());
    let signed: Vec<_> = crate::station::LotwSigned::of(&fixed).into_iter().collect();
    assert!(stamp(
        &engine,
        &fixed,
        UploadService::Lotw,
        UploadOutcome::Accepted
    ));
    assert_eq!(
        queued_calls(&engine, id),
        vec![call.clone()],
        "premise: the re-send waits"
    );

    let (undone, dur) = undo_entry(&engine, correction_seq(&engine, id));
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        undone.owed,
        vec![Owed {
            id,
            call: before.call.clone(),
            services: vec!["LoTW"],
        }],
        "LoTW alone is named"
    );
    let want = without(&before, &[UploadService::Lotw]);
    assert_eq!(
        *row_of(&engine, id).unwrap(),
        want,
        "QRZ, Club Log and eQSL put back"
    );
    assert!(
        queued_calls(&engine, id).is_empty(),
        "the waiting re-send is cancelled"
    );

    // The guards: what went out under the corrected call never stamps the restored contact.
    for service in [
        UploadService::Qrz,
        UploadService::Clublog,
        UploadService::Eqsl,
    ] {
        assert!(
            !stamp(&engine, &fixed, service, UploadOutcome::Accepted),
            "{service:?}'s push of {call} stamps nothing"
        );
    }
    let (late, dur) = logwrite::stamp_lotw_batch(
        &engine,
        &signed,
        &UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_900_000_300,
            detail: None,
        },
    );
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(late.stamped, 0, "the LoTW batch's late stamp lands nowhere");
    assert_eq!(
        *row_of(&engine, id).unwrap(),
        want,
        "still exactly as the undo left it"
    );
}

/// ★ PER SERVICE, ONCE THE WORKER TOOK THE RE-SEND: each service it sends to may have the
/// corrected call and stays marked not uploaded, named; one it does not send to (switched off)
/// was never tried and gets its marks back; one that REFUSED the corrected call never took it
/// and gets them back; LoTW, with no batch since, gets its stamp and `LOTW_QSL_SENT` back. A
/// failed leg's retry under the corrected call is taken off the queue.
#[test]
fn a_corrected_calls_undo_after_the_worker_took_the_resend_decides_each_service() {
    // (QRZ, Club Log, eQSL, HRDLog.net switched on)
    for on in [(true, true, true, true), (true, false, false, false)] {
        let d = Dir::new("history-call-fix-taken");
        let engine = shared(&d, 3);
        toggles(&engine, on.0, on.1, on.2, on.3);
        let (id, before) = uploaded_contact(&engine);
        let call = correct_call(&engine, id, &before);
        let taken = engine_lock(&engine).take_pending_uploads();
        assert!(
            taken.iter().any(|p| p.rec.call == call),
            "the worker took it"
        );
        let fixed = QsoRecord::clone(&row_of(&engine, id).unwrap());
        // QRZ took it. With everything on: Club Log failed for now (retried later, no stamp),
        // and eQSL refused it.
        assert!(stamp(
            &engine,
            &fixed,
            UploadService::Qrz,
            UploadOutcome::Duplicate
        ));
        if on.1 {
            engine_lock(&engine).requeue_upload(
                fixed.clone(),
                crate::engine::upload_legs::CLUBLOG,
                1,
            );
            assert!(stamp(
                &engine,
                &fixed,
                UploadService::Eqsl,
                UploadOutcome::Rejected
            ));
        }

        let (undone, dur) = undo_entry(&engine, correction_seq(&engine, id));
        let undone = undone.expect("undone");
        dur.wait(DURABLE_WAIT).unwrap();
        let (services, gone): (Vec<&str>, &[UploadService]) = if on.1 {
            (
                vec!["QRZ", "Club Log", "HRDLog.net"],
                &[UploadService::Qrz, UploadService::Clublog],
            )
        } else {
            (vec!["QRZ"], &[UploadService::Qrz])
        };
        assert_eq!(
            undone.owed,
            vec![Owed {
                id,
                call: before.call.clone(),
                services,
            }],
            "named (switched on: {on:?})"
        );
        assert_eq!(
            *row_of(&engine, id).unwrap(),
            without(&before, gone),
            "the rest put back (switched on: {on:?})"
        );
        assert!(
            queued_calls(&engine, id).is_empty(),
            "nothing left queued under {call} (switched on: {on:?})"
        );
    }
}

/// ★ ALL SENT: every service took the corrected call — the four stamps on it — so the undo puts
/// the call back with no stamp and no `LOTW_QSL_SENT`, everything else exact, and names them all.
#[test]
fn a_corrected_calls_undo_after_every_service_took_it_leaves_it_owed_and_says_so() {
    let d = Dir::new("history-call-fix-sent");
    let engine = shared(&d, 3);
    let (id, before) = uploaded_contact(&engine);
    correct_call(&engine, id, &before);
    engine_lock(&engine).take_pending_uploads();
    let fixed = QsoRecord::clone(&row_of(&engine, id).unwrap());
    for service in [
        UploadService::Qrz,
        UploadService::Clublog,
        UploadService::Eqsl,
        UploadService::Lotw,
    ] {
        assert!(stamp(&engine, &fixed, service, UploadOutcome::Accepted));
    }
    let (undone, dur) = undo_entry(&engine, correction_seq(&engine, id));
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        undone.owed,
        vec![Owed {
            id,
            call: before.call.clone(),
            services: vec!["LoTW", "QRZ", "Club Log", "eQSL"],
        }]
    );
    let now = row_of(&engine, id).unwrap();
    assert_eq!(now.upload, Default::default(), "no stamp it cannot show");
    assert_eq!(
        *now,
        without(
            &before,
            &[
                UploadService::Qrz,
                UploadService::Clublog,
                UploadService::Eqsl,
                UploadService::Lotw
            ]
        ),
        "the rest of the row comes back exactly"
    );
}

/// ★ A LoTW batch RUNNING when the undo is made may be sending the corrected call: LoTW's marks
/// stay off and LoTW is named, while the services the untried re-send waits for get theirs back.
#[test]
fn a_corrected_calls_undo_while_a_lotw_batch_runs_leaves_lotw_owed() {
    let d = Dir::new("history-call-fix-tqsl");
    let engine = shared(&d, 3);
    let (id, before) = uploaded_contact(&engine);
    correct_call(&engine, id, &before);
    engine_lock(&engine).lotw_batch_begun();
    let (undone, dur) = undo_entry(&engine, correction_seq(&engine, id));
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    engine_lock(&engine).lotw_batch_over();
    assert_eq!(
        undone.owed,
        vec![Owed {
            id,
            call: before.call.clone(),
            services: vec!["LoTW"],
        }]
    );
    assert_eq!(
        *row_of(&engine, id).unwrap(),
        without(&before, &[UploadService::Lotw])
    );
}

/// ★ AFTER A RESTART nothing is known of what was sent under the corrected call (the queue and
/// the record of the correction lived in memory): the undo puts the call back marked not uploaded
/// to every service and names the contact with no service, for the answer's general wording.
#[test]
fn a_corrected_calls_undo_after_a_restart_leaves_it_owed_to_every_service() {
    let d = Dir::new("history-call-fix-restart");
    let engine = shared(&d, 3);
    let (id, before) = uploaded_contact(&engine);
    correct_call(&engine, id, &before);
    drop(engine);
    let engine = Arc::new(Mutex::new(engine_on_store(&d)));
    let (undone, dur) = undo_entry(&engine, correction_seq(&engine, id));
    let undone = undone.expect("undone");
    dur.wait(DURABLE_WAIT).unwrap();
    assert_eq!(
        undone.owed,
        vec![Owed {
            id,
            call: before.call.clone(),
            services: Vec::new(),
        }],
        "named, with no service"
    );
    let now = row_of(&engine, id).unwrap();
    assert_eq!(now.call, before.call, "the call is back");
    assert_eq!(
        *now,
        without(
            &before,
            &[
                UploadService::Qrz,
                UploadService::Clublog,
                UploadService::Eqsl,
                UploadService::Lotw
            ]
        ),
        "no stamp, the rest exact"
    );
}
