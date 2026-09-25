//! SPEC-2 v3 C19 Part B's proofs for the write path as a command makes it: planned with the
//! Engine lock released, made under it — and the FT auto-log, which reads nothing at all.

use super::*;
use crate::dto::Tier;
use crate::engine::{engine_lock, LogWriteOutcome};
use crate::logstore::tests::{
    engine_on_store, eventually, flush, id_at, legacy_log, qso, same_log, stored, Dir,
};
use crate::logstore::DURABLE_WAIT;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::{Duration, Instant};
use tempo_core::logbook::sqlite::WriteHold;
use tempo_core::logbook::{adif_header, adif_record, LogOp, QsoRecord, UploadOutcome};

thread_local! {
    // A writer racing the planned changes made on this thread: run by `change_row` after its
    // plan's read, with no lock held, before the change is made. (A plain comment: a doc comment
    // cannot attach through `thread_local!`.)
    static RACE: RefCell<Option<Box<dyn FnMut()>>> = const { RefCell::new(None) };
}

/// `change_row`'s seam: the racing writer, if a test set one. Taken out while it runs, so a
/// racing writer that plans a change of its own does not race itself.
pub(super) fn race() {
    let hook = RACE.with(|r| r.borrow_mut().take());
    if let Some(mut hook) = hook {
        hook();
        RACE.with(|r| *r.borrow_mut() = Some(hook));
    }
}

/// Run `f` with `hook` racing every change planned on this thread.
fn racing<T>(hook: impl FnMut() + 'static, f: impl FnOnce() -> T) -> T {
    RACE.with(|r| *r.borrow_mut() = Some(Box::new(hook)));
    let out = f();
    RACE.with(|r| *r.borrow_mut() = None);
    out
}

/// A connector's stamp, as another writer on the log makes it.
fn qrz_stamp(id: RecordId, when_unix: i64) -> LogOp {
    LogOp::Stamp {
        id,
        service: UploadService::Qrz,
        status: UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix,
            detail: None,
        },
    }
}

/// The paper card, as the operator marks it — the change the tests plan.
fn card(id: RecordId) -> LogOp {
    LogOp::MarkQslCard { id, received: true }
}

/// The row `id` as the store holds it.
fn stored_row(d: &Dir, id: RecordId) -> QsoRecord {
    stored(d)
        .into_iter()
        .find(|r| r.id == Some(id))
        .expect("the row is in the store")
}

/// A shared engine on the store of `d`, holding `n` contacts.
fn shared(d: &Dir, n: usize) -> Arc<Mutex<Engine>> {
    std::fs::write(d.log(), legacy_log(n)).unwrap();
    let e = engine_on_store(d);
    flush(&e);
    Arc::new(Mutex::new(e))
}

/// ★ THE PRECONDITION, AGAINST ANOTHER WRITER IN THIS PROCESS (SPEC-2 v3 §4.6). The row changes
/// between the plan's read and the commit — a connector's stamp lands in exactly that window —
/// so the commit refuses the plan and plans again, on the row as it now stands: BOTH changes
/// stand. A commit that made the first plan anyway would have written the card over a row
/// without the stamp, and the stamp would be gone (a lost update).
#[test]
fn a_row_another_writer_changes_under_the_plan_is_planned_again_and_both_changes_stand() {
    let d = Dir::new("race-once");
    let engine = shared(&d, 6);
    let id = id_at(&engine_lock(&engine), 3);
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let (engine, plans) = (Arc::clone(&engine), Rc::clone(&plans));
        move || {
            plans.set(plans.get() + 1);
            if plans.get() == 1 {
                let (made, _) = change_ops(&engine, id, None, &[qrz_stamp(id, 7)], "theirs");
                assert!(matches!(made, Ok(Ok(_))), "their stamp is made");
            }
        }
    };
    let (made, durability) = racing(hook, || change_ops(&engine, id, None, &[card(id)], "ours"));
    assert!(matches!(made, Ok(Ok(_))), "ours is made: {made:?}");
    assert_eq!(
        plans.get(),
        2,
        "planned twice: the first plan read a row that then changed"
    );
    durability.wait(DURABLE_WAIT).expect("on disk");
    let row = stored_row(&d, id);
    assert!(row.qsl_rcvd.card, "our card");
    assert_eq!(
        row.upload.qrz.map(|s| s.when_unix),
        Some(7),
        "and their stamp, which a commit of the first plan would have written over"
    );
    same_log(
        &stored(&d),
        engine_lock(&engine).log_records(),
        "the store is the log in memory",
    );
}

