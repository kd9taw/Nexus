//! ⭐ **The New York QSO Party, end to end** — every number here is the sponsor's own.
//!
//! Source: the NYQP committee's own pages, read 2026-09-29 — <https://nyqp.org/wordpress/>,
//! its rules page <https://nyqp.org/wordpress/nyqp-rules/> and the rules PDF it serves
//! (*"2026 New York QSO Party Rules v1.1 2026-09-25"*), the county checklist PDF and CSV
//! (<https://nyqp.org/wordpress/counties/nyqp-county-checklist/>), and the log robot the rules
//! name, <https://nyqp.contesting.com/nyqpsubmitlog.php>. The sentences are quoted at the
//! assertions they decide, so a rules change is a red test with the sentence it contradicts
//! attached rather than a silent rescore.
//!
//! ⭐ **The sponsor's own sample log is the oracle for the score.** The rules close with a
//! complete sample Cabrillo file claiming `CLAIMED-SCORE: 1560`, and three of the rules this
//! file pins are exactly the ones a misreading moves: without New York counting as a state
//! multiplier the log scores 1482, with DX counted as a multiplier 1638, and with RTTY at two
//! points 1480. Only the reading below reproduces the sponsor's number.
//!
//! ⚠️ **The country file here is a STUB**, the shape `ilqp.rs` established: nothing NYQP
//! scores reads an entity, and the stub exists only so the location warning has a call to
//! place.
use tempo_core::contest::{
    boards, install_call_resolver, CabrilloEntrant, CallLocation, ContestSession, StationData,
};
use tempo_core::fd_rules::{ruleset_by_id, FdRuleset, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

/// The stub country file: enough to tell a US call from a German one.
fn place(call: &str) -> Option<CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    let entity = match up.as_str() {
        c if c.starts_with("DL") => "Fed. Rep. of Germany",
        c if c.starts_with('W') || c.starts_with('K') || c.starts_with('N') => "United States",
        _ => return None,
    };
    Some(CallLocation {
        entity,
        continent: "NA",
        cq_zone: None,
    })
}

fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(place).expect("the first install in this test binary");
    });
}

fn nyqp() -> &'static FdRuleset {
    ruleset_by_id("nyqp", CURRENT_RULES_YEAR).expect("nyqp is a shipped ruleset")
}

fn station(state: &str, county: &str) -> StationData {
    StationData {
        contest_qth_state: state.to_string(),
        contest_qth_county: county.to_string(),
        ..Default::default()
    }
}

