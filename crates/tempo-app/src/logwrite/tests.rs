//! SPEC-2 v3 C19 Part B's proofs for the write path as a command makes it: planned with the
//! Engine lock released, made under it — and the FT auto-log, which reads nothing at all.

use super::*;
use crate::dto::Tier;
use crate::engine::{engine_lock, LogWriteOutcome};
use crate::logstore::tests::{
    engine_on_store, eventually, flush, id_at, legacy_log, qso, same_log, stored, Dir,
};
use crate::logstore::DURABLE_WAIT;
use crate::stage1_tests::{lotw_fingerprint, Stage1};
use crate::station::StationKeys;
use crate::test_util::StoredLog;
use std::cell::{Cell, RefCell};
use std::collections::HashSet;
use std::rc::Rc;
use std::time::{Duration, Instant};
use tempo_core::logbook::hot::HotIndex;
use tempo_core::logbook::sqlite::{LogDb, WriteHold};
use tempo_core::logbook::{
    adif_header, adif_record, LogOp, Logbook, QsoRecord, UploadDetail, UploadOutcome,
};

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
    let id = id_at(&*engine, 3);
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
    let id = id_at(&*engine, 3);
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
                crate::engine::sync_shared_log(&engine),
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
    let held = engine
        .stored_records()
        .into_iter()
        .find(|r| r.id == Some(id))
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
    let id = id_at(&*engine, 2);
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
    let id = id_at(&*a, 4);
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

/// Flag (b), the operator's rule (SPEC-2 v3 C19, Q2 (a)): another window's commit since a plan
/// was taken plans the change again, even one to ANOTHER row — the second plan's read is the
/// store's word, and nothing is ever written over. That second plan takes the other window's
/// commit in first, off the lock ([`crate::engine::log_plan`]), and the change is made; both
/// stand. (Before the cut a commit to another row cost no second plan: the commit compared the
/// rows it read with the log in memory, which is gone.)
#[test]
fn another_windows_change_to_another_row_plans_again_and_both_stand() {
    let d = Dir::new("race-window-other");
    let a = shared(&d, 6);
    let b = Arc::new(Mutex::new(engine_on_store(&d)));
    let (id, other) = (id_at(&*a, 4), id_at(&*a, 1));
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
    assert_eq!(
        plans.get(),
        2,
        "planned again: another window committed after the plan was taken"
    );
    durability.wait(DURABLE_WAIT).expect("on disk");
    assert!(stored_row(&d, id).qsl_rcvd.card, "A's card");
    assert_eq!(
        stored_row(&d, other).upload.qrz.map(|s| s.when_unix),
        Some(9),
        "B's stamp on its row"
    );
    assert!(
        !engine_lock(&a).log_store_foreign_pending(),
        "and A took B's commit in, off the lock, before its second plan"
    );
}

// ── another window's commit this window has not counted yet ────────────────────
//
// The two tests above let window A commit only once A's writer has counted B's commit. These do
// not: B commits inside A's plan-to-commit gap and A commits at once, before its writer's next
// look (`FOREIGN_POLL`), so the count the commit check compares has not moved. Only the store,
// checking each row as it writes it, can see B's change there. A command waits for its change and
// makes it again when the store turns it back ([`until_written`]); the upload worker does not.

/// Wait until `e`'s changes are committed: the writer alone, not the `log.adi` mirror behind it,
/// so the racing window's commit lands as close to the plan it races as a real one would.
fn committed(e: &Engine) {
    e.log_store_writer().flush(DURABLE_WAIT).expect("committed");
}

/// ⛔ ANOTHER WINDOW'S STAMP, MADE UNDER THE PLAN, IS NOT WRITTEN OVER. Window B stamps a contact
/// between window A's plan and A's commit. The store turns A's change back, A plans it again on
/// the row as it now stands, and both stand. (Before the store checked the rows it writes, A wrote
/// the row as it had read it, and B's stamp was gone: 10 runs out of 10.)
#[test]
fn another_windows_stamp_made_under_the_plan_is_not_written_over() {
    let d = Dir::new("cas-stamp");
    let a = shared(&d, 6);
    let b = Arc::new(Mutex::new(engine_on_store(&d)));
    let id = id_at(&*a, 4);
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let (b, plans) = (Arc::clone(&b), Rc::clone(&plans));
        move || {
            plans.set(plans.get() + 1);
            if plans.get() == 1 {
                let (made, _) = change_ops(&b, id, None, &[qrz_stamp(id, 9)], "window B");
                assert!(matches!(made, Ok(Ok(_))), "B's stamp is made");
                committed(&engine_lock(&b));
            }
        }
    };
    let (made, durability) = racing(hook, || {
        until_written(|| change_ops(&a, id, None, &[card(id)], "window A"))
    });
    assert!(matches!(made, Ok(Ok(_))), "A's card is made: {made:?}");
    durability.wait(DURABLE_WAIT).expect("on disk");
    let row = stored_row(&d, id);
    assert_eq!(
        row.upload.qrz.map(|s| s.when_unix),
        Some(9),
        "B's stamp stands"
    );
    assert!(row.qsl_rcvd.card, "and A's card with it");
    assert_eq!(plans.get(), 2, "A's change was planned again");
}

/// ⛔ A CONTACT ANOTHER WINDOW DELETED UNDER THE PLAN STAYS DELETED. Window B deletes the contact
/// window A is changing, between A's plan and A's commit. The store turns A's change back, and A's
/// change, planned again, finds the contact gone and says so. (Before: A wrote the row as it had
/// read it, and the deleted contact was back in the store with A's card on it: 11 runs out of 11.)
#[test]
fn a_contact_another_window_deleted_under_the_plan_stays_deleted() {
    let d = Dir::new("cas-delete");
    let a = shared(&d, 6);
    let mut b = engine_on_store(&d);
    let id = id_at(&*a, 4);
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let plans = Rc::clone(&plans);
        move || {
            plans.set(plans.get() + 1);
            if plans.get() == 1 {
                assert!(b.delete_qso(id), "B deletes the contact");
                committed(&b);
            }
        }
    };
    let (made, _) = racing(hook, || {
        until_written(|| change_ops(&a, id, None, &[card(id)], "window A"))
    });
    flush(&engine_lock(&a));
    assert_eq!(
        stored_row_opt(&d, id),
        None,
        "the contact B deleted stays deleted"
    );
    assert!(
        matches!(made, Ok(Err(RowRefusal::Gone))),
        "and A's change finds it gone: {made:?}"
    );
}

/// ⛔ A BACKGROUND STAMP PLANNED BEFORE ANOTHER WINDOW'S EDIT DOES NOT UNDO THE EDIT. The upload
/// worker's stamp does not wait for the disk, so a stamp the store turns back is simply not made,
/// and the next push stamps the contact again: the edit, the operator's own data, stands, and the
/// stamp lands on top of it. (Before: the stamp wrote the row as the worker had read it, and the
/// edit was gone.)
#[test]
fn a_background_stamp_planned_before_another_windows_edit_does_not_undo_it() {
    let d = Dir::new("cas-edit");
    let a = shared(&d, 6);
    let mut b = engine_on_store(&d);
    let id = id_at(&*a, 4);
    let pushed = row_at(&a, 4);
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let (plans, pushed) = (Rc::clone(&plans), pushed.clone());
        move || {
            plans.set(plans.get() + 1);
            if plans.get() == 1 {
                let mut edited = pushed.clone();
                edited.comment = Some("from window B".into());
                assert!(b.update_qso(id, edited), "B edits the contact");
                committed(&b);
            }
        }
    };
    let status = UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix: 7,
        detail: None,
    };
    let (stamped, _) = racing(hook, || {
        stamp_push(&a, &pushed, UploadService::Qrz, status.clone())
    });
    assert!(
        stamped,
        "premise: the worker made its stamp, and does not wait for it"
    );
    flush(&engine_lock(&a));
    assert_eq!(
        stored_row(&d, id).comment.as_deref(),
        Some("from window B"),
        "B's edit stands"
    );
    let (stamped, durability) = stamp_push(&a, &pushed, UploadService::Qrz, status);
    assert!(stamped, "the next push stamps the contact again");
    durability.wait(DURABLE_WAIT).expect("on disk");
    let row = stored_row(&d, id);
    assert_eq!(
        row.comment.as_deref(),
        Some("from window B"),
        "B's edit still stands"
    );
    assert_eq!(
        row.upload.qrz.map(|s| s.when_unix),
        Some(7),
        "with the stamp on top of it"
    );
}

/// ⛔ A LOTW MERGE PLANNED BEFORE ANOTHER WINDOW'S CARD KEEPS THE CARD — a merge of more contacts
/// than the store writes in one transaction. The chunk holding the contact B changed is turned
/// back, the merge is planned again on the log as it now stands, and it lands its confirmations
/// on top: the chunk before it is already in and changes nothing the second time. (Before: the
/// merge wrote the row as it had read it, and B's card was gone.)
#[test]
fn a_lotw_merge_planned_before_another_windows_card_keeps_the_card() {
    let n = tempo_core::logbook::writer::CHUNK_ROWS + 44;
    let d = Dir::new("cas-merge");
    let a = shared(&d, n);
    let mut b = engine_on_store(&d);
    // The last contact, in the merge's second chunk.
    let id = id_at(&*a, n - 1);
    let text: String = (0..n)
        .map(|at| adif_record(&row_at(&a, at)).replace("<EOR>", "<LOTW_QSL_RCVD:1>Y<EOR>"))
        .collect();
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let plans = Rc::clone(&plans);
        move || {
            plans.set(plans.get() + 1);
            if plans.get() == 1 {
                assert!(b.mark_qsl_card(id, true), "B marks the card");
                committed(&b);
            }
        }
    };
    let (made, durability) = racing(hook, || until_written(|| merge_lotw_report(&a, &text)));
    assert!(made.is_ok(), "the merge is made: {made:?}");
    durability.wait(DURABLE_WAIT).expect("on disk");
    let rows = stored(&d);
    let row = rows
        .iter()
        .find(|r| r.id == Some(id))
        .expect("the contact is stored");
    assert!(row.qsl_rcvd.card, "B's card stands");
    assert!(
        rows.iter().all(|r| r.qsl_rcvd.lotw),
        "and every contact carries the merge's confirmation"
    );
    assert_eq!(plans.get(), 2, "the merge was planned again");
}

// ── each path that plans a change and then makes it ────────────────────────────

/// A hook for [`racing`]: at the first plan, window `b` makes `change` and has it committed; every
/// plan is counted in `plans`.
fn b_changes_first(
    mut b: Engine,
    plans: &Rc<Cell<usize>>,
    mut change: impl FnMut(&mut Engine) + 'static,
) -> impl FnMut() + 'static {
    let plans = Rc::clone(plans);
    move || {
        plans.set(plans.get() + 1);
        if plans.get() == 1 {
            change(&mut b);
            committed(&b);
        }
    }
}

/// Window B's QRZ stamp on the contact `row`.
fn b_stamps(row: QsoRecord) -> impl FnMut(&mut Engine) + 'static {
    move |b| {
        assert!(
            b.stamp_qrz_upload(&row, UploadOutcome::Accepted, 9, None),
            "B's stamp is made"
        );
    }
}