/// ★ ON THE 1.13 PATH, A `log.adi` TAKEN IN UNDER A PLAN IS PLANNED AGAIN, NOT WRITTEN OVER
/// (SPEC-2 v3 C19 Part C with Part B). Another computer on the shared `log.adi` confirms a contact
/// through LoTW — 1.13's own report merge and save, as a second Nexus on the folder runs them —
/// and the freshness poll takes that file in, between our plan's read of the contact and the
/// commit of our change to it. The commit plans again on the contact as it now stands, so the
/// confirmation and our paper card both stand: in the log, and in `log.adi`, this path's home. A
/// commit of the first plan would write the contact back without the confirmation. The store is
/// the 1.13 path's, in memory: no other process ever commits to it, so a check on another
/// window's commits cannot see this.
#[test]
fn a_log_adi_taken_in_under_a_plan_on_the_1_13_path_is_planned_again() {
    let d = Dir::new("take-in-under-plan");
    std::fs::write(d.log(), legacy_log(6)).unwrap();
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_log_path(d.log());
    assert!(e.log_on_file(), "premise: the 1.13 path");
    let engine = Arc::new(Mutex::new(e));
    let id = id_at(&engine_lock(&engine), 3);
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let (engine, plans, path) = (Arc::clone(&engine), Rc::clone(&plans), d.log());
        move || {
            plans.set(plans.get() + 1);
            if plans.get() != 1 {
                return;
            }
            let mut other = tempo_core::logbook::Logbook::load(&path);
            let row = other
                .records()
                .iter()
                .find(|r| r.id == Some(id))
                .map(|r| QsoRecord::clone(r))
                .expect("the other computer holds the contact");
            let report =
                tempo_core::logbook::adif_record(&row).replace("<EOR>", "<LOTW_QSL_RCVD:1>Y<EOR>");
            let _ = other.merge_report(&report);
            other.save(&path).unwrap();
            assert!(
                engine_lock(&engine).sync_shared_log_if_changed(),
                "premise: the freshness poll takes the file in"
            );
        }
    };
    let (made, durability) = racing(hook, || change_ops(&engine, id, None, &[card(id)], "ours"));
    assert!(matches!(made, Ok(Ok(_))), "ours is made: {made:?}");
    assert_eq!(
        plans.get(),
        2,
        "planned again: the first plan read the contact before the take-in changed it"
    );
    durability.wait(DURABLE_WAIT).expect("in log.adi");
    let held = engine_lock(&engine)
        .log_records()
        .iter()
        .find(|r| r.id == Some(id))
        .map(|r| QsoRecord::clone(r))
        .expect("held");
    assert!(
        held.qsl_rcvd.card && held.qsl_rcvd.lotw,
        "both stand in the log: {:?}",
        held.qsl_rcvd
    );
    let on_disk = tempo_core::logbook::Logbook::load(&d.log());
    let row = on_disk
        .records()
        .iter()
        .find(|r| r.id == Some(id))
        .expect("in log.adi");
    assert!(
        row.qsl_rcvd.card && row.qsl_rcvd.lotw,
        "and in log.adi: {:?}",
        row.qsl_rcvd
    );
}

/// ★ `LogBusy` (SPEC-1 v2 R5): a row that changes under EVERY plan is refused after
/// [`PLANS`] of them, and nothing of ours is written — the racing writer's changes all stand.
#[test]
fn a_row_that_keeps_changing_answers_log_busy_and_changes_nothing() {
    let d = Dir::new("race-always");
    let engine = shared(&d, 6);
    let id = id_at(&engine_lock(&engine), 2);
    let plans = Rc::new(Cell::new(0i64));
    let hook = {
        let (engine, plans) = (Arc::clone(&engine), Rc::clone(&plans));
        move || {
            plans.set(plans.get() + 1);
            let (made, _) = change_ops(&engine, id, None, &[qrz_stamp(id, plans.get())], "theirs");
            assert!(matches!(made, Ok(Ok(_))), "their stamp is made");
        }
    };
    let (made, durability) = racing(hook, || change_ops(&engine, id, None, &[card(id)], "ours"));
    assert!(
        matches!(made, Ok(Err(RowRefusal::Busy))),
        "refused as LogBusy: {made:?}"
    );
    assert_eq!(
        plans.get(),
        PLANS as i64,
        "planned {PLANS} times, and then refused"
    );
    assert!(durability.is_empty(), "nothing of ours to wait for");
    flush(&engine_lock(&engine));
    let row = stored_row(&d, id);
    assert!(!row.qsl_rcvd.card, "our card was never made");
    assert_eq!(
        row.upload.qrz.map(|s| s.when_unix),
        Some(PLANS as i64),
        "every one of their stamps was"
    );
    same_log(
        &stored(&d),
        engine_lock(&engine).log_records(),
        "the store is the log in memory",
    );
}

