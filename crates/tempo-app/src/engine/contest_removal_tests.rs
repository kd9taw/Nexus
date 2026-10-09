//! Removing the newest contest contact, through the engine: refused while club sync runs, kept
//! across a mode change and a restart, carried from a Logbook delete, never in the club outbox,
//! and never a change to the transmitter. Each answer says where the contact already went.

use super::contest_removal::{ContestRefusal, ContestSentTo};
use super::*;
use crate::test_util::StoredLog;
use tempo_core::logbook::{LogOp, UploadOutcome};

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// An Illinois QSO Party engine, worked from Kane County, with `edit` applied to its settings.
fn ilqp(edit: impl FnOnce(&mut Settings)) -> Engine {
    ilqp_on(Engine::new("W9XYZ", "EN50", 0), edit)
}

/// [`ilqp`] on the engine `e` — one with a logbook store attached, for a Logbook change made
/// the way a command makes it.
fn ilqp_on(mut e: Engine, edit: impl FnOnce(&mut Settings)) -> Engine {
    let mut s = e.settings().clone();
    s.mycall = "W9XYZ".into();
    s.fd_active = true;
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "KANE".into();
    s.fd_position_id = "aaaa0001".into();
    edit(&mut s);
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("the party runs");
    // The app's snapshot poll opens the session's sweep of the general log at once; a test that
    // merged first would open it with contest rows already in the log (`with_session_rows`).
    let _ = e.snapshot();
    e
}

fn party(e: &mut Engine, call: &str, county: &str) {
    assert!(e
        .contest_log_manual(
            call,
            &fields(&[("RST", "599"), ("QTH", county)]),
            "CW",
            None
        )
        .unwrap());
}

/// The snapshot's contest log, as `(call, received QTH)`.
fn logged(e: &Engine) -> Vec<(String, String)> {
    e.snapshot()
        .field_day
        .expect("the contest runs")
        .log
        .iter()
        .map(|q| (q.call.clone(), q.rcvd.get(1).cloned().unwrap_or_default()))
        .collect()
}

/// The newest contact as the strip names it.
fn newest(e: &Engine) -> (String, u64) {
    let fd = e.snapshot().field_day.expect("the contest runs");
    let q = fd.log.last().expect("a contact");
    (q.call.clone(), q.when_unix)
}

/// What says whether anything transmits, or could on its own — the clock-repair hold's tuple.
fn transmit_state(e: &Engine) -> (bool, Option<TxOwner>, bool, bool, bool) {
    (
        e.tx_enabled(),
        e.tx_owner(),
        e.manual_ptt(),
        e.tuning(),
        e.slot_tx_abort,
    )
}

/// ⭐ **Refused while club sync runs — at the host and at a position — and nothing
/// changes.** With sync off, the same removal is made: the control that says the refusal is
/// club sync's and not some other gate's.
#[test]
fn removal_is_refused_while_club_sync_runs_and_changes_nothing() {
    for (role, on) in [
        (
            "host",
            (|s: &mut Settings| s.fd_host_enable = true) as fn(&mut Settings),
        ),
        ("position", |s: &mut Settings| {
            s.fd_join_addr = "192.168.1.20:42073".into()
        }),
    ] {
        let mut e = ilqp(on);
        assert!(e.fd_sync_enabled(), "premise: club sync runs at the {role}");
        party(&mut e, "K9AAA", "COOK");
        let before = logged(&e);
        let journal = e.field_day_log_adif();
        let (call, when) = newest(&e);
        assert_eq!(
            e.contest_remove_last(&call, when),
            Ok(Err(ContestRefusal::ClubSync)),
            "{role}"
        );
        assert_eq!(logged(&e), before, "{role}: nothing was removed");
        assert_eq!(
            e.field_day_log_adif(),
            journal,
            "{role}: the journal did not move"
        );
        assert!(e.contest_removed().is_empty());
    }
    // The control: sync off, the same contact goes.
    let mut e = ilqp(|_| {});
    assert!(!e.fd_sync_enabled());
    party(&mut e, "K9AAA", "COOK");
    let (call, when) = newest(&e);
    assert!(matches!(e.contest_remove_last(&call, when), Ok(Ok(_))));
    assert!(logged(&e).is_empty());
    // …and a restore is refused once sync is on, as the removal is.
    let id = e.contest_removed()[0].entry.id();
    let mut s = e.settings().clone();
    s.fd_host_enable = true;
    e.apply_settings(s);
    assert_eq!(e.contest_restore(id), Ok(Err(ContestRefusal::ClubSync)));
    assert!(logged(&e).is_empty(), "nothing was restored");
}

