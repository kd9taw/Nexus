//! ⭐ **Correcting a row already in the contest log — so the Cabrillo cannot disagree
//! with the log the operator can see.**
//!
//! A busted callsign fixed in the general log updated that log and stopped there. The
//! Cabrillo is written from [`FieldDayLog`]'s own rows, so the submitted file kept the
//! busted call while the ADIF showed the corrected one — silently, and in the file that
//! is scored. This is the propagation path, and the three things it has to get right
//! are each here with a test that fails when it is broken:
//!
//! 1. **The row is found by `seq`.** There is NO `qid` on a contest row; the qid is
//!    MINTED at merge time as `"<session>:<posid>:<seq>"`, so the contest-side identity
//!    is the seq and the qid is a function of it. ⚠️ A session id contains a colon of
//!    its own (`"CQ-WW-CW:IL"`), so the qid is parsed from the RIGHT.
//! 2. **Call-derived scoring re-resolves.** `entity`, `continent` and `prefix` are
//!    resolved once at log time and deliberately never re-read per snapshot — but a
//!    CORRECTION is the case that comment is not about. A corrected call keeping the
//!    busted call's country multiplier and WPX prefix is silent and scoring-grade.
//! 3. **Every row's dupe mark is re-derived.** The mark is order-derived against the
//!    dupe index, so correcting one row can make a LATER row stop being a duplicate —
//!    or start being one. Since a reporting ruleset can hold several rows under one
//!    key, the index cannot be patched; it is rebuilt.
use tempo_core::contest::{install_call_resolver, CallLocation, ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::{FdEvent, FieldDayLog};

/// The stub country file — the real resolver lives in a crate that depends on this one.
/// Two entities on different continents, so a corrected call visibly moves the score.
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

const START: u64 = 1_795_996_800;

/// A CQ WW CW log — a contest that REPORTS its duplicates, so the index is genuinely
/// multi-valued and a patch-the-key edit would be wrong.
fn cqww_log() -> FieldDayLog {
    resolver();
    let rs = ruleset_by_id("cqww_cw", CURRENT_RULES_YEAR).expect("cqww_cw is shipped");
    let st = StationData {
        mycall: "W9XYZ".to_string(),
        contest_cq_zone: "4".to_string(),
        contest_qth_state: "IL".to_string(),
        ..Default::default()
    };
    FieldDayLog::new(
        "W9XYZ",
        ContestSession::for_ruleset(rs, &st).expect("cqww_cw session"),
        "20m",
    )
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

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

fn score(log: &FieldDayLog) -> (u32, u32, Option<u32>, u32) {
    log.ruleset().scoring.score(log.score_rows(), 1)
}

/// The seq of the row at `i` — the identity a correction addresses.
fn seq(log: &FieldDayLog, i: usize) -> u64 {
    log.qsos()[i].seq
}

// ---------------------------------------------------------------------------
// The bug this exists for
// ---------------------------------------------------------------------------

/// ⭐ **THE REPRO.** The ADIF said the corrected call and the Cabrillo said the busted
/// one. Both are written from these rows, so once the correction lands here they agree
/// by construction — and this asserts BOTH files, because agreeing on the wrong value
/// would also pass a test that only read one.
#[test]
fn a_corrected_callsign_reaches_both_the_cabrillo_and_the_adif() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "K1ABC", "5", 0));
    let s = seq(&log, 0);

    assert!(log.correct_row(s, "K1ABD", "20m"), "the row is addressable");

    let cab = log.cabrillo(14_030).expect("one entry");
    assert!(
        cab.contains("K1ABD"),
        "the Cabrillo carries the correction\n{cab}"
    );
    assert!(!cab.contains("K1ABC"), "and not the busted call\n{cab}");
    let adif = log.adif();
    assert!(adif.contains("K1ABD"), "{adif}");
    assert!(!adif.contains("K1ABC"), "{adif}");
}

