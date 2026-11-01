//! A restored backup opens its contest session as a settings save opens one
//! ([`restore_with_session_rows`]): a bundle that turns Contest mode on, or moves the pick off the
//! contest running, has the session's rows read from the logbook first, off the lock, and the
//! session's dupe sweep built from them in the same hold as the restore. So a station in the
//! Logbook is flagged in the restored contest exactly as after a Save, and a logbook too busy to
//! read refuses the restore whole, in the Save's words. Each test holds a restore to the Save BY
//! VALUE, on two stations built alike; the controls hold every restore that opens no session to
//! the restore as it was made before, `apply_restored_settings` alone.

use super::*;
use crate::logstore::tests::{engine_on_store, flush, qso, stored, Dir};
use crate::logstore::SESSION_READS;
use std::collections::HashSet;
use std::sync::Mutex;
use tempo_core::logbook::sqlite::WriteHold;
use tempo_core::logbook::{WorkedSince, LOG_SWEEPS};

/// The shell's settings save, as `save_and_publish` makes it: the reference every restore here is
/// held to. Whether the session's sweep was installed, or the refusal.
fn save(m: &Mutex<Engine>, next: Settings) -> Result<bool, String> {
    let asked = next.clone();
    with_session_rows(
        m,
        |e| e.save_opens_session(&asked),
        |e, rows| {
            e.apply_settings(next);
            rows.is_some_and(|r| e.open_session_from(r))
        },
    )
}

/// The shell's restore: `bundle` restored, and the settings the shell persists in the same hold.
fn restore(m: &Mutex<Engine>, bundle: Settings) -> Result<Settings, String> {
    restore_with_session_rows(m, bundle, |e| e.settings().clone())
}

/// What `f` answers, and how many times it read a session's rows from the logbook.
fn reads<T>(f: impl FnOnce() -> T) -> (T, usize) {
    SESSION_READS.with(|c| c.set(0));
    let answer = f();
    (answer, SESSION_READS.with(|c| c.get()))
}

/// A station on a store of its own, with the station data Field Day and the Illinois QSO Party
/// need and Contest mode off, and two FT8 contacts in its Logbook on the band it is on (FT8 is
/// the digital class it operates in): W1AW, dated after any session opened now starts, so it is
/// that session's, and N0OLD from the day before, which is not.
fn station(d: &Dir) -> Mutex<Engine> {
    let mut e = engine_on_store(d);
    e.set_fd_log_path(d.0.join("fd.adi"));
    let mut s = e.settings().clone();
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    s.fd_position_id = "a1b2c3d4".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "COOK".into();
    e.apply_settings(s);
    let now = now_unix_secs();
    for (call, when) in [("W1AW", now + 60), ("N0OLD", now - 86_400)] {
        let mut contact = qso(call, when);
        contact.band = e.settings().band.clone();
        contact.mode = "FT8".into();
        e.log_qso(contact);
    }
    flush(&e);
    Mutex::new(e)
}

/// The station's settings with `edit` made to them.
fn edited(m: &Mutex<Engine>, edit: impl FnOnce(&mut Settings)) -> Settings {
    let mut s = engine_lock(m).settings().clone();
    edit(&mut s);
    s
}

/// The open contest session, by value: its contest, and its dupe sweep of the Logbook — `None`
/// for a session opened without one.
fn session(m: &Mutex<Engine>) -> Option<(String, Option<WorkedSince>)> {
    let e = engine_lock(m);
    let Mode::FieldDay { station, .. } = &e.mode else {
        return None;
    };
    let sweep = e
        .station
        .worked_since(station.log.session.start_unix, &station.log.dupe_rule());
    Some((station.log.session.event_id.clone(), sweep))
}

/// How the open session flags `call`: worked this session (the badge), and a DUPE on the band and
/// mode in force. Asked only of a session opened with its sweep (a debug build asserts it).
fn flags(m: &Mutex<Engine>, call: &str) -> (bool, bool) {
    let b4 = engine_lock(m).session_b4().expect("a session is open");
    (b4.worked_this_session(call), b4.is_dupe(call))
}

