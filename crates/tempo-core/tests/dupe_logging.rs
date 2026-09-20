//! ⭐ **A duplicate contact in a CROSS-CHECKED contest is LOGGED, MARKED and scored
//! ZERO — and Field Day keeps refusing one.**
//!
//! The sponsors say the same thing twice and mean it literally. ARRL LGCK.1 and CQ
//! XII.E.1: *"Duplicate contacts are removed with no additional penalty."* CQ's own
//! entrant FAQ is an instruction, not a permission: *"Yes! Please log all contacts that
//! you make even if they are duplicates"* and *"No, please do not remove any QSOs from
//! your log! This will cause the other station that worked you to lose credit for the
//! contact."* Dropping the row is what costs points — the other station's NIL is 1×
//! extra at ARRL, 2× extra at CQ — so the log carries the dupe and the checker zeroes it.
//!
//! ⚠️ **ARRL FIELD DAY IS NOT ONE OF THESE CONTESTS.** It has no log checking at all —
//! no cross-check, no NIL, and *"Complete station logs are NOT required for submission,
//! and ARRL does not use the logs."* The NIL justification has zero force there, Field
//! Day's deliverable is a dupe sheet plus **raw non-dupe** QSO counts, and so Field Day
//! keeps refusing the contact exactly as it always has. That split is the whole point of
//! this file, and `field_day_still_refuses_a_dupe` is the half that must never move.
//!
//! ⚠️ **"Marked" means marked in OUR data model**, for scoring and display. Neither
//! sponsor asks for a marker, and CQ's worked example logs the dupe as a plain `QSO:`
//! line; `X-QSO` means "I want this EXCLUDED" and carries CQ's anti-abuse warning. So
//! `a_logged_dupe_is_a_plain_qso_line_in_cabrillo` pins that the mark stays inside.
//!
//! ⚠️ **The country file here is a STUB** — the real resolver lives in a crate that
//! depends on this one. Every callsign is an example call.
use tempo_core::contest::{install_call_resolver, CallLocation, ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::{FdEvent, FieldDayLog};

/// The stub country file: prefix → (entity, continent), longest prefix wins. Enough
/// entities that CQ WW's `ByRelation` point table prices these contacts at something
/// other than zero — a score of 0 either way would make "the dupe added nothing"
/// unfalsifiable.
const TABLE: &[(&str, &str, &str)] = &[
    ("JA", "Japan", "AS"),
    ("DL", "Fed. Rep. of Germany", "EU"),
    ("VE", "Canada", "NA"),
    ("W", "United States", "NA"),
    ("K", "United States", "NA"),
];

fn place(call: &str) -> Option<CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    let mut best: Option<(usize, &'static str, &'static str)> = None;
    for (p, entity, cont) in TABLE {
        if up.starts_with(p) && best.is_none_or(|(n, _, _)| p.len() > n) {
            best = Some((p.len(), entity, cont));
        }
    }
    best.map(|(_, entity, continent)| CallLocation {
        entity,
        continent,
        cq_zone: None,
    })
}

fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(place).expect("the first install in this test binary");
    });
}

/// 2026-11-28 00:00:00Z — inside CQ WW CW's weekend. Only ordering matters here.
const START: u64 = 1_795_996_800;

/// A CQ WW CW log for a US station in zone 4 — one of the contests the sponsor
/// cross-checks, and therefore one that logs its dupes.
fn cqww_log() -> FieldDayLog {
    resolver();
    let rs = ruleset_by_id("cqww_cw", CURRENT_RULES_YEAR).expect("cqww_cw is a shipped ruleset");
    let st = StationData {
        mycall: "W9XYZ".to_string(),
        contest_cq_zone: "4".to_string(),
        contest_qth_state: "IL".to_string(),
        contest_category_power: "LOW".to_string(),
        contest_category_assisted: "NON-ASSISTED".to_string(),
        ..Default::default()
    };
    let session = ContestSession::for_ruleset(rs, &st)
        .unwrap_or_else(|e| panic!("cqww_cw session refused: {e}"));
    FieldDayLog::new("W9XYZ", session, "20m")
}