/// ⭐ **A log whose every contact was removed still journals them.** The journal on disk held
/// the contact LIVE, and the write was skipped for an "empty" log, so the next mode change
/// (which re-reads the journal) brought the removed contact back as live.
#[test]
fn removing_the_only_contact_survives_a_mode_change_and_a_restart() {
    let path = std::env::temp_dir().join(format!(
        "nexus_contest_removal_{}_{}.adi",
        std::process::id(),
        line!()
    ));
    let _ = std::fs::remove_file(&path);
    let engine = |path: &std::path::Path| {
        let mut e = Engine::new("W9XYZ", "EN50", 0);
        let mut s = e.settings().clone();
        s.fd_active = true;
        s.fd_event = "ilqp".into();
        s.contest_qth_state = "IL".into();
        s.contest_qth_county = "KANE".into();
        e.apply_settings(s);
        e.set_fd_log_path(path.to_path_buf());
        e.set_mode("fieldday-sp").expect("the party runs");
        e
    };
    let mut e = engine(&path);
    party(&mut e, "K9AAA", "COOK");
    e.journal_mark().wait();
    let (call, when) = newest(&e);
    assert!(matches!(e.contest_remove_last(&call, when), Ok(Ok(_))));
    assert!(logged(&e).is_empty());
    e.journal_mark().wait();

    // A crash right now — no mode change, no exit flush: the journal on disk already holds the
    // removal, read by a second engine exactly as a restart reads it.
    let crashed = engine(&path);
    assert!(
        logged(&crashed).is_empty(),
        "the journal still held the removed contact live"
    );
    assert_eq!(crashed.contest_removed().len(), 1);
    drop(crashed);

    // A run ↔ S&P toggle rebuilds the station from the journal.
    e.set_mode("fieldday-run").expect("the party runs");
    assert!(
        logged(&e).is_empty(),
        "a mode change brought the removed contact back"
    );
    assert_eq!(e.contest_removed().len(), 1, "…and it is still kept");
    e.journal_mark().wait();
    drop(e);

    // A restart reads it back removed.
    let e = engine(&path);
    assert!(
        logged(&e).is_empty(),
        "a restart brought the removed contact back"
    );
    assert_eq!(e.contest_removed()[0].rows[0].call, "K9AAA");
    let _ = std::fs::remove_file(&path);
}