fn select(station: &StationData) -> ContestSession {
    resolver();
    ContestSession::for_ruleset(nyqp(), station).unwrap_or_else(|e| panic!("nyqp refused: {e}"))
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// `(qso_points, powered, multipliers, total)` — the ruleset's own arithmetic.
fn score(log: &FieldDayLog) -> (u32, u32, Option<u32>, u32) {
    log.ruleset().scoring.score(log.score_rows(), 1)
}

/// 2026-10-17 14:00:00Z, the first minute of the 2026 running.
const START_2026: u64 = 1_792_245_600;

/// ⭐ **The window: the third Saturday of October, 1400Z, for twelve hours.**
///
/// *"Third Saturday in October: October 17, 2026 1400 UTC (10 AM Eastern) … 12 Hours"* and
/// *"12-hour Contest Period: October 17, 2026, from 14:00 UTC through 01:59 UTC."* The rules
/// file says "third FULL weekend", which is the same Saturday in every year there is: a
/// month's first Saturday always has its Sunday in the month, so the third Saturday is the
/// third full weekend's. Seven years are pinned, one for each weekday October can start on,
/// so that claim is checked rather than argued.
#[test]
fn the_window_is_the_third_saturday_of_october_from_1400z_for_twelve_hours() {
    let rs = nyqp();
    for (year, start, note) in [
        (
            2025,
            1_760_796_000,
            "Sat 18 Oct 2025 — October starts on a Wednesday",
        ),
        (
            2026,
            START_2026,
            "Sat 17 Oct 2026 — the sponsor's own date, a Thursday start",
        ),
        (2027, 1_823_695_200, "Sat 16 Oct 2027 — a Friday start"),
        (2028, 1_855_749_600, "Sat 21 Oct 2028 — a Sunday start"),
        (2029, 1_887_199_200, "Sat 20 Oct 2029 — a Monday start"),
        (2030, 1_918_648_800, "Sat 19 Oct 2030 — a Tuesday start"),
        (2033, 2_012_997_600, "Sat 15 Oct 2033 — a Saturday start"),
    ] {
        let w = rs.event_window(year);
        assert_eq!(w.start_unix, start, "{note}");
        assert_eq!(w.end_unix - w.start_unix, 12 * 3600, "{note}: twelve hours");
    }
}

/// ⭐ **The 62 counties and the sponsor's three-letter codes** — the list in the rules PDF
/// (*"New York County List"*), which the county checklist PDF and its CSV repeat code for
/// code. New York has 62 counties, so a list that parsed short is a multiplier nobody can
/// log.
#[test]
fn the_county_domain_is_the_sponsors_62_three_letter_codes() {
    let counties = nyqp()
        .domains
        .iter()
        .find(|d| d.id == "ny_counties")
        .expect("the county domain");
    assert_eq!(counties.values.len(), 62, "New York has 62 counties");
    assert!(
        counties.values.iter().all(|(c, _)| c.len() == 3),
        "*\"three-letter county abbreviation\"*, every one"
    );
    // The codes a first-three-letters rule gets WRONG, each a contact logged under the
    // wrong county if it is: Broome and Bronx, Chenango, Montgomery, and the three S-C
    // counties.
    for (code, name) in [
        ("BRM", "Broome"),
        ("BRX", "Bronx"),
        ("CGO", "Chenango"),
        ("MTG", "Montgomery"),
        ("SCH", "Schenectady"),
        ("SCO", "Schoharie"),
        ("SCU", "Schuyler"),
        ("STL", "St. Lawrence"),
        ("NEW", "New York"),
    ] {
        assert_eq!(
            counties.values.iter().find(|(c, _)| *c == code),
            Some(&(code, name)),
            "{code}"
        );
    }
    assert!(counties.contains("mon") && counties.contains(" MON "));
    // …and New York the STATE is not a county.
    assert!(!counties.contains("NY"));
}

/// ⭐ **The three exchanges.** *"New York stations send signal report and three-letter
/// county abbreviation."* *"Stations outside of New York send signal report plus state or
/// Canadian province (see list below). For those outside the US and Canada, send signal
/// report and "DX.""*
///
/// The last word is where New York parts from Illinois: a DX entrant here sends the literal
/// `DX`, where ILQP's sends its country.
#[test]
fn a_new_york_station_sends_its_county_and_a_dx_station_sends_dx() {
    let ny = select(&station("NY", "MON"));
    assert_eq!(ny.role().id, "in_state");
    assert_eq!(ny.field("RST"), "599");
    assert_eq!(ny.field("QTH"), "MON");

    let ct = select(&station("CT", ""));
    assert_eq!(ct.role().id, "w_ve");
    assert_eq!(ct.field("QTH"), "CT");
    let on = select(&station("ON", ""));
    assert_eq!(on.role().id, "w_ve", "a Canadian province is the W/VE role");
    assert_eq!(on.field("QTH"), "ON");

    let dx = select(&StationData {
        dxcc: true,
        ..station("", "")
    });
    assert_eq!(dx.role().id, "dx");
    assert_eq!(dx.field("QTH"), "DX", "the literal the sponsor names");
    // CONTROL, against the party this one is modelled on: ILQP's DX entrant sends the
    // country it typed, so the `DX` above is this ruleset's choice and not the session's.
    let il_dx = ContestSession::for_ruleset(
        ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("shipped"),
        &StationData {
            dxcc: true,
            ..station("GERMANY", "")
        },
    )
    .expect("ilqp session");
    assert_eq!(il_dx.field("QTH"), "GERMANY");
}

/// A county the sponsor does not list is refused BEFORE it goes on the air — the four-letter
/// abbreviation another party would use for the same county is the likely typo.
#[test]
fn a_county_code_the_sponsor_does_not_list_is_refused_at_session_start() {
    let e = ContestSession::for_ruleset(nyqp(), &station("NY", "MONR"))
        .expect_err("MONR is not an NYQP county");
    assert!(e.contains("MONR"), "{e}");
    // POSITIVE CONTROL: the sponsor's code for the same county starts a session.
    assert!(ContestSession::for_ruleset(nyqp(), &station("NY", "MON")).is_ok());
}

/// ⭐ **Points, and a station counted once per band per MODE, three modes deep.**
///
/// *"Each complete, non-duplicate Phone contact is worth 1 point per band. Each complete,
/// non-duplicate CW contact is worth 2 points per band. Each complete, non-duplicate RTTY (or
/// other valid digital mode) contact is worth 3 points per band. You may contact each
/// station once on each band and mode, for a maximum of three contacts per station, i.e.
/// Phone, CW, and RTTY/digital–one QSO on each mode, per band."* and *"All RTTY/digital modes
/// are considered to be the same mode."*
#[test]
fn phone_cw_and_digital_score_one_two_and_three_and_each_counts_once_per_band() {
    let mut log = FieldDayLog::new("W2XYZ", select(&station("NY", "ALB")), "40m");
    let suf = fields(&[("RST", "599"), ("QTH", "SUF")]);
    assert!(log.log_fields_at("K2AAA", &suf, "CW", "", 0, START_2026));
    // The same station, same band, on RTTY: a SECOND mode here — where ILQP counts CW and
    // digital as one.
    assert!(log.log_fields_at("K2AAA", &suf, "DIG", "RTTY", 0, START_2026 + 60));
    assert!(log.log_fields_at(
        "K2AAA",
        &fields(&[("RST", "59"), ("QTH", "SUF")]),
        "PH",
        "",
        0,
        START_2026 + 120
    ));
    assert_eq!(
        log.qso_count(),
        3,
        "phone, CW and RTTY: the sponsor's three"
    );
    assert_eq!(score(&log).0, 2 + 3 + 1);
    assert!(log.qsos().iter().all(|q| !q.dupe));
    // A second digital mode on the same band is the SAME mode — "all RTTY/digital modes
    // are considered to be the same mode" — so PSK after RTTY is a duplicate.
    assert!(log.log_fields_at("K2AAA", &suf, "DIG", "PSK31", 0, START_2026 + 180));
    assert!(
        log.qsos()[3].dupe,
        "RTTY then PSK31 on one band is one mode"
    );
    assert_eq!(log.qso_count(), 3, "and earns nothing");
    // CONTROL: ILQP's rule groups CW and digital, so the RTTY contact above is a dupe there.
    let mut il = FieldDayLog::new(
        "W9XYZ",
        ContestSession::for_ruleset(
            ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("shipped"),
            &station("IL", "COOK"),
        )
        .expect("ilqp session"),
        "40m",
    );
    let cook = fields(&[("RST", "599"), ("QTH", "COOK")]);
    assert!(il.log_fields_at("K9AAA", &cook, "CW", "", 0, START_2026));
    assert!(!il.log_fields_at("K9AAA", &cook, "DIG", "RTTY", 0, START_2026 + 60));
}

/// ⭐ **A duplicate is LOGGED, marked and worth nothing** — *"Do not remove duplicates from
/// your log before submission. They are used for cross-checking other logs."* ILQP says
/// nothing of the kind, so it refuses one; NYQP asks for the row.
///
/// (The flag itself is pinned in `dupe_logging.rs`'s policy table. This test asserts only
/// what it DOES, so a control that flips it reddens the behaviour and not a restatement.)
#[test]
fn a_duplicate_is_kept_in_the_log_and_the_file_and_scores_nothing() {
    let mut log = FieldDayLog::new("W2XYZ", select(&station("NY", "ALB")), "20m");
    let suf = fields(&[("RST", "599"), ("QTH", "SUF")]);
    assert!(log.log_fields_at("K2AAA", &suf, "CW", "", 0, START_2026));
    assert!(
        log.log_fields_at("K2AAA", &suf, "CW", "", 0, START_2026 + 60),
        "a duplicate still ENTERS the log"
    );
    assert!(log.qsos()[1].dupe);
    assert_eq!(log.qso_count(), 1);
    assert_eq!(
        score(&log),
        (2, 2, Some(2), 4),
        "1 county + New York, one contact"
    );
    let cab = log.cabrillo(14_000).expect("one entry");
    assert_eq!(
        cab.lines().filter(|l| l.starts_with("QSO:")).count(),
        2,
        "both rows reach the file the checker reads:\n{cab}"
    );
    // CONTROL: ILQP refuses the same second contact.
    let mut il = FieldDayLog::new(
        "W9XYZ",
        ContestSession::for_ruleset(
            ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("shipped"),
            &station("IL", "COOK"),
        )
        .expect("ilqp session"),
        "20m",
    );
    let cook = fields(&[("RST", "599"), ("QTH", "COOK")]);
    assert!(il.log_fields_at("K9AAA", &cook, "CW", "", 0, START_2026));
    assert!(!il.log_fields_at("K9AAA", &cook, "CW", "", 0, START_2026 + 60));
}

/// ⭐ **A New York station's multipliers: counties, the states (New York among them) and the
/// provinces — and DX is none of them.**
///
/// *"New York Stations: Count US states (50), New York Counties (62) and Canadian provinces
/// (13) … Maximum of 125 multipliers. (DX counts as QSO points but not multipliers.)"* and
/// *"The first valid New York county logged will count as the multiplier for New York."*
///
/// ⚠️ The New York multiplier is the one to watch: New York stations send counties, never
/// `NY`, so it is earned by the FIRST county and by no later one. A log of ONE county is the
/// case that tells "counts New York once" from "does not count New York" (2 vs 1), and a log
/// of THREE counties tells it from "counts every county twice" (4 vs 6).
#[test]
fn a_new_york_entrant_counts_counties_new_york_states_and_provinces_and_no_dx() {
    let mut log = FieldDayLog::new("W2XYZ", select(&station("NY", "ALB")), "20m");
    let mut t = START_2026;
    let mut work = |log: &mut FieldDayLog, call: &str, qth: &str| {
        t += 60;
        assert!(
            log.log_fields_at(
                call,
                &fields(&[("RST", "599"), ("QTH", qth)]),
                "CW",
                "",
                0,
                t
            ),
            "{call} must log"
        );
    };
    let counts = |log: &FieldDayLog| log.ruleset().scoring.mult_counts(log.score_rows());
    work(&mut log, "K2AAA", "SUF");
    assert_eq!(
        counts(&log),
        vec![("county", 1), ("ny", 1), ("mult", 0)],
        "one county is two multipliers: the county, and New York"
    );
    work(&mut log, "K2BBB", "MON");
    work(&mut log, "K2CCC", "ERI");
    assert_eq!(
        counts(&log),
        vec![("county", 3), ("ny", 1), ("mult", 0)],
        "…and New York is counted ONCE, however many counties follow"
    );
    // Two states, Hawaii among them (a US state here, not DX), and a province.
    work(&mut log, "W1AAA", "CT");
    work(&mut log, "KH6AAA", "HI");
    work(&mut log, "VE3AAA", "ON");
    // DX: three QSO points each, and no multiplier.
    work(&mut log, "DL1AAA", "DX");
    work(&mut log, "JA1AAA", "DX");
    assert_eq!(
        counts(&log),
        vec![("county", 3), ("ny", 1), ("mult", 3)],
        "CT, HI and ON; DX counts for points only"
    );
    let (qso, _, mults, total) = score(&log);
    assert_eq!(qso, 8 * 2, "eight CW contacts, the two DX among them");
    assert_eq!(mults, Some(7));
    assert_eq!(total, 16 * 7);
}

/// A New York station that works nobody in New York has no New York multiplier — the rule
/// is the first COUNTY logged, not the entrant's own state.
#[test]
fn a_new_york_entrant_earns_new_york_only_by_logging_a_county() {
    let mut log = FieldDayLog::new("W2XYZ", select(&station("NY", "ALB")), "20m");
    assert!(log.log_fields_at(
        "W1AAA",
        &fields(&[("RST", "599"), ("QTH", "CT")]),
        "CW",
        "",
        0,
        START_2026
    ));
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("county", 0), ("ny", 0), ("mult", 1)]
    );
}

