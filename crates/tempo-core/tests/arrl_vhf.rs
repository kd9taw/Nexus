//! ⭐ **One end-to-end run per ARRL VHF running** — January, June and September.
//!
//! The contest family that exercises the three things nothing else in the shipped set
//! does: a [`FieldKind::Grid`] exchange slot, a multiplier counted **per band** off that
//! slot, and QSO points priced by **band group** rather than by mode class or by where
//! the other station is. It is also the family where the sponsor's own point table is
//! not the same in all three runnings, which is why there are three rulesets and not one.
//!
//! Every number here is read off `JanJunSep-VHF-Rules.pdf` Version 1.2 (all 14 pages,
//! <https://contests.arrl.org/ContestRules/JanJunSep-VHF-Rules.pdf>, read 2026-09-09) and
//! the sponsor's sentence is quoted at the assertion, so a rules change is a red test
//! carrying the words it contradicts rather than a silent rescore.
//!
//! ⚠️ Integration tests: the public API only. If an operator cannot reach a contest
//! through it, neither can this file.
use tempo_core::contest::{ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

/// "Select the June VHF Contest" — the two calls the engine makes.
fn select(event_id: &str, station: &StationData) -> ContestSession {
    let rs = ruleset_by_id(event_id, CURRENT_RULES_YEAR)
        .unwrap_or_else(|| panic!("{event_id} is a shipped ruleset"));
    ContestSession::for_ruleset(rs, station)
        .unwrap_or_else(|e| panic!("{event_id} session refused: {e}"))
}

/// A VHF entrant: their own grid is the whole exchange they send.
fn station(grid: &str) -> StationData {
    StationData {
        mygrid: grid.to_string(),
        contest_qth_state: "WI".to_string(),
        ..Default::default()
    }
}

/// The entry strip's boxes, in order — `(slot id, required)`.
fn entry_boxes(s: &ContestSession) -> Vec<(&'static str, bool)> {
    s.role()
        .receives
        .iter()
        .filter_map(|k| s.exchange.field(k))
        .map(|f| (f.key, f.required))
        .collect()
}

/// What the session is composing, as `(slot, value)` pairs.
fn composing(s: &ContestSession) -> Vec<(&'static str, String)> {
    s.my_exchange
        .iter()
        .map(|v| (v.key, v.raw.clone()))
        .collect()
}

/// Work one station on a named band. ARRL VHF is a seven-band contest and both the
/// point table and the multiplier are functions of the band, so every contact names it.
fn work(log: &mut FieldDayLog, band: &str, call: &str, grid: &str, mode: &str, at: u64) -> bool {
    log.band = band.to_string();
    log.log_fields_at(
        call,
        &[("GRID".to_string(), grid.to_string())],
        mode,
        "",
        0,
        at,
    )
}

/// `(qso_points, powered, multipliers, total)` through the ruleset the log runs.
fn score(log: &FieldDayLog) -> (u32, u32, Option<u32>, u32) {
    log.ruleset().scoring.score(log.score_rows(), 1)
}

/// The seven contacts every end-to-end test below works, so the three runnings are
/// compared on ONE log and the only thing that varies is the point table.
///
/// `(band, call, their grid, mode class)` — one contact in each of the four band groups
/// the sponsor's table names, plus a second grid on 6 m and a second band for one call.
const SEVEN: [(&str, &str, &str, &str); 7] = [
    ("6m", "K2DEF", "FN31", "CW"),
    ("6m", "W1ABC", "FN42", "PH"),
    ("2m", "K2DEF", "FN31", "CW"),
    ("1.25m", "N0GHI", "EN61", "CW"),
    ("70cm", "K2DEF", "FN31", "CW"),
    ("23cm", "W4XYZ", "EM48", "CW"),
    ("13cm", "K5AAA", "EM12", "CW"),
];

/// The seven, logged into a fresh session for `event_id`, starting at that running's own
/// window opening.
fn seven(event_id: &str, year: u16) -> FieldDayLog {
    let s = select(event_id, &station("EN52"));
    let start = ruleset_by_id(event_id, CURRENT_RULES_YEAR)
        .expect("shipped")
        .event_window(year)
        .start_unix;
    let mut log = FieldDayLog::new("W9XYZ", s, "6m");
    for (i, (band, call, grid, mode)) in SEVEN.iter().enumerate() {
        assert!(
            work(&mut log, band, call, grid, mode, start + 60 * i as u64),
            "{call} on {band} must log"
        );
    }
    log
}

// ---------------------------------------------------------------------------
// The three runnings, end to end
// ---------------------------------------------------------------------------

/// ⭐ **The June VHF Contest, worked from Wisconsin, end to end.**
///
/// Sponsor (VHF-Rules.pdf v1.2, read 2026-09-09):
/// * exchange — §4.1 *"4-character Maidenhead grid-square locator"*, §4.2 *"A signal
///   report is optional."*;
/// * dupe — §2.2 *"Stations may be contacted for contest credit once per band from any
///   specific grid square."* and, on page 1, *"Contact stations only once per band."*;
/// * points — §5.2 *"QSO points for June and September contests"*, 1 / 2 / 3 / 4;
/// * multipliers — §5.3.1 *"The number of different grid squares contacted from each
///   band. Each grid square counts as a multiplier on each band."*;
/// * final score — §5.4.1 *"Fixed stations: total QSO points from all bands times the
///   total multipliers."*
#[test]
fn arrl_vhf_june_from_wisconsin_end_to_end() {
    let s = select("arrlvhf_jun", &station("EN52"));

    assert_eq!(s.contest_id, "ARRL-VHF-JUN");
    // THE ENTRY STRIP: one box, the grid. No RST — the sponsor's own worked example
    // carries none, and §4.2 makes the report optional rather than part of the exchange.
    assert_eq!(entry_boxes(&s), vec![("GRID", true)]);
    // THE SENT EXCHANGE: my own grid, from `mygrid`.
    assert_eq!(composing(&s), vec![("GRID", "EN52".into())]);
    // One symmetric role: *"W/VE stations contact any station."*
    assert_eq!(s.role().id, "");

    let mut log = seven("arrlvhf_jun", 2026);

    // THE DUPE VERDICT. §2.2 keys on the call, the band and the grid they sent — and
    // NOT on the mode: page 1's *"Contact stations only once per band"* has no mode
    // qualifier, so the same station on the same band on phone is still a dupe.
    let start = log.ruleset().event_window(2026).start_unix;
    assert!(
        !work(&mut log, "6m", "K2DEF", "FN31", "PH", start + 3600),
        "same call, same band, same grid — a dupe whatever the mode"
    );
    // POSITIVE CONTROL: the same station on a DIFFERENT band is a legal contact, so the
    // refusal above is the key and not a log that stopped accepting.
    assert!(work(&mut log, "2m", "W1ABC", "FN42", "CW", start + 3660));

    // THE SCORE. §5.2's table over the eight rows: 6 m 1 + 1, 2 m 1 + 1, 222 MHz 2,
    // 432 MHz 2, 1296 MHz 3, 2.3 GHz 4 = 15 QSO points.
    // §5.3.1's multiplier, per band: 6 m {FN31, FN42} = 2, 2 m {FN31, FN42} = 2,
    // 1.25 m {EN61} = 1, 70 cm {FN31} = 1, 23 cm {EM48} = 1, 13 cm {EM12} = 1 = 8.
    let (qso, powered, mults, total) = score(&log);
    assert_eq!((qso, powered), (15, 15), "no power tier in a VHF contest");
    assert_eq!(mults, Some(8), "each grid square counts once on each band");
    assert_eq!(total, 15 * 8);

    // THE CABRILLO the operator submits.
    let cab = log.cabrillo(50_125).expect("one entry");
    assert!(cab.contains("CONTEST: ARRL-VHF-JUN\n"), "{cab}");
    assert!(cab.contains("CALLSIGN: W9XYZ\n"), "{cab}");
    assert!(cab.contains("CATEGORY-OPERATOR: SINGLE-OP\n"), "{cab}");
    assert!(cab.contains("LOCATION: WI\n"), "{cab}");
    // ⚠️ 50 MHz and up is a BAND TOKEN, not kilohertz — and the line carries the grid
    // each side sent and nothing else. No RST column anywhere in the file.
    assert!(
        cab.contains("QSO: 50 CW 2026-06-13 1800 W9XYZ EN52 K2DEF FN31\n"),
        "{cab}"
    );
    assert!(
        cab.contains("QSO: 144 CW 2026-06-13 1802 W9XYZ EN52 K2DEF FN31\n"),
        "{cab}"
    );
    assert!(
        cab.contains("QSO: 222 CW 2026-06-13 1803 W9XYZ EN52 N0GHI EN61\n"),
        "{cab}"
    );
    assert!(
        cab.contains("QSO: 2.3G CW 2026-06-13 1806 W9XYZ EN52 K5AAA EM12\n"),
        "{cab}"
    );
    assert!(
        !cab.contains("599"),
        "the exchange carries no signal report\n{cab}"
    );
    // …and no trailing transmitter column: ARRL publishes no VHF QSO template and the
    // contest has no two-transmitter category.
    assert!(cab.contains("W9XYZ EN52 K5AAA EM12\n"), "{cab}");
    // The score is complete for the categories this build can declare, so no note.
    assert_eq!(log.ruleset().score_note_key, "");
}

/// ⭐ **The September VHF Contest** — the same rules as June, its own weekend and its own
/// ADIF id. §5.2 prices *"June and September contests"* together, so the two rulesets
/// carry the same table and this test is what proves they do.
#[test]
fn arrl_vhf_september_is_junes_table_on_its_own_weekend() {
    let s = select("arrlvhf_sep", &station("EN52"));
    assert_eq!(s.contest_id, "ARRL-VHF-SEP");

    let log = seven("arrlvhf_sep", 2026);
    let (qso, _, mults, total) = score(&log);
    // The seven, under §5.2: 1 + 1 + 1 + 2 + 2 + 3 + 4 = 14.
    assert_eq!(qso, 14);
    // 6 m {FN31, FN42}, 2 m {FN31}, 1.25 m {EN61}, 70 cm {FN31}, 23 cm {EM48},
    // 13 cm {EM12} = 7.
    assert_eq!(mults, Some(7));
    assert_eq!(total, 14 * 7);

    let cab = log.cabrillo(50_125).expect("one entry");
    assert!(cab.contains("CONTEST: ARRL-VHF-SEP\n"), "{cab}");
    // §1.2 — *"June and September: 1800 UTC Saturday through 0259 UTC Monday."*
    // Second full weekend of September 2026 is the 12th.
    assert!(
        cab.contains("QSO: 50 CW 2026-09-12 1800 W9XYZ EN52 K2DEF FN31\n"),
        "{cab}"
    );
}

/// ⭐ **The January VHF Contest** — its own weekend, its own start hour, and its own
/// point table.
#[test]
fn arrl_vhf_january_is_its_own_running_with_its_own_table() {
    let s = select("arrlvhf_jan", &station("EN52"));
    assert_eq!(s.contest_id, "ARRL-VHF-JAN");

    let log = seven("arrlvhf_jan", 2026);
    let (qso, _, mults, total) = score(&log);
    // The seven, under §5.1: 1 + 1 + 1 + 2 + 2 + 4 + 8 = 19.
    assert_eq!(qso, 19);
    assert_eq!(
        mults,
        Some(7),
        "the multiplier rule is shared, the table is not"
    );
    assert_eq!(total, 19 * 7);

    let cab = log.cabrillo(50_125).expect("one entry");
    assert!(cab.contains("CONTEST: ARRL-VHF-JAN\n"), "{cab}");
    // §1.1 — *"January: 1900 UTC Saturday through 0359 UTC Monday."* Third full weekend
    // of January 2026 is the 17th, which is the date ARRL's own contest calendar
    // announces (contests.arrl.org/calendar.php, read 2026-09-09: "Jan 17-19").
    assert!(
        cab.contains("QSO: 50 CW 2026-01-17 1900 W9XYZ EN52 K2DEF FN31\n"),
        "{cab}"
    );
}

// ---------------------------------------------------------------------------
// The finding the source verification made: one point table cannot serve three runnings
// ---------------------------------------------------------------------------

/// ⭐ **§5.1 and §5.2 are different tables, and the difference is 2× on 2.3 GHz and up.**
///
/// > *"5.1 QSO points for January contest: … 5.1.3 Count four points for each 902- or
/// > 1296-MHz QSO. 5.1.4 Count eight points for each 2.3 GHz (or higher) QSO."*
/// > *"5.2 QSO points for June and September contests: … 5.2.3 Count three points for
/// > each 902- or 1296-MHz QSO. 5.2.4 Count four points for each 2.3 GHz (or higher)
/// > QSO."*
///
/// The SAME seven contacts are scored under all three rulesets and the two totals must
/// differ. `assert_ne!` is the half that matters: a ruleset that quietly used the other
/// running's table would pass every equality above and fail only here.
#[test]
fn the_january_table_is_not_the_june_september_table() {
    let jan = score(&seven("arrlvhf_jan", 2026)).0;
    let jun = score(&seven("arrlvhf_jun", 2026)).0;
    let sep = score(&seven("arrlvhf_sep", 2026)).0;

    assert_eq!(jan, 19, "§5.1 — four on 1296 MHz, eight on 2.3 GHz");
    assert_eq!(jun, 14, "§5.2 — three on 1296 MHz, four on 2.3 GHz");
    assert_ne!(
        jan, jun,
        "one ByBandGroup table cannot serve both runnings — that is the whole reason \
         there are three rulesets"
    );
    assert_eq!(jun, sep, "§5.2 prices June and September together");

    // …and the divergence is exactly where the sponsor puts it: the two lower band
    // groups are identical in all three runnings.
    let low: [(&str, &str, &str, &str); 4] = [
        ("6m", "K2DEF", "FN31", "CW"),
        ("2m", "W1ABC", "FN42", "CW"),
        ("1.25m", "N0GHI", "EN61", "CW"),
        ("70cm", "W4XYZ", "EM48", "CW"),
    ];
    let mut totals = Vec::new();
    for event in ["arrlvhf_jan", "arrlvhf_jun", "arrlvhf_sep"] {
        let mut log = FieldDayLog::new("W9XYZ", select(event, &station("EN52")), "6m");
        for (i, (band, call, grid, mode)) in low.iter().enumerate() {
            assert!(work(&mut log, band, call, grid, mode, 100 + i as u64));
        }
        totals.push(score(&log).0);
    }
    assert_eq!(totals, vec![6, 6, 6], "1 + 1 + 2 + 2 in every running");
}

// ---------------------------------------------------------------------------
// The multiplier: a grid square, on each band
// ---------------------------------------------------------------------------

/// ⭐ §5.3.1 — *"The number of different grid squares contacted from each band. Each grid
/// square counts as a multiplier on each band."*
///
/// The same grid on two bands is TWO multipliers; two calls in one grid on one band is
/// ONE. Both directions, because a per-band count that collapsed to per-log would pass a
/// test that only worked one band.
#[test]
fn a_grid_square_counts_once_on_every_band_it_is_worked_on() {
    let mut log = FieldDayLog::new("W9XYZ", select("arrlvhf_jun", &station("EN52")), "6m");
    // FN31 on 6 m and again on 2 m — two multipliers out of one grid square.
    assert!(work(&mut log, "6m", "K2DEF", "FN31", "CW", 100));
    assert!(work(&mut log, "2m", "K2DEF", "FN31", "CW", 110));
    // A SECOND station in FN31, on 6 m — a QSO, never a second multiplier.
    assert!(work(&mut log, "6m", "W1ABC", "FN31", "CW", 120));
    assert_eq!(log.qso_count(), 3);

    let (qso, _, mults, total) = score(&log);
    assert_eq!(qso, 3, "three 50/144 MHz contacts at one point each");
    assert_eq!(
        mults,
        Some(2),
        "FN31 on 6 m and FN31 on 2 m — and W1ABC adds none"
    );
    assert_eq!(total, 6);
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("grid", 2)],
        "one multiplier universe, named `grid`"
    );
}