/// A wrong BAND is the other correction `Logbook::update_record` names in its own doc,
/// and it moves both the Cabrillo line and the per-band multiplier bucket.
///
/// ⚠️ The two stations share a ZONE and sit on different bands, so before the fix the
/// log claims that zone twice — once per band, which is how CQ WW counts. Moving the
/// second onto the first's band collapses them to one. A single-row version of this
/// test cannot fail: moving the only row from 20 m to 40 m still leaves exactly one
/// zone and one country in the totals, whatever the code does.
#[test]
fn a_corrected_band_moves_the_row_and_its_per_band_multiplier() {
    let mut log = cqww_log();
    let mults = |l: &FieldDayLog| l.ruleset().scoring.mult_counts(l.score_rows());
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    assert!(
        work(&mut log, "40m", "JA2XYZ", "25", 10),
        "zone 25 again, 40 m"
    );
    assert_eq!(
        mults(&log),
        vec![("zone", 2), ("country", 2)],
        "zone 25 counts once on each band it was worked on"
    );

    // The second contact was really on 20 m.
    let s = seq(&log, 1);
    assert!(log.correct_row(s, "JA2XYZ", "20m"));

    assert_eq!(log.qsos()[1].band, "20m");
    assert_eq!(
        mults(&log),
        vec![("zone", 1), ("country", 1)],
        "both contacts are now the same zone on the same band — one multiplier"
    );
    let cab = log.cabrillo(14_030).expect("one entry");
    assert!(
        !cab.contains(" 7000 "),
        "no 40 m line is left in the file\n{cab}"
    );
}

// ---------------------------------------------------------------------------
// Call-derived scoring
// ---------------------------------------------------------------------------

/// ⚠️ **SCORING-GRADE.** `entity`, `continent` and `prefix` are resolved once at log
/// time, and the comment at that site says they are never re-resolved — which is right
/// about a snapshot and wrong about a correction. A corrected call that kept the busted
/// call's country would claim a multiplier it never worked and price the contact off
/// the wrong continent.
#[test]
fn a_corrected_callsign_re_resolves_the_country_the_continent_and_the_prefix() {
    let mut log = cqww_log();
    // Logged as a Japanese station — different continent, 3 points under CQ WW.
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    assert_eq!(log.qsos()[0].entity.as_deref(), Some("Japan"));
    assert_eq!(log.qsos()[0].prefix.as_deref(), Some("JA1"));
    let points_as_ja = score(&log).0;

    // It was actually a German station.
    let s = seq(&log, 0);
    assert!(log.correct_row(s, "DL1ABC", "20m"));

    assert_eq!(
        log.qsos()[0].entity.as_deref(),
        Some("Fed. Rep. of Germany"),
        "the busted call's country must not ride along"
    );
    assert_eq!(log.qsos()[0].continent.as_deref(), Some("EU"));
    assert_eq!(
        log.qsos()[0].prefix.as_deref(),
        Some("DL1"),
        "CQ WPX's whole multiplier is this prefix"
    );
    // Both are 3-pointers from the US, so the POINTS are unchanged — which is exactly
    // why the fields are asserted above rather than inferred from a score that moved.
    assert_eq!(score(&log).0, points_as_ja);
}

/// A call the country file cannot place resolves to NOTHING rather than keeping the old
/// answer — the same rule, in the direction that a "only overwrite when we found
/// something" shortcut would get wrong.
#[test]
fn a_correction_to_an_unplaceable_call_clears_the_old_country() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "JA1ABC", "25", 0));
    let s = seq(&log, 0);

    assert!(log.correct_row(s, "QQ1ZZZ", "20m"));

    assert_eq!(
        log.qsos()[0].entity,
        None,
        "the country file cannot place QQ1ZZZ, and Japan is not the answer"
    );
    assert_eq!(log.qsos()[0].continent, None);
}

// ---------------------------------------------------------------------------
// The dupe index, which the correction can move under other rows
// ---------------------------------------------------------------------------