/// ⭐ **Everyone else counts New York counties, and nothing else** — *"US/Canada/DX
/// Stations: Count New York counties for a maximum of 62 multipliers."*
#[test]
fn an_entrant_outside_new_york_counts_only_new_york_counties() {
    for entrant in [
        station("CT", ""),
        station("ON", ""),
        StationData {
            dxcc: true,
            ..station("", "")
        },
    ] {
        let mut log = FieldDayLog::new("W1ABC", select(&entrant), "20m");
        for (i, (call, qth)) in [("K2AAA", "SUF"), ("K2BBB", "MON"), ("K2CCC", "SUF")]
            .into_iter()
            .enumerate()
        {
            assert!(log.log_fields_at(
                call,
                &fields(&[("RST", "599"), ("QTH", qth)]),
                "CW",
                "",
                0,
                START_2026 + 60 * i as u64
            ));
        }
        assert_eq!(
            log.ruleset().scoring.mult_counts(log.score_rows()),
            vec![("county", 2), ("ny", 0), ("mult", 0)],
            "{}: two counties; no New York, states or provinces",
            log.session.role().id
        );
        assert_eq!(score(&log).3, 6 * 2);
    }
}

/// ⭐ **Mobiles, portables and county lines — the ILQP shape, from New York's sentences.**
///
/// *"New York stations that change counties are considered to be a new station and may be
/// contacted again for points and multiplier credit."* (they moved: the received county is
/// in the dupe key); *"With each county change, the station may again make up to three mode
/// contacts per band"* (I moved: the SENT county is too); and *"County line exchanges should
/// be logged as two separate QSOs"* — the operator logs the contact once per county, and the
/// county-keyed rule admits each. Nothing here multiplies a single row, exactly as in ILQP.
#[test]
fn a_county_change_on_either_side_is_a_new_contact_and_a_county_line_is_two() {
    // (The rule's two slot lists are pinned in `fd_rules`'s party table; this asserts what
    // they DO, so a control that drops one reddens the contact it would refuse.)
    let mut log = FieldDayLog::new("W2XYZ", select(&station("NY", "DUT")), "40m");
    // THEY are on the Dutchess/Putnam line, sending "DUT/PUT": two rows, one per county.
    for (i, qth) in ["DUT", "PUT"].into_iter().enumerate() {
        assert!(log.log_fields_at(
            "KX2AAA",
            &fields(&[("RST", "599"), ("QTH", qth)]),
            "CW",
            "",
            0,
            START_2026 + i as u64
        ));
    }
    assert!(
        log.qsos().iter().all(|q| !q.dupe),
        "two counties, two contacts"
    );
    // I move to Putnam and work the same station again — its exchange is unchanged, and the
    // contact is new because MY county is.
    log.session
        .move_to(&[("QTH", "PUT")])
        .expect("Dutchess to Putnam is a move within New York");
    assert!(log.log_fields_at(
        "KX2AAA",
        &fields(&[("RST", "599"), ("QTH", "PUT")]),
        "CW",
        "",
        0,
        START_2026 + 120
    ));
    assert!(!log.qsos()[2].dupe, "I moved, so it is a new contact");
    assert_eq!(log.qsos()[2].sent("QTH"), "PUT");
    assert_eq!(
        log.qsos()[0].sent("QTH"),
        "DUT",
        "and the rows already logged keep the county they were sent from"
    );
    // CONTROL: the same contact again from Putnam IS a duplicate.
    assert!(log.log_fields_at(
        "KX2AAA",
        &fields(&[("RST", "599"), ("QTH", "PUT")]),
        "CW",
        "",
        0,
        START_2026 + 180
    ));
    assert!(log.qsos()[3].dupe);
}