// ---------------------------------------------------------------------------
// The rover, both directions
// ---------------------------------------------------------------------------

/// ⭐ **Working SOMEBODY ELSE'S rover** — `by_fields`, the received half.
///
/// > §2.2.1 *"Rover stations using the same call sign may be contacted from more than one
/// > grid square."*
/// > VCAT.5.1.1 *"Rover stations may be worked from each four-character grid locator in
/// > which they operate."*
#[test]
fn a_rover_that_moves_is_a_new_contact_and_a_new_multiplier() {
    let mut log = FieldDayLog::new("W9XYZ", select("arrlvhf_jun", &station("EN52")), "2m");
    assert!(work(&mut log, "2m", "K2DEF", "FN31", "CW", 100));
    // Same call, same band, same grid — VCAT.5.1.4, *"only one contact is counted"*.
    assert!(
        !work(&mut log, "2m", "K2DEF", "FN31", "CW", 110),
        "the rover has not moved yet"
    );
    // …and the moment they cross into FN32 they are a new station.
    assert!(
        work(&mut log, "2m", "K2DEF", "FN32", "CW", 120),
        "§2.2.1 — the same call from another grid square"
    );

    let (qso, _, mults, _) = score(&log);
    assert_eq!(qso, 2);
    assert_eq!(mults, Some(2), "FN31 and FN32, both on 2 m");
}