/// Another window — a second engine on the same data folder — changes the row between the plan
/// and the commit. Its commit reaches this window's store at once and this window's memory only
/// when it is taken in; the commit takes it in, sees the row it planned on is not the row now,
/// and plans again. Both changes stand.
#[test]
fn another_windows_change_under_the_plan_is_planned_again() {
    let d = Dir::new("race-window");
    let a = shared(&d, 6);
    let b = Arc::new(Mutex::new(engine_on_store(&d)));
    let id = id_at(&engine_lock(&a), 4);
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let (a, b, plans) = (Arc::clone(&a), Arc::clone(&b), Rc::clone(&plans));
        move || {
            plans.set(plans.get() + 1);
            if plans.get() == 1 {
                let (made, _) = change_ops(&b, id, None, &[qrz_stamp(id, 9)], "window B");
                assert!(matches!(made, Ok(Ok(_))), "B's stamp is made");
                flush(&engine_lock(&b));
                assert!(
                    eventually(|| engine_lock(&a).log_store_foreign_pending()),
                    "A's writer sees B's commit"
                );
            }
        }
    };
    let (made, durability) = racing(hook, || change_ops(&a, id, None, &[card(id)], "window A"));
    assert!(matches!(made, Ok(Ok(_))), "{made:?}");
    assert_eq!(plans.get(), 2, "planned again after B's change to the row");
    durability.wait(DURABLE_WAIT).expect("on disk");
    let row = stored_row(&d, id);
    assert!(row.qsl_rcvd.card, "A's card");
    assert_eq!(
        row.upload.qrz.map(|s| s.when_unix),
        Some(9),
        "and B's stamp"
    );
}

/// The control for the test above: another window's commit to a DIFFERENT row is no reason to
/// plan again. It is taken in, the row this change planned on is still the row it read, and the
/// change is made on the first plan.
#[test]
fn another_windows_change_to_another_row_is_no_reason_to_plan_again() {
    let d = Dir::new("race-window-other");
    let a = shared(&d, 6);
    let b = Arc::new(Mutex::new(engine_on_store(&d)));
    let (id, other) = {
        let e = engine_lock(&a);
        (id_at(&e, 4), id_at(&e, 1))
    };
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let (a, b, plans) = (Arc::clone(&a), Arc::clone(&b), Rc::clone(&plans));
        move || {
            plans.set(plans.get() + 1);
            if plans.get() == 1 {
                let (made, _) = change_ops(&b, other, None, &[qrz_stamp(other, 9)], "window B");
                assert!(matches!(made, Ok(Ok(_))), "B's stamp is made");
                flush(&engine_lock(&b));
                assert!(
                    eventually(|| engine_lock(&a).log_store_foreign_pending()),
                    "A's writer sees B's commit"
                );
            }
        }
    };
    let (made, durability) = racing(hook, || change_ops(&a, id, None, &[card(id)], "window A"));
    assert!(matches!(made, Ok(Ok(_))), "{made:?}");
    assert_eq!(plans.get(), 1, "B changed another row: planned once");
    durability.wait(DURABLE_WAIT).expect("on disk");
    assert!(stored_row(&d, id).qsl_rcvd.card, "A's card");
    assert_eq!(
        stored_row(&d, other).upload.qrz.map(|s| s.when_unix),
        Some(9),
        "B's stamp on its row"
    );
    assert_eq!(
        engine_lock(&a)
            .log_records()
            .iter()
            .find(|r| r.id == Some(other))
            .and_then(|r| r.upload.qrz.as_ref())
            .map(|s| s.when_unix),
        Some(9),
        "and A took it in"
    );
}

/// ★ A PLAN READS THIS PROCESS'S OWN CHANGES THE STORE HAS NOT TAKEN YET (SPEC-2 v3 C19). With
/// the writer stalled, a change to a row is on its way — not in the store. A second change to the
/// same row plans on the row as this process knows it (the first change laid over the store's
/// row), so it carries the first change, and when the writer clears, both stand. A plan on the
/// store alone would have read the row without the first change and written it back that way,
/// after it: a lost update the writer itself would make.
#[test]
fn a_plan_reads_this_processs_changes_the_store_has_not_taken_yet() {
    let d = Dir::new("overlay");
    let engine = shared(&d, 6);
    let id = id_at(&engine_lock(&engine), 3);
    let hold = WriteHold::take(&d.db()).expect("stall the writer");
    let (first, first_durable) = change_ops(&engine, id, None, &[card(id)], "first");
    assert!(matches!(first, Ok(Ok(_))), "{first:?}");
    assert!(
        !stored_row(&d, id).qsl_rcvd.card,
        "control: the store has not taken the first change"
    );
    let view = engine_lock(&engine).log_view();
    assert!(
        view.row(id).expect("read").expect("held").qsl_rcvd.card,
        "a plan reads it all the same"
    );
    let (second, second_durable) = change_ops(&engine, id, None, &[qrz_stamp(id, 5)], "second");
    assert!(matches!(second, Ok(Ok(_))), "{second:?}");
    drop(hold);
    first_durable.wait(DURABLE_WAIT).expect("the first lands");
    second_durable.wait(DURABLE_WAIT).expect("the second lands");
    let row = stored_row(&d, id);
    assert!(row.qsl_rcvd.card, "the first change stands");
    assert_eq!(
        row.upload.qrz.map(|s| s.when_unix),
        Some(5),
        "and the second"
    );
    same_log(
        &stored(&d),
        engine_lock(&engine).log_records(),
        "the store is the log in memory",
    );
}