/// ⭐ **One board per universe.** New York itself is a multiplier drawn from the county
/// list, so the rule that counts it reads the same slot and the same domain as the county
/// board — and a second 62-cell grid showing "9 of 62" for a term worth at most 1 would be
/// a board that lies.
#[test]
fn the_new_york_multiplier_does_not_draw_a_second_county_board() {
    let rs = nyqp();
    let ny = select(&station("NY", "ALB"));
    let ids = |s: &ContestSession| -> Vec<&str> {
        boards(&rs.scoring, rs.exchange, s.role())
            .iter()
            .map(|b| b.id)
            .collect()
    };
    assert_eq!(ids(&ny), ["county", "mult"]);
    assert_eq!(ids(&select(&station("CT", ""))), ["county"]);
    // CONTROL: ILQP's two boards are untouched.
    let il = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("shipped");
    let il_s = ContestSession::for_ruleset(il, &station("IL", "COOK")).expect("ilqp session");
    assert_eq!(
        boards(&il.scoring, il.exchange, il_s.role())
            .iter()
            .map(|b| b.id)
            .collect::<Vec<_>>(),
        ["county", "mult"]
    );
}

/// ⭐ **FT8 and FT4 earn nothing, and the bands are the sponsor's.**
///
/// *"Only digital modes that natively support the NYQP exchange are permitted for contest
/// credit."* The sponsor names no mode, so the ban is this build's reading, and it is read
/// off WSJT-X's own packer rather than assumed: the one FT8/FT4 message with a contest
/// exchange (`i3=3`, `libtempo/vendor/wsjtx/lib/77bit/packjt77.f90`) carries a report and one
/// entry of a fixed table of states, provinces and serials, and no New York county is in it.
/// RTTY and PSK are keyboard modes and carry anything.
///
/// *"All FCC allocated amateur frequencies (excluding the 30, 17, and 12 meter bands) are
/// available for QSO credit."* — so 60 m IS in (ILQP excludes it) and so is everything from
/// 6 m up; the sponsor's own sample log works 50, 144, 222, 432, 902, 1.2G and 10G. "FCC
/// allocated" is read off 47 CFR 97.301 as eCFR serves it (read 2026-09-29), which has no
/// 4 m band and no longer any 3.3–3.5 GHz (9 cm) one.
#[test]
fn ft8_and_ft4_are_banned_and_the_bands_are_the_sponsors() {
    let rs = nyqp();
    assert!(rs.mode_banned("FT8") && rs.mode_banned("FT4"));
    assert!(!rs.mode_banned("RTTY") && !rs.mode_banned("PSK31"));
    for band in ["60m", "6m", "1.25m", "23cm", "3cm"] {
        assert!(rs.bands.contains(&band), "{band} is an FCC allocation");
    }
    for band in ["30m", "17m", "12m"] {
        assert!(!rs.bands.contains(&band), "{band} is excluded by name");
    }
    for band in ["4m", "9cm"] {
        assert!(!rs.bands.contains(&band), "{band} is no FCC allocation");
    }
}