/// ⭐ **BEING the rover** — `by_sent_fields`, and the §3.3 provenance rule the mobile fix
/// was built for: the exchange sent from grid A must stay recorded as grid A.
///
/// The station I work never moves and never changes what it sends, so nothing on the
/// received side can tell the two contacts apart. Only my own sent grid can — and the
/// row logged before the move must keep it, in the file I submit.
#[test]
fn being_the_rover_moves_the_key_and_never_relabels_a_row_already_logged() {
    let mut s = select("arrlvhf_jun", &station("EN52"));
    let mut log = FieldDayLog::new("W9XYZ/R", s.clone(), "2m");
    assert!(work(&mut log, "2m", "K2DEF", "FN31", "CW", 100));
    assert!(
        !work(&mut log, "2m", "K2DEF", "FN31", "CW", 110),
        "I have not moved yet — a dupe"
    );

    // ⭐ "I moved." §6.3 forbids a FIXED station changing location; a rover is the
    // category the whole rule exists for, and the grid slot is not a role selector, so
    // this is a move and not a new entry.
    log.session
        .move_to(&[("GRID", "EN53")])
        .expect("a rover changing grid square is a move, never a new entry");
    assert!(
        work(&mut log, "2m", "K2DEF", "FN31", "CW", 120),
        "a new grid on MY side is a new contact — nothing on theirs changed"
    );

    // ⭐ THE PROVENANCE. The first row still says EN53's predecessor.
    assert_eq!(log.qsos()[0].sent("GRID"), "EN52");
    assert_eq!(log.qsos()[1].sent("GRID"), "EN53");
    let cab = log.cabrillo(144_200).expect("one entry");
    assert!(
        cab.contains("W9XYZ/R EN52 K2DEF FN31\n"),
        "the contact made from EN52 is still recorded as EN52\n{cab}"
    );
    assert!(cab.contains("W9XYZ/R EN53 K2DEF FN31\n"), "{cab}");

    // …and the two grids I operated from are NOT multipliers here: §5.4.2's rover term
    // — *"plus one additional multiplier for every grid square from which a contact was
    // successfully completed"* — is not modelled, because it applies only to a rover
    // ENTRY and this build declares no rover category. §5.4.1's fixed-station formula is
    // what is computed, and it counts the grids WORKED.
    let (_, _, mults, _) = score(&log);
    assert_eq!(mults, Some(1), "FN31, once, on 2 m");
    // The unused session is the one the log took a copy of — the row's sent side comes
    // from the LOG's session, never from this one.
    assert_eq!(s.field("GRID"), "EN52");
    s.move_to(&[("GRID", "EN60")]).expect("a legal grid");
    assert_eq!(log.qsos()[1].sent("GRID"), "EN53", "a row is never re-read");
}

