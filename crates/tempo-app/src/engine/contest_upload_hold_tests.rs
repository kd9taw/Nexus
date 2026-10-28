//! ⭐ **While contest mode is on, no contact goes to the logging services live.** A contest's
//! contacts reach QRZ, ClubLog, eQSL, HRDLog, Cloudlog and WRL when the contest log is merged
//! into the Logbook at the end, under the session's own upload choice, and LoTW's automatic
//! upload waits for the contest to end. A contact that reaches the Logbook another way while the
//! switch is on (the everyday log form, the Logbook's own add, a log form whose screen lost the
//! contest) is held, and sent once, paced, when contest mode is turned off.
//!
//! A CW contest operator was answered 403 by QRZ on every contact while working a contest, and
//! never outside one: the contacts were going out live, one per QSO, while the contest ran.

use super::*;
use crate::test_util::StoredLog;

/// ARRL Field Day, 3A from Wisconsin, entered in S&P with nothing queued.
fn contest_on() -> Engine {
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp")
        .expect("Wisconsin is a current section");
    assert!(
        e.take_pending_uploads().is_empty(),
        "premise: nothing queued"
    );
    e
}

/// A CW contact as a log form sends it.
fn cw(call: &str) -> QsoRecord {
    let adif = format!(
        "<CALL:{}>{call}<BAND:3>20m<MODE:2>CW<FREQ:6>14.025<QSO_DATE:8>20261017\
         <TIME_ON:6>120000<RST_SENT:3>599<RST_RCVD:3>599<EOR>",
        call.len()
    );
    tempo_core::logbook::parse_adif(&adif)
        .pop()
        .expect("one record")
}

fn contest_off(e: &mut Engine) {
    let mut s = e.settings().clone();
    s.fd_active = false;
    e.apply_settings(s);
}

fn calls(pending: &[PendingUpload]) -> Vec<&str> {
    pending.iter().map(|p| p.rec.call.as_str()).collect()
}