/// ⭐ **A Logbook delete of a contact merged from the RUNNING contest removes it from the
/// contest log too** — as a removal, restorable — through the command path every delete takes
/// and the engine's own twin. While club sync runs the contest row stays.
#[test]
fn a_logbook_delete_of_a_merged_contact_removes_it_from_the_contest_log() {
    let d = crate::logstore::tests::Dir::new("contest-removal-delete");
    let mut e = ilqp_on(crate::logstore::tests::engine_on_store(&d), |_| {});
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    assert_eq!(e.fd_merge_to_general().unwrap().added(), 2);
    // On disk before the next change, as the merge command waits for it to be.
    crate::logstore::tests::flush(&e);
    let id_of = |e: &Engine, call: &str| {
        e.stored_log()
            .iter()
            .find(|r| r.call == call)
            .and_then(|r| r.id)
            .expect("merged")
    };

    // The command path (the desktop's by-id delete, a Remote browser's).
    let w9bbb = id_of(&e, "W9BBB");
    let shared = std::sync::Mutex::new(e);
    let (made, _) =
        crate::logwrite::change_ops(&shared, w9bbb, None, &[LogOp::Delete(w9bbb)], "delete");
    assert!(made.is_ok(), "the logbook delete is made: {made:?}");
    let mut e = shared.into_inner().unwrap();
    assert_eq!(
        logged(&e),
        [("K9AAA".to_string(), "COOK".to_string())],
        "the deleted contact left the contest log"
    );
    assert_eq!(
        e.contest_removed()[0].rows[0].call,
        "W9BBB",
        "…kept, restorable"
    );

    // The engine's own twin.
    let k9aaa = id_of(&e, "K9AAA");
    assert!(e.delete_qso(k9aaa));
    assert!(logged(&e).is_empty());
    assert_eq!(e.contest_removed().len(), 2);

    // While club sync runs, the logbook delete stands and the contest row stays.
    let mut e = ilqp(|s| s.fd_host_enable = true);
    party(&mut e, "K9AAA", "COOK");
    assert_eq!(e.fd_merge_to_general().unwrap().added(), 1);
    let id = id_of(&e, "K9AAA");
    assert!(
        e.delete_qso(id),
        "the logbook delete is the operator's and stands"
    );
    assert_eq!(
        logged(&e).len(),
        1,
        "the contest row stays: the club log has it"
    );
    assert!(e.contest_removed().is_empty());
}

/// A deleted record whose merge identity names another session's or another position's row is
/// an ordinary logbook delete: nothing in this contest log moves.
#[test]
fn a_logbook_delete_never_reaches_a_row_it_does_not_name() {
    let mut e = ilqp(|_| {});
    party(&mut e, "K9AAA", "COOK");
    let session = match &e.mode {
        Mode::FieldDay { station, .. } => station.log.session.id.clone(),
        _ => unreachable!("the party runs"),
    };
    for qid in [
        tempo_core::contest::qid_for("ILQP:IL:last-year", "aaaa0001", 1),
        tempo_core::contest::qid_for(&session, "bbbb0002", 1), // another position, seq 1 too
    ] {
        let mut rec = crate::logstore::tests::qso("K9AAA", 1_792_342_800);
        rec.contest = Some(Box::new(tempo_core::logbook::ContestFields {
            qid,
            ..Default::default()
        }));
        e.contest_row_deleted(&rec);
        assert_eq!(logged(&e).len(), 1, "the row it does not name stays");
    }
    // The control: the qid the merge gives this very row reaches it.
    let qid = tempo_core::contest::qid_for(&session, "aaaa0001", 1);
    let mut rec = crate::logstore::tests::qso("K9AAA", 1_792_342_800);
    rec.contest = Some(Box::new(tempo_core::logbook::ContestFields {
        qid,
        ..Default::default()
    }));
    e.contest_row_deleted(&rec);
    assert!(logged(&e).is_empty());
}

/// ⭐ **The answer names where the contact already went** — the forwarder's destinations
/// once a slot boundary had passed it, and the services holding the logbook's merged copy —
/// and claims nothing for a contact that never left.
#[test]
fn the_answer_names_where_the_contact_already_went() {
    let mut e = ilqp(|s| {
        s.n3fjp_host = "192.168.1.20".into();
        s.n1mm_addr = "192.168.1.255:12060".into();
        s.wsjtx_udp = true;
    });
    party(&mut e, "K9AAA", "COOK");
    let (call, when) = newest(&e);
    // Removed before any slot boundary: it never went anywhere.
    let gone = e.contest_remove_last(&call, when).unwrap().unwrap();
    assert_eq!(
        gone.sent,
        ContestSentTo::default(),
        "nothing claimed for a contact that never left"
    );

    party(&mut e, "W9BBB", "LAKE");
    let seq = match &e.mode {
        Mode::FieldDay { station, .. } => station.log.max_seq(),
        _ => unreachable!(),
    };
    e.note_fd_forwarded(seq); // a slot boundary handed it on
    assert_eq!(e.fd_merge_to_general().unwrap().added(), 1);
    let id = e.stored_log()[0].id.unwrap();
    assert!(e.stamp_qrz_upload(
        &e.stored_log()[0].as_ref().clone(),
        UploadOutcome::Accepted,
        1_792_343_000,
        None
    ));
    let _ = id;
    let (call, when) = newest(&e);
    let shared = std::sync::Mutex::new(e);
    let answer = super::contest_removal::remove_last(&shared, &call, when)
        .unwrap()
        .unwrap();
    assert_eq!(
        answer.removal.sent,
        ContestSentTo {
            n3fjp: Some("192.168.1.20".into()),
            n1mm: Some("192.168.1.255:12060".into()),
            wsjtx: true,
        }
    );
    assert_eq!(
        answer.logbook,
        Some(vec!["qrz"]),
        "the merged copy, uploaded to QRZ"
    );
    let e = shared.into_inner().unwrap();
    assert_eq!(
        e.stored_log().len(),
        1,
        "…and it stays in the logbook: nothing taken back"
    );
}

