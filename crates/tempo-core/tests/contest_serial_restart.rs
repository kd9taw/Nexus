//! ⭐ **A restart mid-contest must not re-issue a serial the log already sent.**
//!
//! A serial is scoring-grade: the other operator copied the number that went out, and
//! a checker matches it against their log. Two of my contacts carrying the same
//! number is not a cosmetic defect — it is two QSO lines a sponsor can reject.
//!
//! ⚠️ **This file reconstructs the session the way the APP does, and that is the
//! whole point of it being an integration test.** `engine.rs`'s `set_operating_spec`
//! builds a FRESH [`ContestSession`] whenever Field Day is entered from another mode
//! (a cold start is exactly that), hands it to a fresh [`FieldDayLog`], and only then
//! merges the position journal back in with [`FieldDayLog::merge_adif`]. The rows
//! come back; whatever the session alone was carrying does not. A test that cloned
//! the live session instead would restore the counter for free and prove nothing.
use tempo_core::contest::{ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

/// 2026-11-07T21:04:00Z — inside the Sweepstakes CW weekend, the same instant
/// `tests/sweepstakes.rs` works its contacts at.
const SAT: u64 = 1_794_085_440;

/// "Pick ARRL November Sweepstakes (CW)" — the two calls the engine makes.
fn select() -> ContestSession {
    let rs = ruleset_by_id("arrlss_cw", CURRENT_RULES_YEAR).expect("arrlss_cw is shipped");
    ContestSession::for_ruleset(rs, &w9xyz()).expect("the entrant is fully declared")
}

/// A fully declared Sweepstakes entrant — `tests/sweepstakes.rs`'s own operator.
fn w9xyz() -> StationData {
    StationData {
        mycall: "W9XYZ".into(),
        fd_section: "WI".into(),
        contest_check: "74".into(),
        contest_category_operator: "SINGLE-OP".into(),
        contest_category_power: "LOW".into(),
        contest_category_assisted: "NON-ASSISTED".into(),
        ..Default::default()
    }
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// The serial on a sent exchange — `NR` is Sweepstakes' first sent slot.
fn serial_of(tx: &[tempo_core::contest::FieldValue]) -> &str {
    tx.iter()
        .find(|v| v.key == "NR")
        .map(|v| v.raw.as_str())
        .expect("Sweepstakes sends a serial")
}

/// Work `call` the way the operator does: compose (which ISSUES the serial), then log.
fn work(log: &mut FieldDayLog, call: &str, their_nr: &str, at: u64) -> String {
    let issued = serial_of(&log.session.compose_for(call, at).tx).to_string();
    assert!(
        log.log_fields_at(
            call,
            &fields(&[
                ("NR", their_nr),
                ("PREC", "A"),
                ("CALL", call),
                ("CK", "71"),
                ("SEC", "CT"),
            ]),
            "CW",
            "",
            0,
            at,
        ),
        "{call} is not a dupe"
    );
    issued
}

/// ⭐ **THE DEFECT.** Three contacts, a restart, and the fourth contact's serial.
#[test]
fn a_restart_mid_contest_continues_the_serial_run() {
    let mut log = FieldDayLog::new("W9XYZ", select(), "20m");
    let issued: Vec<String> = ["K2DEF", "W1AW", "N5XYZ"]
        .iter()
        .enumerate()
        .map(|(i, c)| work(&mut log, c, "12", SAT + 60 * i as u64))
        .collect();
    assert_eq!(issued, ["1", "2", "3"], "the run before the restart");

    // THE RESTART. `persist_fd_log` writes exactly this text; the fresh session and
    // the fresh log are what `set_operating_spec` builds on the way back in.
    let journal = log.adif();
    let mut restored = FieldDayLog::new("W9XYZ", select(), "20m");
    restored.merge_adif(&journal, 0);

    // ⭐ THE POSITIVE CONTROL — without it a red below could just mean the journal
    // never restored anything, which is a different bug with the same symptom.
    assert_eq!(restored.qso_count(), 3, "every row came back");
    assert_eq!(
        restored
            .qsos()
            .iter()
            .map(|q| serial_of(&q.tx))
            .collect::<Vec<_>>(),
        ["1", "2", "3"],
        "…carrying the serial each one was worked with"
    );

    // …and the counter that issues the NEXT one must have come back with them.
    assert_eq!(
        restored.session.next_serial, 4,
        "the counter continues past every restored row"
    );
    assert_eq!(
        serial_of(&restored.session.compose_for("K3GHI", SAT + 600).tx),
        "4",
        "the first contact after the restart gets a number nobody has copied yet"
    );
}

/// ⭐ **The run is the MAXIMUM of the rows, not the last one read** — and a row that
/// was never composed for numbers nothing.
///
/// Both halves of "re-deriving cannot drift" are here. A journal is a file: two
/// sessions' files concatenated, or one an operator opened in an editor, arrive in
/// whatever order they arrive in, and a counter that simply followed the last record
/// would hand the next contact a number already on the air. The `"0"` row is the
/// template placeholder a contact logged without a compose carries (`sent_value`'s
/// `"serial"` arm) — it is the absence of a serial, not serial zero.
#[test]
fn the_run_takes_the_highest_serial_in_the_journal_and_ignores_the_placeholder() {
    let mut log = FieldDayLog::new("W9XYZ", select(), "20m");
    // Logged with NO compose: `tx_for_row` falls back to the session template, so this
    // row's serial slot holds the placeholder.
    assert!(log.log_fields_at(
        "K2DEF",
        &fields(&[
            ("NR", "12"),
            ("PREC", "A"),
            ("CALL", "K2DEF"),
            ("CK", "71"),
            ("SEC", "CT"),
        ]),
        "CW",
        "",
        0,
        SAT,
    ));
    assert_eq!(
        serial_of(&log.qsos()[0].tx),
        "0",
        "the placeholder, not a number"
    );
    let issued: Vec<String> = ["W1AW", "N5XYZ"]
        .iter()
        .enumerate()
        .map(|(i, c)| work(&mut log, c, "12", SAT + 60 * (i as u64 + 1)))
        .collect();
    assert_eq!(issued, ["1", "2"]);

    // The records reversed — the highest-numbered row is now the FIRST one read.
    let journal = log.adif();
    let (head, body) = journal.split_at(journal.find("<EOH>\n").expect("a header") + 6);
    let mut records: Vec<&str> = body.split_inclusive("<EOR>\n").collect();
    records.reverse();
    let shuffled = format!("{head}{}", records.concat());

    let mut restored = FieldDayLog::new("W9XYZ", select(), "20m");
    restored.merge_adif(&shuffled, 0);
    assert_eq!(restored.qso_count(), 3, "every row came back");
    // ⭐ THE CONTROL for the shuffle itself: rows restore in the order they are read,
    // so the highest-numbered contact must now be the first row. Without this, a
    // reversal that silently did nothing would leave the assertion below testing the
    // in-order case a second time.
    assert_eq!(
        (
            restored.qsos()[0].call.as_str(),
            serial_of(&restored.qsos()[0].tx)
        ),
        ("N5XYZ", "2"),
        "the highest serial is the FIRST record read"
    );
    assert_eq!(
        restored.session.next_serial, 3,
        "past the highest issued, whatever order the rows arrived in"
    );
}

/// ⭐ **A PREVIOUS EVENT'S journal must not advance THIS event's run** — the case a
/// stored counter gets wrong by construction.
///
/// `merge_adif`'s age gate drops rows older than the caller's bound (the engine passes
/// four days), so last weekend's contacts do not join this weekend's log. A counter
/// persisted beside them would survive that gate and start this contest at 400; the
/// run re-derived from the rows that actually restored starts where the sponsor
/// requires, at 1.
///
/// ⚠️ **Its final assertion is not a repro** — a build with no restore at all also
/// leaves the run at 1. What discriminates here is the CONTROL, which is red without
/// the fix, and what the test guards is the FUTURE: a counter moved into storage would
/// turn this green case red, which is the point of writing it down now.
#[test]
fn last_events_journal_does_not_carry_its_serial_run_forward() {
    let mut log = FieldDayLog::new("W9XYZ", select(), "20m");
    for (i, c) in ["K2DEF", "W1AW", "N5XYZ"].iter().enumerate() {
        work(&mut log, c, "12", SAT + 60 * i as u64);
    }
    let journal = log.adif();

    // ⭐ THE CONTROL: the SAME text, merged with no age bound, restores three rows and
    // a run of 4. An empty or malformed journal would satisfy every assertion below
    // for the wrong reason.
    let mut in_window = FieldDayLog::new("W9XYZ", select(), "20m");
    in_window.merge_adif(&journal, 0);
    assert_eq!(
        (in_window.qso_count(), in_window.session.next_serial),
        (3, 4)
    );

    // Entering the contest a week later: every row in the file is out of window.
    let mut restored = FieldDayLog::new("W9XYZ", select(), "20m");
    restored.merge_adif(&journal, SAT + 7 * 86_400);
    assert_eq!(restored.qso_count(), 0, "the age gate dropped them all");
    assert_eq!(
        restored.session.next_serial, 1,
        "a new event starts its own run at 1"
    );
}

/// An exchange with NO serial slot is untouched — both Field Day events send a class
/// and a section and issue no number at all, and the §8(a) ADIF goldens are pinned
/// byte for byte over exactly this path.
#[test]
fn an_exchange_with_no_serial_slot_restores_unchanged() {
    let fd = || ContestSession::field_day(tempo_core::fieldday::FdEvent::ArrlFd, "3A", "WI");
    let mut log = FieldDayLog::new("W9XYZ", fd(), "20m");
    assert!(log.log_at("K2DEF", "2A", "CT", 0, SAT));
    assert!(log.log_at("W1AW", "3A", "EMA", 0, SAT + 60));

    let mut restored = FieldDayLog::new("W9XYZ", fd(), "20m");
    restored.merge_adif(&log.adif(), 0);
    assert_eq!(restored.qso_count(), 2);
    assert_eq!(
        restored.session.next_serial, 1,
        "nothing to derive, nothing moved"
    );
}
