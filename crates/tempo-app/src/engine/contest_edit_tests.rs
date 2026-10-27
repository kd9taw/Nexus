//! Editing a contest contact through the engine: what the operator sees move (each row's dupe
//! mark, the count, the multipliers), the files the edit reaches (the Cabrillo, the ADIF, a merge
//! made after it), the restart that keeps it and the numbers it keeps, what it names, and the
//! club it is refused at. Nothing here keys, unkeys or stops a transmission.

use super::contest_edit::{edit, ContestEditRefusal};
use super::*;
use crate::logstore::tests::{engine_on_store, flush, Dir};
use crate::test_util::StoredLog;
use tempo_core::contest::{ContestSession, StationData};
use tempo_core::fieldday::{ContactEdit, FieldDayLog};

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The edit the screen sends for a one-row CW contact on 20 m.
fn cw(call: &str, county: &str) -> ContactEdit {
    ContactEdit {
        call: call.into(),
        rows: vec![fields(&[("RST", "599"), ("QTH", county)])],
        band: "20m".into(),
        mode: "CW".into(),
    }
}

/// The Illinois QSO Party's contest journal holding `rows` — `(call, county, when)` — logged as
/// the strip logs them from Kane County, each at a time of its own. (Two contacts the engine
/// logs in one second share one, and a contact is named by its call and its time.)
fn journal(rows: &[(&str, &str, u64)]) -> String {
    let rs = tempo_core::fd_rules::ruleset_by_id("ilqp", tempo_core::fd_rules::CURRENT_RULES_YEAR)
        .expect("the party's rules");
    let data = StationData {
        mycall: "W9XYZ".into(),
        contest_qth_state: "IL".into(),
        contest_qth_county: "KANE".into(),
        ..Default::default()
    };
    let session = ContestSession::for_ruleset(rs, &data).expect("the party's session");
    let mut log = FieldDayLog::new("W9XYZ", session, "20m");
    for &(call, county, when) in rows {
        log.session.working(call, when);
        assert!(log.log_fields_at(
            call,
            &fields(&[("RST", "599"), ("QTH", county)]),
            "CW",
            "",
            0,
            when
        ));
        log.session.logged(call);
    }
    log.journal_adif()
}

/// The party running on the contest journal at `path`, as a launch reads it, on `e`.
fn party_on(mut e: Engine, path: &std::path::Path, edit: impl FnOnce(&mut Settings)) -> Engine {
    let mut s = e.settings().clone();
    s.mycall = "W9XYZ".into();
    s.fd_active = true;
    s.fd_event = "ilqp".into();
    s.contest_qth_state = "IL".into();
    s.contest_qth_county = "KANE".into();
    s.fd_position_id = "aaaa0001".into();
    edit(&mut s);
    e.apply_settings(s);
    e.set_fd_log_path(path.to_path_buf());
    e.set_mode("fieldday-sp").expect("the party runs");
    e
}