/// An ARRL Field Day log — the contest with no log checking, which keeps refusing.
fn fd_log() -> FieldDayLog {
    resolver();
    FieldDayLog::new(
        "W9XYZ",
        ContestSession::field_day(FdEvent::ArrlFd, "2A", "WI"),
        "20m",
    )
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// Work one CQ WW CW contact. `min` minutes in, so rows order deterministically.
fn work(log: &mut FieldDayLog, band: &str, call: &str, zone: &str, min: u64) -> bool {
    log.band = band.to_string();
    log.log_fields_at(
        call,
        &fields(&[("RST", "599"), ("ZN", zone)]),
        "CW",
        "",
        0,
        START + min * 60,
    )
}

/// `(qso points, powered, multipliers, total)` for a log, by its own ruleset.
fn score(log: &FieldDayLog) -> (u32, u32, Option<u32>, u32) {
    log.ruleset().scoring.score(log.score_rows(), 1)
}

// ---------------------------------------------------------------------------
// The cross-checked contests: logged, marked, worth nothing
// ---------------------------------------------------------------------------

/// ⭐ **THE CHANGE.** CQ XII.E.1 + the entrant FAQ: the duplicate is written to the log
/// and the checker removes it. It must reach the log, say of itself that it is a dupe,
/// and move no number.
#[test]
fn a_cross_checked_dupe_is_logged_marked_and_scores_zero() {
    let mut log = cqww_log();
    assert!(
        work(&mut log, "20m", "JA1ABC", "25", 0),
        "the first contact"
    );
    let before = score(&log);

    // The same station, same band, same mode — a duplicate by CQ WW's own rule.
    assert!(
        work(&mut log, "20m", "JA1ABC", "25", 30),
        "a dupe in a cross-checked contest is ACCEPTED, not refused: dropping it costs \
         the other station a NIL at 2x the contact"
    );
    assert_eq!(log.qsos().len(), 2, "the duplicate row is in the log");
    assert!(log.qsos()[1].dupe, "…and it is MARKED as a duplicate");
    assert!(!log.qsos()[0].dupe, "…while the original is not");

    assert_eq!(
        score(&log),
        before,
        "XII.E.1 'duplicate contacts are removed': the dupe is worth zero points and \
         earns no multiplier"
    );
}

/// ⚠️ **A dupe earns no MULTIPLIER — and the case that proves it is a MIS-COPIED one.**
///
/// A dupe that repeats the exchange exactly cannot move a multiplier count whatever the
/// code does, because a multiplier is a set of distinct values and the original already
/// contributed this one; a test built on that shape passes no matter what and would say
/// nothing. So the second copy carries a DIFFERENT zone, which is what actually happens
/// when an operator re-works a station and mis-copies it. CQ WW keys a dupe on call and
/// band alone, so it is still a duplicate — and if it reached the multiplier board it
/// would claim a zone the station never worked.
#[test]
fn a_mis_copied_dupe_claims_no_multiplier_it_never_worked() {
    let mut log = cqww_log();
    let mults = |l: &FieldDayLog| l.ruleset().scoring.mult_counts(l.score_rows());
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    let before = mults(&log);

    // The same station, same band — a dupe — but copied as zone 26 this time.
    assert!(work(&mut log, "20m", "JA1ABC", "26", 30));
    assert!(log.qsos()[1].dupe, "call + band is CQ WW's whole dupe key");
    assert_eq!(
        mults(&log),
        before,
        "a zone that only a duplicate carried was never worked, and must not be claimed"
    );
    assert_eq!(
        log.worked_values("ZN"),
        vec!["25".to_string()],
        "…and the multiplier board does not colour it in either"
    );
}

/// ⚠️ **SCORING-GRADE.** Field Day's deliverable is a dupe sheet plus **raw non-dupe**
/// QSO counts, and `qso_count` is what every summary surface reads. A dupe entering the
/// log must not enter the count, or the submitted summary sheet overstates the score.
#[test]
fn the_count_excludes_a_logged_dupe() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    assert!(work(&mut log, "20m", "DL1ABC", "14", 10));
    assert!(work(&mut log, "20m", "JA1ABC", "25", 30), "the dupe");

    assert_eq!(log.qsos().len(), 3, "three rows are in the log");
    assert_eq!(
        log.qso_count(),
        2,
        "…but the RAW NON-DUPE count is two — the number a summary sheet claims"
    );
}

/// The per-mode points sum is the other number a summary sheet reads, and it walks the
/// rows independently of `score_rows`. A dupe must be invisible to it too.
#[test]
fn the_points_sum_excludes_a_logged_dupe() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    let before = log.qso_points();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 30));
    assert_eq!(log.qso_points(), before, "a dupe adds no QSO points");
}

/// ⚠️ **The mark must survive a restart**, and it is RE-DERIVED from the dupe index on
/// the way back in rather than carried as a tag — the same rule `restore_row` already
/// applies to the serial run. A journal reload that un-marked the row would restore a
/// log that scores higher than the one that was written.
#[test]
fn a_logged_dupe_survives_the_journal_round_trip_still_marked() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    assert!(work(&mut log, "20m", "DL1ABC", "14", 10));
    assert!(work(&mut log, "20m", "JA1ABC", "25", 30), "the dupe");
    let journal = log.adif();

    let mut restored = cqww_log();
    restored.merge_adif(&journal, 0);

    assert_eq!(
        restored.qsos().len(),
        3,
        "every row came back, dupe included"
    );
    assert_eq!(restored.qso_count(), 2, "…and the count still excludes it");
    assert!(restored.qsos()[2].dupe, "…and it came back MARKED");
    assert_eq!(
        score(&restored),
        score(&log),
        "a restart rescores identically"
    );
}