/// The open session's start, `None` outside one.
fn session_start(m: &Mutex<Engine>) -> Option<u64> {
    match &engine_lock(m).mode {
        Mode::FieldDay { station, .. } => Some(station.log.session.start_unix),
        _ => None,
    }
}

/// What a restore lands, by value: the settings, the contest session (its contest, and whether it
/// is running), and the CQ run.
fn landed(m: &Mutex<Engine>) -> (Settings, Option<(String, bool)>, bool) {
    let e = engine_lock(m);
    let contest = match &e.mode {
        Mode::FieldDay { station, running } => {
            Some((station.log.session.event_id.clone(), *running))
        }
        _ => None,
    };
    (e.settings().clone(), contest, e.cq_running)
}

/// ★ A RESTORE THAT TURNS CONTEST MODE ON OPENS ITS SESSION AS A SAVE DOES. Two stations alike,
/// Contest mode off; one restores a backup with it on, the other saves the same settings. Each
/// reads the session's rows once, first, and builds the session's dupe sweep from them, so the
/// station in the Logbook (W1AW) is flagged in the restored contest exactly as after the Save:
/// worked this session, and a DUPE on the band and mode in force. N0OLD, from before the session,
/// is not. The first snapshot sweeps nothing (the sweep was ready), and the restore writes nothing
/// to the Logbook.
#[test]
fn a_restore_that_turns_contest_mode_on_knows_the_logbook_as_a_save_does() {
    let (d1, d2) = (Dir::new("restore-on"), Dir::new("save-on"));
    let (restored, saved) = (station(&d1), station(&d2));
    let logbook = stored(&d1);
    let bundle = edited(&restored, |s| s.fd_active = true);
    let (by_restore, read) = reads(|| restore(&restored, bundle.clone()));
    let (by_save, read_by_save) = reads(|| save(&saved, bundle));
    assert!(by_restore.is_ok(), "restored: {by_restore:?}");
    assert_eq!(
        by_save,
        Ok(true),
        "premise: the Save opens Field Day with its sweep"
    );
    assert_eq!(
        session(&restored),
        session(&saved),
        "the restored contest's dupe check knows the Logbook as the saved one's does"
    );
    assert_eq!(
        session(&restored).map(|(event, sweep)| (event, sweep.map(|w| w.worked_this_session))),
        Some((
            "arrlfd".to_string(),
            Some(HashSet::from(["W1AW".to_string()]))
        )),
        "Field Day, holding W1AW, this session's, and not N0OLD"
    );
    assert_eq!(
        (read, read_by_save),
        (1, 1),
        "the session's rows read once, first, by either"
    );
    assert_eq!(flags(&restored, "W1AW"), flags(&saved, "W1AW"));
    assert_eq!(
        (flags(&restored, "W1AW"), flags(&restored, "N0OLD")),
        ((true, true), (false, false)),
        "W1AW flagged, worked and a DUPE on the band and mode in force; N0OLD not"
    );
    LOG_SWEEPS.with(|c| c.set(0));
    let _ = engine_lock(&restored).snapshot();
    assert_eq!(
        LOG_SWEEPS.with(|c| c.get()),
        0,
        "the first snapshot sweeps nothing: the sweep was ready"
    );
    assert_eq!(stored(&d1), logbook, "nothing written to the Logbook");
}