/// The sponsor's sample file, QSO lines only, whitespace collapsed (Cabrillo's fields are
/// whitespace-separated, so the padding carries nothing). Quoted from the rules PDF's
/// *"Sample Cabrillo File"*; its header block carries a real entrant's name and address and
/// is deliberately not reproduced.
const SPONSOR_SAMPLE: &str = "\
QSO: 14006 CW 2022-09-05 2117 N2ZN 599 MON KH7X 599 HI
QSO: 14006 CW 2022-09-05 2118 N2ZN 599 MON W2VJN 599 OR
QSO: 7234 PH 2022-09-05 2118 N2ZN 59 MON KR2N 59 SUF
QSO: 7234 PH 2022-09-05 2118 N2ZN 59 MON KD2RD 59 SUF
QSO: 7234 PH 2022-09-05 2118 N2ZN 59 MON KS2G 59 NAS
QSO: 7234 PH 2022-09-05 2118 N2ZN 59 MON W2RTY 59 WAY
QSO: 7050 CW 2022-09-05 2118 N2ZN 599 MON K2UA 599 ULS
QSO: 50 CW 2022-09-05 2119 N2ZN 599 MON K2UA 599 ULS
QSO: 144 CW 2022-09-05 2119 N2ZN 599 MON K2UA 599 ULS
QSO: 7020 CW 2022-09-05 2119 N2ZN 599 MON N2MG 599 ONE
QSO: 7020 CW 2022-09-05 2119 N2ZN 599 MON WJ2O 599 ONE
QSO: 7020 CW 2022-09-05 2119 N2ZN 599 MON KU2M 599 NJ
QSO: 7020 CW 2022-09-05 2119 N2ZN 599 MON N9RV 599 MT
QSO: 7020 CW 2022-09-05 2120 N2ZN 599 MON WW2DX 599 DUT
QSO: 1.2G FM 2022-09-05 2120 N2ZN 59 MON K2UA 59 ULS
QSO: 1.2G FM 2022-09-05 2120 N2ZN 59 MON N2MG 59 ONE
QSO: 902 FM 2022-09-05 2120 N2ZN 59 MON K2UA 59 ULS
QSO: 432 FM 2022-09-05 2120 N2ZN 59 MON K2UA 59 ULS
QSO: 10G CW 2022-09-05 2121 N2ZN 599 MON K2UA 599 ULS
QSO: 3539 CW 2022-09-05 2121 N2ZN 599 MON KW8N 599 OH
QSO: 3539 CW 2022-09-05 2121 N2ZN 599 MON KR2N 599 SUF
QSO: 3539 CW 2022-09-05 2121 N2ZN 599 MON VE3NZ 599 ON
QSO: 3539 RY 2022-09-05 2121 N2ZN 599 MON K2RNY 599 MON
QSO: 3539 RY 2022-09-05 2122 N2ZN 599 MON KA2S 599 SUF
QSO: 3539 RY 2022-09-05 2122 N2ZN 599 MON KA2D 599 SUF
QSO: 3539 RY 2022-09-05 2122 N2ZN 599 MON N6DX 599 CA
QSO: 14249 PH 2022-09-05 2122 N2ZN 59 MON K1TO 59 FL
QSO: 14249 PH 2022-09-05 2122 N2ZN 59 MON K2UA 59 ULS
QSO: 14249 PH 2022-09-05 2122 N2ZN 59 MON W1RM 59 CT
QSO: 14029 CW 2022-09-05 2122 N2ZN 599 MON W1RM 599 CT
QSO: 432 CW 2022-09-05 2122 N2ZN 599 MON W1RM 599 CT
QSO: 222 CW 2022-09-05 2123 N2ZN 599 MON W1RM 599 CT
QSO: 10G CW 2022-09-05 2123 N2ZN 599 MON W1RM 599 CT
QSO: 10G CW 2022-09-05 2123 N2ZN 599 MON W2FU 599 MON
QSO: 14035 CW 2022-09-05 2123 N2ZN 599 MON N2MG 599 ONE
QSO: 14035 CW 2022-09-05 2123 N2ZN 599 MON N1RR 599 MA
QSO: 14035 CW 2022-09-05 2123 N2ZN 599 MON KB2TYO 599 ALB
QSO: 14035 CW 2022-09-05 2125 N2ZN 599 MON K1ZM 599 DUT
QSO: 14035 CW 2022-09-05 2125 N2ZN 599 MON ZS1EL 599 DX
QSO: 14035 CW 2022-09-05 2125 N2ZN 599 MON JA3YBK 599 DX
QSO: 14035 CW 2022-09-05 2125 N2ZN 599 MON BG7QYX/9 599 DX
QSO: 14250 PH 2022-09-05 2125 N2ZN 59 MON KB2CHM 59 MON
QSO: 14250 PH 2022-09-05 2125 N2ZN 59 MON K2SA 59 MON
QSO: 14250 PH 2022-09-05 2125 N2ZN 59 MON N2WK 59 ORL
";