/// ★ A CORRECTION BY A `RowRef` IS MADE ONLY ON THE VERSION FOUND (SPEC-2 v3 C19). The desktop's
/// edit of the row it found by what the view showed, and a Remote browser's edit, are made by
/// [`update_row`], which checks the edit key where the change is made — the check C16's `locate`
/// made under the lock. A stamp since the row was found moves no edit key, so it is no reason to
/// refuse; an edit since is, and nothing is written. The control: the key the row now carries.
#[test]
fn a_correction_by_an_edit_key_is_made_only_on_the_version_found() {
    let d = Dir::new("update-row-key");
    let engine = shared(&d, 6);
    let id = id_at(&engine_lock(&engine), 3);
    let key = || {
        let view = engine_lock(&engine).log_view();
        QsoEdit::project(&view.row(id).expect("read").expect("held")).key()
    };
    let correct = |key: &str, comment: &str| {
        update_row(
            &engine,
            id,
            key,
            |row| QsoRecord {
                comment: Some(comment.into()),
                ..row.clone()
            },
            |_, (_, after)| after.clone(),
        )
    };

    let found = key();
    let (stamped, _) = change_ops(&engine, id, None, &[qrz_stamp(id, 7)], "a connector");
    assert!(matches!(stamped, Ok(Ok(_))), "{stamped:?}");
    let (made, durability) = correct(&found, "first");
    assert!(
        matches!(made, Ok(Ok(Some(Some(_))))),
        "a stamp since is no reason to refuse: {made:?}"
    );
    durability.wait(DURABLE_WAIT).expect("on disk");
    assert_eq!(stored_row(&d, id).comment.as_deref(), Some("first"));

    // `found` is now the version before "first".
    let before = stored(&d);
    let (made, durability) = correct(&found, "second");
    assert!(
        matches!(made, Ok(Err(RowRefusal::Changed(_)))),
        "an edit since is: {made:?}"
    );
    assert!(durability.is_empty(), "nothing to wait for");
    flush(&engine_lock(&engine));
    assert_eq!(stored(&d), before, "and nothing is written");

    let (made, durability) = correct(&key(), "second");
    assert!(matches!(made, Ok(Ok(Some(Some(_))))), "control: {made:?}");
    durability.wait(DURABLE_WAIT).expect("on disk");
    assert_eq!(stored_row(&d, id).comment.as_deref(), Some("second"));
    same_log(
        &stored(&d),
        engine_lock(&engine).log_records(),
        "the store is the log in memory",
    );
}

/// One decode, as the air hands it to the sequencer.
fn heard(message: &str) -> modes::Decode {
    modes::Decode {
        message: message.to_string(),
        sync: 1.0,
        snr: -7,
        dt: 0.1,
        freq: 1500.0,
        nap: 0,
        qual: 1.0,
        rv: None,
        raw: None,
        mode: None,
    }
}