/// ★ A RESTORE THAT MOVES THE PICK WHILE A CONTEST RUNS OPENS THE NEW SESSION AS A SAVE DOES. Both
/// stations run ARRL Field Day, opened by a Save. One restores a backup picking the Illinois QSO
/// Party; the other saves the same settings. Each ends Field Day and opens the party's session,
/// its rows read once, first, and its sweep built from them, so W1AW is flagged in the party
/// alike. The restore writes nothing to the Logbook.
#[test]
fn a_restore_that_moves_the_pick_while_a_contest_runs_knows_the_logbook_as_a_save_does() {
    let (d1, d2) = (Dir::new("restore-pick"), Dir::new("save-pick"));
    let (restored, saved) = (station(&d1), station(&d2));
    for m in [&restored, &saved] {
        assert_eq!(
            save(m, edited(m, |s| s.fd_active = true)),
            Ok(true),
            "premise: Field Day runs"
        );
    }
    let logbook = stored(&d1);
    let bundle = edited(&restored, |s| s.fd_event = "ilqp".into());
    let (by_restore, read) = reads(|| restore(&restored, bundle.clone()));
    let (by_save, read_by_save) = reads(|| save(&saved, bundle));
    assert!(by_restore.is_ok(), "restored: {by_restore:?}");
    assert_eq!(
        by_save,
        Ok(true),
        "premise: the Save switches to the party with its sweep"
    );
    assert_eq!(
        session(&restored),
        session(&saved),
        "the restored party's dupe check knows the Logbook as the saved one's does"
    );
    assert_eq!(
        session(&restored).map(|(event, sweep)| (event, sweep.map(|w| w.worked_this_session))),
        Some((
            "ilqp".to_string(),
            Some(HashSet::from(["W1AW".to_string()]))
        )),
        "the party, holding W1AW, and not N0OLD"
    );
    assert_eq!(
        (read, read_by_save),
        (1, 1),
        "the party's rows read once, first, by either"
    );
    assert_eq!(flags(&restored, "W1AW"), flags(&saved, "W1AW"));
    assert!(flags(&restored, "W1AW").0, "W1AW flagged as worked");
    assert_eq!(stored(&d1), logbook, "nothing written to the Logbook");
}

/// ★ WHILE THE LOGBOOK IS BUSY, THE RESTORE SAYS SO AND WAITS AS A SAVE DOES. Another program
/// holds the logbook's write lock with a contact stuck behind it, so no read of the session's rows
/// can be the log's. The Save that turns Contest mode on reads three times and is refused with
/// `SESSION_BUSY`; the restore of the same settings reads three times and is refused in the same
/// words — and nothing of it is applied, nor handed to the shell to persist. The CONTROL: once the
/// logbook answers, the same restore goes through, its session opened with its sweep, the contact
/// that was held up in it.
#[test]
fn a_busy_logbook_refuses_the_restore_whole_as_it_refuses_the_save() {
    let d = Dir::new("restore-busy");
    let m = station(&d);
    let before = engine_lock(&m).settings().clone();
    let bundle = edited(&m, |s| {
        s.fd_active = true;
        s.mycall = "W9REST".into();
    });
    let hold = WriteHold::take(&d.db()).expect("hold the write lock");
    engine_lock(&m).log_qso(qso("W9HLD", now_unix_secs() + 60));
    assert_eq!(
        reads(|| save(&m, bundle.clone())),
        (Err(SESSION_BUSY.to_string()), 3),
        "premise: the Save reads three times, then is refused"
    );
    let mut persisted = false;
    assert_eq!(
        reads(|| restore_with_session_rows(&m, bundle.clone(), |_| persisted = true)),
        (Err(SESSION_BUSY.to_string()), 3),
        "the restore reads three times, then is refused in the Save's words"
    );
    assert!(!persisted, "nothing handed to the shell to persist");
    {
        let e = engine_lock(&m);
        assert_eq!(*e.settings(), before, "nothing applied");
        assert!(!e.in_field_day(), "and Contest mode was not entered");
    }
    drop(hold);
    let persisted = restore(&m, bundle).expect("CONTROL: once the logbook answers, restored");
    assert_eq!(
        (persisted.mycall.as_str(), persisted.fd_active),
        ("W9REST", true),
        "the backup's settings, handed to the shell to persist"
    );
    assert_eq!(
        session(&m).map(|(event, sweep)| (event, sweep.map(|w| w.worked_this_session))),
        Some((
            "arrlfd".to_string(),
            Some(HashSet::from(["W1AW".to_string(), "W9HLD".to_string()]))
        )),
        "Field Day opened with its sweep, the contact that was held up in it"
    );
}