/// The contest strip's CW contact goes to the contest journal and nowhere else; the everyday
/// log form's contact, logged while the switch is on, reaches the Logbook and is not sent.
#[test]
fn a_cw_contact_in_contest_mode_is_journalled_and_nothing_is_sent_live() {
    let dir = std::env::temp_dir().join(format!(
        "nexus-contest-hold-{}-{}",
        std::process::id(),
        now_unix_secs()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let journal = dir.join("fieldday.adi");
    let mut e = contest_on();
    e.set_fd_log_path(journal.clone());

    assert!(e.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
    e.journal_mark().wait();
    let text = std::fs::read_to_string(&journal).expect("the journal was written");
    assert!(
        text.contains("<CALL:5>K1ABC"),
        "the contest contact is journalled"
    );
    assert_eq!(calls(&e.take_pending_uploads()), Vec::<&str>::new());

    e.log_form_qso(cw("W1AW"), StateSource::Operator, Vec::new());
    assert_eq!(
        calls(&e.take_pending_uploads()),
        Vec::<&str>::new(),
        "a contact logged with contest mode on is not sent live"
    );
    let logged: Vec<String> = e.stored_log().iter().map(|r| r.call.clone()).collect();
    assert_eq!(logged, ["W1AW"], "it is in the Logbook all the same");
    let _ = std::fs::remove_dir_all(&dir);
}

/// The switch is what counts, not whether the screen still holds the contest: a station that
/// visited Chat with contest mode on has left the contest's session, and its CW log form logs
/// through the Logbook. That contact is not sent live either.
#[test]
fn with_the_switch_on_a_contact_logged_outside_the_contest_screen_is_not_sent_live() {
    let mut e = contest_on();
    e.set_mode("chat").unwrap();
    assert!(
        e.snapshot().field_day.is_none(),
        "premise: the contest screen is gone"
    );
    assert!(e.settings().fd_active, "premise: the switch is still on");

    e.log_qso(cw("K1ABC"));
    assert_eq!(calls(&e.take_pending_uploads()), Vec::<&str>::new());
}

/// Turning contest mode off sends what it held, once, as catch-up (paced, one slot apart); a
/// second save with it off sends nothing again, and a contact logged after goes out live.
#[test]
fn turning_contest_mode_off_sends_the_held_contacts_once_and_paced() {
    let mut e = contest_on();
    e.log_qso(cw("K1ABC"));
    e.log_qso(cw("W1AW"));
    assert!(e.take_pending_uploads().is_empty(), "premise: held");

    // A save with the switch still on sends nothing.
    let s = e.settings().clone();
    e.apply_settings(s);
    assert_eq!(calls(&e.take_pending_uploads()), Vec::<&str>::new());

    contest_off(&mut e);
    let sent = e.take_pending_uploads();
    assert_eq!(calls(&sent), ["K1ABC", "W1AW"]);
    for p in &sent {
        assert_eq!(
            p.origin,
            UploadOrigin::CatchUp,
            "{} is history, paced",
            p.rec.call
        );
        assert_eq!(
            p.legs,
            upload_legs::ALL,
            "{} owes every service",
            p.rec.call
        );
        assert_eq!(p.attempts, 0);
    }
    assert_eq!(
        sent[1].retry_after_unix - sent[0].retry_after_unix,
        CATCHUP_UPLOAD_SPACING_SECS,
        "one slot apart"
    );

    contest_off(&mut e);
    assert_eq!(
        calls(&e.take_pending_uploads()),
        Vec::<&str>::new(),
        "sent once"
    );
    // …and the next contest's end does not send them again.
    let mut s = e.settings().clone();
    s.fd_active = true;
    e.apply_settings(s);
    contest_off(&mut e);
    assert_eq!(
        calls(&e.take_pending_uploads()),
        Vec::<&str>::new(),
        "sent once, across contests"
    );

    e.log_qso(cw("N0CALL"));
    let live = e.take_pending_uploads();
    assert_eq!(
        calls(&live),
        ["N0CALL"],
        "with the switch off a contact goes out live"
    );
    assert_eq!(live[0].origin, UploadOrigin::Live);
}

/// LoTW's automatic upload waits while contest mode is on, and runs as before once it is off.
#[test]
fn the_lotw_automatic_upload_waits_while_contest_mode_is_on() {
    let mut s = Engine::new("W9XYZ", "EN61", 0).settings().clone();
    s.lotw_auto_upload = true;
    s.lotw_use_adif_location = false;
    s.lotw_station_location = "Home".into();
    s.lotw_last_auto_upload_unix = 0;
    s.fd_active = false;
    assert!(
        lotw_auto_upload_due(&s, false, 1_800_000_000),
        "premise: due"
    );
    s.fd_active = true;
    assert!(!lotw_auto_upload_due(&s, false, 1_800_000_000));
}

/// A scratch directory of this test's own, standing in for the settings directory.
fn scratch_dir(label: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "nexus-contest-held-{label}-{}-{}",
        std::process::id(),
        now_unix_secs()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// 2026-10-18 12:00:00 UTC, for the name a kept-aside file gets.
const NOW: i64 = 1_792_324_800;

/// ⭐ A restart with contest mode on keeps the held list: the contacts go out once, paced, when
/// the switch goes off after the restart, each exactly as it was held, and never again.
#[test]
fn the_held_contacts_survive_a_restart_and_go_once_when_contest_mode_is_turned_off() {
    let dir = scratch_dir("restart");
    let path = dir.join("contest_held_uploads.json");
    let mut e = contest_on();
    e.set_contest_held_uploads_path(path.clone());
    e.restore_contest_held_uploads(NOW);
    e.log_qso(cw("K1ABC"));
    e.log_qso(cw("W1AW"));
    assert!(e.take_pending_uploads().is_empty(), "premise: held");
    let held: Vec<QsoRecord> = e
        .contest_held_uploads
        .iter()
        .map(|h| h.rec.clone())
        .collect();
    assert_eq!(held.len(), 2, "premise: two held");
    drop(e);

    // The restart: a new engine, the switch still on, the list read back.
    let mut e = contest_on();
    e.set_contest_held_uploads_path(path.clone());
    e.restore_contest_held_uploads(NOW);
    assert_eq!(
        calls(&e.take_pending_uploads()),
        Vec::<&str>::new(),
        "still held while the switch is on"
    );

    contest_off(&mut e);
    let sent = e.take_pending_uploads();
    assert_eq!(calls(&sent), ["K1ABC", "W1AW"]);
    for (p, was) in sent.iter().zip(&held) {
        assert_eq!(&p.rec, was, "{} goes out as it was held", was.call);
        assert_eq!(p.origin, UploadOrigin::CatchUp);
        assert_eq!(p.legs, upload_legs::ALL);
    }
    assert_eq!(
        sent[1].retry_after_unix - sent[0].retry_after_unix,
        CATCHUP_UPLOAD_SPACING_SECS,
        "paced, one slot apart"
    );
    assert!(path.exists(), "kept until each upload is done");
    for p in &sent {
        settle(&mut e, p, 0);
    }
    assert!(!path.exists(), "nothing is left to send at the next launch");

    // Neither another save nor another restart sends them again.
    contest_off(&mut e);
    assert_eq!(calls(&e.take_pending_uploads()), Vec::<&str>::new());
    drop(e);
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    e.set_contest_held_uploads_path(path.clone());
    e.restore_contest_held_uploads(NOW);
    assert_eq!(calls(&e.take_pending_uploads()), Vec::<&str>::new());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Contest mode was turned off while Nexus was closed (a restored backup, another copy of the
/// settings): the held contacts go out once, paced, at the launch.
#[test]
fn a_launch_with_contest_mode_off_sends_the_held_list_once() {
    let dir = scratch_dir("launch-off");
    let path = dir.join("contest_held_uploads.json");
    let mut e = contest_on();
    e.set_contest_held_uploads_path(path.clone());
    e.log_qso(cw("K1ABC"));
    drop(e);

    let mut e = Engine::new("W9XYZ", "EN61", 0);
    assert!(!e.settings().fd_active, "premise: the switch is off");
    e.set_contest_held_uploads_path(path.clone());
    e.restore_contest_held_uploads(NOW);
    let sent = e.take_pending_uploads();
    assert_eq!(calls(&sent), ["K1ABC"]);
    assert_eq!(sent[0].origin, UploadOrigin::CatchUp);
    settle(&mut e, &sent[0], 0);
    assert!(!path.exists());
    e.restore_contest_held_uploads(NOW);
    assert_eq!(calls(&e.take_pending_uploads()), Vec::<&str>::new());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A held list this build cannot read is kept aside untouched and named for the screen, never
/// cleared or written over; a contact held after it starts a new file beside it.
#[test]
fn an_unreadable_held_list_is_kept_aside_and_reported_never_cleared() {
    let wrong_count = format!(
        "{{\"version\":1,\"count\":2,\"adif\":\"{}\",\"owed\":[{all},{all}]}}",
        tempo_core::logbook::adif_record_own_log(&cw("K1ABC")).replace('\n', "\\n"),
        all = upload_legs::ALL
    );
    let short_owed = format!(
        "{{\"version\":1,\"count\":1,\"adif\":\"{}\",\"owed\":[]}}",
        tempo_core::logbook::adif_record_own_log(&cw("K1ABC")).replace('\n', "\\n")
    );
    for (label, text) in [
        ("owed", short_owed),
        (
            "torn",
            "{\"version\":1,\"count\":1,\"adif\":\"<CALL:5>K1A".to_string(),
        ),
        ("count", wrong_count),
    ] {
        let dir = scratch_dir(label);
        let path = dir.join("contest_held_uploads.json");
        std::fs::write(&path, &text).unwrap();
        let mut e = contest_on();
        e.set_contest_held_uploads_path(path.clone());
        e.restore_contest_held_uploads(NOW);
        assert!(
            e.contest_held_uploads.is_empty(),
            "{label}: nothing guessed"
        );
        let aside = tempo_core::keep_aside::kept()
            .into_iter()
            .find(|k| k.path.parent() == Some(dir.as_path()))
            .unwrap_or_else(|| panic!("{label}: the file is reported"));
        assert_eq!(aside.store, "contestHeldUploads");
        assert!(!aside.kept_in_place);
        assert_eq!(
            std::fs::read_to_string(&aside.path).unwrap(),
            text,
            "{label}: kept untouched"
        );

        e.log_qso(cw("W1AW"));
        assert_eq!(
            std::fs::read_to_string(&aside.path).unwrap(),
            text,
            "{label}: never written over"
        );
        assert!(path.exists(), "{label}: a new list beside it");
        contest_off(&mut e);
        assert_eq!(calls(&e.take_pending_uploads()), ["W1AW"]);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

/// What the upload worker reports for one pushed record: `failed` is the legs that failed for a
/// reason that may pass and go back on the queue; every other leg it owed is done.
fn settle(e: &mut Engine, p: &PendingUpload, failed: u8) {
    e.contest_upload_done(&p.rec, p.legs & !failed);
}

/// ⭐ Quitting part-way through the paced release loses nothing and sends nothing twice: a held
/// contact leaves the file only when its upload is done, leg by leg, so the next launch sends
/// exactly what was still owed.
#[test]
fn a_quit_part_way_through_the_release_sends_only_what_was_still_owed() {
    let dir = scratch_dir("part-way");
    let path = dir.join("contest_held_uploads.json");
    let mut e = contest_on();
    e.set_contest_held_uploads_path(path.clone());
    for call in ["K1ABC", "W1AW", "N0CALL", "K9XYZ"] {
        e.log_qso(cw(call));
    }
    contest_off(&mut e);
    let sent = e.take_pending_uploads();
    assert_eq!(calls(&sent), ["K1ABC", "W1AW", "N0CALL", "K9XYZ"]);
    // K1ABC went everywhere; W1AW reached all but Club Log, which was busy; every service
    // was busy for N0CALL; K9XYZ's slot had not come round when Nexus was closed.
    settle(&mut e, &sent[0], 0);
    settle(&mut e, &sent[1], upload_legs::CLUBLOG);
    settle(&mut e, &sent[2], upload_legs::ALL);
    // A second word on a record already done changes nothing.
    settle(&mut e, &sent[0], 0);
    assert!(path.exists(), "three of the four are still owed");
    drop(e);

    let mut e = Engine::new("W9XYZ", "EN61", 0);
    assert!(!e.settings().fd_active, "premise: the switch is off");
    e.set_contest_held_uploads_path(path.clone());
    e.restore_contest_held_uploads(NOW);
    let again = e.take_pending_uploads();
    let owed: Vec<(&str, u8)> = again
        .iter()
        .map(|p| (p.rec.call.as_str(), p.legs))
        .collect();
    assert_eq!(
        owed,
        [
            ("W1AW", upload_legs::CLUBLOG),
            ("N0CALL", upload_legs::ALL),
            ("K9XYZ", upload_legs::ALL)
        ],
        "exactly what was still owed, and K1ABC not again"
    );
    assert!(again.iter().all(|p| p.origin == UploadOrigin::CatchUp));
    for p in &again {
        settle(&mut e, p, 0);
    }
    assert!(!path.exists(), "all done: nothing for the next launch");
    drop(e);

    let mut e = Engine::new("W9XYZ", "EN61", 0);
    e.set_contest_held_uploads_path(path.clone());
    e.restore_contest_held_uploads(NOW);
    assert_eq!(calls(&e.take_pending_uploads()), Vec::<&str>::new());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A contact still being released is not sent a second time by the next save with the switch
/// off, nor by the next contest's end; the list on disk still names it until it is done.
#[test]
fn a_contact_being_released_is_not_released_again_before_it_is_done() {
    let dir = scratch_dir("in-flight");
    let path = dir.join("contest_held_uploads.json");
    let mut e = contest_on();
    e.set_contest_held_uploads_path(path.clone());
    e.log_qso(cw("K1ABC"));
    contest_off(&mut e);
    let sent = e.take_pending_uploads();
    assert_eq!(calls(&sent), ["K1ABC"]);
    contest_off(&mut e);
    let mut s = e.settings().clone();
    s.fd_active = true;
    e.apply_settings(s);
    e.log_qso(cw("W1AW"));
    contest_off(&mut e);
    assert_eq!(
        calls(&e.take_pending_uploads()),
        ["W1AW"],
        "only the newly held one"
    );
    assert!(path.exists(), "K1ABC is not done yet");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A contact held during a contest is one made before a first login's answer: when the operator
/// answers "Only contacts made from now on" for QRZ mid-contest, the contest's held contacts still
/// go everywhere else when contest mode is turned off, and not to QRZ.
#[test]
fn a_held_contact_made_before_a_first_logins_answer_is_not_sent_to_that_service() {
    let mut e = contest_on();
    e.log_form_qso(cw("W1AW"), StateSource::Operator, Vec::new());
    assert!(e.take_pending_uploads().is_empty(), "premise: held");
    // 2027-01-15, after the contact's 2026-10-17.
    e.first_login_saved(upload_legs::QRZ, true, false, 1_800_000_000);
    contest_off(&mut e);
    let sent: Vec<(String, u8)> = e
        .take_pending_uploads()
        .into_iter()
        .map(|p| (p.rec.call, p.legs))
        .collect();
    assert_eq!(
        sent,
        [("W1AW".to_string(), upload_legs::ALL & !upload_legs::QRZ)]
    );
}