/// ⛔ THE FT GATE'S PROOF (SPEC-2 v3 §4.6, C19 Part B): the FT auto-log reads NOTHING from the
/// store. Under the Engine lock — a guard held, so the `io_fence` is armed on this thread and a
/// read of the store would stop a debug build (the control below) — with the store's writer
/// stalled (its write lock held elsewhere), a QSO the sequencer completes on the air is logged,
/// and one logged through the funnel directly, and a duplicate of it refused from the hot index:
/// all at once, the radio loop's lock never waiting on the disk. The contacts reach the store
/// when the write clears.
#[test]
fn the_ft_auto_log_reads_nothing_from_the_store_under_the_engine_lock() {
    let d = Dir::new("ft-no-sql");
    let engine = shared(&d, 20);
    let hold = WriteHold::take(&d.db()).expect("stall the writer");
    let started = Instant::now();
    {
        let mut e = engine_lock(&engine);
        #[cfg(debug_assertions)]
        assert!(
            tempo_core::logbook::io_fence::engine_guards_held() > 0,
            "premise: the fence is armed on this thread"
        );
        // The sequencer's own auto-log: a QSO completed on the air.
        e.set_tier(Tier::TempoFast);
        e.call_station("W9XYZ");
        e.ingest_decodes_for_test(&[heard("K2DEF W9XYZ -10")], 1);
        e.ingest_decodes_for_test(&[heard("K2DEF W9XYZ RR73")], 3);
        // The funnel every log path passes through, and its duplicate guard.
        let first = e.log_qso_for_sync(qso("W1FT", 1_788_000_000));
        assert!(!matches!(first, LogWriteOutcome::Duplicate));
        assert!(
            matches!(
                e.log_qso_for_sync(qso("W1FT", 1_788_000_000)),
                LogWriteOutcome::Duplicate
            ),
            "a duplicate is refused from the hot index, though the store has not seen the first"
        );
    }
    let under_lock = started.elapsed();
    assert!(
        under_lock < Duration::from_millis(500),
        "the auto-log under the Engine lock took {under_lock:?} with the writer stalled"
    );
    assert!(
        stored(&d)
            .iter()
            .all(|r| r.call != "W9XYZ" && r.call != "W1FT"),
        "control: the writer really was stalled — the store holds neither contact yet"
    );
    drop(hold);
    let e = engine_lock(&engine);
    flush(&e);
    let rows = stored(&d);
    for call in ["W9XYZ", "W1FT"] {
        assert_eq!(
            rows.iter().filter(|r| r.call == call).count(),
            1,
            "{call} reaches the store once the write clears"
        );
    }
    same_log(&rows, e.log_records(), "the store is the log in memory");
}

/// ★ A BULK CHANGE'S PRECONDITION (SPEC-2 v3 §4.6, C19 Part B). An import is planned on the rows of
/// its calls; a contact of one of those calls logged before the import is made is a row the plan
/// never read, so the import plans again — and finds the contact there. Made on the first plan,
/// it would have added the same contact a second time.
#[test]
fn an_import_planned_before_a_contact_of_its_call_is_logged_plans_again() {
    let d = Dir::new("bulk-race");
    let engine = shared(&d, 6);
    let contact = qso("W9RACE", 1_788_000_500);
    let text = adif_header() + &adif_record(&contact);
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let (engine, plans, contact) = (Arc::clone(&engine), Rc::clone(&plans), contact.clone());
        move || {
            plans.set(plans.get() + 1);
            if plans.get() == 1 {
                // The FT auto-log, as the radio loop makes it: no plan, no read.
                engine_lock(&engine).log_qso(contact.clone());
            }
        }
    };
    let (made, durability) = racing(hook, || import_adif(&engine, &text));
    let (added, skipped, _, _) = made.expect("the import is made");
    assert_eq!(
        plans.get(),
        2,
        "planned again: a row of its call was logged under the plan"
    );
    assert_eq!(
        (added, skipped),
        (0, 1),
        "the import finds the contact already logged, and adds it no second time"
    );
    durability.wait(DURABLE_WAIT).expect("on disk");
    flush(&engine_lock(&engine));
    assert_eq!(
        stored(&d).iter().filter(|r| r.call == "W9RACE").count(),
        1,
        "one contact"
    );
    same_log(
        &stored(&d),
        engine_lock(&engine).log_records(),
        "the store is the log in memory",
    );
}

/// ★ THE FIELD DAY MERGE'S PRECONDITION: its "already there" is every merge identity the log holds,
/// read off the store with the lock released; a change to the log before the merge is made — here
/// another merge of the same session, in another command — plans it again, and the second merge
/// finds every contact already there. Made on its first plan, it would have merged the session
/// twice.
#[test]
fn a_field_day_merge_planned_before_another_merge_plans_again() {
    let d = Dir::new("fd-race");
    let engine = shared(&d, 3);
    {
        let mut e = engine_lock(&engine);
        let mut s = e.settings().clone();
        s.fd_active = true;
        s.fd_class = "3A".into();
        s.fd_section = "WI".into();
        s.fd_position_id = "a1b2c3d4".into();
        e.apply_settings(s);
        e.set_mode("fieldday-run").unwrap();
        assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
        assert!(e.fd_log_manual("W1AW", "1D", "CT", "PH").unwrap());
    }
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let (engine, plans) = (Arc::clone(&engine), Rc::clone(&plans));
        move || {
            plans.set(plans.get() + 1);
            if plans.get() == 1 {
                let (other, _) = fd_merge_to_general(&engine);
                assert_eq!(other.expect("in Field Day").added(), 2, "the other merge");
            }
        }
    };
    let (made, _) = racing(hook, || fd_merge_to_general(&engine));
    let report = made.expect("in Field Day");
    assert_eq!(plans.get(), 2, "planned again after the other merge");
    assert_eq!(
        (report.added(), report.already),
        (0, 2),
        "every contact already there"
    );
    flush(&engine_lock(&engine));
    let merged = |r: &QsoRecord| r.contest.as_deref().is_some_and(|c| !c.qid.is_empty());
    assert_eq!(
        stored(&d).iter().filter(|r| merged(r)).count(),
        2,
        "the session merged once"
    );
}