/// ⚠️ **The mark stays INSIDE.** CQ's worked example logs a dupe as a plain `QSO:` line
/// and lets the checker zero it; `X-QSO` means "I want this EXCLUDED" and carries CQ's
/// own anti-abuse warning. Exporting our marker would be a different claim to the
/// sponsor than the one the operator made.
#[test]
fn a_logged_dupe_is_a_plain_qso_line_in_cabrillo() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    assert!(work(&mut log, "20m", "JA1ABC", "25", 30), "the dupe");
    let file = log.cabrillo(14_030).expect("a one-mode log exports");

    assert_eq!(
        file.lines().filter(|l| l.starts_with("QSO:")).count(),
        2,
        "both contacts are plain QSO: lines"
    );
    assert!(
        !file.contains("X-QSO"),
        "never X-QSO: that asks the sponsor to EXCLUDE the contact"
    );
}

// ---------------------------------------------------------------------------
// Field Day: unchanged, because nothing checks its log
// ---------------------------------------------------------------------------

/// ⚠️ **THE HALF THAT MUST NOT MOVE.** ARRL Field Day has no log checking — no
/// cross-check, no NIL, and the logs are not used — so the reason to keep a dupe does
/// not exist there. It is refused and nothing is written, exactly as it shipped.
#[test]
fn field_day_still_refuses_a_dupe() {
    let mut log = fd_log();
    assert!(
        log.log_mode_at("K1ABC", "2A", "CT", "CW", 0, START),
        "the first contact"
    );
    assert!(
        !log.log_mode_at("K1ABC", "2A", "CT", "CW", 0, START + 1800),
        "Field Day REFUSES the dupe — no log checking means no NIL to avoid"
    );
    assert_eq!(log.qsos().len(), 1, "and writes nothing");
    assert_eq!(log.qso_count(), 1);
}

/// Winter Field Day is the same event family and the same ruling.
#[test]
fn winter_field_day_still_refuses_a_dupe() {
    resolver();
    let mut log = FieldDayLog::new(
        "W9XYZ",
        ContestSession::field_day(FdEvent::WinterFd, "2O", "WI"),
        "20m",
    );
    assert!(log.log_mode_at("K1ABC", "2O", "WI", "CW", 0, START));
    assert!(!log.log_mode_at("K1ABC", "2O", "WI", "CW", 0, START + 1800));
    assert_eq!(log.qsos().len(), 1);
}

/// A dupe is still a DUPE to the while-typing verdict in a contest that logs it — the
/// operator is told before they commit, which is the whole reason the check exists. What
/// changed is what happens when they commit anyway.
#[test]
fn the_pre_log_verdict_still_calls_it_a_dupe() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    assert!(
        log.is_dupe_mode("JA1ABC", "CW"),
        "the cockpit still warns before the operator commits"
    );
}

// ---------------------------------------------------------------------------
// The serial run, which a logged dupe now advances
// ---------------------------------------------------------------------------

/// ⚠️ **SCORING-GRADE, and a consequence rather than a goal.** In a serial contest the
/// number is SENT before the contact is logged, so working a dupe spends it — the other
/// operator copied it and it is in their log. Refusing the row used to leave that number
/// in flight for the NEXT station, which is the failure `restore_row` names in its own
/// words: *"a re-issued SERIAL is copied by the other operator and lands in their log, so
/// two of my QSO lines claim a number the checker can only match to one of them."*
/// Logging the dupe records the number against the contact that actually received it.
#[test]
fn a_logged_dupe_spends_the_serial_it_sent_and_does_not_re_issue_it() {
    resolver();
    let rs = ruleset_by_id("cqwpx_cw", CURRENT_RULES_YEAR).expect("cqwpx_cw is shipped");
    let st = StationData {
        mycall: "W9XYZ".to_string(),
        contest_cq_zone: "4".to_string(),
        ..Default::default()
    };
    let mut log = FieldDayLog::new(
        "W9XYZ",
        ContestSession::for_ruleset(rs, &st).expect("cqwpx_cw session"),
        "20m",
    );
    // Compose first, exactly as the engine does when the over goes out: that is where a
    // serial is ISSUED, and it is issued whether or not the station turns out to be a dupe.
    let work_wpx = |log: &mut FieldDayLog, call: &str, at: u64| {
        log.session.compose_for(call, at);
        log.log_fields_at(call, &fields(&[("RST", "599")]), "CW", "", 0, at)
    };
    assert!(work_wpx(&mut log, "JA1ABC", START));
    assert!(work_wpx(&mut log, "JA1ABC", START + 1800), "the dupe");
    assert!(work_wpx(&mut log, "DL1ABC", START + 3600));

    assert!(log.qsos()[1].dupe, "same call, same band");
    assert_eq!(
        log.qsos().iter().map(|q| q.sent("NR")).collect::<Vec<_>>(),
        vec!["1", "2", "3"],
        "three stations copied three different numbers; none may be issued twice"
    );
}