/// ⛔ THE LOGBOOK FORM'S EDIT, PLANNED BEFORE ANOTHER WINDOW'S STAMP, LANDS ON TOP OF IT. A stamp
/// changes nothing the form edits, so the edit is still the one the operator made: made again on
/// the stamped row, and both stand.
#[test]
fn an_edit_planned_before_another_windows_stamp_lands_on_top_of_it() {
    let d = Dir::new("cas-form");
    let a = shared(&d, 6);
    let id = id_at(&*a, 4);
    let row = row_at(&a, 4);
    let key = QsoEdit::project(&row).key();
    let mut edit = QsoEdit::project(&row);
    edit.comment = Some("from window A".into());
    let plans = Rc::new(Cell::new(0usize));
    let hook = b_changes_first(engine_on_store(&d), &plans, b_stamps(row));
    let (made, durability) = racing(hook, || until_written(|| edit_row(&a, id, &key, &edit)));
    assert!(matches!(made, Ok(Ok(_))), "A's edit is made: {made:?}");
    durability.wait(DURABLE_WAIT).expect("on disk");
    let row = stored_row(&d, id);
    assert_eq!(row.comment.as_deref(), Some("from window A"), "A's edit");
    assert!(row.upload.qrz.is_some(), "and B's stamp");
    assert_eq!(plans.get(), 2, "the edit was made again");
}

/// ⛔ THE FORM'S EDIT, PLANNED BEFORE ANOTHER WINDOW EDITED THE SAME CONTACT, IS NOT MADE: the
/// contact is no longer the one the operator was editing, and the answer shows it as it now stands
/// ([`RowRefusal::Changed`]). The other window's edit stands.
#[test]
fn an_edit_planned_before_another_windows_edit_answers_changed() {
    let d = Dir::new("cas-form-both");
    let a = shared(&d, 6);
    let id = id_at(&*a, 4);
    let row = row_at(&a, 4);
    let key = QsoEdit::project(&row).key();
    let mut edit = QsoEdit::project(&row);
    edit.comment = Some("from window A".into());
    let theirs = {
        let mut r = row.clone();
        r.comment = Some("from window B".into());
        r
    };
    let plans = Rc::new(Cell::new(0usize));
    let hook = b_changes_first(engine_on_store(&d), &plans, move |b| {
        assert!(b.update_qso(id, theirs.clone()), "B edits the contact");
    });
    let (made, _) = racing(hook, || until_written(|| edit_row(&a, id, &key, &edit)));
    assert!(
        matches!(&made, Ok(Err(RowRefusal::Changed(now))) if now.comment.as_deref() == Some("from window B")),
        "A's edit answers Changed, with the contact as it now stands: {made:?}"
    );
    flush(&engine_lock(&a));
    assert_eq!(
        stored_row(&d, id).comment.as_deref(),
        Some("from window B"),
        "B's edit stands"
    );
}

/// ⛔ A QSL-SENT MARK PLANNED BEFORE ANOTHER WINDOW'S STAMP IS MADE AGAIN ON TOP OF IT.
#[test]
fn a_qsl_sent_mark_planned_before_another_windows_stamp_keeps_it() {
    let d = Dir::new("cas-sent");
    let a = shared(&d, 6);
    let id = id_at(&*a, 4);
    let plans = Rc::new(Cell::new(0usize));
    let hook = b_changes_first(engine_on_store(&d), &plans, b_stamps(row_at(&a, 4)));
    let via = Some(tempo_core::logbook::QslVia::Bureau);
    let (made, durability) = racing(hook, || {
        until_written(|| change_ops(&a, id, None, &[qsl_sent(id, via)], "sent"))
    });
    assert!(matches!(made, Ok(Ok(_))), "{made:?}");
    durability.wait(DURABLE_WAIT).expect("on disk");
    let row = stored_row(&d, id);
    assert!(row.qsl_sent.sent, "A's QSL-sent mark");
    assert!(row.upload.qrz.is_some(), "and B's stamp");
    assert_eq!(plans.get(), 2, "made again");
}

/// ⛔ A SATELLITE TAG PLANNED BEFORE ANOTHER WINDOW'S PAPER CARD IS MADE AGAIN ON TOP OF IT.
#[test]
fn a_satellite_tag_planned_before_another_windows_card_keeps_it() {
    let d = Dir::new("cas-sat");
    let a = shared(&d, 6);
    let id = id_at(&*a, 4);
    let plans = Rc::new(Cell::new(0usize));
    let hook = b_changes_first(engine_on_store(&d), &plans, move |b| {
        assert!(b.mark_qsl_card(id, true), "B marks the card");
    });
    let tag = LogOp::SetSatTag {
        id,
        sat_name: Some("RS-44".into()),
    };
    let (made, durability) = racing(hook, || {
        until_written(|| change_ops(&a, id, None, std::slice::from_ref(&tag), "sat"))
    });
    assert!(matches!(made, Ok(Ok(_))), "{made:?}");
    durability.wait(DURABLE_WAIT).expect("on disk");
    let row = stored_row(&d, id);
    assert_eq!(row.sat_name.as_deref(), Some("RS-44"), "A's tag");
    assert!(row.qsl_rcvd.card, "and B's card");
    assert_eq!(plans.get(), 2, "made again");
}

/// ⛔ A DELETE PLANNED BEFORE ANOTHER WINDOW EDITED THE CONTACT DOES NOT THROW THE EDIT AWAY. The
/// delete names the version the operator saw (its edit key, as every delete from the Logbook
/// does); made again, it finds the contact changed and answers so, and the contact stays with the
/// other window's edit.
#[test]
fn a_delete_planned_before_another_windows_edit_answers_changed() {
    let d = Dir::new("cas-del-edit");
    let a = shared(&d, 6);
    let id = id_at(&*a, 4);
    let row = row_at(&a, 4);
    let key = QsoEdit::project(&row).key();
    let theirs = {
        let mut r = row.clone();
        r.comment = Some("from window B".into());
        r
    };
    let plans = Rc::new(Cell::new(0usize));
    let hook = b_changes_first(engine_on_store(&d), &plans, move |b| {
        assert!(b.update_qso(id, theirs.clone()), "B edits the contact");
    });
    let (made, _) = racing(hook, || {
        until_written(|| change_ops(&a, id, Some(&key), &[LogOp::Delete(id)], "delete"))
    });
    assert!(
        matches!(made, Ok(Err(RowRefusal::Changed(_)))),
        "A's delete answers Changed: {made:?}"
    );
    flush(&engine_lock(&a));
    assert_eq!(
        stored_row_opt(&d, id).and_then(|r| r.comment),
        Some("from window B".to_string()),
        "the contact stays, with B's edit"
    );
}

/// ⛔ THE OPERATOR'S PUSH WAITS FOR ITS STAMP, AND MAKES IT AGAIN ON TOP OF ANOTHER WINDOW'S EDIT.
#[test]
fn a_push_stamp_planned_before_another_windows_edit_lands_on_top_of_it() {
    let d = Dir::new("cas-push");
    let a = shared(&d, 6);
    let id = id_at(&*a, 4);
    let pushed = row_at(&a, 4);
    let theirs = {
        let mut r = pushed.clone();
        r.comment = Some("from window B".into());
        r
    };
    let plans = Rc::new(Cell::new(0usize));
    let hook = b_changes_first(engine_on_store(&d), &plans, move |b| {
        assert!(b.update_qso(id, theirs.clone()), "B edits the contact");
    });
    let status = UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix: 7,
        detail: None,
    };
    let (stamped, durability) = racing(hook, || {
        until_written(|| stamp_push(&a, &pushed, UploadService::Qrz, status.clone()))
    });
    assert!(stamped, "the push stamps the contact");
    durability.wait(DURABLE_WAIT).expect("on disk");
    let row = stored_row(&d, id);
    assert_eq!(row.comment.as_deref(), Some("from window B"), "B's edit");
    assert_eq!(
        row.upload.qrz.map(|s| s.when_unix),
        Some(7),
        "and the push's stamp"
    );
    assert_eq!(plans.get(), 2, "made again");
}

/// ⛔ TQSL'S STAMPS, PLANNED BEFORE ANOTHER WINDOW'S QRZ STAMP ON ONE OF THE CONTACTS, ARE MADE
/// AGAIN ON TOP OF IT: a connector's stamp changes nothing TQSL signed ([`LotwSigned`]), so every
/// contact is still the one it signed. (A paper card would: the contact is then no longer the one
/// signed, and is counted changed, as it always was.)
#[test]
fn lotw_stamps_planned_before_another_windows_stamp_keep_it() {
    let d = Dir::new("cas-tqsl");
    let a = shared(&d, 6);
    let ids: Vec<RecordId> = (0..6).map(|at| id_at(&*a, at)).collect();
    let view = engine_lock(&a).log_view();
    let rows = view.rows(&ids).expect("read");
    let signed: Vec<LotwSigned> = ids
        .iter()
        .filter_map(|id| rows.get(id))
        .filter_map(|r| LotwSigned::of(r))
        .collect();
    assert_eq!(signed.len(), 6, "premise: TQSL signed all six");
    let theirs = ids[3];
    let plans = Rc::new(Cell::new(0usize));
    let hook = b_changes_first(engine_on_store(&d), &plans, b_stamps(row_at(&a, 3)));
    let status = UploadStatus {
        outcome: UploadOutcome::Pending,
        when_unix: 11,
        detail: None,
    };
    let (done, durability) = racing(hook, || {
        until_written(|| stamp_lotw_batch(&a, &signed, &status))
    });
    assert_eq!(done.stamped, 6, "every contact stamped: {done:?}");
    durability.wait(DURABLE_WAIT).expect("on disk");
    let stored = stored(&d);
    assert!(
        stored.iter().all(|r| r.upload.lotw.is_some()),
        "every contact carries TQSL's stamp"
    );
    assert!(
        stored
            .iter()
            .find(|r| r.id == Some(theirs))
            .is_some_and(|r| r.upload.qrz.is_some()),
        "and B's stamp"
    );
    assert_eq!(plans.get(), 2, "made again");
}

/// ⛔ "ALREADY UPLOADED", A CHUNK AT A TIME: A CHUNK PLANNED BEFORE ANOTHER WINDOW'S QRZ STAMP IS
/// MADE AGAIN ON TOP OF IT, and every contact owed is stamped.
#[test]
fn already_uploaded_planned_before_another_windows_stamp_keeps_it() {
    let d = Dir::new("cas-declared");
    let a = shared(&d, 6);
    let rows = engine_lock(&a).log_rows();
    let owed = station::lotw_unsent_ids(&rows).expect("the store reads");
    assert!(
        owed.len() > 3,
        "premise: more than one chunk owed: {owed:?}"
    );
    let theirs = owed[1];
    let row = stored_row(&d, theirs);
    let plans = Rc::new(Cell::new(0usize));
    let hook = b_changes_first(engine_on_store(&d), &plans, b_stamps(row));
    let (made, durability) = racing(hook, || mark_lotw_uploaded_in_chunks(&a, 1_900_000_000, 3));
    assert_eq!(made, Ok(owed.len()), "every contact owed is stamped");
    durability.wait(DURABLE_WAIT).expect("on disk");
    let stored = stored(&d);
    assert!(
        owed.iter().all(|id| stored
            .iter()
            .any(|r| r.id == Some(*id) && r.upload.lotw.is_some())),
        "all stamped"
    );
    assert!(
        stored
            .iter()
            .find(|r| r.id == Some(theirs))
            .is_some_and(|r| r.upload.qrz.is_some()),
        "and B's stamp stands"
    );
    assert_eq!(plans.get(), 3, "two chunks, the first made again");
}