// ---------------------------------------------------------------------------
// The grid slot itself
// ---------------------------------------------------------------------------

/// ⭐ **A six-character locator goes on the air as the four the sponsor asks for**, and
/// something that is not a locator at all is refused before it is transmitted.
///
/// > §4.1 *"4-character Maidenhead grid-square locator"*
///
/// `mygrid` is the station's identity setting and a six-character value is correct there
/// — it is what awards want, and what an FT8 CQ is built from. The cut is made at the
/// exchange boundary, exactly as `qso::air_grid` makes it at the FT message boundary,
/// because a `EN52AA` on the air is a multiplier bucket of its own in every log that
/// copies it.
#[test]
fn a_six_character_grid_is_cut_to_four_and_a_non_grid_is_refused() {
    let s = select("arrlvhf_jun", &station("EN52aa"));
    assert_eq!(composing(&s), vec![("GRID", "EN52".into())]);

    // POSITIVE CONTROL: a four-character grid is untouched.
    let s4 = select("arrlvhf_jun", &station("FN42"));
    assert_eq!(composing(&s4), vec![("GRID", "FN42".into())]);

    // …and a value that is not a locator refuses the session rather than transmitting.
    let rs = ruleset_by_id("arrlvhf_jun", CURRENT_RULES_YEAR).expect("shipped");
    let err =
        ContestSession::for_ruleset(rs, &station("EN5")).expect_err("EN5 is not a grid square");
    assert!(err.contains("GRID"), "{err}");
    let err =
        ContestSession::for_ruleset(rs, &station("ENXX")).expect_err("ENXX is not a grid square");
    assert!(err.contains("GRID"), "{err}");
    // An empty grid is the one an operator who never filled the field in has.
    let err = ContestSession::for_ruleset(rs, &station("")).expect_err("no grid, no exchange");
    assert!(err.contains("GRID"), "{err}");

    // A move to a value that is not a grid is refused the same way, which is the site a
    // rover reaches.
    let mut s = select("arrlvhf_jun", &station("EN52"));
    let err = s.move_to(&[("GRID", "NOPE")]).expect_err("not a grid");
    assert!(err.contains("NOPE"), "{err}");
    assert_eq!(s.field("GRID"), "EN52", "a refused move changes nothing");
    // POSITIVE CONTROL: a real one is accepted, and a six-character one is cut.
    s.move_to(&[("GRID", "en53xx")]).expect("a legal locator");
    assert_eq!(s.field("GRID"), "EN53");
}