/// ★ THE FT AUTO-LOG LOGS WHAT IT LOGGED BEFORE (the FT gate's parity). Seeded runs of the funnel
/// every logged contact passes through — the sequencer's `log_qso` and the synced
/// `log_qso_for_sync` — with repeats inside and outside the duplicate window, each held to a model
/// kept apart from the station: a `Logbook` of what was logged, asked by the scan the hot index
/// replaced. A contact is refused exactly when the scan calls it a duplicate; one that is logged
/// is the contact as handed in with the next id the station mints; and the store ends as the
/// model, byte for byte.
#[test]
fn the_ft_auto_log_logs_what_the_scan_and_the_contact_say() {
    const CALLS: [&str; 3] = ["W1AW", "JA1AA", "DL1AB"];
    const BANDS: [(&str, f64); 2] = [("20m", 14.074), ("40m", 7.074)];
    const MODES: [&str; 2] = ["FT8", "FT4"];
    let (mut logged, mut refused) = (0, 0);
    for seed in 0..8u64 {
        let d = Dir::new(&format!("ft-parity-{seed}"));
        let engine = shared(&d, 0);
        let mut model = tempo_core::logbook::Logbook::new();
        let mut g = Gen(seed);
        let mut when = 1_788_000_000u64;
        let mut last: Option<QsoRecord> = None;
        let mut minted = 0u32;
        for step in 0..40 {
            let rec = match &last {
                // The same contact handed in again moments later: the sequencer's own repeat.
                Some(prev) if g.below(4) == 0 => {
                    let mut again = prev.clone();
                    again.id = None;
                    again.when_unix += 15;
                    again
                }
                _ => {
                    when += [30, 90, 200, 400][g.below(4)];
                    let mut r = qso(CALLS[g.below(CALLS.len())], when);
                    let (band, freq) = BANDS[g.below(BANDS.len())];
                    r.band = band.into();
                    r.freq_mhz = freq;
                    r.mode = MODES[g.below(MODES.len())].into();
                    // Every field the funnel would fill is given, so what it logs is this.
                    r.station_callsign = Some("K2DEF".into());
                    r.my_rig = Some("TEST RIG".into());
                    r
                }
            };
            let at = format!(
                "seed {seed} step {step}: {} {} {}",
                rec.call, rec.band, rec.mode
            );
            let duplicate = tempo_core::logbook::dedup::scan_for_duplicate(&model, &rec);
            let mut e = engine_lock(&engine);
            let outcome = if step % 2 == 0 {
                e.log_qso(rec.clone());
                None
            } else {
                Some(e.log_qso_for_sync(rec.clone()))
            };
            let (held, row) = (e.log_records().len(), e.log_records().last().cloned());
            drop(e);
            last = Some(rec.clone());
            if duplicate {
                refused += 1;
                assert!(
                    matches!(outcome, None | Some(LogWriteOutcome::Duplicate)),
                    "{at}: refused"
                );
                assert_eq!(held, model.len(), "{at}: nothing logged");
                continue;
            }
            logged += 1;
            assert_eq!(held, model.len() + 1, "{at}: one contact logged");
            if let Some(outcome) = outcome {
                assert!(
                    matches!(&outcome, LogWriteOutcome::PendingSync(r) if r.len() == 1),
                    "{at}: logged, with its receipt"
                );
            }
            let row = row.expect("logged");
            let Some(id @ RecordId::Minted { seq, .. }) = row.id else {
                panic!("{at}: the station mints the id: {:?}", row.id);
            };
            assert!(seq > minted, "{at}: the next id the station hands out");
            minted = seq;
            let mut expected = rec;
            expected.id = Some(id);
            assert_eq!(
                *row, expected,
                "{at}: the contact as handed in, with its id"
            );
            model.add(expected);
        }
        let e = engine_lock(&engine);
        assert_eq!(
            e.log_records().len(),
            model.len(),
            "seed {seed}: nothing else was logged"
        );
        flush(&e);
        same_log(
            &stored(&d),
            model.records(),
            &format!("seed {seed}: the store"),
        );
    }
    // Both halves of the guard ran, many times each.
    assert!(
        logged >= 100 && refused >= 40,
        "logged {logged}, refused {refused}"
    );
}