/// The band a sample line's frequency column names: kHz below 30 MHz, Cabrillo's band
/// tokens above it.
fn sample_band(freq: &str) -> &'static str {
    match freq {
        "50" => "6m",
        "144" => "2m",
        "222" => "1.25m",
        "432" => "70cm",
        "902" => "33cm",
        "1.2G" => "23cm",
        "10G" => "3cm",
        khz => match khz.parse::<u32>().expect("an HF line carries kHz") {
            3_500..=4_000 => "80m",
            7_000..=7_300 => "40m",
            14_000..=14_350 => "20m",
            other => panic!("the sample has no line on {other} kHz"),
        },
    }
}

/// ⭐⭐ **THE SPONSOR'S OWN SAMPLE LOG SCORES THE SPONSOR'S OWN CLAIMED SCORE — and the QSO
/// lines this build writes for those contacts are the sponsor's own lines.**
///
/// The sample is a portable entry from Monroe county claiming `CLAIMED-SCORE: 1560`. Its 44
/// contacts are 26 CW, 14 phone (four of them FM) and 4 RTTY — 78 QSO points — against 9
/// counties, New York, 9 states and Ontario: 20 multipliers. Three DX stations send `DX` and
/// count for points alone.
///
/// ⚠️ One token differs by the sponsor's own leave: the four FM lines come back `PH`, and
/// *"PH and FM (only) are valid modes for Phone"*.
#[test]
fn the_sponsors_sample_log_scores_1560_and_writes_the_sponsors_own_lines() {
    let n2zn = StationData {
        mycall: "N2ZN".into(),
        ..station("NY", "MON")
    };
    let mut log = FieldDayLog::new("N2ZN", select(&n2zn), "20m");
    for line in SPONSOR_SAMPLE.lines() {
        let t: Vec<&str> = line.split_whitespace().collect();
        let [_, freq, mo, _date, time, _me, _rst_s, _qth_s, call, rst, qth] = t[..] else {
            panic!("a sample line has eleven fields: {line}");
        };
        let (class, submode) = match mo {
            "CW" => ("CW", ""),
            "PH" => ("PH", ""),
            "FM" => ("PH", "FM"),
            "RY" => ("DIG", "RTTY"),
            other => panic!("the sample has no {other} line"),
        };
        log.band = sample_band(freq).to_string();
        log.dial_khz = freq.parse().unwrap_or(0);
        let hhmm: u64 = time.parse().expect("hhmm");
        let when = 1_662_336_000 + (hhmm / 100) * 3600 + (hhmm % 100) * 60; // 2022-09-05
        assert!(
            log.log_fields_at(
                call,
                &fields(&[("RST", rst), ("QTH", qth)]),
                class,
                submode,
                0,
                when
            ),
            "{line}"
        );
    }
    assert_eq!(log.qso_count(), 44, "the sample holds no duplicate");
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("county", 9), ("ny", 1), ("mult", 10)]
    );
    assert_eq!(score(&log), (78, 78, Some(20), 1560));

    let cab = log
        .cabrillo_with(14_000, &CabrilloEntrant::default())
        .expect("one entry");
    assert!(cab.contains("CONTEST: NY-QSO-PARTY\n"), "{cab}");
    assert!(cab.contains("LOCATION: MON\n"), "{cab}");
    assert!(cab.contains("CLAIMED-SCORE: 1560\n"), "{cab}");
    let ours: Vec<&str> = cab.lines().filter(|l| l.starts_with("QSO:")).collect();
    let theirs: Vec<String> = SPONSOR_SAMPLE
        .lines()
        .map(|l| l.replace(" FM ", " PH "))
        .collect();
    assert_eq!(ours, theirs, "the sponsor's own QSO lines, token for token");
}