/// ⭐ **Correcting a row UN-DUPES a later one.** Row 2 duplicated row 1 only because
/// row 1 carried the busted call; once row 1 is corrected they are two different
/// stations, and row 2 is a real contact that must start counting.
#[test]
fn correcting_a_call_re_derives_a_later_rows_dupe_mark() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "K1ABC", "5", 0));
    assert!(work(&mut log, "20m", "K1ABC", "5", 30), "logged as a dupe");
    assert!(log.qsos()[1].dupe);
    assert_eq!(log.qso_count(), 1);

    // The FIRST contact was the busted one.
    let s = seq(&log, 0);
    assert!(log.correct_row(s, "K1ABD", "20m"));

    assert!(!log.qsos()[0].dupe);
    assert!(
        !log.qsos()[1].dupe,
        "K1ABC no longer duplicates anything — it is a contact with its own station"
    );
    assert_eq!(log.qso_count(), 2, "and it starts counting");
}

/// …and the other direction: correcting a call INTO a station already worked makes that
/// row the duplicate. A rebuild that only ever cleared marks would pass the test above
/// and fail this one.
#[test]
fn correcting_a_call_into_a_worked_station_makes_that_row_the_dupe() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "K1ABC", "5", 0));
    assert!(work(&mut log, "20m", "W2DEF", "5", 30));
    assert!(!log.qsos()[1].dupe);
    assert_eq!(log.qso_count(), 2);

    // The second contact was really the first station again.
    let s = seq(&log, 1);
    assert!(log.correct_row(s, "K1ABC", "20m"));

    assert!(!log.qsos()[0].dupe, "the earlier row keeps the contact");
    assert!(log.qsos()[1].dupe, "the later one becomes the duplicate");
    assert_eq!(log.qso_count(), 1);
}

/// ⚠️ **The rebuild and the log path must agree**, because they are two expressions of
/// one rule — "a row is a duplicate when an earlier row has its key". Rebuilding a log
/// that was built by logging must therefore change nothing at all. This is the test
/// that catches the two drifting apart.
#[test]
fn a_rebuild_changes_nothing_about_a_log_that_was_built_by_logging() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "K1ABC", "5", 0));
    assert!(work(&mut log, "20m", "W2DEF", "5", 10));
    assert!(work(&mut log, "20m", "K1ABC", "5", 20), "a dupe");
    assert!(work(&mut log, "40m", "K1ABC", "5", 30), "a new band");
    let before: Vec<bool> = log.qsos().iter().map(|q| q.dupe).collect();
    let count_before = log.qso_count();

    // A correction that changes NOTHING still runs the whole rebuild.
    let s = seq(&log, 0);
    assert!(log.correct_row(s, "K1ABC", "20m"));

    assert_eq!(
        log.qsos().iter().map(|q| q.dupe).collect::<Vec<_>>(),
        before,
        "the batch rebuild and the incremental log path disagree about the same rule"
    );
    assert_eq!(log.qso_count(), count_before);
}

// ---------------------------------------------------------------------------
// What a correction may NOT do
// ---------------------------------------------------------------------------

/// Seq 0 means UNSTAMPED, and `correct_row` refuses it rather than treating it as an
/// ordinal into the rows.
///
/// ⚠️ **WHAT THIS DOES NOT PROVE, stated because the assertion cannot say it.** Deleting
/// the `seq == 0` guard leaves this test — and the whole suite — GREEN, which was
/// measured, so it is not evidence that the guard fires. It cannot be: `next_seq` starts
/// at 1, the live write path takes it verbatim and the journal restore filters
/// `APP_NEXUS_QSEQ` to `> 0` before falling back to it, so **no row reachable through
/// this crate's API carries seq 0** and the `find` below returns `None` with or without
/// the guard. Against today's tree this is `an_unknown_sequence_is_refused` said twice.
///
/// It is kept, and the guard with it, because 0 is a SENTINEL that `contest::merge`
/// already acts on (it refuses such a row and mints no qid for it); what is pinned is
/// that the two agree about the meaning, so a future path that can produce an unstamped
/// row cannot silently start addressing the first one.
#[test]
fn a_row_with_no_sequence_cannot_be_addressed() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "K1ABC", "5", 0));
    assert!(
        !log.correct_row(0, "K1ABD", "20m"),
        "seq 0 means UNSTAMPED, never the first row"
    );
    assert_eq!(log.qsos()[0].call, "K1ABC", "and nothing was written");
}