// ---------------------------------------------------------------------------
// The two registries, and the three weekends
// ---------------------------------------------------------------------------

/// ⭐ **Three runnings, three ADIF ids, three Cabrillo tokens, three weekends.**
///
/// ADIF 3.1.7's `Contest_ID` enumeration (<https://adif.org/317/ADIF_317.htm>, page
/// states *"updated 2026-03-22"*, read 2026-09-09) carries `ARRL-VHF-JAN` = *"ARRL
/// January VHF Sweepstakes"*, `ARRL-VHF-JUN` = *"ARRL June VHF QSO Party"* and
/// `ARRL-VHF-SEP` = *"ARRL September VHF QSO Party"*. The WA7BNM master list
/// (<https://www.contestcalendar.com/cabnames.php>, *"Revision Date: February 23,
/// 2026"*, read 2026-09-09) carries the same three strings at ids 231, 43 and 113.
///
/// The WEEKENDS are what make one row impossible whatever the tokens said: a
/// `WindowRule` holds ONE month, and these are January, June and September.
#[test]
fn the_three_runnings_carry_their_own_id_and_their_own_weekend() {
    for (event, id, month, start_h) in [
        ("arrlvhf_jan", "ARRL-VHF-JAN", 1u32, 19u64),
        ("arrlvhf_jun", "ARRL-VHF-JUN", 6, 18),
        ("arrlvhf_sep", "ARRL-VHF-SEP", 9, 18),
    ] {
        let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("shipped");
        assert_eq!(rs.contest_id, id);
        assert_eq!(rs.window.month, month);
        assert_eq!(rs.window.start_hour_utc, start_h);
        assert_eq!(rs.window.duration_hours, 33);
    }

    // The sponsor's own announced dates, from ARRL's contest calendar
    // (contests.arrl.org/calendar.php, read 2026-09-09): Jan VHF "Jan 17-19" (2026) and
    // "Jan 16-18" (2027); Jun VHF "Jun 13-15"; Sep VHF "Sep 12-14".
    let w = |e: &str, y: u16| {
        ruleset_by_id(e, CURRENT_RULES_YEAR)
            .expect("shipped")
            .event_window(y)
    };
    assert_eq!(w("arrlvhf_jan", 2026).start_unix, 1_768_676_400);
    assert_eq!(w("arrlvhf_jan", 2026).end_unix, 1_768_795_200);
    assert_eq!(w("arrlvhf_jan", 2027).start_unix, 1_800_126_000);
    assert_eq!(w("arrlvhf_jun", 2026).start_unix, 1_781_373_600);
    assert_eq!(w("arrlvhf_sep", 2026).start_unix, 1_789_236_000);
}