/// ★ POSITIVE CONTROL for the proof above: a read of the store under the Engine lock is exactly
/// what the fence stops, so the auto-log's silence there is not a fence that cannot see. A plan's
/// read of one row, under the lock, trips it.
#[cfg(debug_assertions)]
#[test]
#[should_panic(expected = "io_fence")]
fn a_plan_read_under_the_engine_lock_trips_the_fence() {
    let d = Dir::new("ft-fence-control");
    let engine = shared(&d, 3);
    let e = engine_lock(&engine);
    let id = id_at(&e, 1);
    let _ = e.logged_row(id);
}

/// The contact at `at` in the log in memory, as a report restating it is written from.
fn row_at(engine: &Arc<Mutex<Engine>>, at: usize) -> QsoRecord {
    QsoRecord::clone(&engine_lock(engine).log_records()[at])
}

/// A generator whose failing case replays from its seed.
struct Gen(u64);
impl Gen {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n.max(1) as u64) as usize
    }
}

/// ★ P6 THROUGH EVERY WRITE PATH (SPEC-2 v3 C19 Part B): seeded random runs of every kind of
/// change the app makes — the commands' planned changes (the marks, the tag, a delete, the
/// form's edit, a correction of a row found by what was shown, the connector stamps by id and by
/// push key, the LoTW batch and the "already uploaded" declaration, the fill job), the FT
/// append, the purge, and the bulk merges — with the store compared to the log in memory after
/// EACH. The log in memory follows every change the station commits; nothing plans on it.
#[test]
fn the_store_is_the_log_in_memory_after_every_write_path() {
    const CALLS: [&str; 5] = ["K1ABC", "W9XYZ", "DL1AB", "K2DEF/P", "JA1AA"];
    let mut kinds = std::collections::BTreeMap::<&str, usize>::new();
    for seed in 0..6u64 {
        let d = Dir::new(&format!("p6-{seed}"));
        let engine = shared(&d, 10);
        let mut g = Gen(seed);
        for step in 0..45u64 {
            let len = engine_lock(&engine).log_records().len();
            let at = g.below(len);
            let pick = |e: &Engine| (len > 0).then(|| id_at(e, at));
            let id = pick(&engine_lock(&engine));
            let call = CALLS[g.below(CALLS.len())];
            let when = 1_788_000_000 + step * 97;
            let kind = match (g.below(18), id) {
                (0 | 1, _) | (_, None) => {
                    engine_lock(&engine).log_qso(qso(call, when));
                    "append"
                }
                (2, Some(id)) => {
                    let _ = change_ops(&engine, id, None, &[card(id)], "card");
                    "card"
                }
                (3, Some(id)) => {
                    let via = (g.below(2) == 0).then_some(tempo_core::logbook::QslVia::Bureau);
                    let _ = change_ops(&engine, id, None, &[qsl_sent(id, via)], "sent");
                    "qsl sent"
                }
                (4, Some(id)) => {
                    let tag = LogOp::SetSatTag {
                        id,
                        sat_name: (g.below(2) == 0).then(|| "RS-44".into()),
                    };
                    let _ = change_ops(&engine, id, None, &[tag], "sat");
                    "sat tag"
                }
                (5, Some(id)) => {
                    let _ = change_ops(&engine, id, None, &[LogOp::Delete(id)], "delete");
                    "delete"
                }
                (6, Some(id)) => {
                    let view = engine_lock(&engine).log_view();
                    let row = view.row(id).expect("read").expect("held");
                    let key = QsoEdit::project(&row).key();
                    let mut edit = QsoEdit::project(&row);
                    if g.below(2) == 0 {
                        edit.call = call.into();
                    } else {
                        edit.comment = Some(format!("edit {step}"));
                        edit.qsl_card = !edit.qsl_card;
                    }
                    let (made, _) = edit_row(&engine, id, &key, &edit);
                    assert!(matches!(made, Ok(Ok(_))), "{made:?}");
                    "edit"
                }
                (7, Some(id)) => {
                    let view = engine_lock(&engine).log_view();
                    let mut pushed = QsoRecord::clone(&view.row(id).expect("read").expect("held"));
                    if g.below(2) == 0 {
                        // A push from a page older than the ids: the newest with its key.
                        pushed.id = None;
                    }
                    let service = [
                        UploadService::Qrz,
                        UploadService::Clublog,
                        UploadService::Eqsl,
                    ][g.below(3)];
                    let status = UploadStatus {
                        outcome: UploadOutcome::Accepted,
                        when_unix: when as i64,
                        detail: None,
                    };
                    let _ = stamp_push(&engine, &pushed, service, status);
                    "connector stamp"
                }
                (8, Some(_)) => {
                    let view = engine_lock(&engine).log_view();
                    let ids: Vec<RecordId> = {
                        let e = engine_lock(&engine);
                        e.log_records()
                            .iter()
                            .filter_map(|r| r.id)
                            .take(4)
                            .collect()
                    };
                    let rows = view.rows(&ids).expect("read");
                    let signed: Vec<LotwSigned> = ids
                        .iter()
                        .filter_map(|id| rows.get(id))
                        .filter_map(|r| LotwSigned::of(r))
                        .collect();
                    let status = UploadStatus {
                        outcome: UploadOutcome::Pending,
                        when_unix: when as i64,
                        detail: None,
                    };
                    let _ = stamp_lotw_batch(&engine, &signed, &status);
                    "lotw batch"
                }
                (9, Some(_)) => {
                    let _ = mark_lotw_uploaded_all(&engine, when as i64);
                    "already uploaded"
                }
                (10, Some(id)) => {
                    let fills = [LogFill {
                        id,
                        country: Some("Found".into()),
                        state: Some("WI".into()),
                    }];
                    fill(&engine, &fills, step as i64).expect("the fill job writes");
                    "fill"
                }
                (11, Some(_)) => {
                    // A LoTW report's confirmation of a contact the log holds (the bulk lane).
                    let text = adif_record(&row_at(&engine, at))
                        .replace("<EOR>", "<LOTW_QSL_RCVD:1>Y<EOR>");
                    let (made, _) = merge_lotw_report(&engine, &text);
                    made.expect("the report merges");
                    "report merge"
                }
                (12, Some(_)) => {
                    // An import: a contact the log lacks, and one it holds restated with a card.
                    let new = qso(CALLS[g.below(CALLS.len())], when + 7);
                    let text = adif_header()
                        + &adif_record(&new)
                        + &adif_record(&row_at(&engine, at)).replace("<EOR>", "<QSL_RCVD:1>Y<EOR>");
                    let (made, _) = import_adif(&engine, &text);
                    made.expect("the import is made");
                    "import"
                }
                (13, Some(_)) => {
                    // QRZ's download: a contact the log lacks, and one it holds confirmed there.
                    let new = qso(CALLS[g.below(CALLS.len())], when + 11);
                    let text = adif_record(&new)
                        + &adif_record(&row_at(&engine, at))
                            .replace("<EOR>", "<APP_QRZLOG_STATUS:1>C<EOR>");
                    let (made, _) = merge_qrz_report(&engine, &text);
                    made.expect("the download merges");
                    "qrz download"
                }
                (14, Some(_)) => {
                    // A pota.app export naming the park of a contact the log holds.
                    let text = adif_record(&row_at(&engine, at))
                        .replace("<EOR>", "<SIG:4>POTA<SIG_INFO:6>K-0001<EOR>");
                    let (made, _) = import_pota_log(&engine, &text);
                    made.expect("the stamps are made");
                    "pota stamps"
                }
                (15, Some(_)) => {
                    // LoTW's own-QSO report of a contact the log holds.
                    let text = adif_record(&row_at(&engine, at));
                    let (made, _) = merge_lotw_own_echo(&engine, &text, step as i64 + 1);
                    made.expect("the report merges");
                    "own echo"
                }
                (16, Some(id)) => {
                    // A correction of the row found by what the view or a browser showed.
                    let view = engine_lock(&engine).log_view();
                    let row = view.row(id).expect("read").expect("held");
                    let key = QsoEdit::project(&row).key();
                    let (made, _) = update_row(
                        &engine,
                        id,
                        &key,
                        |row| QsoRecord {
                            call: call.into(),
                            comment: Some(format!("corrected {step}")),
                            ..row.clone()
                        },
                        |_, _| (),
                    );
                    assert!(matches!(made, Ok(Ok(Some(())))), "{made:?}");
                    "correction"
                }
                (_, Some(_)) if g.below(4) == 0 => {
                    engine_lock(&engine).clear_logbook();
                    "purge"
                }
                (_, Some(_)) => {
                    engine_lock(&engine).log_qso(qso(call, when));
                    "append"
                }
            };
            *kinds.entry(kind).or_default() += 1;
            let e = engine_lock(&engine);
            flush(&e);
            same_log(
                &stored(&d),
                e.log_records(),
                &format!("seed {seed} step {step}: {kind}"),
            );
        }
    }
    // Every path was taken, each more than once: a run that never reached one proves nothing
    // about it.
    for kind in [
        "append",
        "card",
        "qsl sent",
        "sat tag",
        "delete",
        "edit",
        "correction",
        "connector stamp",
        "lotw batch",
        "already uploaded",
        "fill",
        "report merge",
        "import",
        "qrz download",
        "pota stamps",
        "own echo",
        "purge",
    ] {
        assert!(
            kinds.get(kind).copied().unwrap_or(0) >= 2,
            "{kind} ran {:?} times",
            kinds.get(kind)
        );
    }
}