/// A contest journal of its own for a test, removed first.
fn journal_path(tag: &str) -> std::path::PathBuf {
    let path = std::env::temp_dir().join(format!(
        "nexus_contest_edit_{}_{tag}.adi",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    path
}

/// The contest log as the screen shows it: each row's call, county and mark, the count and the
/// multipliers.
fn shown(e: &Engine) -> (Vec<(String, String, bool)>, usize, Option<u32>) {
    let fd = e.snapshot().field_day.expect("the contest runs");
    let rows = fd
        .log
        .iter()
        .map(|q| (q.call.clone(), q.rcvd[1].clone(), q.dupe))
        .collect();
    (rows, fd.qso_count, fd.mult_count)
}

/// The contact `call` at `when` as the screen shows it: the rows an edit opens with.
fn opened(e: &Engine, call: &str, when: u64) -> Vec<crate::dto::FieldDayQso> {
    let fd = e.snapshot().field_day.expect("the contest runs");
    fd.log
        .into_iter()
        .filter(|q| q.call == call && q.when_unix == when)
        .collect()
}

fn row(call: &str, county: &str, dupe: bool) -> (String, String, bool) {
    (call.into(), county.into(), dupe)
}

/// What says whether anything transmits, or could on its own.
fn transmit_state(e: &Engine) -> (bool, Option<TxOwner>, bool, bool, bool) {
    (
        e.tx_enabled(),
        e.tx_owner(),
        e.manual_ptt(),
        e.tuning(),
        e.slot_tx_abort,
    )
}

/// ⭐ **What the operator sees move, by value.** A busted call edited into the station it was is
/// marked the dupe it was and leaves the count; that dupe, edited into the station it really
/// was, counts again and is a new multiplier (a county the log had not worked). Nothing
/// transmits.
#[test]
fn an_edit_moves_the_dupe_marks_the_count_and_the_multipliers() {
    let path = journal_path("moves");
    let t0 = now_unix_secs() - 600;
    std::fs::write(
        &path,
        journal(&[("K9AAA", "COOK", t0), ("K9AAB", "COOK", t0 + 60)]),
    )
    .expect("the journal");
    let mut e = party_on(Engine::new("W9XYZ", "EN50", 0), &path, |_| {});
    let (rows, count, mults) = shown(&e);
    assert_eq!(
        (rows, count),
        (
            vec![row("K9AAA", "COOK", false), row("K9AAB", "COOK", false)],
            2
        ),
        "premise"
    );
    let calm = transmit_state(&e);

    e.contest_edit(
        "K9AAB",
        t0 + 60,
        &opened(&e, "K9AAB", t0 + 60),
        &cw("K9AAA", "COOK"),
    )
    .unwrap()
    .expect("edited");
    assert_eq!(
        shown(&e),
        (
            vec![row("K9AAA", "COOK", false), row("K9AAA", "COOK", true)],
            1,
            mults
        ),
        "the busted call is the dupe it was"
    );

    e.contest_edit(
        "K9AAA",
        t0 + 60,
        &opened(&e, "K9AAA", t0 + 60),
        &cw("W9CCC", "LAKE"),
    )
    .unwrap()
    .expect("edited");
    assert_eq!(
        shown(&e),
        (
            vec![row("K9AAA", "COOK", false), row("W9CCC", "LAKE", false)],
            2,
            mults.map(|m| m + 1)
        ),
        "the dupe counts again, and Lake is a new multiplier"
    );
    assert_eq!(transmit_state(&e), calm, "an edit never keys or stops TX");
    let _ = std::fs::remove_file(&path);
}

/// ⭐ **The edit reaches the files and the logbook**: the Cabrillo, the ADIF, and a merge into the
/// logbook made after the edit all carry the contact as edited — call, county, band and mode —
/// and none of them the busted call.
#[test]
fn an_edit_reaches_the_cabrillo_the_adif_and_a_merge_made_after_it() {
    let d = Dir::new("contest-edit-files");
    let path = d.0.join("fieldday.adi");
    let t0 = now_unix_secs() - 600;
    std::fs::write(&path, journal(&[("K9AAB", "COOK", t0)])).expect("the journal");
    let mut e = party_on(engine_on_store(&d), &path, |_| {});
    let fix = ContactEdit {
        call: "K9AAA".into(),
        rows: vec![fields(&[("RST", "579"), ("QTH", "LAKE")])],
        band: "40m".into(),
        mode: "USB".into(),
    };
    e.contest_edit("K9AAB", t0, &opened(&e, "K9AAB", t0), &fix)
        .unwrap()
        .expect("edited");

    let cabrillo = e.export_log("cabrillo").expect("a Cabrillo entry");
    let line = cabrillo
        .lines()
        .find(|l| l.starts_with("QSO:"))
        .expect("a QSO line");
    let words: Vec<&str> = line.split_whitespace().collect();
    assert_eq!((words[1], words[2]), ("7000", "PH"), "40 m, phone: {line}");
    assert!(
        line.contains("K9AAA 579 LAKE") && !cabrillo.contains("K9AAB"),
        "{cabrillo}"
    );
    let adif = e.export_log("adif").expect("the ADIF");
    assert!(
        adif.contains("<CALL:5>K9AAA")
            && adif.contains("<BAND:3>40m")
            && adif.contains("<SUBMODE:3>USB")
            && !adif.contains("K9AAB"),
        "{adif}"
    );

    assert_eq!(e.fd_merge_to_general().expect("merged").added(), 1);
    flush(&e);
    let merged = e.stored_log();
    assert_eq!(merged.len(), 1);
    let r = &merged[0];
    let sub = r.extra.iter().find(|(k, _)| k == "SUBMODE");
    assert_eq!(
        (
            r.call.as_str(),
            r.band.as_str(),
            r.mode.as_str(),
            sub.map(|(_, v)| v.as_str())
        ),
        ("K9AAA", "40m", "SSB", Some("USB")),
        "the edited sideband, as the logbook holds it: SSB with USB as its SUBMODE"
    );
    assert_eq!(
        r.contest.as_ref().and_then(|c| c.srx_string.as_deref()),
        Some("579 LAKE"),
        "what they sent, as edited"
    );
}

/// ⭐ **A restart keeps the edit, and the edit spent no number**: the next contact after the
/// restart is numbered on from the last one logged, as it would have been without the edit.
#[test]
fn an_edit_survives_a_restart_and_spends_no_number() {
    let path = journal_path("restart");
    let t0 = now_unix_secs() - 600;
    std::fs::write(
        &path,
        journal(&[("K9AAB", "COOK", t0), ("W9BBB", "LAKE", t0 + 60)]),
    )
    .expect("the journal");
    let mut e = party_on(Engine::new("W9XYZ", "EN50", 0), &path, |_| {});
    let high = e.contest_seq_high_water();
    assert_eq!(high, 2, "premise");
    e.contest_edit("K9AAB", t0, &opened(&e, "K9AAB", t0), &cw("K9AAA", "DUPG"))
        .unwrap()
        .expect("edited");
    e.journal_mark().wait();
    drop(e);

    let mut e = party_on(Engine::new("W9XYZ", "EN50", 0), &path, |_| {});
    assert_eq!(
        shown(&e).0,
        [row("K9AAA", "DUPG", false), row("W9BBB", "LAKE", false)],
        "the restart reads the edit back"
    );
    assert_eq!(e.contest_seq_high_water(), high, "no number spent");
    assert!(e
        .contest_log_manual(
            "N9DDD",
            &fields(&[("RST", "599"), ("QTH", "WILL")]),
            "CW",
            None
        )
        .unwrap());
    assert_eq!(
        e.contest_seq_high_water(),
        high + 1,
        "the next contact numbers on"
    );
    let _ = std::fs::remove_file(&path);
}

/// ⭐ **At a club an edit is refused, and nothing changes** — at the host and at a position: club
/// sync v4 cannot carry it (`fdevent`'s
/// `a_row_sent_again_with_its_call_edited_changes_nothing_at_a_v4_host`), so the log, the club's
/// outbox and marks, and the journal stay as they were. CONTROL: with club sync off the same
/// edit applies.
#[test]
fn at_a_club_an_edit_is_refused_and_changes_nothing() {
    for (role, on) in [
        (
            "host",
            (|s: &mut Settings| s.fd_host_enable = true) as fn(&mut Settings),
        ),
        ("position", |s: &mut Settings| {
            s.fd_join_addr = "192.168.1.20:42073".into()
        }),
    ] {
        let path = journal_path(role);
        let t0 = now_unix_secs() - 600;
        std::fs::write(&path, journal(&[("K9AAB", "COOK", t0)])).expect("the journal");
        let mut e = party_on(Engine::new("W9XYZ", "EN50", 0), &path, on);
        assert!(e.fd_sync_enabled(), "premise: club sync runs at the {role}");
        let (log, outbox, marks) = (shown(&e), e.fd_sync_outbox(0), e.fd_sync_marks());
        e.journal_mark().wait();
        let journaled = std::fs::read_to_string(&path).expect("the journal");
        assert_eq!(
            e.contest_edit("K9AAB", t0, &opened(&e, "K9AAB", t0), &cw("K9AAA", "COOK"))
                .unwrap(),
            Err(ContestEditRefusal::Club),
            "{role}"
        );
        assert_eq!(shown(&e), log, "{role}: the log is as it was");
        assert_eq!(e.fd_sync_outbox(0), outbox, "{role}: nothing for the club");
        assert_eq!(e.fd_sync_marks(), marks, "{role}");
        e.journal_mark().wait();
        assert_eq!(
            std::fs::read_to_string(&path).expect("the journal"),
            journaled,
            "{role}: the journal is as it was"
        );
        let _ = std::fs::remove_file(&path);
    }
    // CONTROL: no club, the same edit applies.
    let path = journal_path("solo");
    let t0 = now_unix_secs() - 600;
    std::fs::write(&path, journal(&[("K9AAB", "COOK", t0)])).expect("the journal");
    let mut e = party_on(Engine::new("W9XYZ", "EN50", 0), &path, |_| {});
    assert!(!e.fd_sync_enabled());
    assert!(e
        .contest_edit("K9AAB", t0, &opened(&e, "K9AAB", t0), &cw("K9AAA", "COOK"))
        .unwrap()
        .is_ok());
    assert_eq!(shown(&e).0, [row("K9AAA", "COOK", false)]);
    let _ = std::fs::remove_file(&path);
}

/// ⭐ **An edit names what it cannot reach**: the N3FJP host, the N1MM broadcast and WSJT-X
/// listeners a contact had already gone to, and the logbook copy of a contact merged before the
/// edit — which keeps the busted call until it is edited there. CONTROL: a contact the
/// forwarder has not passed, and one never merged, name nothing.
#[test]
fn an_edit_names_where_the_contact_had_already_gone() {
    let d = Dir::new("contest-edit-sent");
    let path = d.0.join("fieldday.adi");
    let t0 = now_unix_secs() - 600;
    std::fs::write(
        &path,
        journal(&[("K9AAB", "COOK", t0), ("W9BBC", "LAKE", t0 + 60)]),
    )
    .expect("the journal");
    let mut e = party_on(engine_on_store(&d), &path, |s| {
        s.n3fjp_host = "192.168.1.30".into();
        s.n1mm_addr = "192.168.1.255:12060".into();
        s.wsjtx_udp = true;
    });
    // What the edit opens with, read as the screen read it before the merge below.
    let was = opened(&e, "K9AAB", t0);
    assert_eq!(e.fd_merge_to_general().expect("merged").added(), 2);
    flush(&e);
    e.note_fd_forwarded(1);
    let shared = std::sync::Mutex::new(e);

    let gone = edit(&shared, "K9AAB", t0, &was, &cw("K9AAA", "COOK"))
        .unwrap()
        .expect("edited");
    assert_eq!(gone.edit.before[0].call, "K9AAB");
    assert_eq!(gone.edit.after[0].call, "K9AAA");
    assert_eq!(
        (
            gone.edit.sent.n3fjp.as_deref(),
            gone.edit.sent.n1mm.as_deref(),
            gone.edit.sent.wsjtx
        ),
        (Some("192.168.1.30"), Some("192.168.1.255:12060"), true)
    );
    assert_eq!(gone.logbook, Some(Vec::new()), "merged, uploaded nowhere");
    let e = shared.into_inner().unwrap();
    flush(&e);
    assert!(
        e.stored_log().iter().any(|r| r.call == "K9AAB"),
        "the logbook's copy keeps what it said"
    );

    // CONTROL: past the forwarder's cursor nothing had gone out; never merged, nothing names it.
    let d = Dir::new("contest-edit-unsent");
    let path = d.0.join("fieldday.adi");
    std::fs::write(&path, journal(&[("K9AAB", "COOK", t0)])).expect("the journal");
    let e = party_on(engine_on_store(&d), &path, |s| {
        s.n3fjp_host = "192.168.1.30".into();
    });
    let was = opened(&e, "K9AAB", t0);
    let shared = std::sync::Mutex::new(e);
    let fresh = edit(&shared, "K9AAB", t0, &was, &cw("K9AAA", "COOK"))
        .unwrap()
        .expect("edited");
    assert_eq!(
        fresh.edit.sent,
        super::contest_removal::ContestSentTo::default()
    );
    assert_eq!(fresh.logbook, None);
}

/// An edit naming a contact that is not there changes nothing and says it is gone; one removed
/// after its edit opened is gone too. With no contest running there is nothing to edit.
#[test]
fn an_edit_of_a_contact_that_is_not_there_is_refused() {
    let path = journal_path("gone");
    let t0 = now_unix_secs() - 600;
    std::fs::write(&path, journal(&[("K9AAB", "COOK", t0)])).expect("the journal");
    let mut e = party_on(Engine::new("W9XYZ", "EN50", 0), &path, |_| {});
    let log = shown(&e);
    assert_eq!(
        e.contest_edit(
            "K9AAB",
            t0 + 1,
            &opened(&e, "K9AAB", t0 + 1),
            &cw("K9AAA", "COOK")
        )
        .unwrap(),
        Err(ContestEditRefusal::Gone)
    );
    assert_eq!(shown(&e), log);
    // Removed elsewhere while its edit was open.
    let was = opened(&e, "K9AAB", t0);
    assert!(matches!(e.contest_remove_last("K9AAB", t0), Ok(Ok(_))));
    let log = shown(&e);
    assert_eq!(
        e.contest_edit("K9AAB", t0, &was, &cw("K9AAA", "COOK"))
            .unwrap(),
        Err(ContestEditRefusal::Gone)
    );
    assert_eq!(shown(&e), log);
    let _ = std::fs::remove_file(&path);
    let mut idle = Engine::new("W9XYZ", "EN50", 0);
    assert!(idle
        .contest_edit("K9AAB", t0, &[], &cw("K9AAA", "COOK"))
        .is_err());
}

/// ⛔ **An edit opened before the contact changed elsewhere is refused, and changes nothing.**
/// Two windows can each have an edit of one contact open; the first saves, and the second's form
/// still holds what the contact was. Its save must not write that back: the log, the journal and
/// every row stay exactly as the first edit left them. CONTROL: the same edit opened from the
/// contact as it now is applies.
#[test]
fn an_edit_opened_before_the_contact_changed_is_refused() {
    let path = journal_path("stale");
    let t0 = now_unix_secs() - 600;
    std::fs::write(
        &path,
        journal(&[("K9AAB", "COOK", t0), ("W9BBB", "LAKE", t0 + 60)]),
    )
    .expect("the journal");
    let mut e = party_on(Engine::new("W9XYZ", "EN50", 0), &path, |_| {});
    let stale = opened(&e, "K9AAB", t0);
    // The other window fixes the county; the call stays, so the contact is still named.
    e.contest_edit("K9AAB", t0, &stale, &cw("K9AAB", "DUPG"))
        .unwrap()
        .expect("the first edit");
    e.journal_mark().wait();
    let log = e.snapshot().field_day.expect("the contest runs").log;
    let journaled = std::fs::read_to_string(&path).expect("the journal");

    assert_eq!(
        e.contest_edit("K9AAB", t0, &stale, &cw("K9AAB", "COOK"))
            .unwrap(),
        Err(ContestEditRefusal::Changed),
        "the second form still holds Cook"
    );
    assert_eq!(
        e.snapshot().field_day.expect("the contest runs").log,
        log,
        "every row as the first edit left it"
    );
    e.journal_mark().wait();
    assert_eq!(
        std::fs::read_to_string(&path).expect("the journal"),
        journaled,
        "the journal is as it was"
    );

    // CONTROL: opened from the contact as it now is, the same edit applies.
    let now = opened(&e, "K9AAB", t0);
    e.contest_edit("K9AAB", t0, &now, &cw("K9AAB", "COOK"))
        .unwrap()
        .expect("edited");
    assert_eq!(
        shown(&e).0,
        [row("K9AAB", "COOK", false), row("W9BBB", "LAKE", false)]
    );
    let _ = std::fs::remove_file(&path);
}

/// The contact as another window's edit leaves it — then the stale save, refused: what it
/// returned, and whether the log stayed as the first edit left it.
fn stale_save(
    e: &mut Engine,
    call: &str,
    when: u64,
    first: &ContactEdit,
    stale: &ContactEdit,
) -> (Result<(), ContestEditRefusal>, bool) {
    let was = opened(e, call, when);
    e.contest_edit(call, when, &was, first)
        .unwrap()
        .expect("the first edit");
    let log = e.snapshot().field_day.expect("the contest runs").log;
    let answer = e.contest_edit(&first.call, when, &was, stale).unwrap();
    let kept = e.snapshot().field_day.expect("the contest runs").log == log;
    (answer.map(|_| ()), kept)
}

/// The time of the newest contact in the log.
fn newest_when(e: &Engine) -> u64 {
    let fd = e.snapshot().field_day.expect("the contest runs");
    fd.log.last().expect("a contact").when_unix
}

/// ⛔ **A stale edit is refused whichever field the other window changed** — the band alone,
/// the mode alone or the sideband alone (the RST the same), the call — and changes nothing. A changed call leaves the
/// contact at its time, so it is changed, not gone.
#[test]
fn a_band_mode_or_call_changed_elsewhere_refuses_the_stale_edit() {
    let t0 = now_unix_secs() - 600;
    let party = |tag: &str| {
        let path = journal_path(tag);
        std::fs::write(&path, journal(&[("K9AAB", "COOK", t0)])).expect("the journal");
        (
            party_on(Engine::new("W9XYZ", "EN50", 0), &path, |_| {}),
            path,
        )
    };
    let on = |band: &str, mode: &str| ContactEdit {
        band: band.into(),
        mode: mode.into(),
        ..cw("K9AAB", "COOK")
    };

    let (mut e, path) = party("stale-band");
    assert_eq!(
        stale_save(&mut e, "K9AAB", t0, &on("40m", "CW"), &on("20m", "CW")),
        (Err(ContestEditRefusal::Changed), true),
        "the band alone changed"
    );
    let _ = std::fs::remove_file(&path);

    // A phone contact logged at the strip keeps an empty on-air mode, as a CW one does, so
    // edited to CW only its mode moves.
    let path = journal_path("stale-mode");
    std::fs::write(&path, journal(&[])).expect("the journal");
    let mut e = party_on(Engine::new("W9XYZ", "EN50", 0), &path, |_| {});
    assert!(e
        .contest_log_manual(
            "K9AAB",
            &fields(&[("RST", "599"), ("QTH", "COOK")]),
            "PH",
            None
        )
        .unwrap());
    let when = newest_when(&e);
    let was = &opened(&e, "K9AAB", when)[0];
    assert_eq!(
        (was.mode.as_str(), was.submode.as_str()),
        ("PH", ""),
        "premise"
    );
    assert_eq!(
        stale_save(&mut e, "K9AAB", when, &on("20m", "CW"), &on("20m", "")),
        (Err(ContestEditRefusal::Changed), true),
        "the mode alone changed"
    );
    let _ = std::fs::remove_file(&path);

    let (mut e, path) = party("stale-submode");
    e.contest_edit("K9AAB", t0, &opened(&e, "K9AAB", t0), &on("20m", "USB"))
        .unwrap()
        .expect("on USB");
    assert_eq!(
        stale_save(&mut e, "K9AAB", t0, &on("20m", "LSB"), &on("20m", "USB")),
        (Err(ContestEditRefusal::Changed), true),
        "the sideband alone changed"
    );
    let _ = std::fs::remove_file(&path);

    let (mut e, path) = party("stale-call");
    assert_eq!(
        stale_save(
            &mut e,
            "K9AAB",
            t0,
            &cw("K9AAA", "COOK"),
            &cw("K9AAB", "COOK")
        )
        .0,
        Err(ContestEditRefusal::Changed),
        "the call changed: the stale form names the old one"
    );
    let _ = std::fs::remove_file(&path);
}

/// ⛔ **Field Day: a class or a section changed elsewhere refuses the stale edit**, each on its
/// own. Field Day's received exchange rides the row's class and section, not `rcvd`.
#[test]
fn a_field_day_class_or_section_changed_elsewhere_refuses_the_stale_edit() {
    let fd = || {
        let mut e = Engine::new("W9XYZ", "EN50", 0);
        let mut s = e.settings().clone();
        s.mycall = "W9XYZ".into();
        s.fd_active = true;
        s.fd_event = "arrlfd".into();
        s.fd_class = "2A".into();
        s.fd_section = "WI".into();
        e.apply_settings(s);
        e.set_mode("fieldday-sp").expect("Field Day runs");
        assert!(e
            .contest_log_manual(
                "K1ABC",
                &fields(&[("CLASS", "2A"), ("SECTION", "CT")]),
                "CW",
                None
            )
            .unwrap());
        let when = e.snapshot().field_day.expect("Field Day runs").log[0].when_unix;
        (e, when)
    };
    let sent = |class: &str, section: &str| ContactEdit {
        call: "K1ABC".into(),
        rows: vec![fields(&[("CLASS", class), ("SECTION", section)])],
        band: "20m".into(),
        mode: "CW".into(),
    };

    let (mut e, when) = fd();
    assert_eq!(
        opened(&e, "K1ABC", when)[0].rcvd,
        Vec::<String>::new(),
        "premise: Field Day's exchange is not in rcvd"
    );
    assert_eq!(
        stale_save(&mut e, "K1ABC", when, &sent("3A", "CT"), &sent("2A", "CT")),
        (Err(ContestEditRefusal::Changed), true),
        "the class alone changed"
    );

    let (mut e, when) = fd();
    assert_eq!(
        stale_save(&mut e, "K1ABC", when, &sent("2A", "NH"), &sent("2A", "CT")),
        (Err(ContestEditRefusal::Changed), true),
        "the section alone changed"
    );
}