/// ⭐ **The file an in-state entry submits, whole** — written by hand from the sponsor's
/// Cabrillo specification, not captured from the writer: *"CONTEST: NY-QSO-PARTY"*; *"For
/// fixed stations in New York, use your NYQP-approved three-letter county abbreviation"* in
/// `LOCATION`; the sponsor's own mode tokens (*"RY = RTTY"*); and the headers this build can
/// source out of the sponsor's sample header block (the category declarations, the claimed
/// score, the entrant's name and e-mail). `CLUB`, `OPERATORS`, `ADDRESS*`,
/// `CATEGORY-STATION`, `-TRANSMITTER` and `-OVERLAY` have no source here and are left out.
///
/// The score, by hand: CW 2 × 3 + phone 1 + RTTY 3 = 10 points; SUF and MON, New York, CT and
/// PA = 5 multipliers; 50. The DX contact is points only.
#[test]
fn an_in_state_entrys_cabrillo_file_is_the_sponsors_format() {
    let entrant = StationData {
        mycall: "W2XYZ".into(),
        contest_category_power: "LOW".into(),
        contest_category_assisted: "NON-ASSISTED".into(),
        ..station("NY", "ALB")
    };
    let mut log = FieldDayLog::new("W2XYZ", select(&entrant), "20m");
    for (i, (band, dial, class, submode, call, rst, qth)) in [
        ("20m", 14_025, "CW", "", "K2ABC", "599", "SUF"),
        ("40m", 7_025, "CW", "", "W1ABC", "599", "CT"),
        ("20m", 14_030, "CW", "", "DL1ABC", "599", "DX"),
        ("40m", 7_200, "PH", "", "KB2DEF", "59", "MON"),
        ("80m", 3_580, "DIG", "RTTY", "K3GHI", "599", "PA"),
    ]
    .into_iter()
    .enumerate()
    {
        log.band = band.into();
        log.dial_khz = dial;
        assert!(log.log_fields_at(
            call,
            &fields(&[("RST", rst), ("QTH", qth)]),
            class,
            submode,
            0,
            START_2026 + 60 * i as u64
        ));
    }
    let me = CabrilloEntrant {
        name: "EXAMPLE OPERATOR".into(),
        email: "op@example.com".into(),
        ..Default::default()
    };
    assert_eq!(
        log.cabrillo_with(14_000, &me).expect("one entry"),
        "START-OF-LOG: 3.0\n\
         CONTEST: NY-QSO-PARTY\n\
         CALLSIGN: W2XYZ\n\
         CATEGORY-OPERATOR: SINGLE-OP\n\
         CATEGORY-ASSISTED: NON-ASSISTED\n\
         CATEGORY-POWER: LOW\n\
         LOCATION: ALB\n\
         CLAIMED-SCORE: 50\n\
         CREATED-BY: Nexus\n\
         EMAIL: op@example.com\n\
         NAME: EXAMPLE OPERATOR\n\
         X-NEXUS-RULES-YEAR: 2026\n\
         QSO: 14025 CW 2026-10-17 1400 W2XYZ 599 ALB K2ABC 599 SUF\n\
         QSO: 7025 CW 2026-10-17 1401 W2XYZ 599 ALB W1ABC 599 CT\n\
         QSO: 14030 CW 2026-10-17 1402 W2XYZ 599 ALB DL1ABC 599 DX\n\
         QSO: 7200 PH 2026-10-17 1403 W2XYZ 59 ALB KB2DEF 59 MON\n\
         QSO: 3580 RY 2026-10-17 1404 W2XYZ 599 ALB K3GHI 599 PA\n\
         END-OF-LOG:\n"
    );
}