/// CONTROLS: A RESTORE THAT OPENS NO SESSION READS NOTHING, as a Save that opens none reads nothing,
/// so a busy logbook does not hold it up, and it lands exactly as the restore did before
/// (`apply_restored_settings` alone, on a station built alike): a restore with Contest mode off,
/// while no contest runs and while one does (it ends it, the master's off half), and a restore of
/// the contest already running, which keeps its session, its start and its CQ run.
#[test]
fn a_restore_that_opens_no_session_reads_nothing_and_lands_as_before() {
    /// The case, whether a contest runs before the restore, and the backup's edit.
    type Case = (&'static str, bool, fn(&mut Settings));
    let cases: [Case; 3] = [
        ("Contest mode off", false, |s| s.mycall = "W9REST".into()),
        ("Contest mode off while a contest runs", true, |s| {
            s.fd_active = false
        }),
        ("the contest already running", true, |s| {
            s.fd_bonuses.push("media".into())
        }),
    ];
    for (case, running, edit) in cases {
        let (d1, d2) = (Dir::new("restore-none"), Dir::new("restore-before"));
        let (restored, before) = (station(&d1), station(&d2));
        if running {
            for m in [&restored, &before] {
                assert_eq!(
                    save(m, edited(m, |s| s.fd_active = true)),
                    Ok(true),
                    "{case}: premise: Field Day runs"
                );
                engine_lock(m)
                    .set_mode("fieldday-run")
                    .expect("premise: calling CQ");
                // A restore while TX is armed between overs is refused (its hold can wait on the
                // keychain across a slot boundary): the CQ run stays, with TX off.
                engine_lock(m).set_tx_enabled(false);
            }
        }
        let start = session_start(&restored);
        let bundle = edited(&restored, edit);
        // Another program holds the logbook, a contact stuck behind it: a read would be refused.
        let hold = WriteHold::take(&d1.db()).expect("hold the write lock");
        engine_lock(&restored).log_qso(qso("W9HLD", now_unix_secs() + 60));
        let (by_restore, read) = reads(|| restore(&restored, bundle.clone()));
        drop(hold);
        engine_lock(&before).apply_restored_settings(bundle);
        assert_eq!(
            (by_restore.is_ok(), read),
            (true, 0),
            "{case}: no read, so the busy logbook does not hold it up"
        );
        assert_eq!(
            landed(&restored),
            landed(&before),
            "{case}: it lands exactly as the restore did before"
        );
        if case == "the contest already running" {
            let (_, contest, cq) = landed(&restored);
            assert_eq!(
                (contest.map(|c| c.1), cq, session_start(&restored)),
                (Some(true), true, start),
                "{case}: the same session, still calling CQ"
            );
        }
    }
}

/// CONTROL: THE RESTORED SETTINGS ROUND-TRIP. A station running the Illinois QSO Party is backed up
/// (its settings as the backup file carries them, JSON) and restored on a station built alike with
/// Contest mode off: the settings the restore lands, and hands the shell to persist, are the
/// backup's, field for field, and backed up again they make the same file.
#[test]
fn the_restored_settings_round_trip() {
    let (d1, d2) = (Dir::new("backed-up"), Dir::new("restored-onto"));
    let (source, target) = (station(&d1), station(&d2));
    let party = edited(&source, |s| {
        s.fd_active = true;
        s.fd_event = "ilqp".into();
        s.mycall = "W9SRC".into();
    });
    assert_eq!(save(&source, party), Ok(true), "premise: the party runs");
    let file = serde_json::to_string(engine_lock(&source).settings()).expect("the backup");
    let bundle: Settings = serde_json::from_str(&file).expect("read back");
    let persisted = restore(&target, bundle).expect("restored");
    let landed = engine_lock(&target).settings().clone();
    assert_eq!(
        landed,
        *engine_lock(&source).settings(),
        "the backup's settings, field for field"
    );
    assert_eq!(persisted, landed, "and what the shell persists");
    assert_eq!(
        serde_json::to_string(&landed).expect("backed up again"),
        file,
        "the same file"
    );
    assert_eq!(
        session(&target).map(|s| s.0),
        Some("ilqp".to_string()),
        "the party restored"
    );
}
