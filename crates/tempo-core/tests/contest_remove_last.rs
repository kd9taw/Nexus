//! ⭐ **Removing the newest contest contact is a STATE, never a delete.**
//!
//! A removed contact leaves the live log, so the score, the multipliers, the rate, the
//! dupe check and every export drop it — and it is kept, marked removed, in the log and in
//! the journal on disk, so Restore puts it back exactly: same time, serial and seq.
//!
//! ⚠️ **Every assertion is on the ROWS, by value** (`LoggedQso` derives `PartialEq`), with the
//! export bytes as a second claim. An export is entitled to drop things, so two logs that
//! differ in exactly what was lost can export alike; the rows cannot.
//!
//! ⚠️ **The restart tests rebuild the session the way the app does** (a fresh session and a
//! fresh log, then the journal merged back in — see `contest_serial_restart.rs`), because a
//! test that kept the live session would restore the counters for free and prove nothing.
use tempo_core::contest::{ContestSession, FieldValue, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::{FdEvent, FieldDayLog, LoggedQso, RemoveRefusal, RemovedQso};

/// 2026-10-18T17:00:00Z, the Illinois QSO Party's start.
const T0: u64 = 1_792_342_800;

/// 2026-11-07T21:04:00Z — inside the Sweepstakes CW weekend.
const SS: u64 = 1_794_085_440;

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// An Illinois QSO Party log, worked from Kane County.
fn ilqp_log() -> FieldDayLog {
    let rs = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("ilqp is a shipped ruleset");
    let station = StationData {
        contest_qth_state: "IL".into(),
        contest_qth_county: "KANE".into(),
        ..Default::default()
    };
    let session =
        ContestSession::for_ruleset(rs, &station).unwrap_or_else(|e| panic!("ilqp refused: {e}"));
    FieldDayLog::new("W9XYZ", session, "20m")
}

/// One CW party contact from `county`.
fn party(log: &mut FieldDayLog, call: &str, county: &str, at: u64) -> bool {
    log.log_fields_at(
        call,
        &fields(&[("RST", "599"), ("QTH", county)]),
        "CW",
        "",
        0,
        at,
    )
}

/// An ARRL Field Day log.
fn fd_log() -> FieldDayLog {
    FieldDayLog::new(
        "W9XYZ",
        ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
        "20m",
    )
}

/// "Pick ARRL November Sweepstakes (CW)": a serial contest that LOGS its duplicates.
fn ss_session() -> ContestSession {
    let rs = ruleset_by_id("arrlss_cw", CURRENT_RULES_YEAR).expect("arrlss_cw is shipped");
    let station = StationData {
        mycall: "W9XYZ".into(),
        fd_section: "WI".into(),
        contest_check: "74".into(),
        contest_category_operator: "SINGLE-OP".into(),
        contest_category_power: "LOW".into(),
        contest_category_assisted: "NON-ASSISTED".into(),
        ..Default::default()
    };
    ContestSession::for_ruleset(rs, &station).expect("the entrant is fully declared")
}

fn serial_of(tx: &[FieldValue]) -> &str {
    tx.iter()
        .find(|v| v.key == "NR")
        .map(|v| v.raw.as_str())
        .expect("Sweepstakes sends a serial")
}

/// Work `call` in Sweepstakes the way the operator does — compose (which ISSUES the
/// serial), then log — and answer the serial sent.
fn ss_work(log: &mut FieldDayLog, call: &str, at: u64) -> String {
    let issued = serial_of(&log.session.compose_for(call, at).tx).to_string();
    assert!(log.log_fields_at(
        call,
        &fields(&[
            ("NR", "12"),
            ("PREC", "A"),
            ("CALL", call),
            ("CK", "71"),
            ("SEC", "CT"),
        ]),
        "CW",
        "",
        0,
        at,
    ));
    issued
}

/// The journal, read back into a fresh session the way a restart does.
fn restart(journal: &str, session: ContestSession) -> FieldDayLog {
    let mut log = FieldDayLog::new("W9XYZ", session, "20m");
    log.merge_adif(journal, 0);
    log
}

fn score(log: &FieldDayLog) -> (u32, u32, Option<u32>, u32) {
    log.ruleset().scoring.score(log.score_rows(), 1)
}

fn rows(log: &FieldDayLog) -> Vec<LoggedQso> {
    log.qsos().to_vec()
}

/// ⭐ **The newest contact leaves every count and every export, and is kept.**
///
/// Asserted against a TWIN — the same log that never had the contact — because "the score
/// dropped by two" can be true of a log that lost the wrong row.
#[test]
fn the_newest_contact_leaves_every_count_and_export_and_is_kept() {
    let mut log = ilqp_log();
    let mut twin = ilqp_log();
    for l in [&mut log, &mut twin] {
        assert!(party(l, "K9AAA", "COOK", T0));
        assert!(party(l, "W9BBB", "LAKE", T0 + 60));
    }
    assert!(party(&mut log, "K9CCC", "DUPG", T0 + 120));
    let gone = log.qsos()[2].clone();
    // The POSITIVE CONTROL for the dupe claim below: before the removal the key is held.
    assert!(log.is_dupe_row("K9CCC", "20m", "CW", &gone.rx, &gone.tx));

    let removed = log
        .remove_last("K9CCC", T0 + 120, T0 + 130)
        .expect("the newest contact is removed");

    assert_eq!(
        removed,
        RemovedQso {
            rows: vec![gone.clone()],
            removed_unix: T0 + 130,
        },
        "the entry is the row exactly as it stood"
    );
    assert_eq!(
        rows(&log),
        rows(&twin),
        "the live log is the log without it"
    );
    assert_eq!(
        log.removed(),
        [removed],
        "…and the contact is kept, marked removed"
    );
    assert_eq!(log.qso_count(), 2);
    assert_eq!(score(&log), score(&twin), "points and multipliers drop it");
    assert_eq!(
        log.worked_values("QTH"),
        ["COOK", "LAKE"],
        "a county worked only with it is unworked again"
    );
    assert_eq!(log.worked_values("QTH"), twin.worked_values("QTH"));
    assert_eq!(
        log.cabrillo(14_000),
        twin.cabrillo(14_000),
        "the Cabrillo leaves it out"
    );
    assert_eq!(
        log.submission_adif(),
        twin.submission_adif(),
        "the submitted ADIF leaves it out, byte for byte"
    );
    assert!(
        !log.is_dupe_row("K9CCC", "20m", "CW", &gone.rx, &gone.tx),
        "that station on that band and mode is new again"
    );
    assert!(
        party(&mut log, "K9CCC", "DUPG", T0 + 200),
        "…so working it again logs"
    );
}

/// ⭐ **A county line is ONE contact** — all its rows go, and come back, together. And
/// the restore is BYTE-IDENTICAL: the rows by value, then the journal and the Cabrillo.
#[test]
fn a_county_line_goes_and_comes_back_as_one_contact_byte_identical() {
    let mut log = ilqp_log();
    assert!(party(&mut log, "K9AAA", "COOK", T0));
    // One contact on the air, two rows: one time for both, as the engine writes a line.
    assert!(party(&mut log, "K9NR", "COOK", T0 + 60));
    assert!(party(&mut log, "K9NR", "DUPG", T0 + 60));
    let before = rows(&log);
    let journal_before = log.adif();
    let cabrillo_before = log.cabrillo(14_000);
    assert_eq!(
        log.newest_contact(),
        &before[1..],
        "the line is the newest contact"
    );

    // The strip names the contact as the operator typed it; the case does not matter.
    let removed = log
        .remove_last("k9nr", T0 + 60, T0 + 90)
        .expect("the line is removed");
    assert_eq!(removed.rows, before[1..], "both counties go");
    assert_eq!(rows(&log), before[..1]);
    assert_eq!(
        log.worked_values("QTH"),
        ["COOK"],
        "DUPG was worked only by the line"
    );

    let back = log.restore(removed.id()).expect("nothing was worked since");
    assert_eq!(back, removed, "the restore answers the entry it put back");
    assert_eq!(
        rows(&log),
        before,
        "BYTE-IDENTICAL: every row back exactly as it was"
    );
    assert!(log.removed().is_empty());
    assert_eq!(
        log.adif(),
        journal_before,
        "the journal is back byte for byte"
    );
    assert_eq!(log.cabrillo(14_000), cabrillo_before);
}

/// If the newest contact is not the one the strip showed, nothing is removed — and an
/// empty log has nothing to remove.
#[test]
fn a_changed_or_empty_log_refuses_and_changes_nothing() {
    let mut log = ilqp_log();
    assert_eq!(
        log.remove_last("K9AAA", T0, T0 + 5),
        Err(RemoveRefusal::Empty)
    );
    assert!(party(&mut log, "K9AAA", "COOK", T0));
    assert!(party(&mut log, "W9BBB", "LAKE", T0 + 60));
    let before = rows(&log);
    let journal = log.adif();
    for (call, when) in [
        ("K9AAA", T0),      // an older contact, not the newest
        ("W9BBB", T0 + 61), // the newest call, another time
        ("W1AW", T0 + 60),  // the newest time, another call
    ] {
        assert_eq!(
            log.remove_last(call, when, T0 + 90),
            Err(RemoveRefusal::Changed),
            "{call} @ {when} is not the newest contact"
        );
        assert_eq!(rows(&log), before, "nothing was removed");
        assert!(log.removed().is_empty());
        assert_eq!(log.adif(), journal);
    }
}

/// ⭐ **The restart trap: the journal keeps the contact REMOVED, and a restart never
/// hands its serial or its club seq to the next contact.**
///
/// Both numbers are rebuilt from the journal's rows on a restart. A contact deleted from the
/// journal gives its numbers back: the serial goes to a second station, and the seq makes the
/// club host drop the next real contact as a repeat of one it already merged.
#[test]
fn a_restart_keeps_a_removed_contact_removed_and_never_reissues_its_numbers() {
    let mut log = FieldDayLog::new("W9XYZ", ss_session(), "20m");
    let issued: Vec<String> = ["K2DEF", "W1AW", "N5XYZ"]
        .iter()
        .enumerate()
        .map(|(i, c)| ss_work(&mut log, c, SS + 60 * i as u64))
        .collect();
    assert_eq!(issued, ["1", "2", "3"]);
    log.remove_last("N5XYZ", SS + 120, SS + 150)
        .expect("the newest contact is removed");
    assert_eq!(log.session.next_serial, 4, "a removal gives no number back");

    let restored = restart(&log.adif(), ss_session());

    assert_eq!(
        rows(&restored),
        rows(&log),
        "the live rows come back as they were"
    );
    assert_eq!(
        restored.removed(),
        log.removed(),
        "…and the removed contact comes back REMOVED, not live and not lost"
    );
    assert_eq!(restored.qso_count(), 2);
    assert_eq!(
        (restored.max_seq(), restored.session.next_serial),
        (3, 4),
        "(the club seq high-water, the next serial) both continue past the removed contact"
    );
    let mut restored = restored;
    assert_eq!(
        ss_work(&mut restored, "K3GHI", SS + 600),
        "4",
        "the next contact gets a number nobody has copied"
    );
    assert_eq!(restored.qsos().last().unwrap().seq, 4);
}

/// ⭐ **Restore is refused once the same station was worked again on that band and mode.**
#[test]
fn restore_is_refused_once_the_station_is_worked_again() {
    let mut log = fd_log();
    assert!(log.log_mode_at("W1AW", "2A", "CT", "CW", 0, T0));
    assert!(log.log_mode_at("K1ABC", "1D", "EMA", "CW", 0, T0 + 60));
    let removed = log.remove_last("K1ABC", T0 + 60, T0 + 70).unwrap();
    assert!(
        log.log_mode_at("K1ABC", "1D", "EMA", "CW", 0, T0 + 120),
        "worked again, on the band and mode it was removed from"
    );
    let before = rows(&log);
    assert_eq!(log.restore(removed.id()), Err(RemoveRefusal::WorkedAgain));
    assert_eq!(rows(&log), before, "nothing was restored");
    assert_eq!(log.removed(), [removed], "…and it is still kept");
    assert_eq!(log.restore(9_999), Err(RemoveRefusal::NotRemoved));

    // The POSITIVE CONTROL: on another mode it is a different contact, and it restores.
    let mut log = fd_log();
    assert!(log.log_mode_at("K1ABC", "1D", "EMA", "CW", 0, T0));
    let removed = log.remove_last("K1ABC", T0, T0 + 10).unwrap();
    assert!(log.log_mode_at("K1ABC", "1D", "EMA", "PH", 0, T0 + 60));
    assert!(
        log.restore(removed.id()).is_ok(),
        "CW and phone are two contacts"
    );
}

/// Under a ruleset that LOGS its duplicates, a removed dupe of an EARLIER contact was not
/// "worked again since" — it restores, still the dupe it was.
#[test]
fn a_logged_dupe_restores_as_the_dupe_it_was() {
    let mut log = FieldDayLog::new("W9XYZ", ss_session(), "20m");
    ss_work(&mut log, "K2DEF", SS);
    ss_work(&mut log, "W1AW", SS + 60);
    ss_work(&mut log, "K2DEF", SS + 120);
    assert!(log.qsos()[2].dupe, "the second K2DEF is a logged dupe");
    let before = rows(&log);
    let removed = log.remove_last("K2DEF", SS + 120, SS + 130).unwrap();
    assert!(log.restore(removed.id()).is_ok());
    assert_eq!(rows(&log), before, "back exactly, the dupe mark included");

    // The POSITIVE CONTROL for "since": worked again AFTER the removal, it is refused here
    // too, though this ruleset would log the duplicate.
    let removed = log.remove_last("K2DEF", SS + 120, SS + 130).unwrap();
    ss_work(&mut log, "K2DEF", SS + 180);
    assert_eq!(log.restore(removed.id()), Err(RemoveRefusal::WorkedAgain));
}

/// ⭐ **The journal never drops a removed contact because its station was worked again.**
///
/// The restore path admits each LIVE row through the dupe index and passes over one already
/// there. A removed contact is outside the dupe check by definition; had it been offered to
/// the index, the re-work would have refused it out of the journal and the next save would
/// have dropped it for good.
#[test]
fn the_journal_keeps_a_removed_contact_whose_station_was_worked_again() {
    let mut log = fd_log();
    assert!(log.log_mode_at("K1ABC", "1D", "EMA", "CW", 0, T0));
    log.remove_last("K1ABC", T0, T0 + 10).unwrap();
    assert!(log.log_mode_at("K1ABC", "1D", "EMA", "CW", 0, T0 + 60));

    let restored = restart(
        &log.adif(),
        ContestSession::field_day(FdEvent::ArrlFd, "3A", "WI"),
    );
    assert_eq!(
        rows(&restored),
        rows(&log),
        "the re-work is the live contact"
    );
    assert_eq!(
        restored.removed(),
        log.removed(),
        "the removed one is still kept"
    );
    assert_eq!(restored.removed().len(), 1);
}

/// A row removed BY SEQ (a Logbook delete of its merged copy) — and the dupe mark
/// of a LATER row is recomputed, because the mark is order-derived.
#[test]
fn removing_a_row_by_seq_recomputes_a_later_rows_dupe_mark() {
    let mut log = FieldDayLog::new("W9XYZ", ss_session(), "20m");
    ss_work(&mut log, "K2DEF", SS);
    ss_work(&mut log, "W1AW", SS + 60);
    ss_work(&mut log, "K2DEF", SS + 120);
    let before = rows(&log);
    assert!(before[2].dupe);
    assert_eq!(log.remove_row(0, SS + 130), None, "seq 0 names no row");
    assert_eq!(log.remove_row(99, SS + 130), None);
    assert_eq!(rows(&log), before);

    let removed = log
        .remove_row(1, SS + 130)
        .expect("seq 1 is the first K2DEF");
    assert_eq!(removed.rows, before[..1]);
    assert_eq!(rows(&log).len(), 2);
    assert!(
        !log.qsos()[1].dupe,
        "with the first K2DEF gone, the second is the contact"
    );
    assert!(log.restore(removed.id()).is_ok());
    assert_eq!(rows(&log), before, "back in seq order, the mark re-derived");
}

/// ⭐ **Byte identity across a restart:** remove, restart, restore gives exactly the log a
/// restart gives when nothing was removed.
#[test]
fn a_removal_restored_after_a_restart_is_the_row_a_restart_gives_back() {
    let mut log = ilqp_log();
    assert!(party(&mut log, "K9AAA", "COOK", T0));
    assert!(party(&mut log, "W9BBB", "LAKE", T0 + 60));
    let never_removed = restart(&log.adif(), ilqp_log().session);

    log.remove_last("W9BBB", T0 + 60, T0 + 70).unwrap();
    let mut restored = restart(&log.adif(), ilqp_log().session);
    let id = restored.removed()[0].id();
    restored.restore(id).expect("restores after the restart");

    assert_eq!(rows(&restored), rows(&never_removed));
    assert_eq!(restored.adif(), never_removed.adif());
}

/// ⭐ **The journal carries a removed row's own sent exchange even when it alone moved.** A
/// mobile in COOK works a station, moves back to KANE and removes that contact: every live row
/// now matches the session, so a carrier decided on the live rows alone would write none, and a
/// restart would give the removed contact the session's KANE — a contact it never sent.
#[test]
fn a_removed_contact_keeps_the_exchange_it_sent_across_a_restart() {
    let mut log = ilqp_log();
    // A move is how a sent exchange changes; make the first contact after one too, so every
    // live row is the session's own exchange once the mobile is back in KANE.
    log.session.move_to(&[("QTH", "KANE")]).expect("in KANE");
    assert!(party(&mut log, "K9AAA", "COOK", T0));
    log.session
        .move_to(&[("QTH", "COOK")])
        .expect("a county move");
    assert!(party(&mut log, "W9BBB", "LAKE", T0 + 60));
    log.session.move_to(&[("QTH", "KANE")]).expect("and back");
    let sent_from_cook = log.qsos()[1].tx.clone();
    log.remove_last("W9BBB", T0 + 60, T0 + 90).unwrap();
    assert_eq!(
        log.qsos()[0].tx,
        log.session.my_exchange,
        "PREMISE: every live row matches the session's sent exchange again"
    );

    let mut restored = restart(&log.adif(), ilqp_log().session);
    assert_eq!(
        restored.removed(),
        log.removed(),
        "kept exactly, its sent exchange included"
    );
    let id = restored.removed()[0].id();
    let back = restored.restore(id).unwrap();
    assert_eq!(back.rows[0].tx, sent_from_cook, "it still sent COOK");
}