/// ⭐ **The file an out-of-state entry submits, whole** — *"For fixed stations outside New
/// York, use your state abbreviation, Canadian province abbreviation, or DX"* — and a DX
/// entry's `LOCATION` is `DX`.
///
/// The score, by hand: CW 2 + phone 1 + RTTY 3 = 6 points (the second contact with W2AAA is
/// a new MODE on the same band), ALB and NEW = 2 multipliers; 12.
#[test]
fn an_out_of_state_entrys_cabrillo_file_is_the_sponsors_format() {
    let entrant = StationData {
        mycall: "W1ABC".into(),
        ..station("CT", "")
    };
    let mut log = FieldDayLog::new("W1ABC", select(&entrant), "20m");
    for (i, (band, dial, class, submode, call, rst, qth)) in [
        ("20m", 14_025, "CW", "", "W2AAA", "599", "ALB"),
        ("40m", 7_200, "PH", "", "K2BBB", "59", "NEW"),
        ("20m", 14_080, "DIG", "RTTY", "W2AAA", "599", "ALB"),
    ]
    .into_iter()
    .enumerate()
    {
        log.band = band.into();
        log.dial_khz = dial;
        assert!(log.log_fields_at(
            call,
            &fields(&[("RST", rst), ("QTH", qth)]),
            class,
            submode,
            0,
            START_2026 + 60 * i as u64
        ));
    }
    assert_eq!(
        log.cabrillo_with(14_000, &CabrilloEntrant::default())
            .expect("one entry"),
        "START-OF-LOG: 3.0\n\
         CONTEST: NY-QSO-PARTY\n\
         CALLSIGN: W1ABC\n\
         CATEGORY-OPERATOR: SINGLE-OP\n\
         LOCATION: CT\n\
         CLAIMED-SCORE: 12\n\
         CREATED-BY: Nexus\n\
         X-NEXUS-RULES-YEAR: 2026\n\
         QSO: 14025 CW 2026-10-17 1400 W1ABC 599 CT W2AAA 599 ALB\n\
         QSO: 7200 PH 2026-10-17 1401 W1ABC 59 CT K2BBB 59 NEW\n\
         QSO: 14080 RY 2026-10-17 1402 W1ABC 599 CT W2AAA 599 ALB\n\
         END-OF-LOG:\n"
    );
    // A DX entry heads `LOCATION: DX`, the third of the sponsor's three.
    let mut dx = FieldDayLog::new(
        "DL1ABC",
        select(&StationData {
            mycall: "DL1ABC".into(),
            dxcc: true,
            ..station("", "")
        }),
        "20m",
    );
    assert!(dx.log_fields_at(
        "W2AAA",
        &fields(&[("RST", "599"), ("QTH", "ALB")]),
        "CW",
        "",
        0,
        START_2026
    ));
    let cab = dx.cabrillo(14_000).expect("one entry");
    assert!(cab.contains("\nLOCATION: DX\n"), "{cab}");
    assert!(cab.contains(" DL1ABC 599 DX W2AAA 599 ALB\n"), "{cab}");
}

/// ⭐ **A MOBILE's `LOCATION` is a county it ACTIVATED** — *"For mobile and portable stations
/// in New York, use the NYQP-approved three-letter county abbreviation of any county
/// activated during the contest."* A mobile that has moved on and not yet made a contact
/// from its new county has not activated it, so the header reads the county its first
/// contact was sent from, not the county it is composing now.
#[test]
fn a_mobiles_location_is_a_county_it_actually_worked_from() {
    let mut log = FieldDayLog::new("W2XYZ", select(&station("NY", "DUT")), "40m");
    assert!(log.log_fields_at(
        "K2AAA",
        &fields(&[("RST", "599"), ("QTH", "SUF")]),
        "CW",
        "",
        0,
        START_2026
    ));
    log.session
        .move_to(&[("QTH", "PUT")])
        .expect("a move within New York");
    let cab = log.cabrillo(7_000).expect("one entry");
    assert!(
        cab.contains("\nLOCATION: DUT\n"),
        "Putnam has no contact yet:\n{cab}"
    );
    // CONTROL: an entry with nothing logged has activated nothing, and heads the county
    // it is sending from.
    let empty = FieldDayLog::new("W2XYZ", select(&station("NY", "PUT")), "40m");
    assert!(empty
        .cabrillo(7_000)
        .expect("one entry")
        .contains("\nLOCATION: PUT\n"));
}

/// ⭐ **The warning ILQP's entrants get reaches New York's**: a section typed where the
/// state belongs is named, with the state to type instead (`EMA` is Eastern Massachusetts,
/// so `MA`).
#[test]
fn a_section_typed_where_a_state_belongs_is_warned_about() {
    let s = select(&StationData {
        mycall: "W1ABC".to_string(),
        ..station("EMA", "")
    });
    assert_eq!(
        s.role().id,
        "dx",
        "a section is in neither exchange universe"
    );
    let w = s
        .location_warning
        .as_ref()
        .expect("a W/VE call about to send DX");
    assert_eq!(w.hints, vec!["MA"]);
    // CONTROL: the state itself is the w_ve role, and no warning.
    let ok = select(&StationData {
        mycall: "W1ABC".to_string(),
        ..station("MA", "")
    });
    assert_eq!(ok.role().id, "w_ve");
    assert!(ok.location_warning.is_none());
}