/// An unknown sequence changes nothing and says so, rather than corrupting a row.
#[test]
fn an_unknown_sequence_is_refused() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "K1ABC", "5", 0));
    assert!(!log.correct_row(9_999, "K1ABD", "20m"));
    assert_eq!(log.qsos()[0].call, "K1ABC");
}

/// A correction never renumbers the log: the seq is the merge identity, and a row that
/// changed its seq would merge as a second contact rather than as a correction.
#[test]
fn a_correction_keeps_the_rows_sequence_and_its_timestamp() {
    let mut log = cqww_log();
    assert!(work(&mut log, "20m", "K1ABC", "5", 0));
    let (s, when) = (seq(&log, 0), log.qsos()[0].when_unix);

    assert!(log.correct_row(s, "K1ABD", "20m"));

    assert_eq!(log.qsos()[0].seq, s, "the merge identity is stable");
    assert_eq!(
        log.qsos()[0].when_unix,
        when,
        "a correction is not a re-log"
    );
}

// ---------------------------------------------------------------------------
// Field Day — the refusing ruleset corrects exactly the same way
// ---------------------------------------------------------------------------

/// Correcting is not a contest-policy question: Field Day refuses DUPLICATES, and a
/// correction is not a duplicate. The row is corrected and the refusal is untouched.
#[test]
fn field_day_corrects_a_row_and_still_refuses_a_dupe() {
    resolver();
    let mut log = FieldDayLog::new(
        "W9XYZ",
        ContestSession::field_day(FdEvent::ArrlFd, "2A", "WI"),
        "20m",
    );
    assert!(log.log_mode_at("K1ABC", "2A", "CT", "CW", 0, START));
    let s = log.qsos()[0].seq;

    assert!(log.correct_row(s, "K1ABD", "20m"));
    assert_eq!(log.qsos()[0].call, "K1ABD");

    // …and the corrected call is now the one that dupes.
    assert!(
        !log.log_mode_at("K1ABD", "2A", "CT", "CW", 0, START + 60),
        "Field Day still refuses, and the key followed the correction"
    );
    assert!(
        log.log_mode_at("K1ABC", "2A", "CT", "CW", 0, START + 120),
        "the busted call is free again — nothing in this log holds it"
    );
}

// ---------------------------------------------------------------------------
// The qid, which is how the general log names one of these rows
// ---------------------------------------------------------------------------

/// ⚠️ **A session id contains a colon of its own** (`"CQ-WW-CW:IL"`, built by BOTH
/// session constructors), so `"<session>:<posid>:<seq>"` has four colon-separated parts
/// and a left-to-right three-way split reads the posid as `"IL"` and the seq as
/// `"7:42"`. It is parsed from the RIGHT, and minted and parsed by one pair of
/// functions so the two cannot drift.
#[test]
fn a_qid_round_trips_through_a_session_id_that_contains_a_colon() {
    use tempo_core::contest::merge::{qid_for, seq_from_qid};
    let session = "CQ-WW-CW:IL";
    let qid = qid_for(session, "7", 42);
    assert_eq!(qid, "CQ-WW-CW:IL:7:42");
    assert_eq!(seq_from_qid(&qid, session), Some(42));
}

/// A qid minted by another session names a row this log does not own — refused, so a
/// correction cannot land on the same seq in the wrong contest.
#[test]
fn a_qid_from_another_session_names_no_row_here() {
    use tempo_core::contest::merge::{qid_for, seq_from_qid};
    let qid = qid_for("CQ-WW-CW:IL", "7", 42);
    assert_eq!(seq_from_qid(&qid, "ARRL-FIELD-DAY:WI"), None);
    assert_eq!(seq_from_qid("nonsense", "CQ-WW-CW:IL"), None);
    assert_eq!(
        seq_from_qid("CQ-WW-CW:IL:7:notanumber", "CQ-WW-CW:IL"),
        None
    );
}
