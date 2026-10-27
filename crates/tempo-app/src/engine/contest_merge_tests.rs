//! ⭐ **A contest log merged into the Logbook twice writes each contact once** — across I moved,
//! a County edited in Settings, a mode change and a restart, and for contacts merged before
//! this build.
//!
//! The merge names each Logbook copy of a contest contact by its `APP_NEXUS_QID` and skips a
//! contact whose copy is already there. That name was built from where the operator was, so
//! after a move and a restart (or a County edited in Settings and the contest entered again) the
//! same contact had a new name, and a second merge wrote it again; a Logbook edit or delete of
//! an earlier copy no longer reached its contest row either. Every test here goes through the
//! engine, the real logbook store and the real contest journal, and counts the Logbook's rows.
//! The last section holds each contact to the exchange it was sent with across the same, and
//! to a number of its own however long after the first ones it is logged.

use super::*;
use crate::logstore::tests::{flush, open_fast, Dir};
use crate::test_util::StoredLog;
use std::sync::Mutex;
use tempo_core::logbook::{LogOp, UploadOutcome};

/// This laptop's position id: the second part of every merge identity it mints.
const POS: &str = "a1b2c3d4";

/// The Illinois QSO Party, worked by an Illinois mobile starting in Kane County.
fn ilqp_settings() -> Settings {
    let mut s = Engine::new("W9XYZ", "EN51", 0).settings().clone();
    s.mycall = "W9XYZ".into();
    s.fd_active = true;
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "KANE".into();
    s.fd_position_id = POS.into();
    s
}

/// ARRL Field Day, 3A from Wisconsin.
fn field_day_settings() -> Settings {
    let mut s = Engine::new("W9XYZ", "EN51", 0).settings().clone();
    s.mycall = "W9XYZ".into();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    s.fd_position_id = POS.into();
    s
}

/// A launch on the data folder `d`, as the app makes one: the settings loaded, the logbook store
/// opened, the contest journal's path set, and the contest the master left on entered again with
/// the session's rows read first — a new session, built from those settings, with the journal's
/// rows read back into it.
fn launch(d: &Dir, s: &Settings) -> Engine {
    let mut e = Engine::with_settings(s.clone());
    e.attach_log_store(open_fast(d));
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
    let e = shared.into_inner().unwrap();
    assert!(
        e.in_field_day(),
        "the contest the master left on runs again"
    );
    e
}

/// A switch to `spec`, as the mode command makes it: the session's rows read first.
fn switch(e: Engine, spec: &str) -> Engine {
    let shared = Mutex::new(e);
    with_session_rows(
        &shared,
        |e| e.mode_opens_session(spec),
        |e, rows| {
            e.set_mode(spec).expect("the switch");
            if let Some(rows) = rows {
                e.open_session_from(rows);
            }
        },
    )
    .expect("the session's rows are read");
    shared.into_inner().unwrap()
}

/// A quit, as the app makes one: the contest journal written and on disk, and the logbook's
/// writes on disk. The settings the next launch starts from.
fn quit(e: Engine) -> Settings {
    e.persist_fd_log();
    e.journal_mark().wait();
    flush(&e);
    e.settings().clone()
}

fn party(e: &mut Engine, call: &str, county: &str) {
    let rx = [("RST", "599"), ("QTH", county)].map(|(k, v)| (k.to_string(), v.to_string()));
    assert!(
        e.contest_log_manual(call, &rx, "CW", None).unwrap(),
        "{call} logs"
    );
}

/// The Logbook's calls, in log order.
fn calls(e: &Engine) -> Vec<String> {
    e.stored_log().iter().map(|r| r.call.clone()).collect()
}

/// The Logbook's merge identities, in log order.
fn qids(e: &Engine) -> Vec<String> {
    e.stored_log()
        .iter()
        .map(|r| {
            r.contest
                .as_deref()
                .map_or(String::new(), |c| c.qid.clone())
        })
        .collect()
}

/// The merge, as `(written, already there)`.
fn merge(e: &mut Engine) -> (usize, usize) {
    let r = e.fd_merge_to_general().expect("the contest runs");
    (r.added(), r.already)
}

/// The contest log's calls, in log order.
fn contest_calls(e: &Engine) -> Vec<String> {
    let Mode::FieldDay { station, .. } = &e.mode else {
        panic!("the contest runs")
    };
    station.log.qsos().iter().map(|q| q.call.clone()).collect()
}