/// ⛔ AN IMPORT THAT BRINGS A CONTACT UP TO DATE, PLANNED BEFORE ANOTHER WINDOW'S CARD, KEEPS THE
/// CARD: made again on the log as it now stands.
#[test]
fn an_import_planned_before_another_windows_card_keeps_it() {
    let d = Dir::new("cas-import");
    let a = shared(&d, 6);
    let id = id_at(&*a, 2);
    let text = adif_record(&row_at(&a, 2)).replace("<EOR>", "<LOTW_QSL_RCVD:1>Y<EOR>");
    let plans = Rc::new(Cell::new(0usize));
    let hook = b_changes_first(engine_on_store(&d), &plans, move |b| {
        assert!(b.mark_qsl_card(id, true), "B marks the card");
    });
    let (made, durability) = racing(hook, || until_written(|| import_adif(&a, &text)));
    assert!(made.is_ok(), "{made:?}");
    durability.wait(DURABLE_WAIT).expect("on disk");
    let row = stored_row(&d, id);
    assert!(row.qsl_rcvd.lotw, "the import's confirmation");
    assert!(row.qsl_rcvd.card, "and B's card");
    assert_eq!(plans.get(), 2, "made again");
}

/// ⛔ THE FILL JOB DOES NOT WRITE OVER ANOTHER WINDOW'S EDIT. Its chunk planned before the edit is
/// turned back, so the job stops there without claiming the log filled (`fill_ver` is not
/// written), the edit stands, and the job's next pass fills what is still empty.
#[test]
fn the_fill_job_does_not_write_over_another_windows_edit() {
    let d = Dir::new("cas-fill");
    let a = shared(&d, 6);
    let edited = id_at(&*a, 1);
    let theirs = {
        let mut r = row_at(&a, 1);
        r.comment = Some("from window B".into());
        r
    };
    let fills: Vec<LogFill> = stored(&d)
        .iter()
        .filter_map(|r| r.id)
        .map(|id| LogFill {
            id,
            country: Some("Found".into()),
            state: None,
        })
        .collect();
    let plans = Rc::new(Cell::new(0usize));
    let hook = b_changes_first(engine_on_store(&d), &plans, move |b| {
        assert!(b.update_qso(edited, theirs.clone()), "B edits a contact");
    });
    let first = racing(hook, || fill_in_chunks(&a, &fills, 7, 3));
    assert!(
        first.is_err(),
        "the job stops at the chunk turned back: {first:?}"
    );
    flush(&engine_lock(&a));
    let db = LogDb::open(&d.db()).expect("the store opens");
    assert_eq!(
        db.meta(crate::logfill::FILL_VER).expect("reads"),
        None,
        "and does not claim the log filled"
    );
    assert_eq!(
        stored_row(&d, edited).comment.as_deref(),
        Some("from window B"),
        "B's edit stands"
    );

    let again = fill_in_chunks(&a, &fills, 7, 3).expect("the next pass");
    assert!(again > 0, "fills what is still empty");
    flush(&engine_lock(&a));
    let row = stored_row(&d, edited);
    assert_eq!(row.comment.as_deref(), Some("from window B"), "B's edit");
    assert_eq!(row.country.as_deref(), Some("Found"), "filled on top of it");
    assert!(
        stored(&d).iter().all(|r| r.country.is_some()),
        "every contact filled"
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
    let id = id_at(&*engine, 3);
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
}

/// A purge still on its way, and a purge the writer gave up: what a plan reads of the rows around
/// it, and what sending it again takes out (SPEC-2 v3 C19).
mod purge {
    use super::*;

    /// The one contact `call` names, as a plan finds it by its call: the store as it stands, with
    /// this process's changes laid over it — never a wait for a stalled writer.
    fn named(engine: &Mutex<Engine>, call: &str) -> RecordId {
        let view = engine_lock(engine).log_view();
        let rows = view
            .candidates(&std::collections::BTreeSet::from([call.to_string()]))
            .expect("read");
        assert_eq!(rows.len(), 1, "one {call}: {rows:?}");
        rows[0].id.expect("an id")
    }

    /// The row `id` as a plan reads it: `None` when the log does not hold it.
    fn read(engine: &Mutex<Engine>, id: RecordId) -> Option<Arc<QsoRecord>> {
        let view = engine_lock(engine).log_view();
        view.row(id).expect("read")
    }

    /// Poll `f` until it says yes or `limit` passes: longer than [`eventually`] waits, for the
    /// writer's retry ladder.
    fn eventually_for(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        f()
    }

    /// A read of the log held open on a thread of its own until the sender sends or is dropped:
    /// on the 1.13 path, where the store is in memory, what holds its writer off. The thread
    /// answers the read.
    fn read_held_open(
        engine: &Mutex<Engine>,
    ) -> (
        std::sync::mpsc::Sender<()>,
        std::thread::JoinHandle<
            Result<crate::logstore::Freshness, tempo_core::logbook::sqlite::Error>,
        >,
    ) {
        let rows = engine_lock(engine).log_rows();
        let (open, opened) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let reader = std::thread::spawn(move || {
            let mut first = true;
            rows.each_record(Duration::ZERO, &mut |_| {
                if std::mem::take(&mut first) {
                    let _ = open.send(());
                    let _ = released.recv();
                }
                std::ops::ControlFlow::Continue(())
            })
        });
        opened.recv().expect("the read is open");
        (release, reader)
    }

    /// The calls `log.adi` holds, in its order.
    fn calls_in_file(d: &Dir) -> Vec<String> {
        tempo_core::logbook::Logbook::load(&d.log())
            .records()
            .iter()
            .map(|r| r.call.clone())
            .collect()
    }

    /// ★ A CONTACT LOGGED AFTER A PURGE ON ITS WAY IS THERE TO A PLAN. With the writer stalled, a
    /// purge is still on its way when the next contact is logged, and both say something of that
    /// contact: the purge that every row is gone, the append that it is there. The newer is the
    /// log. A plan that read the purge refused every change made by id to the new contact — a
    /// card, an edit, a delete — as a change to a contact that is gone, until the purge landed
    /// and the next change let it go. Each ordering, asserted: a contact the store holds, gone;
    /// one logged before the purge, gone; one logged after it, there; edited after it, the edit;
    /// deleted after it, gone. And once the writer clears, the store holds the same.
    #[test]
    fn a_plan_reads_a_contact_logged_after_a_purge_the_store_has_not_taken_yet() {
        let d = Dir::new("overlay-purge");
        let engine = shared(&d, 6);
        let held = named(&engine, "K3ABC");
        let hold = WriteHold::take(&d.db()).expect("stall the writer");

        engine_lock(&engine).log_qso(qso("K1AAA", 1_788_000_000));
        let before = named(&engine, "K1AAA");
        engine_lock(&engine).clear_logbook();
        assert!(
            stored(&d).iter().any(|r| r.id == Some(held)),
            "control: the store still holds it"
        );
        assert_eq!(read(&engine, held), None, "a contact the store holds: gone");
        assert_eq!(read(&engine, before), None, "logged before the purge: gone");

        engine_lock(&engine).log_qso(qso("K1BBB", 1_788_000_060));
        let after = named(&engine, "K1BBB");
        assert!(
            !stored(&d).iter().any(|r| r.id == Some(after)),
            "control: the store has not taken it"
        );
        let row = read(&engine, after).expect("logged after the purge: there");
        assert_eq!(row.call, "K1BBB");

        let mut edit = QsoEdit::project(&row);
        edit.comment = Some("after the purge".into());
        let (made, _) = edit_row(&engine, after, &QsoEdit::project(&row).key(), &edit);
        assert!(
            matches!(made, Ok(Ok(_))),
            "an edit after the purge is made: {made:?}"
        );
        assert_eq!(
            read(&engine, after)
                .and_then(|r| r.comment.clone())
                .as_deref(),
            Some("after the purge"),
            "edited after the purge: the edit"
        );

        engine_lock(&engine).log_qso(qso("K1CCC", 1_788_000_120));
        let deleted = named(&engine, "K1CCC");
        let (made, _) = change_ops(&engine, deleted, None, &[LogOp::Delete(deleted)], "delete");
        assert!(
            matches!(made, Ok(Ok(_))),
            "a delete after the purge is made: {made:?}"
        );
        assert_eq!(
            read(&engine, deleted),
            None,
            "deleted after the purge: gone"
        );

        drop(hold);
        flush(&engine_lock(&engine));
        let rows = stored(&d);
        assert_eq!(
            rows.len(),
            1,
            "the store holds the one contact left: {rows:?}"
        );
        assert_eq!(rows[0].id, Some(after));
        assert_eq!(rows[0].comment.as_deref(), Some("after the purge"));
        for id in [held, before, deleted] {
            assert_eq!(
                read(&engine, id),
                None,
                "and a plan reads the store the same"
            );
        }
    }

    /// ★ A PURGE SENT AGAIN TAKES OUT THE ROWS IT TOOK OUT, AND NO OTHER (never silently lose a
    /// contact). Another program holds the database past the writer's retry ladder (about 21 s:
    /// this test waits it out), and the writer gives the purge up. The program lets go, and a
    /// contact is logged and lands before the purge is sent again. Sent again as "every row",
    /// the purge took that contact out of the store while the log in memory still held it: gone
    /// from `log.adi`, and from the next launch. Sent as the rows it took out, it leaves it. And
    /// while the purge waits, a plan reads the contact as there, not as gone.
    #[test]
    fn a_purge_sent_again_takes_out_the_rows_it_took_out_and_no_other() {
        let d = Dir::new("purge-resend");
        let engine = shared(&d, 6);
        let hold = WriteHold::take(&d.db()).expect("another program holds the database");
        engine_lock(&engine).clear_logbook();
        assert!(
            eventually_for(Duration::from_secs(90), || {
                engine_lock(&engine).log_unsaved().standing().retryable == 1
            }),
            "premise: the writer gives the purge up"
        );
        drop(hold);
        engine_lock(&engine).log_qso(qso("K1BBB", 1_788_000_060));
        let after = named(&engine, "K1BBB");
        assert!(
            eventually(|| stored(&d).iter().any(|r| r.id == Some(after))),
            "the contact lands while the purge waits"
        );
        // What the writer finished taken in, the purge's wait not up yet: the contact's own
        // change is done with, and the purge is what is left.
        let due = engine_lock(&engine).log_resend_due();
        assert_eq!(
            read(&engine, after).map(|r| r.call.clone()).as_deref(),
            Some("K1BBB"),
            "a plan reads it as there while the purge waits"
        );

        let sent = due + engine_lock(&engine).log_resend_all();
        assert_eq!(sent, 1, "the purge, sent again");
        let unsaved = engine_lock(&engine).log_unsaved();
        assert!(unsaved.wait(DURABLE_WAIT).saved(), "and it lands");
        flush(&engine_lock(&engine));
        let in_store: Vec<String> = stored(&d).into_iter().map(|r| r.call).collect();
        assert_eq!(
            in_store,
            ["K1BBB"],
            "the store holds the contact logged after the purge, and none of the rows it took out"
        );
        assert!(
            eventually(|| engine_lock(&engine).log_unsaved().is_empty()),
            "log.adi catches up"
        );
        assert_eq!(calls_in_file(&d), ["K1BBB"], "and so does log.adi");
    }

    /// ★ …AND ON THE 1.13 PATH, `log.adi` KEEPS IT. There the store is in memory and `log.adi` is
    /// the log's home. A read held open on the store holds its writer off, as another program
    /// holds the database, until the writer gives the purge up. A contact is logged (into
    /// `log.adi` at once, as 1.13 appended it), and the purge is sent again: its rewrite of
    /// `log.adi` reads the store, which holds the contact and none of the rows the purge took
    /// out.
    #[test]
    fn on_the_1_13_path_a_purge_sent_again_leaves_log_adi_the_contact_logged_after_it() {
        let d = Dir::new("purge-resend-1-13");
        std::fs::write(d.log(), legacy_log(6)).unwrap();
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_log_path(d.log());
        assert!(e.log_on_file(), "premise: the 1.13 path");
        e.flush_log_store(DURABLE_WAIT).expect("settled");
        let engine = Arc::new(Mutex::new(e));
        let (release, reader) = read_held_open(&engine);
        engine_lock(&engine).clear_logbook();
        let gave_up = eventually_for(Duration::from_secs(90), || {
            engine_lock(&engine).log_unsaved().standing().retryable == 1
        });
        drop(release);
        reader.join().expect("the read ran").expect("and read");
        assert!(gave_up, "premise: the writer gives the purge up");
        engine_lock(&engine).log_qso(qso("K1BBB", 1_788_000_060));
        assert!(
            eventually(|| calls_in_file(&d).contains(&"K1BBB".to_string())),
            "the contact is in log.adi at once"
        );
        let due = engine_lock(&engine).log_resend_due();
        let sent = due + engine_lock(&engine).log_resend_all();
        assert_eq!(sent, 1, "the purge, sent again");
        let unsaved = engine_lock(&engine).log_unsaved();
        assert!(unsaved.wait(DURABLE_WAIT).saved(), "and it is in log.adi");
        assert_eq!(
            calls_in_file(&d),
            ["K1BBB"],
            "log.adi holds the contact logged after the purge, and none of the rows it took out"
        );
    }

    /// ★ A PURGE SENT AGAIN TAKES OUT EXACTLY THE ROWS IT TOOK OUT: never a contact logged after
    /// it, nor one a later change brought back under its own id (SPEC-2 v3 C19, §4.11). The rows
    /// are the hot index the purge took, and the writer lists them on its own thread when the
    /// re-send arrives. Another program holds the database past the writer's retry ladder (about
    /// 21 s: this test waits it out), and the writer gives the purge up. Then a contact is logged,
    /// and one of the purged contacts is imported back, its id kept; the purge is sent again, and
    /// the store holds those two and none of the other rows it took out.
    #[test]
    fn a_purge_sent_again_spares_a_new_contact_and_one_brought_back() {
        let d = Dir::new("purge-resend-brought-back");
        let engine = shared(&d, 6);
        let back = stored(&d)
            .into_iter()
            .nth(2)
            .expect("a contact to bring back");
        let hold = WriteHold::take(&d.db()).expect("another program holds the database");
        engine_lock(&engine).clear_logbook();
        assert!(
            eventually_for(Duration::from_secs(90), || {
                engine_lock(&engine).log_unsaved().standing().retryable == 1
            }),
            "premise: the writer gives the purge up"
        );
        drop(hold);

        engine_lock(&engine).log_qso(qso("K1NEW", 1_788_000_060));
        let restated = adif_header() + &tempo_core::logbook::adif_record_own_log(&back);
        let (made, _) = import_adif(&engine, &restated);
        let (added, _, _, _) = made.expect("the import is made");
        assert_eq!(added, 1, "premise: the purged contact comes back as new");
        assert_eq!(
            named(&engine, &back.call),
            back.id.expect("an id"),
            "premise: brought back under its own id"
        );
        assert!(
            eventually(|| stored(&d).iter().any(|r| r.call == "K1NEW")),
            "the new contact lands while the purge waits"
        );

        let due = engine_lock(&engine).log_resend_due();
        let sent = due + engine_lock(&engine).log_resend_all();
        assert_eq!(sent, 1, "the purge, sent again");
        let unsaved = engine_lock(&engine).log_unsaved();
        assert!(unsaved.wait(DURABLE_WAIT).saved(), "and it lands");
        flush(&engine_lock(&engine));
        let mut in_store: Vec<String> = stored(&d).into_iter().map(|r| r.call).collect();
        in_store.sort();
        let mut expected = vec!["K1NEW".to_string(), back.call.clone()];
        expected.sort();
        assert_eq!(
            in_store, expected,
            "the store holds the new contact and the one brought back, and none of the rest"
        );
    }

    /// Held by the writer's thread while it frees it, until the test lets go: a writer away from
    /// its channels for as long as the test says — as one retrying a write another program holds
    /// up is, but stalled until released rather than until a timeout ends it.
    struct Stall {
        inside: std::sync::mpsc::Sender<()>,
        until: std::sync::mpsc::Receiver<()>,
    }

    impl Drop for Stall {
        fn drop(&mut self) {
            let _ = self.inside.send(());
            let _ = self.until.recv();
        }
    }

    /// ★ A PURGE THAT LANDED IS LET GO UNDER THE LOCK WITHOUT WAITING ON THE WRITER (SPEC-2 v3
    /// C19, §4.11). The next change finds the purge landed and lets its rows go under the Engine
    /// lock, sending them to the writer's thread to be freed there. With that thread stalled, the
    /// contact is logged and the lock let go all the same: a send that waited for the thread to
    /// take them would hold the Engine lock, and the radio loop behind it, for as long as the
    /// writer is stalled.
    #[test]
    fn a_purge_that_landed_is_let_go_under_the_lock_without_waiting_on_a_stalled_writer() {
        let d = Dir::new("purge-let-go-stalled");
        let engine = shared(&d, 6);
        engine_lock(&engine).clear_logbook();
        let unsaved = engine_lock(&engine).log_unsaved();
        assert!(unsaved.wait(DURABLE_WAIT).saved(), "the purge lands");

        let (inside, stalled) = std::sync::mpsc::channel();
        let (release, until) = std::sync::mpsc::channel();
        let disposer = engine_lock(&engine).log_store_writer().disposer();
        disposer.dispose(Box::new(Stall { inside, until }));
        stalled.recv().expect("the writer's thread is stalled");

        let (logged_tx, logged) = std::sync::mpsc::channel();
        let logger = {
            let engine = Arc::clone(&engine);
            std::thread::spawn(move || {
                engine_lock(&engine).log_qso(qso("K1BBB", 1_788_000_060));
                let _ = logged_tx.send(());
            })
        };
        let in_time = logged.recv_timeout(Duration::from_secs(10)).is_ok();
        drop(release);
        logger.join().expect("the contact is logged");
        assert!(
            in_time,
            "the contact is logged, and the lock let go, while the writer is stalled"
        );

        flush(&engine_lock(&engine));
        let in_store: Vec<String> = stored(&d).into_iter().map(|r| r.call).collect();
        assert_eq!(
            in_store,
            ["K1BBB"],
            "once the writer goes on, the store holds the contact and none of the rows the \
             purge took out"
        );
    }
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
    let id = id_at(&*engine, 3);
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
            drop(e);
            let log = engine.stored_log();
            let (held, row) = (log.len(), log.last().cloned());
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
        assert_eq!(
            engine.stored_log().len(),
            model.len(),
            "seed {seed}: nothing else was logged"
        );
        let e = engine_lock(&engine);
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
    let id = id_at(&*engine, 1);
    let e = engine_lock(&engine);
    let _ = e.logged_row(id);
}

/// The contact at `at` in the log, as a report restating it is written from.
fn row_at(engine: &Arc<Mutex<Engine>>, at: usize) -> QsoRecord {
    QsoRecord::clone(&engine.stored_log()[at])
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

/// The row `id` as the store holds it, if it does.
fn stored_row_opt(d: &Dir, id: RecordId) -> Option<QsoRecord> {
    stored(d).into_iter().find(|r| r.id == Some(id))
}

/// The date the station gave `id`'s QSL-sent mark, or its withdrawal, which is dated too — as
/// the store holds it: the date Stage 1's mark takes (the second difference in
/// [`crate::stage1_tests`]' header).
fn sent_date(d: &Dir, id: RecordId) -> u64 {
    stored_row_opt(d, id)
        .and_then(|r| match r.qsl_sent.sent {
            true => r.qsl_sent.date_unix,
            false => r.qsl_sent.cleared_unix,
        })
        .unwrap_or(0)
}

/// The FT append, as Stage 1 takes it: the contact the station appended — carrying the id the
/// station minted — or nothing, when the station's duplicate guard refused it.
fn appended(engine: &Arc<Mutex<Engine>>, d: &Dir, s1: &mut Stage1) {
    flush(&engine_lock(engine));
    let rows = stored(d);
    if rows.len() == s1.logbook.len() + 1 {
        s1.add_record(rows.last().expect("one more").clone());
    }
}

/// The rows a write appended at `from` and after, whose ids Stage 1's log minted, take the ids
/// the station minted for them — only where the id on each side is new, one Stage 1 did not
/// hold before the write (`stage1_tests`' `adopt_minted`, over the store's rows).
fn adopt_minted(d: &Dir, s1: &mut Stage1, from: usize, held: &HashSet<RecordId>) {
    let station = stored(d);
    if station.len() != s1.logbook.len() {
        return;
    }
    let new = |id: Option<RecordId>| id.is_some_and(|id| !held.contains(&id));
    let minted: Vec<(usize, RecordId)> = s1
        .logbook
        .records()
        .iter()
        .zip(&station)
        .enumerate()
        .skip(from)
        .filter(|(_, (mine, theirs))| mine.id != theirs.id && new(mine.id) && new(theirs.id))
        .map(|(i, (_, theirs))| (i, theirs.id.expect("new")))
        .collect();
    if !minted.is_empty() {
        let records = s1.logbook.records_mut(tempo_core::logbook::OpClass::IdOnly);
        for (i, id) in minted {
            Arc::make_mut(&mut records[i]).id = Some(id);
        }
    }
}

/// The store holds Stage 1's log, contact for contact, each with an id of its own.
fn the_store_is_stage_1s(d: &Dir, s1: &Stage1, what: &str) {
    let want: Vec<QsoRecord> = s1
        .logbook
        .records()
        .iter()
        .map(|r| QsoRecord::clone(r))
        .collect();
    let ids: HashSet<RecordId> = want.iter().filter_map(|r| r.id).collect();
    assert_eq!(
        ids.len(),
        want.len(),
        "{what}: a contact without an id of its own"
    );
    let got = stored(d);
    assert_eq!(
        got.len(),
        want.len(),
        "{what}: the store's count is Stage 1's"
    );
    if let Some((i, (g, w))) = got.iter().zip(&want).enumerate().find(|(_, (g, w))| g != w) {
        panic!("{what}: the store is not Stage 1's log at contact {i}:\n  {g:?}\n  {w:?}");
    }
}

/// The station's hot index answers as one built from the store, keyed as the station keys it —
/// built with the Engine lock released, compared under it.
fn the_index_is_the_stores(engine: &Arc<Mutex<Engine>>, d: &Dir, what: &str) {
    let resolve = engine_lock(engine).station().dxcc_resolve.clone();
    let built = {
        let db = LogDb::open(&d.db()).expect("the store opens");
        db.in_one_snapshot(|db| HotIndex::from_store(db, &StationKeys(resolve.as_deref())))
            .expect("built from the store")
    };
    assert!(
        engine_lock(engine).station().hot().answers_as(&built),
        "{what}: the hot index is not the store's"
    );
}

/// ★ P6 THROUGH EVERY WRITE PATH, AGAINST STAGE 1 (SPEC-2 v3 C19): seeded random runs of every
/// kind of change the app makes as a command — the commands' planned changes (the marks, the
/// tag, a delete, the form's edit, a correction of a row found by what was shown, the connector
/// stamps by id and by push key, the LoTW batch and the "already uploaded" declaration, the fill
/// job), the FT append, the purge, and the bulk merges — each made alike to Stage 1's log, the
/// write path before C19 B verbatim ([`crate::stage1_tests`]). After EACH the store holds Stage
/// 1's log, and the station's hot index answers as one built from the store: the index is the
/// one follower of the store the cut leaves, and what the FT gate's duplicate guard reads. Until
/// the cut this compared the store with the log in memory, which followed each change as well;
/// the in-breath twins of these commands are held to Stage 1 by `stage1_tests` itself.
#[test]
fn every_write_path_leaves_the_store_as_stage_1_would_and_the_index_follows() {
    const CALLS: [&str; 5] = ["K1ABC", "W9XYZ", "DL1AB", "K2DEF/P", "JA1AA"];
    let mut kinds = std::collections::BTreeMap::<&str, usize>::new();
    for seed in 0..6u64 {
        let d = Dir::new(&format!("p6-{seed}"));
        let engine = shared(&d, 10);
        // Stage 1 starts from the log the store was converted to, with the station's resolvers
        // (none).
        let mut s1 = Stage1 {
            logbook: Logbook::from_store(stored(&d)),
            ..Stage1::default()
        };
        let mut g = Gen(seed);
        for step in 0..45u64 {
            let len = engine.stored_log().len();
            let at = g.below(len);
            let id = (len > 0).then(|| id_at(&*engine, at));
            let call = CALLS[g.below(CALLS.len())];
            let when = 1_788_000_000 + step * 97;
            let (from, held): (usize, HashSet<RecordId>) = (
                s1.logbook.len(),
                s1.logbook.records().iter().filter_map(|r| r.id).collect(),
            );
            let kind = match (g.below(18), id) {
                (0 | 1, _) | (_, None) => {
                    engine_lock(&engine).log_qso(qso(call, when));
                    appended(&engine, &d, &mut s1);
                    "append"
                }
                (2, Some(id)) => {
                    let _ = change_ops(&engine, id, None, &[card(id)], "card");
                    s1.mark_qsl_card(id, true);
                    "card"
                }
                (3, Some(id)) => {
                    let via = (g.below(2) == 0).then_some(tempo_core::logbook::QslVia::Bureau);
                    let _ = change_ops(&engine, id, None, &[qsl_sent(id, via)], "sent");
                    flush(&engine_lock(&engine));
                    s1.mark_qsl_sent(id, via, sent_date(&d, id));
                    "qsl sent"
                }
                (4, Some(id)) => {
                    let sat_name = (g.below(2) == 0).then(|| "RS-44".to_string());
                    let tag = LogOp::SetSatTag {
                        id,
                        sat_name: sat_name.clone(),
                    };
                    let _ = change_ops(&engine, id, None, &[tag], "sat");
                    s1.set_sat_tag(id, sat_name.as_deref());
                    "sat tag"
                }
                (5, Some(id)) => {
                    let _ = change_ops(&engine, id, None, &[LogOp::Delete(id)], "delete");
                    s1.delete_qso(id);
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
                    flush(&engine_lock(&engine));
                    let copy = s1.edit_qso(id, &key, &edit, sent_date(&d, id));
                    assert!(matches!(copy, Ok(Ok(()))), "Stage 1's edit: {copy:?}");
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
                    let (outcome, when) = (UploadOutcome::Accepted, when as i64);
                    match service {
                        UploadService::Qrz => s1.stamp_qrz_upload(&pushed, outcome, when, None),
                        UploadService::Clublog => {
                            s1.stamp_clublog_upload(&pushed, outcome, when, None)
                        }
                        _ => s1.stamp_eqsl_upload(&pushed, outcome, when, None),
                    };
                    "connector stamp"
                }
                (8, Some(_)) => {
                    let view = engine_lock(&engine).log_view();
                    let ids: Vec<RecordId> = engine
                        .stored_log()
                        .iter()
                        .filter_map(|r| r.id)
                        .take(4)
                        .collect();
                    let rows = view.rows(&ids).expect("read");
                    let signed_rows: Vec<&Arc<QsoRecord>> = ids
                        .iter()
                        .filter_map(|id| rows.get(id))
                        .filter(|r| LotwSigned::of(r).is_some())
                        .collect();
                    let signed: Vec<LotwSigned> = signed_rows
                        .iter()
                        .filter_map(|r| LotwSigned::of(r))
                        .collect();
                    let pairs: Vec<(RecordId, u64)> = signed_rows
                        .iter()
                        .map(|r| (r.id.expect("an id"), lotw_fingerprint(r)))
                        .collect();
                    let status = UploadStatus {
                        outcome: UploadOutcome::Pending,
                        when_unix: when as i64,
                        detail: None,
                    };
                    let _ = stamp_lotw_batch(&engine, &signed, &status);
                    s1.stamp_lotw_batch(&pairs, UploadOutcome::Pending, when as i64, None);
                    "lotw batch"
                }
                (9, Some(_)) => {
                    let rows = engine_lock(&engine).log_rows();
                    let owed = station::lotw_unsent_ids(&rows).expect("the store reads");
                    let _ = mark_lotw_uploaded_all(&engine, when as i64);
                    s1.stamp_lotw_upload(
                        &owed,
                        UploadOutcome::Accepted,
                        when as i64,
                        Some(UploadDetail::OperatorDeclared),
                    );
                    "already uploaded"
                }
                (10, Some(id)) => {
                    let fills = [LogFill {
                        id,
                        country: Some("Found".into()),
                        state: Some("WI".into()),
                    }];
                    fill(&engine, &fills, step as i64).expect("the fill job writes");
                    s1.apply_log_fills(&fills);
                    "fill"
                }
                (11, Some(_)) => {
                    // A LoTW report's confirmation of a contact the log holds (the bulk lane).
                    let text = adif_record(&row_at(&engine, at))
                        .replace("<EOR>", "<LOTW_QSL_RCVD:1>Y<EOR>");
                    let (made, _) = merge_lotw_report(&engine, &text);
                    made.expect("the report merges");
                    s1.merge_lotw_report(&text);
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
                    s1.import_adif(&text);
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
                    s1.merge_qrz_report(&text);
                    "qrz download"
                }
                (14, Some(_)) => {
                    // A pota.app export naming the park of a contact the log holds.
                    let text = adif_record(&row_at(&engine, at))
                        .replace("<EOR>", "<SIG:4>POTA<SIG_INFO:6>K-0001<EOR>");
                    let (made, _) = import_pota_log(&engine, &text);
                    made.expect("the stamps are made");
                    s1.import_pota_log(&text);
                    "pota stamps"
                }
                (15, Some(_)) => {
                    // LoTW's own-QSO report of a contact the log holds.
                    let text = adif_record(&row_at(&engine, at));
                    let (made, _) = merge_lotw_own_echo(&engine, &text, step as i64 + 1);
                    made.expect("the report merges");
                    s1.merge_lotw_own_echo(&text, step as i64 + 1);
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
                    s1.update_qso(
                        id,
                        QsoRecord {
                            call: call.into(),
                            comment: Some(format!("corrected {step}")),
                            ..QsoRecord::clone(&row)
                        },
                    );
                    "correction"
                }
                (_, Some(_)) if g.below(4) == 0 => {
                    engine_lock(&engine).clear_logbook();
                    s1.clear_logbook();
                    "purge"
                }
                (_, Some(_)) => {
                    engine_lock(&engine).log_qso(qso(call, when));
                    appended(&engine, &d, &mut s1);
                    "append"
                }
            };
            *kinds.entry(kind).or_default() += 1;
            flush(&engine_lock(&engine));
            adopt_minted(&d, &mut s1, from, &held);
            let at = format!("seed {seed} step {step}: {kind}");
            the_store_is_stage_1s(&d, &s1, &at);
            the_index_is_the_stores(&engine, &d, &at);
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

/// How many contacts the store at `db` holds stamped as uploaded to LoTW, once `engine`'s writer
/// has written every change made before the call.
fn lotw_stamped(engine: &Mutex<Engine>, db: &std::path::Path) -> usize {
    flush(&engine_lock(engine));
    lotw_in_store(db)
}

/// How many contacts the store at `db` holds stamped as uploaded to LoTW as it stands, with no
/// wait for a writer.
fn lotw_in_store(db: &std::path::Path) -> usize {
    tempo_core::logbook::sqlite::LogDb::open(db)
        .unwrap()
        .load_all()
        .unwrap()
        .iter()
        .filter(|r| r.upload.lotw.is_some())
        .count()
}

/// ★ "ALREADY UPLOADED" IS DECLARED A CHUNK AT A TIME (SPEC-2 v3 §4.11: no log path holds the
/// Engine lock past 5 ms). The declaration stamps every contact still owed to LoTW — on the
/// bench's 500,000-contact log a third of a million, which one change held the lock over half a
/// second to make. In chunks, each change is bounded by the chunk and not by the log: ten owed
/// contacts in chunks of three are four changes of at most three contacts, as the store says at
/// each chunk's plan, and every contact is stamped. Made again, it finds nothing owed and changes
/// nothing.
#[test]
fn already_uploaded_is_declared_a_chunk_at_a_time_and_again_changes_nothing() {
    let d = Dir::new("lotw-chunks");
    // Eleven contacts, ten owed: the first is logged at a bare midnight, a time the log does not
    // know, and LoTW is never offered a contact without one.
    let engine = shared(&d, 11);
    let rows = engine_lock(&engine).log_rows();
    let ids = station::lotw_unsent_ids(&rows).expect("read");
    assert_eq!(ids.len(), 10, "premise: ten contacts owed to LoTW");
    // What the store holds stamped when each chunk is planned: what the changes before it made.
    let seen = Rc::new(RefCell::new(Vec::new()));
    let hook = {
        let (engine, seen, db) = (Arc::clone(&engine), Rc::clone(&seen), d.db());
        move || seen.borrow_mut().push(lotw_stamped(&engine, &db))
    };
    let (made, durability) = racing(hook, || {
        mark_lotw_uploaded_in_chunks(&engine, 1_900_000_000, 3)
    });
    assert_eq!(made, Ok(10), "every owed contact is stamped");
    let marks: Vec<usize> = std::iter::once(0)
        .chain(seen.borrow().iter().skip(1).copied())
        .chain([lotw_stamped(&engine, &d.db())])
        .collect();
    let changes: Vec<usize> = marks.windows(2).map(|w| w[1] - w[0]).collect();
    assert_eq!(
        changes,
        [3, 3, 3, 1],
        "ten owed contacts in chunks of three: four changes, none of more than three"
    );
    assert_eq!(durability.len(), 4, "four changes to wait for");
    durability.wait(DURABLE_WAIT).expect("on disk");
    for &id in &ids {
        assert_eq!(
            stored_row(&d, id).upload.lotw.map(|s| s.when_unix),
            Some(1_900_000_000),
            "each stamped by the declaration"
        );
    }

    let before = stored(&d);
    let (again, durability) = mark_lotw_uploaded_in_chunks(&engine, 1_900_000_100, 3);
    assert_eq!(again, Ok(0), "made again, nothing is owed");
    assert!(durability.is_empty(), "and nothing is written");
    flush(&engine_lock(&engine));
    assert_eq!(stored(&d), before, "the log is as the declaration left it");
}

/// A declaration stopped partway keeps the chunks it made and says how far it got; made again, it
/// picks only what is still owed, so it completes the rest and stamps nothing twice. Here another
/// writer changes a contact of the second chunk under every plan, which stops it as LogBusy — a
/// crash or a quit between chunks leaves the same log.
#[test]
fn a_declaration_stopped_partway_keeps_its_chunks_and_again_completes_it() {
    let d = Dir::new("lotw-chunks-busy");
    // Ten owed: the first of the eleven has no known time.
    let engine = shared(&d, 11);
    let rows = engine_lock(&engine).log_rows();
    let ids = station::lotw_unsent_ids(&rows).expect("read");
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let (engine, plans, raced) = (Arc::clone(&engine), Rc::clone(&plans), ids[3]);
        move || {
            plans.set(plans.get() + 1);
            if plans.get() > 1 {
                // Every plan of the second chunk: another writer changes one of its contacts.
                let stamp = qrz_stamp(raced, plans.get() as i64);
                let (made, _) = change_ops(&engine, raced, None, &[stamp], "theirs");
                assert!(matches!(made, Ok(Ok(_))), "their stamp is made");
            }
        }
    };
    let (made, durability) = racing(hook, || {
        mark_lotw_uploaded_in_chunks(&engine, 1_900_000_000, 3)
    });
    let said = made.expect_err("the second chunk kept changing");
    assert!(said.contains("3 of 10"), "it says how far it got: {said}");
    assert_eq!(
        plans.get(),
        1 + PLANS,
        "the first chunk planned once, the second {PLANS} times, and nothing after it"
    );
    assert_eq!(durability.len(), 1, "the first chunk's change, to wait for");
    durability
        .wait(DURABLE_WAIT)
        .expect("the first chunk on disk");
    flush(&engine_lock(&engine));
    for (k, &id) in ids.iter().enumerate() {
        assert_eq!(
            stored_row(&d, id).upload.lotw.is_some(),
            k < 3,
            "contact {k}: the first chunk stands, and nothing after it was made"
        );
    }
    assert_eq!(
        stored_row(&d, ids[3]).upload.qrz.map(|s| s.when_unix),
        Some((1 + PLANS) as i64),
        "every one of their stamps was made"
    );

    let (again, durability) = mark_lotw_uploaded_in_chunks(&engine, 1_900_000_100, 3);
    assert_eq!(again, Ok(7), "made again, only what is still owed");
    durability.wait(DURABLE_WAIT).expect("on disk");
    for (k, &id) in ids.iter().enumerate() {
        assert_eq!(
            stored_row(&d, id).upload.lotw.map(|s| s.when_unix),
            Some(if k < 3 { 1_900_000_000 } else { 1_900_000_100 }),
            "contact {k}: stamped once, by the declaration that reached it"
        );
    }
}

/// A contact a change settles after the pick — a LoTW upload stamps it while the declaration works
/// through the chunks before its own — is no longer owed when its chunk is planned, and is left as
/// that change made it: the declaration never writes "already uploaded" over what an upload
/// recorded. Every other contact is declared.
#[test]
fn a_contact_an_upload_stamps_before_its_chunk_is_left_as_the_upload_made_it() {
    let d = Dir::new("lotw-chunks-settled");
    // Ten owed: the first of the eleven has no known time.
    let engine = shared(&d, 11);
    let rows = engine_lock(&engine).log_rows();
    let ids = station::lotw_unsent_ids(&rows).expect("read");
    let uploaded = ids[5];
    let plans = Rc::new(Cell::new(0usize));
    let hook = {
        let (engine, plans) = (Arc::clone(&engine), Rc::clone(&plans));
        move || {
            plans.set(plans.get() + 1);
            if plans.get() == 1 {
                // While the first chunk is planned: an upload's stamp on a contact of the second.
                let pending = LogOp::Stamp {
                    id: uploaded,
                    service: UploadService::Lotw,
                    status: UploadStatus {
                        outcome: UploadOutcome::Pending,
                        when_unix: 7,
                        detail: None,
                    },
                };
                let (made, _) = change_ops(&engine, uploaded, None, &[pending], "a LoTW upload");
                assert!(matches!(made, Ok(Ok(_))), "the upload's stamp is made");
            }
        }
    };
    let (made, durability) = racing(hook, || {
        mark_lotw_uploaded_in_chunks(&engine, 1_900_000_000, 3)
    });
    assert_eq!(
        made,
        Ok(9),
        "every contact still owed when its chunk was planned"
    );
    durability.wait(DURABLE_WAIT).expect("on disk");
    let upload = stored_row(&d, uploaded).upload.lotw.expect("stamped");
    assert_eq!(
        (upload.outcome, upload.when_unix),
        (UploadOutcome::Pending, 7),
        "the upload's own stamp stands"
    );
    for &id in ids.iter().filter(|&&id| id != uploaded) {
        assert_eq!(
            stored_row(&d, id).upload.lotw.map(|s| s.when_unix),
            Some(1_900_000_000),
            "and every other contact is declared"
        );
    }
}

/// On the 1.13 path the declaration is saved once it is in `log.adi`, its home there: the chunks'
/// changes, joined, are waited for to the last one, so when the command returns the file holds
/// every stamp — not only the first chunk's.
#[test]
fn on_the_1_13_path_every_chunk_is_in_log_adi_when_the_declaration_is_saved() {
    let d = Dir::new("lotw-chunks-1-13");
    std::fs::write(d.log(), legacy_log(11)).unwrap();
    let mut e = Engine::new("K2DEF", "FN31", 0);
    e.set_log_path(d.log());
    assert!(e.log_on_file(), "premise: the 1.13 path");
    let engine = Arc::new(Mutex::new(e));
    let (made, durability) = mark_lotw_uploaded_in_chunks(&engine, 1_900_000_000, 3);
    assert_eq!(made, Ok(10), "every owed contact is stamped");
    assert_eq!(durability.len(), 4, "four changes to wait for");
    durability.wait(DURABLE_WAIT).expect("in log.adi");
    let on_disk = tempo_core::logbook::Logbook::load(&d.log());
    assert_eq!(
        on_disk
            .records()
            .iter()
            .filter(|r| r
                .upload
                .lotw
                .as_ref()
                .is_some_and(|s| s.when_unix == 1_900_000_000))
            .count(),
        10,
        "log.adi holds every chunk's stamps"
    );
}

/// The declaration moves at the pace the store takes it: each chunk is in the store before the
/// next is planned. So its changes never pile up in flight, where each later commit, under the
/// Engine lock, and each later plan's read would look through them row by row — a backlog the
/// size of the log on a big one, since the store's writer takes a few thousand rows a second.
/// Here the writer is held while the first chunk is made, and let go a moment later: the second
/// chunk is planned only once the first is in the store.
#[test]
fn each_chunk_is_in_the_store_before_the_next_is_planned() {
    let d = Dir::new("lotw-chunks-paced");
    // Ten owed: the first of the eleven has no known time.
    let engine = shared(&d, 11);
    let seen = Rc::new(RefCell::new(Vec::new()));
    let hook = {
        let (seen, db) = (Rc::clone(&seen), d.db());
        move || {
            // What the store holds as each chunk is planned, with no wait for the writer.
            seen.borrow_mut().push(lotw_in_store(&db));
            if seen.borrow().len() == 1 {
                let hold = WriteHold::take(&db).expect("hold the writer");
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(300));
                    drop(hold);
                });
            }
        }
    };
    let (made, durability) = racing(hook, || {
        mark_lotw_uploaded_in_chunks(&engine, 1_900_000_000, 3)
    });
    assert_eq!(made, Ok(10), "every owed contact is stamped");
    assert_eq!(
        *seen.borrow(),
        [0, 3, 6, 9],
        "each chunk planned once the chunks before it are in the store"
    );
    durability.wait(DURABLE_WAIT).expect("on disk");
}

/// How many contacts the store at `db` holds with a country, as it stands.
fn with_country_in_store(db: &std::path::Path) -> usize {
    tempo_core::logbook::sqlite::LogDb::open(db)
        .unwrap()
        .load_all()
        .unwrap()
        .iter()
        .filter(|r| r.country.is_some())
        .count()
}

/// The fill job moves at the pace the store takes it: a chunk of fills is planned only once every
/// chunk but the one made last is in the store — the writer takes that one while the next is
/// planned — and the last, which carries `fill_ver`, only once every earlier fill is. On the first
/// launch after an update it may fill every contact of a big log while every screen is loading.
/// Here the writer is held while the first chunk is made, and let go a moment later.
#[test]
fn a_chunk_of_fills_is_planned_only_once_the_chunks_before_the_last_are_stored() {
    let d = Dir::new("fill-chunks-paced");
    let engine = shared(&d, 10);
    let fills: Vec<LogFill> = stored(&d)
        .iter()
        .filter_map(|r| r.id)
        .map(|id| LogFill {
            id,
            country: Some("Found".into()),
            state: None,
        })
        .collect();
    assert_eq!(
        (fills.len(), with_country_in_store(&d.db())),
        (10, 0),
        "premise: ten contacts, none with a country yet"
    );
    let seen = Rc::new(RefCell::new(Vec::new()));
    let hook = {
        let (seen, db) = (Rc::clone(&seen), d.db());
        move || {
            // What the store holds as each chunk is planned, with no wait for the writer.
            seen.borrow_mut().push(with_country_in_store(&db));
            if seen.borrow().len() == 1 {
                let hold = WriteHold::take(&db).expect("hold the writer");
                std::thread::spawn(move || {
                    std::thread::sleep(Duration::from_millis(300));
                    drop(hold);
                });
            }
        }
    };
    let filled = racing(hook, || fill_in_chunks(&engine, &fills, 7, 3)).expect("the job writes");
    assert_eq!(filled, 10, "every contact gains its country");
    let seen = seen.borrow().clone();
    assert_eq!(seen.len(), 4, "four chunks planned once each: {seen:?}");
    assert_eq!(
        seen[1], 0,
        "premise: the writer was held while the second chunk was planned"
    );
    assert!(
        seen[2] >= 3,
        "each chunk planned once the chunks before the last made are in the store: {seen:?}"
    );
    assert_eq!(
        seen[3], 9,
        "the last chunk, with fill_ver, planned once every earlier fill is in the store"
    );
}

// ── Logbook ▸ Check confirmations ───────────────────────────────────────────────────────────

/// One ADIF record: `call` on `band` in `mode` at `hhmmss` UTC on 2026-10-01, with `more`.
fn adif_row(call: &str, band: &str, mode: &str, hhmmss: &str, more: &str) -> String {
    format!(
        "<CALL:{}>{call}<BAND:{}>{band}<MODE:{}>{mode}<QSO_DATE:8>20261001<TIME_ON:6>{hhmmss}\
         {more}<EOR>\n",
        call.len(),
        band.len(),
        mode.len()
    )
}

/// LoTW's answer as its downloads read: the banner, `rows`, and the end marker.
fn lotw_answer(rows: &[String]) -> String {
    format!(
        "ARRL Logbook of the World Status Report\n<PROGRAMID:4>LoTW\n<eoh>\n{}<APP_LoTW_EOF>\n",
        rows.concat()
    )
}

/// The contacts an ADIF text holds, as the log would read them.
fn contacts(rows: &[String]) -> Vec<QsoRecord> {
    tempo_core::logbook::parse_adif(&rows.concat())
}

/// A shared engine on the store of `d`, holding `log`: written to `log.adi` as the operator's own
/// ADIF, and taken in by the store as an operator's log is. Every contact `K…` is the United
/// States, so the hot index knows an entity for it.
fn holding(d: &Dir, log: &[QsoRecord]) -> Arc<Mutex<Engine>> {
    let mut text = adif_header();
    for r in log {
        text.push_str(&tempo_core::logbook::adif_record_own_log(r));
    }
    std::fs::write(d.log(), text).unwrap();
    let mut e = engine_on_store(d);
    flush(&e);
    e.set_dxcc_resolver(|call| call.starts_with('K').then(|| "United States".to_string()));
    Arc::new(Mutex::new(e))
}

/// The check, as the command reads it: with the Engine lock released, for K2DEF, the call
/// `engine_on_store`'s station keeps.
fn check_of(
    engine: &Mutex<Engine>,
    report: &str,
    own: &str,
) -> tempo_core::reconcile::check::ConfirmationCheck {
    station::check_lotw_confirmations(&crate::engine::log_plan(engine), report, own, Some("K2DEF"))
        .expect("the check reads the log")
        .check
}

/// The contact `call` at `hhmmss`, as the store holds it.
fn contact_at(engine: &Mutex<Engine>, call: &str, hhmmss: &str) -> QsoRecord {
    let when = contacts(&[adif_row(call, "20m", "FT8", hhmmss, "")])[0].when_unix;
    let found: Vec<QsoRecord> = engine
        .stored_records()
        .into_iter()
        .filter(|r| r.call == call && r.when_unix == when)
        .collect();
    assert_eq!(found.len(), 1, "{call} at {hhmmss}");
    found.into_iter().next().unwrap()
}

/// #400's pair, both its marks 1.17.0's, and an S2 contact, built by replaying 1.17.0's merges
/// (`tempo_core::reconcile::check::replay`): W1AW on 20 m FT8 at 06:00 and 18:00, where 1.17.0's
/// own-QSO pull marked 06:00 Accepted from 18:00's upload and a later sync put 18:00's
/// confirmation (DXCC) on 06:00; and K1ABC on 20 m FT8 at 10:00, which got the confirmation of a
/// 15:00 QSO this log does not hold, while LoTW holds 10:00 itself unconfirmed. The log, LoTW's
/// confirmation download and its own-QSO list.
fn misplaced_scene() -> (Vec<QsoRecord>, String, String) {
    use tempo_core::reconcile::check::replay::{echoed_by_1_17, left_by_1_17};
    let confirmations = lotw_answer(&[
        adif_row(
            "W1AW",
            "20m",
            "FT8",
            "180200",
            "<QSL_RCVD:1>Y<CREDIT_GRANTED:4>DXCC",
        ),
        adif_row(
            "K1ABC",
            "20m",
            "FT8",
            "150000",
            "<QSL_RCVD:1>Y<CREDIT_GRANTED:4>DXCC",
        ),
    ]);
    let own = lotw_answer(&[
        adif_row("W1AW", "20m", "FT8", "180000", "<QSL_RCVD:1>Y"),
        adif_row("K1ABC", "20m", "FT8", "100000", "<QSL_RCVD:1>N"),
        adif_row("K1ABC", "20m", "FT8", "150000", "<QSL_RCVD:1>Y"),
    ]);
    let mut log = contacts(&[
        adif_row("W1AW", "20m", "FT8", "060000", ""),
        adif_row("W1AW", "20m", "FT8", "180000", ""),
        adif_row("K1ABC", "20m", "FT8", "100000", ""),
    ]);
    // 18:00 and 10:00 were uploaded; 06:00 never was.
    for r in &mut log[1..] {
        r.upload.lotw = Some(UploadStatus {
            outcome: UploadOutcome::Pending,
            when_unix: 1,
            detail: None,
        });
    }
    let echoed = echoed_by_1_17(log, &tempo_core::logbook::parse_adif(&own));
    let log = left_by_1_17(echoed, &tempo_core::logbook::report_rows(&confirmations));
    (log, confirmations, own)
}

/// When the tests' Apply happens: 2026-10-07 18:00:00 UTC.
const APPLIED_AT: u64 = 1_791_396_000;

/// LoTW's confirmation download, as Apply is handed it when LoTW is the one service checked.
fn lotw_only(report: &str) -> Vec<(tempo_core::reconcile::check::Channel, String)> {
    vec![(
        tempo_core::reconcile::check::Channel::Lotw,
        report.to_string(),
    )]
}

/// The LoTW confirmations Apply took off.
fn taken_off(applied: &ConfirmationsApplied) -> usize {
    applied
        .services
        .iter()
        .filter(|s| s.channel == tempo_core::reconcile::check::Channel::Lotw)
        .map(|s| s.confirmations)
        .sum()
}

/// The ticked contacts of `ticked` that are not, as the store holds them, exactly what `before`
/// held of them: each named by its call and time.
fn not_as_before(
    engine: &Mutex<Engine>,
    before: &[QsoRecord],
    ticked: &[tempo_core::reconcile::check::CheckLine],
) -> Vec<String> {
    let now = engine.stored_records();
    let mut ids: Vec<RecordId> = Vec::new();
    for id in ticked.iter().filter_map(|l| l.contact.id) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids.iter()
        .filter(|id| {
            let was = before.iter().find(|r| r.id == Some(**id));
            let is = now.iter().find(|r| r.id == Some(**id));
            was != is
        })
        .map(|id| {
            let r = before
                .iter()
                .find(|r| r.id == Some(*id))
                .expect("held before");
            format!("{}@{}", r.call, r.when_unix)
        })
        .collect()
}

/// ★ Apply's before-file puts every change back. Imported through Logbook ▸ Import ADIF, it
/// leaves each contact a ticked line changed exactly as it was before Apply (the confirmation,
/// its credit code and the upload mark all back), while the contact that gained LoTW's
/// confirmation keeps it: an import only adds. The positive control comes first: a before-file
/// holding no contact puts nothing back, and this test sees that.
#[test]
fn the_before_file_imported_puts_every_change_back() {
    use tempo_core::reconcile::check::CheckLine;
    let d = Dir::new("check-before-file");
    let (log, report, own) = misplaced_scene();
    let engine = holding(&d, &log);
    let check = check_of(&engine, &report, &own);
    let ticked: Vec<CheckLine> = check
        .lines
        .iter()
        .filter(|l| l.decisive())
        .cloned()
        .collect();
    assert_eq!(ticked.len(), 3, "{:#?}", check.lines);
    let before = engine.stored_records();

    let (applied, durability) =
        apply_confirmation_check(&engine, &lotw_only(&report), &ticked, APPLIED_AT);
    durability.wait(DURABLE_WAIT).expect("on disk");
    let applied = applied.expect("applied");
    assert_eq!((taken_off(&applied), applied.uploads), (2, 1));
    let file = applied.before_file.expect("a before-file");
    assert_eq!(
        file,
        d.0.join(before_file_name(APPLIED_AT)),
        "beside log.adi"
    );
    assert_eq!(
        not_as_before(&engine, &before, &ticked).len(),
        2,
        "Apply changed both ticked contacts"
    );

    // The control: a before-file holding no contact puts nothing back.
    let (made, written) = until_written(|| import_adif(&engine, &adif_header()));
    written.wait(DURABLE_WAIT).expect("on disk");
    made.expect("imported");
    assert_eq!(
        not_as_before(&engine, &before, &ticked).len(),
        2,
        "an empty before-file puts nothing back, and the comparison sees it"
    );

    let text = std::fs::read_to_string(&file).expect("the before-file reads");
    let (made, written) = until_written(|| import_adif(&engine, &text));
    written.wait(DURABLE_WAIT).expect("on disk");
    let (added, skipped, ..) = made.expect("imported");
    assert_eq!(
        (added, skipped),
        (0, 2),
        "both rows are contacts the log holds"
    );
    assert_eq!(
        not_as_before(&engine, &before, &ticked),
        Vec::<String>::new(),
        "every ticked contact is back as it was"
    );
    let gained = contact_at(&engine, "W1AW", "180000");
    assert!(
        gained.qsl_rcvd.lotw && gained.credit_granted == ["DXCC"],
        "18:00 keeps the confirmation LoTW holds for it: {gained:?}"
    );
}

/// ★ Apply changes a ticked contact only while it still holds what its line showed: a paper card
/// marked after the check is a change, so that contact keeps its LoTW confirmation and the card,
/// while the other ticked contact changes. The before-file holds only the contact that changed.
#[test]
fn a_contact_changed_after_the_check_is_left_alone() {
    use tempo_core::reconcile::check::CheckLine;
    let d = Dir::new("check-changed-since");
    let (log, report, own) = misplaced_scene();
    let engine = holding(&d, &log);
    let check = check_of(&engine, &report, &own);
    let ticked: Vec<CheckLine> = check
        .lines
        .iter()
        .filter(|l| l.decisive())
        .cloned()
        .collect();
    assert_eq!(ticked.len(), 3, "{:#?}", check.lines);

    // After the check, before Apply: K1ABC's paper card arrives.
    let id = contact_at(&engine, "K1ABC", "100000").id.expect("an id");
    let (made, written) = change_ops(&engine, id, None, &[card(id)], "test");
    written.wait(DURABLE_WAIT).expect("on disk");
    assert!(matches!(made, Ok(Ok(_))), "{made:?}");

    let (applied, durability) =
        apply_confirmation_check(&engine, &lotw_only(&report), &ticked, APPLIED_AT);
    durability.wait(DURABLE_WAIT).expect("on disk");
    let applied = applied.expect("applied");
    assert_eq!(
        (taken_off(&applied), applied.uploads),
        (1, 1),
        "W1AW's two lines are made; K1ABC's is not"
    );
    let carded = contact_at(&engine, "K1ABC", "100000");
    assert!(
        carded.qsl_rcvd.card && carded.qsl_rcvd.lotw && carded.credit_granted == ["DXCC"],
        "K1ABC is as the operator left it: {carded:?}"
    );
    let moved = contact_at(&engine, "W1AW", "060000");
    assert!(
        !moved.qsl_rcvd.any() && moved.credit_granted.is_empty() && moved.upload.lotw.is_none(),
        "W1AW at 06:00 is as it was before 1.17.0: {moved:?}"
    );
    let file = applied.before_file.expect("a before-file");
    let kept = tempo_core::logbook::parse_adif(&std::fs::read_to_string(&file).unwrap());
    let kept: Vec<(&str, u64)> = kept
        .iter()
        .map(|r| (r.call.as_str(), r.when_unix))
        .collect();
    assert_eq!(
        kept,
        [("W1AW", moved.when_unix)],
        "the file holds the contact that changed, and only it"
    );
}

/// ★ A confirmation taken off reaches the hot index: the confirmed badge (an entity confirmed on
/// a band) follows the removal, and the index answers as one built from the store. The change
/// is `Key`, which moves the watermarks every fold keyed on the log's content reads; an upload
/// mark cleared alone is a `Stamp`, which moves none of them.
#[test]
fn a_confirmation_taken_off_takes_the_badge_off_and_the_index_follows() {
    use tempo_core::reconcile::check::{LineClass, Mark};
    let d = Dir::new("check-hot-index");
    let (log, report, own) = misplaced_scene();
    let engine = holding(&d, &log);
    let badge = |e: &Mutex<Engine>| {
        engine_lock(e)
            .station()
            .hot()
            .entity_confirmed_on("United States", "20m")
    };
    let marks = |e: &Mutex<Engine>| engine_lock(e).station().marks();
    assert!(badge(&engine), "K1ABC's misplaced confirmation confirms it");
    the_index_is_the_stores(&engine, &d, "before");
    let check = check_of(&engine, &report, &own);
    let k1abc = check
        .lines
        .iter()
        .find(|l| l.class == LineClass::Contradicted)
        .cloned()
        .expect("K1ABC's line");

    let was = marks(&engine);
    let (made, durability) = confirmation_repairs(&engine, std::slice::from_ref(&k1abc));
    durability.wait(DURABLE_WAIT).expect("on disk");
    assert_eq!(made, Ok(1));
    let now = marks(&engine);
    assert!(
        now.index_rev > was.index_rev && now.key_rev > was.key_rev && now.shape_rev > was.shape_rev,
        "a confirmation taken off is a Key change: {was:?} → {now:?}"
    );
    assert!(!badge(&engine), "the badge follows the removal");
    the_index_is_the_stores(&engine, &d, "after a confirmation is taken off");

    let upload = check
        .lines
        .iter()
        .find(|l| l.mark == Mark::LotwUpload)
        .cloned()
        .expect("W1AW's upload line");
    let was = marks(&engine);
    let (made, durability) = confirmation_repairs(&engine, std::slice::from_ref(&upload));
    durability.wait(DURABLE_WAIT).expect("on disk");
    assert_eq!(made, Ok(1));
    let now = marks(&engine);
    assert!(now.revision > was.revision, "the change was made");
    assert_eq!(
        (now.index_rev, now.key_rev, now.shape_rev),
        (was.index_rev, was.key_rev, was.shape_rev),
        "an upload mark cleared alone is a Stamp"
    );
    assert!(
        contact_at(&engine, "W1AW", "060000").upload.lotw.is_none(),
        "06:00 is owed to LoTW again"
    );
    the_index_is_the_stores(&engine, &d, "after an upload mark is cleared");
}

/// ★ The check reads every contact of a call either download names, and counts, never lists, the
/// LoTW marks it cannot judge: a confirmation on a contact whose call LoTW names nowhere, one on a
/// contact logged under another call than the one the downloads are for, and an `Accepted` mark
/// on a contact LoTW's own-QSO list does not name.
#[test]
fn the_check_counts_the_marks_no_download_names() {
    use tempo_core::reconcile::check::Unjudged;
    let d = Dir::new("check-counts");
    let (mut log, report, own) = misplaced_scene();
    let mut others = contacts(&[
        adif_row("W9IMP", "20m", "FT8", "120000", "<LOTW_QSL_RCVD:1>Y"),
        adif_row(
            "W9OLD",
            "20m",
            "FT8",
            "120100",
            "<LOTW_QSL_RCVD:1>Y<STATION_CALLSIGN:5>N0OLD",
        ),
        adif_row("W9UPL", "20m", "FT8", "120200", ""),
    ]);
    others[2].upload.lotw = Some(UploadStatus {
        outcome: UploadOutcome::Accepted,
        when_unix: 1,
        detail: None,
    });
    log.extend(others);
    let engine = holding(&d, &log);
    let check = check_of(&engine, &report, &own);
    let listed: Vec<&str> = check
        .lines
        .iter()
        .map(|l| l.contact.call.as_str())
        .collect();
    assert_eq!(listed, ["W1AW", "K1ABC", "W1AW"], "only the scene's lines");
    assert_eq!(
        (check.flags, check.uploads),
        (
            Unjudged {
                unreached: 1,
                out_of_scope: 1
            },
            Unjudged {
                unreached: 1,
                out_of_scope: 0
            }
        )
    );
    assert_eq!(
        check.gains.len(),
        1,
        "W1AW at 18:00 gains LoTW's confirmation"
    );
}

/// eQSL's InBox as its download reads: the banner, then `rows`, the cards sent to the operator.
fn eqsl_answer(rows: &[String]) -> String {
    format!(
        "Received eQSLs for K2DEF\n<PROGRAMID:21>eQSL.cc DownloadInBox\n<ADIF_Ver:5>3.1.6\n<EOH>\n{}",
        rows.concat()
    )
}

/// The three services' checks of the log `engine` holds, for K2DEF, as the commands read them.
fn checks_of(
    engine: &Mutex<Engine>,
    lotw: &str,
    inbox: &str,
    book: &str,
) -> Vec<tempo_core::reconcile::check::CheckLine> {
    let plan = || crate::engine::log_plan(engine);
    let own = Some("K2DEF");
    [
        station::check_lotw_confirmations(&plan(), lotw, &lotw_answer(&[]), own),
        station::check_eqsl_confirmations(&plan(), inbox, own),
        station::check_qrz_confirmations(&plan(), book, own),
    ]
    .into_iter()
    .flat_map(|checked| checked.expect("the check reads the log").check.lines)
    .collect()
}

/// ★ Apply takes off only what the operator ticked, service by service. #400's pair on W1AW has
/// every service's confirmation of 18:00 on 06:00, each put there by 1.17.0's matcher, and K1ABC
/// holds eQSL's card of a 15:00 QSO this log lacks. Of the four lines only 06:00's eQSL line is
/// ticked: 06:00 loses eQSL's confirmation and keeps LoTW's, its DXCC and QRZ's; K1ABC keeps its
/// card; and 18:00 gains each service's confirmation from the merges. Nothing else moves.
#[test]
fn apply_takes_off_only_the_ticked_lines_of_each_service() {
    use tempo_core::reconcile::check::{replay::left_by_1_17, Channel, Mark};
    let d = Dir::new("check-each-service");
    let lotw = lotw_answer(&[adif_row(
        "W1AW",
        "20m",
        "FT8",
        "180200",
        "<QSL_RCVD:1>Y<CREDIT_GRANTED:4>DXCC",
    )]);
    let inbox = eqsl_answer(&[
        adif_row("W1AW", "20m", "FT8", "180100", "<EQSL_QSL_RCVD:1>Y"),
        adif_row("K1ABC", "20m", "FT8", "150000", "<EQSL_QSL_RCVD:1>Y"),
    ]);
    let book = [
        adif_row("W1AW", "20m", "FT8", "180000", "<APP_QRZLOG_STATUS:1>C"),
        adif_row("W1AW", "20m", "FT8", "060000", ""),
    ]
    .concat();
    let mut log = contacts(&[
        adif_row("W1AW", "20m", "FT8", "060000", ""),
        adif_row("W1AW", "20m", "FT8", "180000", ""),
        adif_row("K1ABC", "20m", "FT8", "100000", ""),
    ]);
    for text in [&lotw, &inbox, &book] {
        log = left_by_1_17(log, &tempo_core::logbook::report_rows(text));
    }
    let engine = holding(&d, &log);
    let lines = checks_of(&engine, &lotw, &inbox, &book);
    let listed: Vec<(&str, Mark, bool)> = lines
        .iter()
        .map(|l| (l.contact.call.as_str(), l.mark, l.decisive()))
        .collect();
    assert_eq!(
        listed,
        [
            ("W1AW", Mark::Confirmation(Channel::Lotw), true),
            ("W1AW", Mark::Confirmation(Channel::Eqsl), true),
            ("K1ABC", Mark::Confirmation(Channel::Eqsl), false),
            ("W1AW", Mark::Confirmation(Channel::Qrz), true),
        ]
    );
    let before = engine.stored_records();
    let ticked = vec![lines[1].clone()];
    let downloads = vec![
        (Channel::Lotw, lotw),
        (Channel::Eqsl, inbox),
        (Channel::Qrz, book),
    ];
    let (applied, durability) = apply_confirmation_check(&engine, &downloads, &ticked, APPLIED_AT);
    durability.wait(DURABLE_WAIT).expect("on disk");
    let applied = applied.expect("applied");
    let made: Vec<(Channel, usize, usize)> = applied
        .services
        .iter()
        .map(|s| (s.channel, s.confirmations, s.gained))
        .collect();
    assert_eq!(
        made,
        [
            (Channel::Lotw, 0, 1),
            (Channel::Eqsl, 1, 1),
            (Channel::Qrz, 0, 1)
        ]
    );
    assert_eq!(applied.uploads, 0);

    let at = |call: &str, hhmmss: &str| {
        let when = contacts(&[adif_row(call, "20m", "FT8", hhmmss, "")])[0].when_unix;
        before
            .iter()
            .find(|r| r.call == call && r.when_unix == when)
            .cloned()
            .expect("held before")
    };
    let mut expected = Vec::new();
    let mut early = at("W1AW", "060000");
    early.qsl_rcvd.eqsl = false;
    expected.push(early.clone());
    let mut late = at("W1AW", "180000");
    late.qsl_rcvd.lotw = true;
    late.qsl_rcvd.eqsl = true;
    late.qsl_rcvd.qrz = true;
    late.confirmed = true;
    late.award_confirmed = true;
    late.credit_granted = vec!["DXCC".into()];
    expected.push(late);
    expected.push(at("K1ABC", "100000"));
    let mut now = engine.stored_records();
    now.sort_by_key(|r| (r.call.clone(), r.when_unix));
    expected.sort_by_key(|r| (r.call.clone(), r.when_unix));
    assert_eq!(
        now, expected,
        "06:00 loses eQSL's alone; 18:00 gains each service's; K1ABC is untouched"
    );
    assert!(
        early.qsl_rcvd.lotw && early.qsl_rcvd.qrz && early.credit_granted == ["DXCC"],
        "premise: the unticked lines' marks are there to keep: {early:?}"
    );
    let file = applied.before_file.expect("a before-file");
    let kept = tempo_core::logbook::parse_adif(&std::fs::read_to_string(&file).unwrap());
    let kept: Vec<(&str, u64)> = kept
        .iter()
        .map(|r| (r.call.as_str(), r.when_unix))
        .collect();
    assert_eq!(
        kept,
        [("W1AW", early.when_unix)],
        "the one contact that changed"
    );
}

/// ★ QRZ's copies of other services' confirmations never reach a contact at Apply. QRZ's book
/// re-reports LoTW's, eQSL's and a card's confirmation, LoTW's credit, and a QSO this log lacks.
/// Apply takes LoTW's misplaced confirmation off 06:00, as ticked, though QRZ's copy of 06:00
/// says LoTW confirms it; 18:00 gains QRZ's own confirmation and LoTW's, and no card or eQSL
/// from QRZ's copies; and no contact is added (Sync from QRZ adds those).
#[test]
fn qrz_s_copies_of_other_services_never_reach_a_contact_at_apply() {
    use tempo_core::reconcile::check::{replay::left_by_1_17, Channel};
    let d = Dir::new("check-qrz-copies");
    let lotw = lotw_answer(&[adif_row(
        "W1AW",
        "20m",
        "FT8",
        "180200",
        "<QSL_RCVD:1>Y<CREDIT_GRANTED:4>DXCC",
    )]);
    let copies = "<LOTW_QSL_RCVD:1>Y<EQSL_QSL_RCVD:1>Y<QSL_RCVD:1>Y<CREDIT_GRANTED:4>DXCC";
    let book = [
        adif_row(
            "W1AW",
            "20m",
            "FT8",
            "180000",
            &format!("<APP_QRZLOG_STATUS:1>C{copies}"),
        ),
        adif_row("W1AW", "20m", "FT8", "060000", copies),
        adif_row("K5NEW", "20m", "FT8", "090000", "<APP_QRZLOG_STATUS:1>C"),
    ]
    .concat();
    let log = left_by_1_17(
        contacts(&[
            adif_row("W1AW", "20m", "FT8", "060000", ""),
            adif_row("W1AW", "20m", "FT8", "180000", ""),
        ]),
        &tempo_core::logbook::report_rows(&lotw),
    );
    let engine = holding(&d, &log);
    let lines = checks_of(&engine, &lotw, &eqsl_answer(&[]), &book);
    assert_eq!(lines.len(), 1, "06:00's LoTW line alone: {lines:#?}");
    let (applied, durability) = apply_confirmation_check(
        &engine,
        &[(Channel::Lotw, lotw), (Channel::Qrz, book)],
        &lines,
        APPLIED_AT,
    );
    durability.wait(DURABLE_WAIT).expect("on disk");
    let applied = applied.expect("applied");
    let made: Vec<(Channel, usize, usize)> = applied
        .services
        .iter()
        .map(|s| (s.channel, s.confirmations, s.gained))
        .collect();
    assert_eq!(made, [(Channel::Lotw, 1, 1), (Channel::Qrz, 0, 1)]);
    let now = engine.stored_records();
    assert_eq!(now.len(), 2, "K5NEW is not added: {now:#?}");
    let early = contact_at(&engine, "W1AW", "060000");
    assert_eq!(
        (early.qsl_rcvd, early.credit_granted.as_slice()),
        (Default::default(), &[] as &[String]),
        "06:00 is as it was before 1.17.0: {early:?}"
    );
    let late = contact_at(&engine, "W1AW", "180000");
    assert_eq!(
        late.qsl_rcvd,
        tempo_core::logbook::QslRcvd {
            lotw: true,
            qrz: true,
            ..Default::default()
        },
        "18:00 holds LoTW's word and QRZ's own, and no copy: {late:?}"
    );
    assert_eq!(late.credit_granted, ["DXCC"], "LoTW's row brought it");
}