/// The club outbox never carries a removed row, and the seq it was given stays spent.
#[test]
fn the_outbox_never_carries_a_removed_row() {
    let mut e = ilqp(|_| {});
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    let (call, when) = newest(&e);
    assert!(matches!(e.contest_remove_last(&call, when), Ok(Ok(_))));
    let seqs: Vec<u64> = e.fd_sync_outbox(0).iter().map(|w| w.seq).collect();
    assert_eq!(seqs, [1], "only the live row is queued for the club");
    party(&mut e, "N9CCC", "WILL");
    let seqs: Vec<u64> = e.fd_sync_outbox(0).iter().map(|w| w.seq).collect();
    assert_eq!(
        seqs,
        [1, 3],
        "the next contact gets a new seq; 2 stays spent"
    );
}

/// ⛔ **Removing and restoring never key, unkey or stop the transmitter** — in a cockpit armed
/// for CW, every transmit fact is the same after as before.
#[test]
fn removal_and_restore_never_touch_the_transmitter() {
    let mut e = ilqp(|_| {});
    e.set_operating_mode("cw", false);
    assert!(e.tx_enabled(), "premise: the CW cockpit arms TX");
    party(&mut e, "K9AAA", "COOK");
    let before = transmit_state(&e);
    let (call, when) = newest(&e);
    let gone = e.contest_remove_last(&call, when).unwrap().unwrap();
    assert_eq!(transmit_state(&e), before, "a removal");
    assert!(e.contest_restore(gone.entry.id()).unwrap().is_ok());
    assert_eq!(transmit_state(&e), before, "a restore");
}

/// ⭐ **The journal is written for a log that holds rows, whatever they score.** The guard used
/// to ask the SCORED count, so a Winter Field Day log holding only a satellite contact (worth
/// nothing there) counted as empty: it was never journaled, and a restart lost the contact.
#[test]
fn a_log_of_contacts_that_score_nothing_is_still_journaled() {
    use tempo_core::doppler::Transponder;
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    e.set_frequency(436.795, "70cm", "USB");
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = "wfd".into();
    s.fd_class = "1O".into();
    s.fd_section = "WI".into();
    e.apply_settings(s);
    e.set_mode("fieldday-run").expect("Winter Field Day runs");
    let _ = e.snapshot();
    e.set_sat_transponder(Some((
        "SAUDISAT 1C (SO-50)|FM Voice Repeater".into(),
        0,
        Transponder::channel(145_850_000, 436_795_000),
    )));
    assert!(e
        .contest_log_satellite(
            "W1AW",
            &fields(&[("CLASS", "2O"), ("SECTION", "IL")]),
            "PH",
            None
        )
        .unwrap());
    let fd = e.snapshot().field_day.expect("the contest runs");
    assert_eq!(
        (fd.qso_count, fd.log.len()),
        (0, 1),
        "PREMISE: the contact is in the log, and worth nothing at Winter Field Day"
    );
    assert!(
        e.field_day_log_adif().is_some(),
        "the journal is written for a log that holds a row"
    );
}