/// ⭐ **Merge, I moved, restart, merge: each contact is in the Logbook once.** The mobile merged
/// from Kane, drove into Will, pressed I moved, and the next launch's session was built in Will.
/// The second merge wrote both contacts again.
#[test]
fn a_merge_after_i_moved_and_a_restart_writes_nothing_already_merged() {
    let d = Dir::new("contest-merge-moved");
    let mut e = launch(&d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    assert_eq!(merge(&mut e), (2, 0), "the first merge writes both");
    // CONTROL: the same merge again, before anything moves, writes nothing.
    assert_eq!(merge(&mut e), (0, 2));
    let merged = qids(&e);
    let moved = e
        .contest_i_moved(vec![("QTH".into(), "WILL".into())])
        .expect("Will is an Illinois county");
    assert_eq!(
        moved.contest_qth_county, "WILL",
        "the next launch starts in Will"
    );
    let s = quit(e);

    let mut e = launch(&d, &s);
    assert_eq!(
        contest_calls(&e),
        ["K9AAA", "W9BBB"],
        "PREMISE: the journal brought both contacts back"
    );
    assert_eq!(
        merge(&mut e),
        (0, 2),
        "nothing merged before is written again"
    );
    assert_eq!(
        calls(&e),
        ["K9AAA", "W9BBB"],
        "each contact once in the Logbook"
    );
    assert_eq!(
        qids(&e),
        merged,
        "the copies already there keep their names"
    );

    // CONTROL: a contact logged after the restart, from Will, is merged — once.
    party(&mut e, "K9CCC", "DUPG");
    assert_eq!(merge(&mut e), (1, 2));
    assert_eq!(merge(&mut e), (0, 3));
    assert_eq!(calls(&e), ["K9AAA", "W9BBB", "K9CCC"]);
}

/// ⭐ **A County edited in Settings mid-party, and the contest entered again** — no I moved and
/// no restart: the session is built from Settings each time the contest is entered from another
/// mode, so the edit renamed it the same way.
#[test]
fn a_county_edited_in_settings_mid_party_renames_nothing_already_merged() {
    let d = Dir::new("contest-merge-county-edit");
    let mut e = launch(&d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    assert_eq!(merge(&mut e), (2, 0));
    let mut s = e.settings().clone();
    s.contest_qth_county = "WILL".into();
    e.apply_settings(s);
    // Out of the contest and back in, as a switch to another mode and back makes it.
    let e = switch(e, "chat");
    let mut e = switch(e, "fieldday-sp");
    assert_eq!(
        contest_calls(&e),
        ["K9AAA", "W9BBB"],
        "PREMISE: the rows came back"
    );
    assert_eq!(merge(&mut e), (0, 2), "the mode change renamed nothing");
    assert_eq!(calls(&e), ["K9AAA", "W9BBB"]);

    // …and a restart on the edited County.
    let s = quit(e);
    let mut e = launch(&d, &s);
    assert_eq!(merge(&mut e), (0, 2), "nor did the restart");
    assert_eq!(calls(&e), ["K9AAA", "W9BBB"]);
}

/// ⭐ **Field Day's I moved** (a section, the same way): merge from Wisconsin, move to Illinois,
/// restart, merge.
#[test]
fn a_field_day_merge_after_i_moved_and_a_restart_writes_nothing_already_merged() {
    let d = Dir::new("contest-merge-fd-moved");
    let mut e = launch(&d, &field_day_settings());
    assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
    assert!(e.fd_log_manual("W1AW", "1D", "CT", "PH").unwrap());
    assert_eq!(merge(&mut e), (2, 0));
    let moved = e
        .contest_i_moved(vec![("SECTION".into(), "IL".into())])
        .expect("IL is a section");
    assert_eq!(moved.fd_section, "IL");
    let s = quit(e);

    let mut e = launch(&d, &s);
    assert_eq!(
        contest_calls(&e),
        ["K1ABC", "W1AW"],
        "PREMISE: both came back"
    );
    assert_eq!(
        merge(&mut e),
        (0, 2),
        "nothing merged from Wisconsin is written again"
    );
    assert_eq!(calls(&e), ["K1ABC", "W1AW"]);
    // CONTROL: a contact from Illinois is merged, once.
    assert!(e.fd_log_manual("K2DEF", "1D", "MN", "CW").unwrap());
    assert_eq!(merge(&mut e), (1, 2));
    assert_eq!(calls(&e), ["K1ABC", "W1AW", "K2DEF"]);
}

/// Two contacts merged from Kane, I moved to Will, and a restart: the state an edit or a delete
/// of an earlier copy meets.
fn merged_then_moved_and_restarted(d: &Dir) -> Engine {
    let mut e = launch(d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    assert_eq!(merge(&mut e), (2, 0));
    e.contest_i_moved(vec![("QTH".into(), "WILL".into())])
        .expect("Will is an Illinois county");
    let s = quit(e);
    launch(d, &s)
}

/// The Logbook copy of `call`.
fn copy_of(e: &Engine, call: &str) -> QsoRecord {
    QsoRecord::clone(
        e.stored_log()
            .iter()
            .find(|r| r.call == call)
            .expect("merged"),
    )
}

/// ⭐ **A Logbook edit to a contact merged before the move still reaches its contest row**, and
/// so the Cabrillo.
#[test]
fn a_logbook_edit_after_i_moved_and_a_restart_reaches_the_contest_row() {
    let d = Dir::new("contest-merge-edit");
    let mut e = merged_then_moved_and_restarted(&d);
    // The operator fixes a busted call in the Logbook.
    let mut fixed = copy_of(&e, "K9AAA");
    fixed.call = "K9AAB".into();
    assert!(e.update_qso(fixed.id.unwrap(), fixed), "the edit applies");
    assert_eq!(
        contest_calls(&e),
        ["K9AAB", "W9BBB"],
        "the contest row follows"
    );
    let cbr = e.export_log("cabrillo").expect("one entry");
    assert!(
        cbr.contains("K9AAB") && !cbr.contains("K9AAA"),
        "the Cabrillo kept the busted call:\n{cbr}"
    );
}

/// ⭐ **…and a Logbook delete of one still takes it out of the contest log**, kept as removed.
#[test]
fn a_logbook_delete_after_i_moved_and_a_restart_reaches_the_contest_row() {
    let d = Dir::new("contest-merge-delete");
    let mut e = merged_then_moved_and_restarted(&d);
    assert!(e.delete_qso(copy_of(&e, "W9BBB").id.unwrap()));
    assert_eq!(
        contest_calls(&e),
        ["K9AAA"],
        "the deleted contact left the contest log"
    );
    assert_eq!(
        e.contest_removed()[0].rows[0].call,
        "W9BBB",
        "…kept, restorable"
    );
}

/// The club's rows are named by position and number alone, and a move and a restart change
/// neither: the outbox after the restart is the outbox before it.
#[test]
fn the_club_outbox_is_the_same_after_i_moved_and_a_restart() {
    let d = Dir::new("contest-merge-club");
    let mut e = launch(&d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    assert_eq!(merge(&mut e), (2, 0));
    let wire = |e: &Engine| -> Vec<(String, u64, String)> {
        e.fd_sync_outbox(0)
            .into_iter()
            .map(|w| (w.pos, w.seq, w.call))
            .collect()
    };
    let before = wire(&e);
    assert_eq!(
        before,
        [
            (POS.to_string(), 1, "K9AAA".to_string()),
            (POS.to_string(), 2, "W9BBB".to_string())
        ]
    );
    e.contest_i_moved(vec![("QTH".into(), "WILL".into())])
        .expect("Will is an Illinois county");
    let s = quit(e);
    let e = launch(&d, &s);
    assert_eq!(wire(&e), before);
}

/// The Logbook as an earlier build's merge left it, through the real store: K9AAA and W9BBB
/// merged from Kane under the name that build built from where the operator was —
/// `<contest>:KANE:<position>:<number>`, the shape of the copies origin/main wrote in tempo-core's
/// `contest-merge` fixtures — and W9BBB's copy uploaded to QRZ. Then I moved to Will and a
/// restart, on this build.
fn merged_by_an_earlier_build_then_moved(d: &Dir) -> Engine {
    earlier_build_merged_as(d, &["IL QSO Party:KANE"])
}

/// [`merged_by_an_earlier_build_then_moved`], with the contacts merged once under each session
/// name in `sessions`: two names is the logbook that build doubled (merged from Kane, then again
/// under Will after the restart).
fn earlier_build_merged_as(d: &Dir, sessions: &[&str]) -> Engine {
    let mut e = launch(d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    for session in sessions {
        let copies: Vec<QsoRecord> = {
            let Mode::FieldDay { station, .. } = &e.mode else {
                panic!("the contest runs")
            };
            let mut lb = tempo_core::logbook::Logbook::new();
            tempo_core::contest::merge_into_general(&station.log, POS, &mut lb);
            lb.records()
                .iter()
                .zip(station.log.qsos())
                .map(|(r, q)| {
                    let mut r = QsoRecord::clone(r);
                    r.id = None;
                    let c = r.contest.as_deref_mut().expect("merged");
                    c.session = session.to_string();
                    c.qid = format!("{session}:{POS}:{}", q.seq);
                    r
                })
                .collect()
        };
        e.station.append(copies, false);
    }
    let w9bbb = copy_of(&e, "W9BBB");
    assert!(e.stamp_qrz_upload(&w9bbb, UploadOutcome::Accepted, 1_792_343_000, None));
    e.contest_i_moved(vec![("QTH".into(), "WILL".into())])
        .expect("Will is an Illinois county");
    let s = quit(e);
    launch(d, &s)
}

/// ⭐ **Copies an earlier build merged are recognised through the store**: the merge reads each
/// copy's name AND when its contact was made off the store, and writes neither contact again —
/// only the one that build never merged.
#[test]
fn copies_an_earlier_build_merged_are_not_written_again() {
    let d = Dir::new("contest-merge-earlier-build");
    let mut e = merged_by_an_earlier_build_then_moved(&d);
    assert_eq!(merge(&mut e), (0, 2));
    party(&mut e, "N9CCC", "DUPG");
    assert_eq!(merge(&mut e), (1, 2));
    assert_eq!(calls(&e), ["K9AAA", "W9BBB", "N9CCC"]);
    assert_eq!(
        qids(&e),
        [
            format!("IL QSO Party:KANE:{POS}:1"),
            format!("IL QSO Party:KANE:{POS}:2"),
            qid_now(&e, 3),
        ],
        "the earlier copies keep their names; the new one is named by the contest's running"
    );
}

/// The name this build gives row `seq` of the running contest.
fn qid_now(e: &Engine, seq: u64) -> String {
    let Mode::FieldDay { station, .. } = &e.mode else {
        panic!("the contest runs")
    };
    let q = station
        .log
        .qsos()
        .iter()
        .find(|q| q.seq == seq)
        .expect("the row");
    tempo_core::contest::MergeIdentity::of(&station.log, POS, q)
        .qid()
        .to_string()
}

/// …a Logbook edit of one reaches its contest row, and a Logbook delete of the other takes it
/// out of the contest log.
#[test]
fn an_edit_or_delete_of_an_earlier_builds_copy_reaches_its_contest_row() {
    let d = Dir::new("contest-merge-earlier-edit");
    let mut e = merged_by_an_earlier_build_then_moved(&d);
    let mut fixed = copy_of(&e, "K9AAA");
    fixed.call = "K9AAB".into();
    assert!(e.update_qso(fixed.id.unwrap(), fixed));
    assert_eq!(contest_calls(&e), ["K9AAB", "W9BBB"]);
    assert!(e.delete_qso(copy_of(&e, "W9BBB").id.unwrap()));
    assert_eq!(contest_calls(&e), ["K9AAB"]);
}

/// …and a removal's answer names the services holding an earlier build's copy, and claims no
/// copy for a contact that was never merged — the copies of the rows before it are not its.
#[test]
fn a_removal_names_the_services_holding_an_earlier_builds_copy() {
    let d = Dir::new("contest-merge-earlier-removal");
    let mut e = merged_by_an_earlier_build_then_moved(&d);
    party(&mut e, "N9CCC", "DUPG");
    let newest = |e: &Engine| {
        let fd = e.snapshot().field_day.expect("the contest runs");
        let q = fd.log.last().expect("a contact");
        (q.call.clone(), q.when_unix)
    };
    let shared = Mutex::new(e);
    let (call, when) = newest(&engine_lock(&shared));
    let gone = super::contest_removal::remove_last(&shared, &call, when)
        .unwrap()
        .unwrap();
    assert_eq!(gone.removal.entry.rows[0].call, "N9CCC");
    assert_eq!(gone.logbook, None, "N9CCC was never merged");
    let (call, when) = newest(&engine_lock(&shared));
    let gone = super::contest_removal::remove_last(&shared, &call, when)
        .unwrap()
        .unwrap();
    assert_eq!(gone.removal.entry.rows[0].call, "W9BBB");
    assert_eq!(
        gone.logbook,
        Some(vec!["qrz"]),
        "the earlier build's copy, uploaded to QRZ"
    );
}

/// A merge after the restart that writes nothing queues nothing, with the session's uploads on:
/// a copy already in the Logbook is never sent again.
#[test]
fn a_merge_after_a_restart_queues_nothing_already_merged() {
    let d = Dir::new("contest-merge-uploads");
    let mut e = launch(&d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    e.fd_set_upload(true, vec!["qrz".into()]).unwrap();
    assert_eq!(merge(&mut e), (1, 0));
    assert_eq!(
        e.take_pending_uploads().len(),
        1,
        "CONTROL: the first merge queues it"
    );
    e.contest_i_moved(vec![("QTH".into(), "WILL".into())])
        .expect("Will is an Illinois county");
    let s = quit(e);
    let mut e = launch(&d, &s);
    e.take_pending_uploads();
    e.fd_set_upload(true, vec!["qrz".into()]).unwrap();
    assert_eq!(merge(&mut e), (0, 1));
    assert!(e.take_pending_uploads().is_empty());
}

/// ⭐ **Deleting one of two copies an earlier build wrote tidies the logbook and keeps the contest
/// row**; deleting the last copy takes the contact out of the contest log, as any delete does —
/// through the command every logbook delete goes through.
#[test]
fn deleting_a_duplicate_an_earlier_build_wrote_keeps_the_contest_row() {
    let d = Dir::new("contest-merge-earlier-doubled");
    let e = earlier_build_merged_as(&d, &["IL QSO Party:KANE", "IL QSO Party:WILL"]);
    assert_eq!(
        calls(&e),
        ["K9AAA", "W9BBB", "K9AAA", "W9BBB"],
        "PREMISE: the logbook that build doubled"
    );
    let copy = |e: &Engine, qid: &str| {
        e.stored_log()
            .iter()
            .find(|r| r.contest.as_deref().is_some_and(|c| c.qid == qid))
            .and_then(|r| r.id)
            .expect("the copy")
    };
    let delete = |e: Engine, id| {
        let shared = Mutex::new(e);
        let (made, _) =
            crate::logwrite::change_ops(&shared, id, None, &[LogOp::Delete(id)], "delete");
        assert!(made.is_ok(), "the logbook delete is made: {made:?}");
        shared.into_inner().unwrap()
    };
    let id = copy(&e, &format!("IL QSO Party:WILL:{POS}:2"));
    let e = delete(e, id);
    assert_eq!(calls(&e), ["K9AAA", "W9BBB", "K9AAA"]);
    assert_eq!(
        contest_calls(&e),
        ["K9AAA", "W9BBB"],
        "another copy of W9BBB is in the logbook"
    );
    assert!(e.contest_removed().is_empty());
    // The last copy: the contact leaves the contest log, kept as removed.
    let id = copy(&e, &format!("IL QSO Party:KANE:{POS}:2"));
    let e = delete(e, id);
    assert_eq!(calls(&e), ["K9AAA", "K9AAA"]);
    assert_eq!(contest_calls(&e), ["K9AAA"]);
    assert_eq!(e.contest_removed()[0].rows[0].call, "W9BBB");
}

/// ⭐ **…and at a club, deleting one of the two takes nothing back from the club either**: the
/// host is owed the take-back with the contact's last copy, when it leaves the contest log.
#[test]
fn at_a_club_deleting_a_duplicate_takes_nothing_back_until_the_last_copy() {
    use tempo_net::fdsync::Mark;
    let d = Dir::new("contest-merge-earlier-doubled-club");
    let e = earlier_build_merged_as(&d, &["IL QSO Party:KANE", "IL QSO Party:WILL"]);
    // The same laptop, joining a club now.
    let mut s = quit(e);
    s.fd_join_addr = "192.168.1.20:42073".into();
    let e = launch(&d, &s);
    assert!(e.fd_sync_enabled(), "PREMISE: club sync runs");
    assert_eq!(
        calls(&e),
        ["K9AAA", "W9BBB", "K9AAA", "W9BBB"],
        "PREMISE: the logbook that build doubled"
    );
    assert!(
        e.fd_sync_marks().is_empty(),
        "PREMISE: nothing owed the host: {:?}",
        e.fd_sync_marks()
    );
    let copy = |e: &Engine, qid: &str| {
        e.stored_log()
            .iter()
            .find(|r| r.contest.as_deref().is_some_and(|c| c.qid == qid))
            .and_then(|r| r.id)
            .expect("the copy")
    };
    let delete = |e: Engine, id| {
        let shared = Mutex::new(e);
        let (made, _) =
            crate::logwrite::change_ops(&shared, id, None, &[LogOp::Delete(id)], "delete");
        assert!(made.is_ok(), "the logbook delete is made: {made:?}");
        shared.into_inner().unwrap()
    };
    let id = copy(&e, &format!("IL QSO Party:WILL:{POS}:2"));
    let e = delete(e, id);
    assert_eq!(contest_calls(&e), ["K9AAA", "W9BBB"]);
    assert!(
        e.fd_sync_marks().is_empty(),
        "another copy of W9BBB is in the logbook, so the club keeps it: {:?}",
        e.fd_sync_marks()
    );
    // The last copy: the contact leaves the contest log, and the club is owed its take-back.
    let id = copy(&e, &format!("IL QSO Party:KANE:{POS}:2"));
    let e = delete(e, id);
    assert_eq!(contest_calls(&e), ["K9AAA"]);
    assert_eq!(
        e.fd_sync_marks(),
        [Mark::Retract {
            pos: POS.into(),
            seq: 2
        }]
    );
}

// ---- the exchange each contact was sent with, and its number, across the same ----------

/// What each contest contact sent in slot `key` (its county, its section), in log order.
fn sent(e: &Engine, key: &str) -> Vec<String> {
    let Mode::FieldDay { station, .. } = &e.mode else {
        panic!("the contest runs")
    };
    station
        .log
        .qsos()
        .iter()
        .map(|q| q.sent(key).to_string())
        .collect()
}

/// The exported Cabrillo file's QSO lines: what the sponsor reads each contact from.
fn qso_lines(e: &Engine) -> Vec<String> {
    e.export_log("cabrillo")
        .expect("one entry")
        .lines()
        .filter(|l| l.starts_with("QSO:"))
        .map(str::to_string)
        .collect()
}

/// Two party contacts sent from Kane, and the QSO lines they export as.
fn two_from_kane(d: &Dir) -> (Engine, Vec<String>) {
    let mut e = launch(d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    party(&mut e, "W9BBB", "LAKE");
    assert_eq!(sent(&e, "QTH"), ["KANE", "KANE"], "PREMISE: sent from Kane");
    let lines = qso_lines(&e);
    (e, lines)
}

/// A Settings save with `edit` applied.
fn save(e: &mut Engine, edit: impl FnOnce(&mut Settings)) {
    let mut s = e.settings().clone();
    edit(&mut s);
    e.apply_settings(s);
}

/// ⭐ **A County edited in Settings re-labels no contact already logged**, and the contest
/// entered again. Settings does not move the session that is running (only I moved does), so
/// every row matched it and the journal wrote none of them a sent side; the session built
/// from Settings on the way back in read both contacts back from Will, and the Cabrillo file
/// said so.
#[test]
fn a_county_edited_in_settings_then_the_contest_entered_again_relabels_nothing() {
    let d = Dir::new("contest-journal-county-in");
    let (mut e, lines) = two_from_kane(&d);
    save(&mut e, |s| s.contest_qth_county = "WILL".into());
    let e = switch(e, "chat");
    let mut e = switch(e, "fieldday-sp");
    assert_eq!(
        contest_calls(&e),
        ["K9AAA", "W9BBB"],
        "PREMISE: the rows came back"
    );
    assert_eq!(qso_lines(&e), lines, "the Cabrillo file still says Kane");
    assert_eq!(sent(&e, "QTH"), ["KANE", "KANE"]);
    // A contact logged now is sent from Will, and a restart keeps all three as they were sent.
    party(&mut e, "N9CCC", "DUPG");
    let s = quit(e);
    let e = launch(&d, &s);
    assert_eq!(sent(&e, "QTH"), ["KANE", "KANE", "WILL"]);
}

/// ⭐ **…edited with no contest running**: out of the contest, County changed, back in. The
/// journal was last written on the way out, before the edit, which is why flushing it when
/// Settings change could not cover this.
#[test]
fn a_county_edited_out_of_the_contest_relabels_nothing_when_it_is_entered_again() {
    let d = Dir::new("contest-journal-county-out");
    let (mut e, lines) = two_from_kane(&d);
    save(&mut e, |s| s.fd_active = false);
    assert!(!e.in_field_day(), "PREMISE: out of the contest");
    save(&mut e, |s| s.contest_qth_county = "WILL".into());
    save(&mut e, |s| s.fd_active = true);
    assert!(e.in_field_day(), "PREMISE: back in");
    assert_eq!(qso_lines(&e), lines, "the Cabrillo file still says Kane");
    assert_eq!(sent(&e, "QTH"), ["KANE", "KANE"]);
}

/// ⭐ **…and before a restart.**
#[test]
fn a_county_edited_in_settings_then_a_restart_relabels_nothing() {
    let d = Dir::new("contest-journal-county-restart");
    let (mut e, lines) = two_from_kane(&d);
    save(&mut e, |s| s.contest_qth_county = "WILL".into());
    let s = quit(e);
    let e = launch(&d, &s);
    assert_eq!(qso_lines(&e), lines, "the Cabrillo file still says Kane");
    assert_eq!(sent(&e, "QTH"), ["KANE", "KANE"]);
}

/// ⭐ **Field Day's section, the same way**: changed in Settings, then a restart.
#[test]
fn a_section_edited_in_settings_relabels_no_field_day_contact() {
    let d = Dir::new("contest-journal-section");
    let mut e = launch(&d, &field_day_settings());
    for call in ["K1ABC", "W1AW"] {
        assert!(
            e.fd_log_manual(call, "2A", "EMA", "CW").unwrap(),
            "{call} logs"
        );
    }
    let lines = qso_lines(&e);
    save(&mut e, |s| s.fd_section = "IL".into());
    let s = quit(e);
    let e = launch(&d, &s);
    assert_eq!(qso_lines(&e), lines, "the Cabrillo file still says WI");
    assert_eq!(sent(&e, "SECTION"), ["WI", "WI"]);
}

/// The control: **I moved keeps each contact's county**, across a restart, as it already did.
#[test]
fn i_moved_keeps_each_contacts_county_across_a_restart() {
    let d = Dir::new("contest-journal-i-moved");
    let mut e = launch(&d, &ilqp_settings());
    party(&mut e, "K9AAA", "COOK");
    e.contest_i_moved(vec![("QTH".into(), "WILL".into())])
        .expect("Will is an Illinois county");
    party(&mut e, "W9BBB", "LAKE");
    assert_eq!(sent(&e, "QTH"), ["KANE", "WILL"]);
    let lines = qso_lines(&e);
    let s = quit(e);
    let e = launch(&d, &s);
    assert_eq!(sent(&e, "QTH"), ["KANE", "WILL"]);
    assert_eq!(qso_lines(&e), lines);
}

/// ⭐ **A contact logged five days after the first ones is merged, once.** The journal restore
/// reached back four days, and the contact numbers went with the rows it let go: five days on,
/// the next contact was numbered 1 again, its name was the first contact's, and the merge
/// counted it already in the Logbook and wrote nothing.
///
/// Updated to the restore that loads the running's own rows whatever their age: its premise was
/// that five days on neither of the first two was restored, and now both are in the session's log
/// (and in the Logbook already), so the new contact numbers on from them.
///
/// The first two are made five days back by the clock, in the running the contest is in now:
/// the Illinois QSO Party, or Field Day in the five days after the party's running begins
/// (five days back is then the party's previous running, whose names this one never shares).
#[test]
fn a_contact_logged_five_days_on_is_merged_once() {
    let now = now_unix_secs();
    let then = now - 5 * 86_400;
    let ilqp =
        tempo_core::fd_rules::ruleset_by_id("ilqp", tempo_core::fd_rules::CURRENT_RULES_YEAR)
            .expect("a shipped ruleset");
    let (s, rx) = if ilqp.running_of(then) == ilqp.running_of(now + 3_600) {
        (ilqp_settings(), [("RST", "599"), ("QTH", "COOK")])
    } else {
        (field_day_settings(), [("CLASS", "2A"), ("SECTION", "EMA")])
    };
    let rx = rx.map(|(k, v)| (k.to_string(), v.to_string()));
    let d = Dir::new("contest-merge-five-days-on");

    // Five days back: two contacts, merged, and the journal they were written to.
    let mut e = launch(&d, &s);
    let mut first = {
        let Mode::FieldDay { station, .. } = &e.mode else {
            panic!("the contest runs")
        };
        tempo_core::fieldday::FieldDayLog::new(
            "W9XYZ",
            station.log.session.clone(),
            &station.log.band,
        )
    };
    assert!(first.log_fields_at("K9AAA", &rx, "CW", "", 0, then));
    assert!(first.log_fields_at("W9BBB", &rx, "CW", "", 0, then + 60));
    assert_eq!(
        first.ruleset().running_of(then),
        first.ruleset().running_of(now + 3_600),
        "PREMISE: one running"
    );
    let mut lb = tempo_core::logbook::Logbook::new();
    tempo_core::contest::merge_into_general(&first, POS, &mut lb);
    let copies: Vec<QsoRecord> = lb
        .records()
        .iter()
        .map(|r| {
            let mut r = QsoRecord::clone(r);
            r.id = None;
            r
        })
        .collect();
    e.station.append(copies, false);
    std::fs::write(d.0.join("fieldday.adi"), first.journal_adif()).expect("the journal");
    let s = quit(e);

    // Today: both are past the journal's four days and in the session's log, and one new
    // contact is merged, once.
    let mut e = launch(&d, &s);
    assert_eq!(
        contest_calls(&e),
        ["K9AAA", "W9BBB"],
        "the session's own, five days on"
    );
    assert!(e.contest_log_manual("N9CCC", &rx, "CW", None).unwrap());
    assert_eq!(merge(&mut e), (1, 2), "the new contact is merged");
    assert_eq!(merge(&mut e), (0, 3), "once");
    assert_eq!(calls(&e), ["K9AAA", "W9BBB", "N9CCC"]);
}

// ---- a contact never merged, past the journal's four days ------------------------------

/// The call of every record in the contest journal on disk, in its order.
fn journal_calls(d: &Dir) -> Vec<String> {
    let text = std::fs::read_to_string(d.0.join("fieldday.adi")).expect("the journal");
    let body = &text[text.find("<EOH>").map_or(0, |i| i + 5)..];
    body.split("<CALL:")
        .skip(1)
        .map(|r| {
            let (len, rest) = r.split_once('>').expect("a CALL tag");
            rest[..len.parse::<usize>().expect("its length")].to_string()
        })
        .collect()
}

/// Two contacts of the Illinois QSO Party's previous running, logged 400 days back by the clock
/// (past the journal's window, and another running's whatever the date), journaled and never
/// merged — the journal on disk as the session that logged them left it. Their log, and when the
/// first was logged.
fn never_merged_last_running(d: &Dir, e: &Engine) -> (tempo_core::fieldday::FieldDayLog, u64) {
    let then = now_unix_secs() - 400 * 86_400;
    let Mode::FieldDay { station, .. } = &e.mode else {
        panic!("the contest runs")
    };
    let mut first = tempo_core::fieldday::FieldDayLog::new(
        "W9XYZ",
        station.log.session.clone(),
        &station.log.band,
    );
    let rx = [("RST", "599"), ("QTH", "COOK")].map(|(k, v)| (k.to_string(), v.to_string()));
    assert!(first.log_fields_at("K9AAA", &rx, "CW", "", 0, then));
    assert!(first.log_fields_at("W9BBB", &rx, "CW", "", 0, then + 60));
    assert_ne!(
        first.ruleset().running_of(then),
        first.ruleset().running_of(now_unix_secs()),
        "PREMISE: another running"
    );
    std::fs::write(d.0.join("fieldday.adi"), first.journal_adif()).expect("the journal");
    (first, then)
}

/// ⭐ **A contact never merged stays in the contest journal when the next one is logged, a running
/// on.** The restore reached back four days, and the next contact wrote the journal again from
/// what the restore had loaded, so a tester who forgot Merge and logged again five days later lost
/// those contacts from Nexus entirely. The running's own are in its log now, whatever their age;
/// another running's are kept in the journal.
#[test]
fn a_contact_never_merged_stays_in_the_journal_when_the_next_running_logs_one() {
    let d = Dir::new("contest-unmerged-next-running");
    let e = launch(&d, &ilqp_settings());
    never_merged_last_running(&d, &e);
    let s = quit(e);

    let mut e = launch(&d, &s);
    assert!(contest_calls(&e).is_empty(), "PREMISE: another running's");
    party(&mut e, "N9CCC", "DUPG");
    quit(e);
    assert_eq!(journal_calls(&d), ["N9CCC", "K9AAA", "W9BBB"]);
}

/// The contest screen asking what the journal keeps that the Logbook does not hold
/// ([`super::contest_unmerged::unmerged`]).
fn offered(e: Engine) -> (Engine, Vec<tempo_core::contest::Unmerged>) {
    let shared = Mutex::new(e);
    let offer = super::contest_unmerged::unmerged(&shared).expect("the contest runs");
    (shared.into_inner().unwrap(), offer)
}

/// Its Merge for `event`, as `(written, already there)`, waited for to disk as the command waits.
fn merge_offered(e: Engine, event: &str) -> (Engine, (usize, usize)) {
    let shared = Mutex::new(e);
    let (report, _) = super::contest_unmerged::merge_unmerged(&shared, event);
    let report = report.expect("the contest runs");
    let e = shared.into_inner().unwrap();
    flush(&e);
    (e, (report.added(), report.already))
}

/// ⭐ **The contest screen offers the contacts never merged — how many, from which contest and
/// when — and one press merges them, once.** Through the engine, the real logbook store and the
/// real journal: the merge writes each once, the end-of-contest merge still merges the session's
/// own contact once, and once their copies are on disk the two leave the journal as merged
/// contacts always did. Nothing already in the Logbook is changed.
#[test]
fn a_contact_never_merged_is_offered_and_one_press_merges_it_once() {
    let d = Dir::new("contest-unmerged-offered");
    let e = launch(&d, &ilqp_settings());
    let (_, then) = never_merged_last_running(&d, &e);
    let s = quit(e);

    let mut e = launch(&d, &s);
    party(&mut e, "N9CCC", "DUPG");
    assert_eq!(
        merge(&mut e),
        (1, 0),
        "the session's own contact, merged first"
    );
    let before: Vec<QsoRecord> = e.stored_log().iter().map(|r| QsoRecord::clone(r)).collect();
    let (e, offer) = offered(e);
    assert_eq!(
        offer,
        [tempo_core::contest::Unmerged {
            event: "ilqp",
            count: 2,
            first_unix: then,
            last_unix: then + 60,
        }]
    );
    let (e, merged) = merge_offered(e, "ilqp");
    assert_eq!(merged, (2, 0));
    assert_eq!(calls(&e), ["N9CCC", "K9AAA", "W9BBB"]);
    let after = e.stored_log();
    assert_eq!(
        after[..before.len()]
            .iter()
            .map(|r| QsoRecord::clone(r))
            .collect::<Vec<_>>(),
        before,
        "nothing already in the Logbook changed"
    );

    let (e, offer) = offered(e);
    assert!(offer.is_empty(), "nothing left to offer: {offer:?}");
    let (mut e, again) = merge_offered(e, "ilqp");
    assert_eq!(again, (0, 0), "a second press writes nothing");
    assert_eq!(merge(&mut e), (0, 1), "nor does the end-of-contest merge");
    assert_eq!(calls(&e), ["N9CCC", "K9AAA", "W9BBB"]);

    // Once the station has the writer's word that their copies are on disk (the next launch at
    // the latest), the Logbook asked lets them go, and the next contact writes the journal
    // without them.
    party(&mut e, "N9DDD", "WILL");
    let s = quit(e);
    let (mut e, offer) = offered(launch(&d, &s));
    assert!(offer.is_empty(), "{offer:?}");
    party(&mut e, "N9EEE", "COOK");
    quit(e);
    assert_eq!(journal_calls(&d), ["N9CCC", "N9DDD", "N9EEE"]);
}

/// The control: **a contact merged before it aged out still leaves the journal**, once the
/// contest screen has asked the Logbook and its running is over — as every merged contact past
/// the journal's four days left it before. Until then it is kept, which loses nothing.
#[test]
fn a_contact_merged_before_it_aged_out_still_leaves_the_journal() {
    let d = Dir::new("contest-unmerged-merged-before");
    let mut e = launch(&d, &ilqp_settings());
    let (first, _) = never_merged_last_running(&d, &e);
    let mut lb = tempo_core::logbook::Logbook::new();
    tempo_core::contest::merge_into_general(&first, POS, &mut lb);
    let copies: Vec<QsoRecord> = lb
        .records()
        .iter()
        .map(|r| {
            let mut r = QsoRecord::clone(r);
            r.id = None;
            r
        })
        .collect();
    e.station.append(copies, false);
    let s = quit(e);

    let mut e = launch(&d, &s);
    party(&mut e, "N9CCC", "DUPG");
    let e = {
        let s = quit(e);
        assert_eq!(
            journal_calls(&d),
            ["N9CCC", "K9AAA", "W9BBB"],
            "kept until the Logbook is asked"
        );
        launch(&d, &s)
    };
    let (mut e, offer) = offered(e);
    assert!(offer.is_empty(), "both are in the Logbook: {offer:?}");
    party(&mut e, "N9DDD", "WILL");
    quit(e);
    assert_eq!(journal_calls(&d), ["N9CCC", "N9DDD"]);
}

/// [`offered`] and [`merge_offered`] with `change` made to the Logbook after the Logbook is read
/// and before what was read is used: another window's change, or the operator's, made as a
/// command makes it, with the Engine lock released.
fn racing<T>(
    e: Engine,
    change: impl FnOnce(&Mutex<Engine>) + 'static,
    ask: impl FnOnce(&Mutex<Engine>) -> T,
) -> (Engine, T) {
    let shared = std::sync::Arc::new(Mutex::new(e));
    let theirs = std::sync::Arc::clone(&shared);
    super::contest_unmerged::RACE
        .with(|r| *r.borrow_mut() = Some(Box::new(move || change(&theirs))));
    let out = ask(&shared);
    assert!(
        super::contest_unmerged::RACE.with(|r| r.borrow().is_none()),
        "PREMISE: the change was made"
    );
    let e = std::sync::Arc::try_unwrap(shared)
        .ok()
        .expect("the one engine")
        .into_inner()
        .unwrap();
    (e, out)
}

/// The two never merged, then merged from the offer and on disk after a relaunch.
fn merged_from_the_offer(d: &Dir) -> Engine {
    let e = launch(d, &ilqp_settings());
    never_merged_last_running(d, &e);
    let s = quit(e);
    let (e, _) = offered(launch(d, &s));
    let (e, merged) = merge_offered(e, "ilqp");
    assert_eq!(merged, (2, 0), "PREMISE: merged");
    launch(d, &quit(e))
}

/// ⭐ **A copy deleted from the Logbook while the contest screen asks lets no contact go.** The
/// Logbook is read, then the operator deletes one copy: the answer is made again on the Logbook
/// as it now is, so that contact stays in the journal, and is offered again.
#[test]
fn a_copy_deleted_while_the_logbook_is_asked_lets_no_contact_go() {
    let d = Dir::new("contest-unmerged-race-delete");
    let e = merged_from_the_offer(&d);
    let id = copy_of(&e, "K9AAA").id.expect("stored");
    let (mut e, offer) = racing(
        e,
        move |shared| {
            let (made, _) =
                crate::logwrite::change_ops(shared, id, None, &[LogOp::Delete(id)], "delete");
            assert!(matches!(made, Ok(Ok(_))), "the delete: {made:?}");
        },
        |shared| super::contest_unmerged::unmerged(shared).expect("the contest runs"),
    );
    assert_eq!(offer.iter().map(|u| u.count).sum::<usize>(), 1, "{offer:?}");
    party(&mut e, "N9CCC", "DUPG");
    quit(e);
    assert_eq!(
        journal_calls(&d),
        ["N9CCC", "K9AAA"],
        "W9BBB let go, K9AAA kept"
    );
}

/// ⭐ **…and a copy whose delete is still on its way to disk is not a copy on disk.** The store
/// still holds the row while the writer is held up, and the contact it named stays.
#[test]
fn a_copy_whose_delete_is_on_its_way_lets_no_contact_go() {
    let d = Dir::new("contest-unmerged-delete-held");
    let mut e = merged_from_the_offer(&d);
    let hold = tempo_core::logbook::sqlite::WriteHold::take(&d.db()).expect("stall the writer");
    assert!(e.delete_qso(copy_of(&e, "K9AAA").id.expect("stored")));
    assert!(
        crate::logstore::tests::stored(&d)
            .iter()
            .any(|r| r.call == "K9AAA"),
        "PREMISE: the store still holds it"
    );
    let (mut e, offer) = offered(e);
    assert_eq!(offer.iter().map(|u| u.count).sum::<usize>(), 1, "{offer:?}");
    drop(hold);
    party(&mut e, "N9CCC", "DUPG");
    quit(e);
    assert_eq!(
        journal_calls(&d),
        ["N9CCC", "K9AAA"],
        "W9BBB let go, K9AAA kept"
    );
}

/// ⭐ **A contact another window merges while this one's merge is planned is written once.** The
/// merge is planned again on the Logbook as it now is, and writes only what is still missing.
#[test]
fn a_contact_merged_elsewhere_while_the_merge_is_planned_is_written_once() {
    let d = Dir::new("contest-unmerged-race-merge");
    let e = launch(&d, &ilqp_settings());
    let (first, _) = never_merged_last_running(&d, &e);
    let s = quit(e);
    let (e, offer) = offered(launch(&d, &s));
    assert_eq!(offer.iter().map(|u| u.count).sum::<usize>(), 2, "PREMISE");
    let mut lb = tempo_core::logbook::Logbook::new();
    tempo_core::contest::merge_into_general(&first, POS, &mut lb);
    let elsewhere: Vec<QsoRecord> = lb
        .records()
        .iter()
        .filter(|r| r.call == "K9AAA")
        .map(|r| {
            let mut r = QsoRecord::clone(r);
            r.id = None;
            r
        })
        .collect();
    let (e, (report, _)) = racing(
        e,
        move |shared| {
            engine_lock(shared).station.append(elsewhere, false);
        },
        |shared| super::contest_unmerged::merge_unmerged(shared, "ilqp"),
    );
    let report = report.expect("the contest runs");
    assert_eq!((report.added(), report.already), (1, 0));
    assert_eq!(calls(&e), ["K9AAA", "W9BBB"], "each once");
}

/// ⭐ **A copy still on its way to disk lets no contact go.** With the writer held up, the copies
/// the offer's merge wrote are in no file yet: the contacts are offered no more, and the journal
/// keeps them until their copies are on disk.
#[test]
fn a_copy_still_on_its_way_to_disk_lets_no_contact_go() {
    let d = Dir::new("contest-unmerged-on-its-way");
    let e = launch(&d, &ilqp_settings());
    never_merged_last_running(&d, &e);
    let s = quit(e);
    let (mut e, _) = offered(launch(&d, &s));
    party(&mut e, "N9CCC", "DUPG");
    let hold = tempo_core::logbook::sqlite::WriteHold::take(&d.db()).expect("stall the writer");
    let shared = Mutex::new(e);
    let (report, _) = super::contest_unmerged::merge_unmerged(&shared, "ilqp");
    assert_eq!(report.expect("the contest runs").added(), 2);
    let offer = super::contest_unmerged::unmerged(&shared).expect("the contest runs");
    assert!(offer.is_empty(), "{offer:?}");
    let e = shared.into_inner().unwrap();
    let journal = e.field_day_log_adif().expect("the contest log");
    assert!(
        journal.contains("K9AAA") && journal.contains("W9BBB"),
        "kept until their copies are on disk"
    );
    drop(hold);
    quit(e);
}

/// **This contest's contacts are merged in the session that is running**, as the restore read its
/// own rows, whatever Settings have said since: a County cleared in Settings, which would stop the
/// contest being entered again, does not stop the merge.
#[test]
fn the_running_contests_contacts_are_merged_whatever_settings_say_since() {
    let d = Dir::new("contest-unmerged-settings-since");
    let e = launch(&d, &ilqp_settings());
    never_merged_last_running(&d, &e);
    let s = quit(e);
    let (mut e, _) = offered(launch(&d, &s));
    save(&mut e, |s| s.contest_qth_county = String::new());
    assert!(e.in_field_day(), "PREMISE: the session runs on");
    let (e, merged) = merge_offered(e, "ilqp");
    assert_eq!(merged, (2, 0));
    assert_eq!(calls(&e), ["K9AAA", "W9BBB"]);
}

// ---- the contest's log holds the whole contest, whatever its age -------------------------

/// A contest whose running holds the last thirty days and the next hour, by the clock: the
/// Illinois QSO Party, or Field Day while the party's running is under thirty days old (the two
/// runnings begin four months apart). Its settings, and what a station of it is received with.
fn a_running_thirty_days_long() -> (Settings, [(String, String); 2]) {
    let now = now_unix_secs();
    let ilqp =
        tempo_core::fd_rules::ruleset_by_id("ilqp", tempo_core::fd_rules::CURRENT_RULES_YEAR)
            .expect("a shipped ruleset");
    let (s, rx) = if ilqp.running_of(now - 30 * 86_400) == ilqp.running_of(now + 3_600) {
        (ilqp_settings(), [("RST", "599"), ("QTH", "COOK")])
    } else {
        (field_day_settings(), [("CLASS", "2A"), ("SECTION", "EMA")])
    };
    (s, rx.map(|(k, v)| (k.to_string(), v.to_string())))
}

/// ⭐ **A contest's contacts logged five, ten and thirty days back are all in its log after a
/// restart**: the Contest screen's rows, count and score, the Cabrillo and ADIF exports, and the
/// dupe check, as the session that logged them left them. The restore loaded only the journal's
/// last four days, so the contest's own export a week on held none of them. The restore writes
/// nothing to the Logbook.
#[test]
fn a_contests_contacts_five_ten_and_thirty_days_back_are_all_in_its_log_after_a_restart() {
    let (s, rx) = a_running_thirty_days_long();
    let now = now_unix_secs();
    let d = Dir::new("contest-log-whole-running");
    let e = launch(&d, &s);
    assert!(
        contest_calls(&e).is_empty(),
        "CONTROL: a fresh contest starts empty"
    );
    let mut first = {
        let Mode::FieldDay { station, .. } = &e.mode else {
            panic!("the contest runs")
        };
        tempo_core::fieldday::FieldDayLog::new(
            "W9XYZ",
            station.log.session.clone(),
            &station.log.band,
        )
    };
    for (call, days) in [("K9AAA", 30), ("W9BBB", 10), ("N9CCC", 5)] {
        assert!(first.log_fields_at(call, &rx, "CW", "", 0, now - days * 86_400));
    }
    assert_eq!(
        first.ruleset().running_of(now - 30 * 86_400),
        first.ruleset().running_of(now + 3_600),
        "PREMISE: one running"
    );
    std::fs::write(d.0.join("fieldday.adi"), first.journal_adif()).expect("the journal");
    let s = quit(e);

    let mut e = launch(&d, &s);
    let fd = e.snapshot().field_day.expect("the contest runs");
    assert_eq!(
        fd.log.iter().map(|q| q.call.as_str()).collect::<Vec<_>>(),
        ["K9AAA", "W9BBB", "N9CCC"],
        "the Contest screen's rows"
    );
    assert_eq!(fd.qso_count, 3);
    let (points, _, mults, scored) = first
        .ruleset()
        .scoring
        .score(first.score_rows(), s.fd_power_mult);
    assert!(points > 0, "PREMISE: the contacts score");
    assert_eq!(
        (fd.points, fd.mult_count, fd.powered_points),
        (points, mults, scored),
        "the score"
    );
    let dial = (s.dial_mhz * 1000.0).round() as u32;
    let lines: Vec<String> = first
        .cabrillo(dial)
        .expect("one entry")
        .lines()
        .filter(|l| l.starts_with("QSO:"))
        .map(str::to_string)
        .collect();
    assert_eq!(lines.len(), 3, "PREMISE: three QSO lines");
    assert_eq!(qso_lines(&e), lines, "the Cabrillo export");
    assert_eq!(
        e.export_log("adif"),
        Ok(first.submission_adif()),
        "the ADIF export"
    );
    assert!(
        calls(&e).is_empty(),
        "the restore wrote nothing to the Logbook"
    );
    assert_eq!(
        e.contest_log_manual("K9AAA", &rx, "CW", None),
        Ok(true),
        "K9AAA worked again thirty days on is logged…"
    );
    let Mode::FieldDay { station, .. } = &e.mode else {
        panic!("the contest runs")
    };
    assert!(
        station.log.qsos().last().is_some_and(|q| q.dupe),
        "…marked a dupe: the dupe check holds the whole running"
    );
}

/// The New York QSO Party, worked from Albany County: a contest other than either one
/// [`a_running_thirty_days_long`] picks.
fn nyqp_settings() -> Settings {
    let mut s = ilqp_settings();
    s.fd_event = "nyqp".into();
    s.contest_qth_state = "NY".into();
    s.contest_qth_county = "ALB".into();
    s
}

/// ⭐ **Another contest's Contest screen lets none of a contest's contacts go while its running
/// lasts.** The contest's two contacts five days back, merged; the New York QSO Party opened, its
/// screen asking the Logbook and a contact logged; then the contest again: its log and its
/// Cabrillo file hold both. The other contest's screen let the merged ones go, and its journal
/// was written without them, so the contest's own file exported after that held none of them.
#[test]
fn another_contests_screen_lets_none_of_a_contests_contacts_go_while_its_running_lasts() {
    let (s, rx) = a_running_thirty_days_long();
    let then = now_unix_secs() - 5 * 86_400;
    let d = Dir::new("contest-log-another-contest-between");
    let mut e = launch(&d, &s);
    let mut first = {
        let Mode::FieldDay { station, .. } = &e.mode else {
            panic!("the contest runs")
        };
        tempo_core::fieldday::FieldDayLog::new(
            "W9XYZ",
            station.log.session.clone(),
            &station.log.band,
        )
    };
    assert!(first.log_fields_at("K9AAA", &rx, "CW", "", 0, then));
    assert!(first.log_fields_at("W9BBB", &rx, "CW", "", 0, then + 60));
    let mut lb = tempo_core::logbook::Logbook::new();
    tempo_core::contest::merge_into_general(&first, POS, &mut lb);
    let copies: Vec<QsoRecord> = lb
        .records()
        .iter()
        .map(|r| {
            let mut r = QsoRecord::clone(r);
            r.id = None;
            r
        })
        .collect();
    e.station.append(copies, false);
    std::fs::write(d.0.join("fieldday.adi"), first.journal_adif()).expect("the journal");
    let s = quit(e);

    let (mut e, offer) = offered(launch(&d, &nyqp_settings()));
    assert!(offer.is_empty(), "PREMISE: both in the Logbook: {offer:?}");
    party(&mut e, "K2AAA", "SUF");
    quit(e);
    assert_eq!(
        journal_calls(&d),
        ["K2AAA", "K9AAA", "W9BBB"],
        "kept while the contest's running lasts"
    );

    let e = launch(&d, &s);
    assert_eq!(contest_calls(&e), ["K9AAA", "W9BBB"], "the contest's log");
    let dial = (s.dial_mhz * 1000.0).round() as u32;
    let lines: Vec<String> = first
        .cabrillo(dial)
        .expect("one entry")
        .lines()
        .filter(|l| l.starts_with("QSO:"))
        .map(str::to_string)
        .collect();
    assert_eq!(qso_lines(&e), lines, "its Cabrillo file");
}

/// **Each contest contact keeps the radio and the power it was made with, through the
/// journal and a restart, into the Logbook** — with the contact's time as its end time. The
/// operator changed radio and power between the two contacts; each merged copy names its own.
#[test]
fn a_contest_contact_reaches_the_logbook_with_its_own_radio_power_and_end_time() {
    let d = Dir::new("contest-merge-kit");
    let mut s = field_day_settings();
    s.rig_model_name = "Icom IC-7300".into();
    s.station_power_w = Some(100.0);
    let mut e = launch(&d, &s);
    assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
    save(&mut e, |s| {
        s.rig_model_name = "Elecraft KX3".into();
        s.station_power_w = Some(5.0);
    });
    assert!(e.fd_log_manual("W1AW", "1D", "CT", "CW").unwrap());
    let s = quit(e);

    let mut e = launch(&d, &s);
    assert_eq!(
        contest_calls(&e),
        ["K1ABC", "W1AW"],
        "PREMISE: both came back"
    );
    assert_eq!(merge(&mut e), (2, 0));
    for (call, rig, watts) in [
        ("K1ABC", "Icom IC-7300", 100.0),
        ("W1AW", "Elecraft KX3", 5.0),
    ] {
        let r = copy_of(&e, call);
        assert_eq!(r.my_rig.as_deref(), Some(rig), "{call}");
        assert_eq!(r.tx_power, Some(watts), "{call}");
        assert_eq!(r.time_off_unix, Some(r.when_unix), "{call}");
        assert_eq!(r.operator.as_deref(), Some("W9XYZ"), "{call}: nobody named");
    }
}

/// **A contest contact carries the power the radio measured on its over**, ahead of Settings ▸
/// Station power, into the Logbook: the operator turned the radio down between the two contacts
/// and never touched Settings.
#[test]
fn a_contest_contact_reaches_the_logbook_with_the_power_the_radio_measured() {
    let d = Dir::new("contest-merge-measured-power");
    let mut s = field_day_settings();
    s.station_power_w = Some(100.0);
    let mut e = launch(&d, &s);
    let over = |e: &mut Engine, w: f32| {
        e.observe_rig_tx_meters(None, None, Some(w), None);
        e.clear_rig_tx_meters();
    };
    over(&mut e, 40.0);
    assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
    over(&mut e, 5.0);
    assert!(e.fd_log_manual("W1AW", "1D", "CT", "CW").unwrap());
    let s = quit(e);

    let mut e = launch(&d, &s);
    assert_eq!(merge(&mut e), (2, 0));
    assert_eq!(copy_of(&e, "K1ABC").tx_power, Some(40.0));
    assert_eq!(copy_of(&e, "W1AW").tx_power, Some(5.0));
}

/// **Each contest contact keeps the grid it was made from, through the journal and a restart,
/// into the Logbook — marked as Nexus's own, so the LoTW upload leaves it out**, as an ordinary
/// contact's Settings grid is. The operator moved between the two contacts; each merged copy
/// names its own grid, every service is sent it, and the LoTW batch carries neither.
#[test]
fn a_contest_contact_reaches_the_logbook_with_its_grid_which_lotw_never_sees() {
    let d = Dir::new("contest-merge-grid");
    let mut s = field_day_settings();
    s.mygrid = "en52".into();
    let mut e = launch(&d, &s);
    assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
    save(&mut e, |s| s.mygrid = "EN61ab".into());
    assert!(e.fd_log_manual("W1AW", "1D", "CT", "CW").unwrap());
    let s = quit(e);

    let mut e = launch(&d, &s);
    assert_eq!(
        contest_calls(&e),
        ["K1ABC", "W1AW"],
        "PREMISE: both came back"
    );
    assert_eq!(merge(&mut e), (2, 0));
    let copies = [copy_of(&e, "K1ABC"), copy_of(&e, "W1AW")];
    let lotw = crate::station::lotw_batch_adif(&copies, false, "W9XYZ", "EN52");
    assert!(
        lotw.contains("<CALL:5>K1ABC") && lotw.contains("<CALL:4>W1AW"),
        "the control: both contacts are in the batch: {lotw}"
    );
    assert!(!lotw.contains("MY_GRIDSQUARE"), "{lotw}");
    for (r, grid) in copies.iter().zip(["EN52", "EN61AB"]) {
        assert_eq!(r.my_grid.as_deref(), Some(grid), "{}", r.call);
        assert_eq!(tempo_core::logbook::self_filled(r), ["MY_GRIDSQUARE"]);
        let sent = tempo_core::logbook::adif_record(r);
        assert!(
            sent.contains(&format!("<MY_GRIDSQUARE:{}>{grid}", grid.len())),
            "{sent}"
        );
    }
}
