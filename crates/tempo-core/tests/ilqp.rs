//! ⭐ **The Illinois QSO Party, end to end** — every number here is the sponsor's own.
//!
//! Source: the Western Illinois Amateur Radio Club's own pages, read 2026-09-17 —
//! <https://w9awe.org/ilqp/>, its rules PDF (*"Announcing the 2025 Illinois QSO
//! Party"*), its county chart PDF, its `Sample_Excel_Log`, and its county/state/province
//! score-records PDF. The sentences are quoted at the assertions they decide, so a rules
//! change is a red test with the sentence it contradicts attached rather than a silent
//! rescore.
//!
//! ⚠️ **The country file here is a STUB**, the shape `cqww.rs` established: the real
//! resolver is installed by src-tauri from the vendored cty.dat, and an integration test
//! that depended on a weekly-refreshed file would fail for reasons that are not about
//! this contest. What it must place is exactly what ILQP's DXCC multiplier excludes.
use tempo_core::contest::{install_call_resolver, CallLocation, ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

/// The stub country file: the four entities ILQP's own multiplier sentence names, plus
/// two ordinary DX ones to count and three more to prove the cap.
fn place(call: &str) -> Option<CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    let base = up.split('/').next().unwrap_or("");
    let entity = match base {
        c if c.starts_with("KH6") => "Hawaii",
        c if c.starts_with("KL7") => "Alaska",
        c if c.starts_with("VE") || c.starts_with("VA") => "Canada",
        c if c.starts_with("DL") => "Germany",
        c if c.starts_with('F') => "France",
        c if c.starts_with("EA") => "Spain",
        c if c.starts_with('I') => "Italy",
        c if c.starts_with("JA") => "Japan",
        c if c.starts_with("PY") => "Brazil",
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

fn station(state: &str, county: &str) -> StationData {
    StationData {
        contest_qth_state: state.to_string(),
        contest_qth_county: county.to_string(),
        ..Default::default()
    }
}

fn select(station: &StationData) -> ContestSession {
    resolver();
    let rs = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("ilqp is a shipped ruleset");
    ContestSession::for_ruleset(rs, station).unwrap_or_else(|e| panic!("ilqp refused: {e}"))
}

fn composing(s: &ContestSession) -> Vec<(&'static str, String)> {
    s.my_exchange
        .iter()
        .map(|v| (v.key, v.raw.clone()))
        .collect()
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

/// ⭐ **The window: 1700Z Sunday of the third full weekend of October, for eight hours.**
///
/// The sponsor's 2025 announcement gives one running — *"Date/Time: 1700 UTC October 19,
/// 2025 to 0100 UTC October 20, 2025"* — and its own page states the rule the runnings
/// follow (*"the third full weekend of October"*, Sunday). Two years are pinned so the
/// weekday walk is exercised rather than one date being memorised.
#[test]
fn the_window_is_sunday_of_the_third_full_october_weekend_for_eight_hours() {
    let rs = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("shipped");
    // 2026: full October weekends are the 3rd/4th, 10th/11th, 17th/18th — so Sunday the
    // 18th at 1700Z, ending 0100Z on the 19th.
    let w = rs.event_window(2026);
    assert_eq!(w.start_unix, 1_792_342_800, "2026-10-18T17:00:00Z");
    assert_eq!(w.end_unix - w.start_unix, 8 * 3600, "eight hours");
    // 2027: the 2nd/3rd, 9th/10th, 16th/17th — Sunday the 17th.
    let w = rs.event_window(2027);
    assert_eq!(w.start_unix, 1_823_792_400, "2027-10-17T17:00:00Z");
    assert_eq!(w.end_unix - w.start_unix, 8 * 3600);
}

/// ⭐ **The 102 counties, from the sponsor's own chart** (`ILQP COUNTIES CHART`, read
/// 2026-09-17) — Illinois has 102 and the chart lists 102, so a parse that dropped one
/// is a missing multiplier for whoever activates it.
#[test]
fn the_county_domain_is_the_sponsors_whole_chart() {
    let rs = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("shipped");
    let counties = rs
        .domains
        .iter()
        .find(|d| d.id == "il_counties")
        .expect("the county domain");
    assert_eq!(counties.values.len(), 102, "Illinois has 102 counties");
    // Spot checks that each cost a real contact if they are wrong: the one THREE-letter
    // code on the chart, a two-word county, and the chart's own spelling of Jo Daviess.
    assert!(counties.contains("LEE"));
    assert_eq!(
        counties.values.iter().find(|(c, _)| *c == "LEE"),
        Some(&("LEE", "Lee"))
    );
    assert_eq!(
        counties.values.iter().find(|(c, _)| *c == "SCLA"),
        Some(&("SCLA", "St. Clair"))
    );
    assert_eq!(
        counties.values.iter().find(|(c, _)| *c == "JODA"),
        Some(&("JODA", "JoDaviess")),
        "the sponsor's chart writes it closed up"
    );
    assert!(counties.contains("COOK") && counties.contains("cook"));
    // …and a code that is NOT a county is not in the universe.
    assert!(!counties.contains("ADAMS"));
}

/// ⭐ **The three roles, and the one that differs from every other party this build
/// ships.**
///
/// *"Exchange: IL stations give RS/T and county; others give RS/T and state, province or
/// country"* — so a DX entrant sends their COUNTRY, where OhQP's sends the literal `DX`.
#[test]
fn an_illinois_station_sends_a_county_and_a_dx_station_sends_its_country() {
    let il = select(&station("IL", "COOK"));
    assert_eq!(il.role().id, "in_state");
    assert_eq!(
        composing(&il),
        vec![("RST", "599".into()), ("QTH", "COOK".into())]
    );
    assert_eq!(il.my_exchange[1].domain, Some("il_counties"));

    let wi = select(&station("WI", ""));
    assert_eq!(wi.role().id, "w_ve");
    assert_eq!(composing(&wi)[1], ("QTH", "WI".to_string()));
    assert_eq!(wi.my_exchange[1].domain, Some("il_mults"));

    // ⭐ THE DIFFERENCE. A declared DX entrant sends the country they typed, and there is
    // no `DX` token in this contest to send instead.
    let dx = select(&StationData {
        dxcc: true,
        ..station("GERMANY", "")
    });
    assert_eq!(dx.role().id, "dx");
    assert_eq!(composing(&dx)[1], ("QTH", "GERMANY".to_string()));
    assert_eq!(
        dx.my_exchange[1].domain, None,
        "a country name is the slot's free-text arm, not a declared universe"
    );
    // CONTROL, against the party this one is modelled on: OhQP still sends `DX`.
    let oh_dx = ContestSession::for_ruleset(
        ruleset_by_id("ohqp", CURRENT_RULES_YEAR).expect("shipped"),
        &StationData {
            dxcc: true,
            ..station("GERMANY", "")
        },
    )
    .expect("ohqp session");
    assert_eq!(oh_dx.role().id, "dx");
    assert_eq!(
        oh_dx.my_exchange[1].raw, "DX",
        "the other four parties keep the sentence they were written against"
    );
}

/// ⭐ **Points and the dupe rule, which the sponsor states in one breath:** *"Scoring:
/// Phone QSO: 1 point; CW/digital QSO 2 points. … Stations may be worked once per band
/// and mode (phone and CW/digital) and once per band/mode/county for IL Mobile and Rover
/// stations."*
///
/// The parenthesis is the part no other shipped contest has: CW and digital are ONE
/// mode here, so the same station on CW and then on RTTY is a dupe.
#[test]
fn cw_and_digital_are_one_mode_for_dupes_and_two_points_each() {
    let mut log = FieldDayLog::new("W9XYZ", select(&station("IL", "COOK")), "40m");
    let cook = fields(&[("RST", "599"), ("QTH", "COOK")]);
    assert!(log.log_fields_at("K9AAA", &cook, "CW", "", 0, 100));
    // The same station, same band, on RTTY: the sponsor counts one mode for both.
    assert!(
        log.log_fields_at("K9AAA", &cook, "DIG", "RTTY", 0, 110) && log.qsos()[1].dupe,
        "CW then digital on one band is a dupe"
    );
    // Phone IS a second mode.
    assert!(log.log_fields_at("K9AAA", &cook, "PH", "", 0, 120));
    // A different county from the same call is a new contact either way — the mobile
    // rule, which the mode grouping must not have swallowed.
    let will = fields(&[("RST", "599"), ("QTH", "WILL")]);
    assert!(log.log_fields_at("K9AAA", &will, "CW", "", 0, 130));
    // 2 (CW) + 1 (phone) + 2 (CW, new county) = 5 QSO points over three contacts.
    assert_eq!(log.qso_count(), 3);
    assert_eq!(score(&log).0, 5, "CW 2, phone 1, CW 2");
    // CONTROL: the same three-mode sequence in OhQP, whose rule has no grouping, logs
    // both CW and digital.
    let mut oh = FieldDayLog::new(
        "W8XYZ",
        ContestSession::for_ruleset(
            ruleset_by_id("ohqp", CURRENT_RULES_YEAR).expect("shipped"),
            &station("OH", "FRAN"),
        )
        .expect("ohqp session"),
        "40m",
    );
    let fran = fields(&[("RST", "599"), ("QTH", "FRAN")]);
    assert!(oh.log_fields_at("K8AAA", &fran, "CW", "", 0, 100));
    assert!(
        oh.log_fields_at("K8AAA", &fran, "DIG", "RTTY", 0, 110),
        "OhQP counts CW and digital separately"
    );
}

/// ⭐ **An Illinois entrant's four multiplier universes, and the cap on the fourth.**
///
/// *"IL stations multiply points by the sum of IL counties, US states, VE provinces and
/// DXCC countries (maximum 5) worked. Canada, KH6 and KL7 do not count as DX entities.
/// Additional DX contacts count for points but not multipliers."*
#[test]
fn an_illinois_entrant_counts_counties_states_provinces_and_five_countries() {
    let mut log = FieldDayLog::new("W9XYZ", select(&station("IL", "COOK")), "20m");
    let mut t = 100;
    let mut work = |log: &mut FieldDayLog, call: &str, qth: &str| {
        t += 10;
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
    // Two Illinois counties, two US states and one Canadian province.
    work(&mut log, "K9AAA", "COOK");
    work(&mut log, "K9BBB", "WILL");
    work(&mut log, "W1AAA", "CT");
    work(&mut log, "W6AAA", "CA");
    work(&mut log, "VE3AAA", "ON");
    // …then SEVEN DXCC entities, of which the rules allow five.
    for (call, country) in [
        ("DL1AAA", "GERMANY"),
        ("F1AAA", "FRANCE"),
        ("EA1AAA", "SPAIN"),
        ("I1AAA", "ITALY"),
        ("JA1AAA", "JAPAN"),
        ("PY1AAA", "BRAZIL"),
    ] {
        work(&mut log, call, country);
    }
    // …and the three entities the sponsor's sentence removes by name. KH6 and KL7 send a
    // state (they are US states to this contest); VE is already counted as a province.
    work(&mut log, "KH6AAA", "HI");
    work(&mut log, "KL7AAA", "AK");
    let counts = log.ruleset().scoring.mult_counts(log.score_rows());
    assert_eq!(
        counts,
        vec![("county", 2), ("mult", 5), ("dxcc", 5)],
        "2 counties; CT, CA, ON, HI and AK; and six entities capped at five"
    );
    // The cap is the whole difference: six were worked, five may be claimed.
    let (qso, _, mults, total) = score(&log);
    assert_eq!(mults, Some(12));
    assert_eq!(qso, 13 * 2, "thirteen CW contacts");
    assert_eq!(total, 26 * 12);
}

/// ⭐ **One province, one multiplier, whichever spelling its station sent.** The sponsor
/// publishes no exchange code list; its records table spells Newfoundland `NF` and Prince
/// Edward Island `PEI`, and `il_mults` carries Canada Post's `NL` and `PE` beside them, so a VE
/// station may send either. *"IL stations multiply points by the sum of IL counties, US states,
/// VE provinces and DXCC countries"*: provinces, each once.
///
/// Measured before the fold (the readiness audit): PE, PEI, NL and NF from four stations
/// scored 32 = 8 points × 4, where the sponsor counts two provinces, 16.
#[test]
fn one_province_in_either_spelling_is_one_multiplier() {
    let mut log = FieldDayLog::new("W9XYZ", select(&station("IL", "COOK")), "20m");
    let work = |log: &mut FieldDayLog, call: &str, qth: &str, t: u64| {
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
    work(&mut log, "VY2AB", "PE", 100);
    work(&mut log, "VY2CD", "PEI", 110);
    work(&mut log, "VO1AB", "NL", 120);
    work(&mut log, "VO1CD", "NF", 130);
    // Each contact keeps what its station sent: that is what its Cabrillo line carries.
    assert_eq!(
        log.qsos()
            .iter()
            .map(|q| q.rcvd("QTH").to_string())
            .collect::<Vec<_>>(),
        ["PE", "PEI", "NL", "NF"]
    );
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("county", 0), ("mult", 2), ("dxcc", 0)],
        "Prince Edward Island and Newfoundland"
    );
    assert_eq!(score(&log), (8, 8, Some(2), 16));
    // CONTROL: another province is still a multiplier of its own.
    work(&mut log, "VE3AB", "ON", 140);
    assert_eq!(score(&log), (10, 10, Some(3), 30));
}

/// ⭐ **A non-Illinois entrant counts Illinois counties and nothing else** — *"Non-IL
/// stations multiply points by the number of IL counties worked."*
#[test]
fn a_non_illinois_entrant_counts_only_illinois_counties() {
    let mut log = FieldDayLog::new("W1ABC", select(&station("CT", "")), "20m");
    for (call, qth, t) in [
        ("W9AAA", "COOK", 100),
        ("W9BBB", "WILL", 110),
        // Another Connecticut station and a German one: points, no multipliers.
        ("W1XYZ", "CT", 120),
        ("DL1AAA", "GERMANY", 130),
    ] {
        assert!(log.log_fields_at(
            call,
            &fields(&[("RST", "599"), ("QTH", qth)]),
            "CW",
            "",
            0,
            t
        ));
    }
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("county", 2), ("mult", 0), ("dxcc", 0)],
        "the state and country universes are the in-state role's"
    );
    assert_eq!(score(&log).3, 8 * 2);
}

/// ⭐ **The two bonus stations, once each, in the total** — *"NEW FOR 2025: BONUS
/// STATIONS!! … any entrant contacting these stations will have a 100 point bonus added
/// to the final score. Total of 200 points possible. W9AWE will be primarily CW, W9OAB
/// will be primarily phone."*
#[test]
fn working_the_clubs_two_calls_adds_two_hundred_points_once_each() {
    let rs = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("shipped");
    let mut log = FieldDayLog::new("W9XYZ", select(&station("IL", "COOK")), "40m");
    let calls =
        |log: &FieldDayLog| -> Vec<String> { log.qsos().iter().map(|q| q.call.clone()).collect() };
    let earned = |log: &FieldDayLog| rs.bonus_station_points(calls(log).iter().map(String::as_str));
    assert_eq!(earned(&log), 0, "nothing worked, nothing earned");
    let adam = fields(&[("RST", "599"), ("QTH", "ADAM")]);
    assert!(log.log_fields_at("W9AWE", &adam, "CW", "", 0, 100));
    assert_eq!(earned(&log), 100);
    // ⭐ The SAME club call again on phone is a legal second contact — a different mode
    // class, so the dupe rule admits it — and it is not a second bonus.
    assert!(log.log_fields_at("W9AWE", &adam, "PH", "", 0, 110));
    assert_eq!(log.qso_count(), 2);
    assert_eq!(earned(&log), 100, "once per LOG, not once per contact");
    // The other call is the second 100, and the pair is the sponsor's stated maximum.
    assert!(log.log_fields_at("W9OAB", &adam, "PH", "", 0, 120));
    assert_eq!(earned(&log), 200, "both calls, the sponsor's maximum");
    // CONTROL: every other shipped party awards none of this.
    let oh = ruleset_by_id("ohqp", CURRENT_RULES_YEAR).expect("shipped");
    assert_eq!(oh.bonus_station_points(["W9AWE", "W9OAB"]), 0);
}

/// ⭐ **The Cabrillo file an ILQP entrant actually submits.**
///
/// The sponsor's `Sample_Excel_Log` (read 2026-09-17) heads it `CONTEST: ILLINOIS QSO
/// PARTY` and `IL-COUNTY: ADAMS`, and its QSO line carries the four-letter county code
/// on both sides — `QSO: 7000 CW 2013-10-20 1714 W9XYZ 599 JODA K9NR 599 KANK`.
#[test]
fn the_cabrillo_header_is_the_sponsors_own_and_the_lines_carry_codes() {
    use tempo_core::contest::CabrilloEntrant;
    let mut log = FieldDayLog::new("W9XYZ", select(&station("IL", "ADAM")), "40m");
    assert!(log.log_fields_at(
        "K9NR",
        &fields(&[("RST", "599"), ("QTH", "KANK")]),
        "CW",
        "",
        0,
        1_792_342_800
    ));
    let me = CabrilloEntrant {
        name: "EXAMPLE OPERATOR".into(),
        email: "op@example.com".into(),
        ..Default::default()
    };
    let cab = log.cabrillo_with(7_000, &me).expect("one entry");
    assert!(
        cab.contains("CONTEST: ILLINOIS QSO PARTY\n"),
        "the sponsor's own token, not the master list's IL-QSO-PARTY:\n{cab}"
    );
    assert!(
        cab.contains("IL-COUNTY: Adams\n"),
        "the county's NAME in the header:\n{cab}"
    );
    assert!(
        cab.contains(" W9XYZ 599 ADAM K9NR 599 KANK"),
        "…and its CODE on the QSO line:\n{cab}"
    );
    // CONTROL: the same entry from outside Illinois heads no IL-COUNTY line.
    let mut out = FieldDayLog::new("W1ABC", select(&station("CT", "")), "40m");
    assert!(out.log_fields_at(
        "W9XYZ",
        &fields(&[("RST", "599"), ("QTH", "ADAM")]),
        "CW",
        "",
        0,
        1_792_342_800
    ));
    let cab = out.cabrillo_with(7_000, &me).expect("one entry");
    assert!(!cab.contains("IL-COUNTY"), "{cab}");
}

/// ⭐ **The entry's own lines, which the sponsor's software reads** — `ENTRY-CLASS`, `CLUB`,
/// `OPERATORS` and `QRP-COMPETITION`, all four in its `Sample_Excel_Log`, and asked for by its
/// 2026 rules (*"Announcing the 2026 Illinois QSO Party"*, read 2026-10-08): *"make sure entry
/// class, personal information, call sign, station location, and club affiliation are
/// correctly shown. The header information is automatically pulled and used by the processing
/// software."* A club running several positions at once enters the new class: *"Stations
/// wishing to employ multiple transmitted signals simultaneously at a single site may enter as
/// "Unlimited"."*
#[test]
fn the_entry_lines_the_sponsors_software_reads_are_written() {
    use tempo_core::contest::{CabrilloEntrant, StationData};
    let rs = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("shipped");
    assert_eq!(
        rs.entry_classes,
        [
            "ILLINOIS FIXED HIGH POWER",
            "ILLINOIS FIXED LOW POWER",
            "ILLINOIS PORTABLE",
            "ILLINOIS MOBILE",
            "ILLINOIS ROVER",
            "OUTSIDE ILLINOIS HIGH POWER",
            "OUTSIDE ILLINOIS LOW POWER",
            "UNLIMITED",
        ],
        "the 2026 rules' entry classes, in their order, in the sample log's spelling"
    );
    let qrp = StationData {
        contest_category_power: "QRP".into(),
        ..station("IL", "ADAM")
    };
    let mut log = FieldDayLog::new("W9AWE", select(&qrp), "40m");
    // Two operators take the seat in turn, and the first comes back for a third contact.
    for (op, call, qth) in [
        ("W9XYZ", "K9NR", "KANK"),
        ("AA9XYZ", "N9ABC", "COOK"),
        ("W9XYZ", "K9DEF", "WILL"),
    ] {
        log.operator = op.into();
        assert!(log.log_fields_at(
            call,
            &fields(&[("RST", "599"), ("QTH", qth)]),
            "CW",
            "",
            0,
            1_792_342_800
        ));
    }
    let me = CabrilloEntrant {
        club: "Western Ill Amateur Radio Club".into(),
        entry_class: "unlimited".into(),
        operators: "w9xyz, KB9QRS".into(),
        ..Default::default()
    };
    let cab = log.cabrillo_with(7_000, &me).expect("one entry");
    assert!(
        cab.contains("ENTRY-CLASS: UNLIMITED\n"),
        "the sponsor's class, as the sponsor spells it:\n{cab}"
    );
    assert!(
        cab.contains("CLUB: Western Ill Amateur Radio Club\n"),
        "{cab}"
    );
    assert!(
        cab.contains("OPERATORS: W9XYZ AA9XYZ KB9QRS\n"),
        "the rows' operators in the order they first logged, then the typed one, each once:\n{cab}"
    );
    assert!(
        cab.contains("QRP-COMPETITION: YES\n"),
        "the declared QRP power is the certification:\n{cab}"
    );

    // CONTROLS. A class that is not one of this sponsor's claims nothing (a pick left over
    // from another contest); a power that is not QRP certifies nothing; nobody named and
    // nothing typed is no OPERATORS line; no club is no CLUB line.
    let mut solo = FieldDayLog::new("W9XYZ", select(&station("IL", "ADAM")), "40m");
    assert!(solo.log_fields_at(
        "K9NR",
        &fields(&[("RST", "599"), ("QTH", "KANK")]),
        "CW",
        "",
        0,
        1_792_342_800
    ));
    let stale = CabrilloEntrant {
        entry_class: "MULTI-OP".into(),
        ..Default::default()
    };
    let cab = solo.cabrillo_with(7_000, &stale).expect("one entry");
    for tag in ["ENTRY-CLASS", "OPERATORS", "QRP-COMPETITION", "CLUB"] {
        assert!(!cab.contains(tag), "{tag} with nothing declared:\n{cab}");
    }
    // …and a party whose sponsor asks for none of them writes none, whatever is set.
    let ny = ruleset_by_id("nyqp", CURRENT_RULES_YEAR).expect("shipped");
    let ny_station = StationData {
        contest_category_power: "QRP".into(),
        ..station("NY", "ALB")
    };
    let mut nylog = FieldDayLog::new(
        "W2XYZ",
        ContestSession::for_ruleset(ny, &ny_station).expect("nyqp starts"),
        "40m",
    );
    nylog.operator = "W2XYZ".into();
    assert!(nylog.log_fields_at(
        "K2AAA",
        &fields(&[("RST", "599"), ("QTH", "SUF")]),
        "CW",
        "",
        0,
        1_792_342_800
    ));
    let cab = nylog.cabrillo_with(7_000, &me).expect("one entry");
    for tag in ["ENTRY-CLASS", "OPERATORS", "QRP-COMPETITION", "CLUB"] {
        assert!(!cab.contains(tag), "NYQP lists no {tag}:\n{cab}");
    }
}

/// ⭐ **A blank entry class or email is named on the Contest screen before the export.** The 2026
/// rules: *"make sure entry class, personal information, call sign, station location, and club
/// affiliation are correctly shown. The header information is automatically pulled and used by
/// the processing software."* With either left blank the file goes without the line, so each is
/// named until it is set; a class is set only when it is one of this sponsor's, because a pick
/// left over from another contest writes no line. Never a refusal: the file still exports.
#[test]
fn a_blank_entry_class_or_email_is_named_before_the_export() {
    use tempo_core::contest::{CabrilloEntrant, OperatorCategory};
    let rs = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("shipped");
    assert_eq!(rs.cabrillo_required, ["ENTRY-CLASS", "EMAIL"]);
    let mut log = FieldDayLog::new("W9XYZ", select(&station("IL", "ADAM")), "40m");
    assert!(log.log_fields_at(
        "K9NR",
        &fields(&[("RST", "599"), ("QTH", "KANK")]),
        "CW",
        "",
        0,
        1_792_342_800
    ));
    let entrant = |entry_class: &str, email: &str| CabrilloEntrant {
        entry_class: entry_class.into(),
        email: email.into(),
        ..Default::default()
    };
    let none: [&str; 0] = [];
    assert_eq!(
        log.cabrillo_missing(&entrant("", "")),
        ["ENTRY-CLASS", "EMAIL"]
    );
    assert_eq!(log.cabrillo_missing(&entrant("UNLIMITED", "")), ["EMAIL"]);
    assert_eq!(
        log.cabrillo_missing(&entrant("MULTI-OP", "op@example.com")),
        ["ENTRY-CLASS"],
        "a class from another contest writes no ENTRY-CLASS line"
    );
    assert_eq!(
        log.cabrillo_missing(&entrant("unlimited", "op@example.com")),
        none
    );
    assert!(
        log.cabrillo_with(7_000, &entrant("", "")).is_ok(),
        "named, never refused"
    );
    // A checklog is placed in no class, so nothing is named for it.
    let mut checking = select(&station("IL", "ADAM"));
    checking.entry_category = OperatorCategory::Checklog;
    let checklog = FieldDayLog::new("W9XYZ", checking, "40m");
    assert_eq!(checklog.cabrillo_missing(&entrant("", "")), none);
    // CONTROL: a party whose rules data requires nothing names nothing, whatever is blank.
    let ny = FieldDayLog::new(
        "W2XYZ",
        ContestSession::for_ruleset(
            ruleset_by_id("nyqp", CURRENT_RULES_YEAR).expect("shipped"),
            &station("NY", "ALB"),
        )
        .expect("nyqp starts"),
        "40m",
    );
    assert_eq!(ny.cabrillo_missing(&entrant("", "")), none);
}

/// ⭐ **Who was at the key survives a restart.** The journal is the only copy of a contest
/// contact on disk, so each row's operator rides it as ADIF's own `OPERATOR`, and a club
/// position that crashes at hour five still heads its file with everyone who operated it.
#[test]
fn the_operator_at_the_key_rides_the_journal() {
    use tempo_core::contest::CabrilloEntrant;
    let mut log = FieldDayLog::new("W9AWE", select(&station("IL", "ADAM")), "40m");
    log.operator = "AA9XYZ".into();
    assert!(log.log_fields_at(
        "K9NR",
        &fields(&[("RST", "599"), ("QTH", "KANK")]),
        "CW",
        "",
        0,
        1_792_342_800
    ));
    // CONTROL in the same journal: a contact nobody was named for writes no tag.
    log.operator = String::new();
    assert!(log.log_fields_at(
        "N9ABC",
        &fields(&[("RST", "599"), ("QTH", "COOK")]),
        "CW",
        "",
        0,
        1_792_342_860
    ));
    let journal = log.adif();
    assert_eq!(journal.matches("<OPERATOR:").count(), 1, "{journal}");
    assert!(journal.contains("<OPERATOR:6>AA9XYZ"), "{journal}");

    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "ADAM")), "40m");
    back.merge_adif(&journal, 0);
    let ops: Vec<&str> = back.qsos().iter().map(|q| q.operator.as_str()).collect();
    assert_eq!(ops, ["AA9XYZ", ""], "each row keeps its own operator");
    let cab = back
        .cabrillo_with(7_000, &CabrilloEntrant::default())
        .expect("one entry");
    assert!(cab.contains("OPERATORS: AA9XYZ\n"), "{cab}");
}

/// ⭐ **The claimed score an ILQP entrant submits includes the bonus stations**, because
/// the sponsor adds them *"to the final score"* and a file that leaves them out is a
/// claim 100 or 200 points light on the artifact the club scores.
///
/// ⚠️ This is the header the rest of the build refuses to write when a score is
/// incomplete, so it is also the assertion that says ILQP's score IS complete: no power
/// tier, no claimed-bonus menu, no `score_note_key`.
#[test]
fn the_claimed_score_carries_the_bonus_stations() {
    use tempo_core::contest::CabrilloEntrant;
    let rs = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("shipped");
    assert!(
        rs.score_note_key.is_empty() && rs.scoring.post.is_empty(),
        "the premise: nothing about an ILQP score is left out"
    );
    let mut log = FieldDayLog::new("W9XYZ", select(&station("IL", "ADAM")), "40m");
    assert!(log.log_fields_at(
        "W9AWE",
        &fields(&[("RST", "599"), ("QTH", "KANK")]),
        "CW",
        "",
        0,
        1_792_342_800
    ));
    // 2 QSO points × 1 county multiplier = 2, and the club's CW call is worth 100 more.
    assert_eq!(score(&log).3, 2);
    let cab = log
        .cabrillo_with(7_000, &CabrilloEntrant::default())
        .expect("one entry");
    assert!(
        cab.contains("CLAIMED-SCORE: 102\n"),
        "the bonus station is part of the claimed score:\n{cab}"
    );
    // CONTROL: the same contact with an ordinary station claims the 2.
    let mut plain = FieldDayLog::new("W9XYZ", select(&station("IL", "ADAM")), "40m");
    assert!(plain.log_fields_at(
        "K9NR",
        &fields(&[("RST", "599"), ("QTH", "KANK")]),
        "CW",
        "",
        0,
        1_792_342_800
    ));
    assert!(plain
        .cabrillo_with(7_000, &CabrilloEntrant::default())
        .expect("one entry")
        .contains("CLAIMED-SCORE: 2\n"));
}

/// ⭐ **An operator who typed a SECTION where this party wants a state is told which
/// state to type** — the warning CQ WW RTTY shipped, reaching a QSO party for the first
/// time.
///
/// The trigger is a W/VE callsign about to send the DX exchange: `EMA` is an ARRL section
/// and not a state, so the role falls through to `dx` and the operator would send
/// "EMA" as a country. The hint is read off the section table (`EMA` is "Eastern
/// Massachusetts", so `MA`), never guessed.
#[test]
fn a_section_typed_where_a_state_belongs_is_warned_about_with_the_state_to_type() {
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
        .expect("a W/VE call about to send the DX exchange");
    assert_eq!(w.typed, "EMA");
    assert_eq!(w.hints, vec!["MA"], "Eastern Massachusetts is MA here");
    // CONTROL: the same operator with a state this party lists gets the w_ve role and no
    // warning at all.
    let ok = select(&StationData {
        mycall: "W1ABC".to_string(),
        ..station("MA", "")
    });
    assert_eq!(ok.role().id, "w_ve");
    assert!(ok.location_warning.is_none());
    // …and a real DX entrant is never warned: they are not a W/VE station at all.
    let dx = select(&StationData {
        mycall: "DL1AAA".to_string(),
        ..station("GERMANY", "")
    });
    assert_eq!(dx.role().id, "dx");
    assert!(dx.location_warning.is_none());
}

/// ⭐ **FT8 and FT4 earn nothing** — *"Due to the complexity of creating a log entry
/// conforming to ILQP log submission rules, FT4 and FT8 contacts will receive no contact
/// credit. Other digital modes are encouraged."* — and the advisory band list is the
/// sponsor's own: *"Bands: 160 through 2 meters, excluding WARC bands (60, 30, 17 and 12
/// meters)"*.
#[test]
fn the_wsjt_modes_are_banned_and_the_bands_are_the_sponsors() {
    let rs = ruleset_by_id("ilqp", CURRENT_RULES_YEAR).expect("shipped");
    assert!(rs.mode_banned("FT8") && rs.mode_banned("FT4"));
    assert!(
        !rs.mode_banned("RTTY") && !rs.mode_banned("PSK31"),
        "other digital modes are encouraged"
    );
    assert_eq!(
        rs.bands,
        ["160m", "80m", "40m", "20m", "15m", "10m", "6m", "2m"],
        "160 through 2, no WARC band"
    );
}

/// 1700Z on the party's Sunday, 18 October 2026.
const PARTY: u64 = 1_792_342_800;

/// A McLean County log with K9AAA on 20 m CW from Cook, then `call` on 20 m in the digital mode
/// `submode`, from `qth`.
fn cook_then_digital(call: &str, qth: &str, submode: &str) -> FieldDayLog {
    let mut log = FieldDayLog::new("W9XYZ", select(&station("IL", "MCLN")), "20m");
    let cook = fields(&[("RST", "599"), ("QTH", "COOK")]);
    assert!(log.log_fields_at("K9AAA", &cook, "CW", "", 0, PARTY + 60));
    let theirs = fields(&[("RST", "599"), ("QTH", qth)]);
    assert!(
        log.log_fields_at(call, &theirs, "DIG", submode, 0, PARTY + 120),
        "{call} on {submode:?} is logged"
    );
    log
}

/// ⭐ **An FT8 contact is logged and filed, and scores nothing** — *"FT4 and FT8 contacts will
/// receive no contact credit"*: no points, no multiplier and no cell on the county board, and
/// the contact stays in the log, the Cabrillo and the ADIF, as a logged duplicate does.
///
/// Measured before (the readiness audit): K9AAA on CW from Cook and K9FT8 on FT8 from Lake
/// claimed 8, both contacts' points times both counties, where the sponsor credits 2.
#[test]
fn a_contact_in_a_banned_mode_is_logged_and_filed_and_scores_nothing() {
    use tempo_core::contest::CabrilloEntrant;
    let log = cook_then_digital("K9FT8", "LAKE", "FT8");
    assert_eq!(
        calls(&log),
        ["K9AAA", "K9FT8"],
        "both contacts are in the log"
    );
    assert_eq!(
        (score(&log), log.qso_count(), log.worked_values("QTH")),
        ((2, 2, Some(1), 2), 1, vec!["COOK".to_string()]),
        "((QSO points, powered, multipliers, total), contacts, the county board): K9AAA alone"
    );
    let cab = log
        .cabrillo_with(14_000, &CabrilloEntrant::default())
        .expect("one entry");
    assert!(cab.contains("CLAIMED-SCORE: 2\n"), "{cab}");
    assert!(
        cab.contains(" W9XYZ 599 MCLN K9FT8 599 LAKE\n"),
        "the FT8 contact is still a line of the file:\n{cab}"
    );
    assert!(
        log.submission_adif().contains("<CALL:5>K9FT8"),
        "…and a record of the ADIF"
    );
    // CONTROL: the same contact on RTTY, the digital mode the sponsor encourages, counts:
    // (2 + 2) points × 2 counties.
    let rtty = cook_then_digital("K9FT8", "LAKE", "RTTY");
    assert_eq!(
        (score(&rtty), rtty.qso_count(), rtty.worked_values("QTH")),
        (
            (4, 4, Some(2), 8),
            2,
            vec!["COOK".to_string(), "LAKE".to_string()]
        )
    );
    assert!(rtty
        .cabrillo_with(14_000, &CabrilloEntrant::default())
        .expect("one entry")
        .contains("CLAIMED-SCORE: 8\n"));
    // CONTROL: a party that bans no mode scores the same FT8 contact.
    let mut oh = FieldDayLog::new(
        "W8XYZ",
        ContestSession::for_ruleset(
            ruleset_by_id("ohqp", CURRENT_RULES_YEAR).expect("shipped"),
            &station("OH", "FRAN"),
        )
        .expect("ohqp session"),
        "20m",
    );
    assert!(oh.log_fields_at(
        "K8AAA",
        &fields(&[("RST", "599"), ("QTH", "DELA")]),
        "CW",
        "",
        0,
        PARTY + 60
    ));
    assert!(oh.log_fields_at(
        "K8FT8",
        &fields(&[("RST", "599"), ("QTH", "LICK")]),
        "DIG",
        "FT8",
        0,
        PARTY + 120
    ));
    assert_eq!(
        (oh.qso_count(), oh.worked_values("QTH")),
        (2, vec!["DELA".to_string(), "LICK".to_string()]),
        "OhQP bans no mode"
    );
}

/// ⭐ **A bonus station worked only on FT8 earns no bonus.** The sponsor adds its 100 points
/// for *"contacting these stations"*, and an FT8 contact is one it gives no credit.
///
/// Measured before (the readiness audit): an FT8-only W9OAB took a club's claimed score from 32
/// to 150, its points, its county and the 100.
#[test]
fn a_bonus_station_worked_only_in_a_banned_mode_earns_no_bonus() {
    use tempo_core::contest::CabrilloEntrant;
    let log = cook_then_digital("W9OAB", "ADAM", "FT4");
    assert_eq!(
        (log.bonus_station_points(), score(&log).3),
        (0, 2),
        "(bonus, score): K9AAA's 2 points × Cook, and nothing for W9OAB"
    );
    assert!(log
        .cabrillo_with(14_000, &CabrilloEntrant::default())
        .expect("one entry")
        .contains("CLAIMED-SCORE: 2\n"));
    // CONTROL: on RTTY the same contact earns the bonus, its points and Adams.
    let rtty = cook_then_digital("W9OAB", "ADAM", "RTTY");
    assert_eq!((rtty.bonus_station_points(), score(&rtty).3), (100, 8));
    assert!(rtty
        .cabrillo_with(14_000, &CabrilloEntrant::default())
        .expect("one entry")
        .contains("CLAIMED-SCORE: 108\n"));
}

/// ⭐ **A legal contact after a banned one with the same station counts.** The sponsor gives the
/// FT8 contact no credit, so the RTTY contact that follows it with the same station, on the same
/// band, in the same mode group and between the same two counties is the first one it credits,
/// and is no duplicate of anything the score holds. A second RTTY contact after it is a duplicate.
///
/// Measured before: K9AAA on CW from Cook, K9FT8 on FT8 from Lake, then K9FT8 on RTTY from Lake
/// claimed 2 and one contact, the RTTY one marked a dupe; the sponsor credits 8 and two.
///
/// CONTROL: the log's dupe index, which the FT sequencer asks whether a station was worked, holds
/// K9FT8 from the FT8 contact on, exactly as before.
#[test]
fn a_legal_contact_after_a_banned_one_with_the_same_station_counts() {
    use tempo_core::contest::CabrilloEntrant;
    let mut log = cook_then_digital("K9FT8", "LAKE", "FT8");
    let worked_before = |log: &FieldDayLog| {
        let q = &log.qsos()[1];
        (
            log.is_dupe_row(&q.call, &q.band, &q.mode, &q.rx, &q.tx),
            log.worked_keys().len(),
        )
    };
    assert_eq!(
        worked_before(&log),
        (true, 2),
        "CONTROL: after the FT8 contact the FT sequencer reads K9FT8 as worked"
    );
    let lake = fields(&[("RST", "599"), ("QTH", "LAKE")]);
    assert!(log.log_fields_at("K9FT8", &lake, "DIG", "RTTY", 0, PARTY + 180));
    assert_eq!(
        worked_before(&log),
        (true, 2),
        "CONTROL: the RTTY contact changes nothing the FT sequencer reads"
    );
    assert_eq!(
        (score(&log), log.qso_count(), log.worked_values("QTH")),
        (
            (4, 4, Some(2), 8),
            2,
            vec!["COOK".to_string(), "LAKE".to_string()]
        ),
        "((QSO points, powered, multipliers, total), contacts, the county board): \
         K9AAA and the RTTY K9FT8"
    );
    assert_eq!(
        log.qsos().iter().map(|q| q.dupe).collect::<Vec<_>>(),
        [false, false, false],
        "the RTTY contact is not marked a dupe, so the log table and the Remote page count it too"
    );
    assert!(log
        .cabrillo_with(14_000, &CabrilloEntrant::default())
        .expect("one entry")
        .contains("CLAIMED-SCORE: 8\n"));
    // CONTROL: a second RTTY contact with K9FT8 is a duplicate and scores nothing.
    assert!(log.log_fields_at("K9FT8", &lake, "DIG", "RTTY", 0, PARTY + 240));
    assert_eq!(
        (
            score(&log).3,
            log.qso_count(),
            log.qsos().last().map(|q| q.dupe)
        ),
        (8, 2, Some(true))
    );
}

/// A digital contact that recorded no on-air mode, as a row from before the mode was recorded,
/// is not read as FT8 (which is what its export falls back to calling it): a mode nobody
/// recorded is never a banned one, and the contact counts as it always has.
#[test]
fn a_digital_contact_with_no_recorded_mode_still_counts() {
    let log = cook_then_digital("K9OLD", "LAKE", "");
    assert_eq!(
        log.qsos()[1].recorded_mode(),
        "FT8",
        "PREMISE: its export says FT8"
    );
    assert_eq!(
        (score(&log).3, log.qso_count()),
        (8, 2),
        "(2 + 2) points × Cook and Lake"
    );
}

// ---- the journal restore: this running of this contest, and nothing else -------------

/// 2026-10-14T23:00:00Z — the Wednesday evening of a club's rehearsal, four days before the
/// 2026 party (which starts at 1_792_342_800, Sunday 18 October 1700Z).
const WED: u64 = 1_792_018_800;
/// 2026-10-18T19:30:00Z — two and a half hours into the party.
const SUN: u64 = 1_792_351_800;
/// The journal restore's window (the engine's four days): past it, another contest's records and
/// another running's are kept until the Logbook holds them, and the session's own are restored.
const FOUR_DAYS: u64 = 4 * 86_400;
/// 2027-10-17T19:30:00Z — the party's next running, a year on.
const NEXT_YEAR: u64 = SUN + 364 * 86_400;

/// One contact logged at `when`, from Cook County, of a station in `qth`.
fn worked(log: &mut FieldDayLog, call: &str, qth: &str, when: u64) {
    assert!(
        log.log_fields_at(
            call,
            &fields(&[("RST", "599"), ("QTH", qth)]),
            "CW",
            "",
            0,
            when
        ),
        "{call} logged"
    );
}

/// Every ADIF record in a journal, as written: its tags through its `<EOR>`.
fn records(journal: &str) -> Vec<String> {
    let body = &journal[journal.find("<EOH>").map_or(0, |i| i + 5)..];
    body.split_inclusive("<EOR>")
        .map(|r| r.trim_start().to_string())
        .filter(|r| r.ends_with("<EOR>"))
        .collect()
}

fn calls(log: &FieldDayLog) -> Vec<&str> {
    log.qsos().iter().map(|q| q.call.as_str()).collect()
}

/// ⭐ **A rehearsal days before the party is kept out of the party's log, and kept on
/// disk.** The journal restore used to load every row of the last four days whatever
/// contest or day it was from, so a club that rehearsed on the Wednesday started the
/// party on the Sunday with the rehearsal's contacts in every position's log — in the
/// club's merged log too, once each position streamed them up. Nothing is deleted: the
/// rows not loaded ride every rewrite of the journal unchanged, and the sequence numbers
/// they used are not given out again (a host or a general log holding the rehearsal's
/// `(position, seq)` would take a re-used one for a contact it already has).
#[test]
fn a_rehearsal_days_before_the_party_stays_out_of_its_log_and_on_disk() {
    let mut rehearsal = FieldDayLog::new("W9AWE", select(&station("IL", "COOK")), "40m");
    for (i, call) in ["K9AAA", "K9BBB", "K9CCC"].into_iter().enumerate() {
        worked(&mut rehearsal, call, "KANE", WED + 60 * i as u64);
    }
    let journal = rehearsal.adif();
    assert_eq!(records(&journal).len(), 3);

    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "COOK")), "40m");
    party.restore_journal(&journal, SUN - FOUR_DAYS, SUN);
    assert_eq!(
        calls(&party),
        Vec::<&str>::new(),
        "not one rehearsal contact is loaded"
    );
    assert_eq!(
        party.held(),
        (0, 3),
        "three kept out, as another running of the party"
    );
    for record in records(&journal) {
        assert!(
            party.journal_adif().contains(&record),
            "kept, byte for byte: {record}"
        );
    }
    worked(&mut party, "N9ZZZ", "LAKE", SUN);
    assert_eq!(
        party.qsos()[0].seq,
        4,
        "the rehearsal's sequence numbers are not re-used"
    );

    // The party's own restart, from the journal the party wrote: its contact comes back,
    // and the rehearsal's three are still kept out, and still there.
    let mut again = FieldDayLog::new("W9AWE", select(&station("IL", "COOK")), "40m");
    again.restore_journal(&party.journal_adif(), SUN + 60 - FOUR_DAYS, SUN + 60);
    assert_eq!(calls(&again), vec!["N9ZZZ"]);
    assert_eq!(again.held(), (0, 3));
    assert_eq!(
        records(&again.journal_adif()).len(),
        4,
        "nothing was deleted"
    );
}

/// ⭐ **A journal of another contest is kept out of this one, and kept.** A position that
/// ran Field Day mode for another contest in the last four days (an ARRL Field Day log
/// here) would have had those rows loaded into the party's log, read against the party's
/// exchange — and written back as party rows, their class and section gone.
#[test]
fn a_journal_of_another_contest_stays_out_of_this_one_and_on_disk() {
    let fd = tempo_core::contest::ContestSession::field_day(
        tempo_core::fieldday::FdEvent::ArrlFd,
        "3A",
        "IL",
    );
    let mut field_day = FieldDayLog::new("W9AWE", fd, "40m");
    assert!(field_day.log_fields_at(
        "K1ABC",
        &fields(&[("CLASS", "2A"), ("SECTION", "EMA")]),
        "CW",
        "",
        0,
        SUN - 3600
    ));
    let journal = field_day.adif();

    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "COOK")), "40m");
    party.restore_journal(&journal, SUN - FOUR_DAYS, SUN);
    assert_eq!(
        calls(&party),
        Vec::<&str>::new(),
        "no Field Day contact in the party's log"
    );
    assert_eq!(party.held(), (1, 0), "one kept out, as another contest");
    let kept = records(&party.journal_adif());
    assert_eq!(kept, records(&journal), "kept as Field Day wrote it");
    assert!(kept[0].contains("<CONTEST_ID:14>ARRL-FIELD-DAY") && kept[0].contains("<CLASS:2>2A"));
}

/// ⭐ **A restart inside the same session restores everything** — this is what the journal
/// is for: a crash during the party (its contacts, and the one logged during the setup
/// before it began), the day after (when its file is exported), and the rehearsal's own
/// restart on the Wednesday all come back whole, with nothing kept out.
#[test]
fn a_restart_in_the_same_session_restores_every_contact() {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "COOK")), "40m");
    worked(&mut party, "N9SET", "KANE", 1_792_341_000); // 1630Z, setting up
    worked(&mut party, "N9AAA", "LAKE", 1_792_343_100); // 1705Z
    worked(&mut party, "N9BBB", "WILL", 1_792_346_400); // 1800Z
    let journal = party.adif();
    for now in [SUN, 1_792_422_000] {
        // during the party, and Monday 1500Z
        let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "COOK")), "40m");
        back.restore_journal(&journal, now - FOUR_DAYS, now);
        assert_eq!(calls(&back), vec!["N9SET", "N9AAA", "N9BBB"], "at {now}");
        assert_eq!(back.held(), (0, 0));
    }
    let mut rehearsal = FieldDayLog::new("W9AWE", select(&station("IL", "COOK")), "40m");
    worked(&mut rehearsal, "K9AAA", "KANE", WED);
    worked(&mut rehearsal, "K9BBB", "KANE", WED + 60);
    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "COOK")), "40m");
    back.restore_journal(&rehearsal.adif(), WED + 3600 - FOUR_DAYS, WED + 3600);
    assert_eq!(
        calls(&back),
        vec!["K9AAA", "K9BBB"],
        "the rehearsal's own restart"
    );
    assert_eq!(back.held(), (0, 0));
}

// ---- the journal across a County changed in Settings, and past its four days ---------

/// The county (or state) each contact was SENT from, in log order.
fn sent_from(log: &FieldDayLog) -> Vec<&str> {
    log.qsos().iter().map(|q| q.sent("QTH")).collect()
}

/// The Cabrillo file's QSO lines: what the sponsor reads each contact from.
fn qso_lines(log: &FieldDayLog) -> Vec<String> {
    log.cabrillo(7_000)
        .expect("one entry")
        .lines()
        .filter(|l| l.starts_with("QSO:"))
        .map(str::to_string)
        .collect()
}

/// ⭐ **A County changed in Settings re-labels no contact already logged.** Settings does not
/// move the session that is running (only I moved does), so every row matched the session and
/// the journal wrote none of them a sent side. The session the next entry built from Settings,
/// after a restart or a switch out of the contest and back, then read every earlier contact
/// back with the NEW county, and the Cabrillo file said so.
#[test]
fn a_county_changed_in_settings_relabels_no_contact_already_logged() {
    let mut kane = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut kane, "K9AAA", "COOK", SUN);
    worked(&mut kane, "W9BBB", "LAKE", SUN + 60);
    let sent = qso_lines(&kane);

    // The session the next entry builds, from a County now set to Will.
    let mut will = FieldDayLog::new("W9AWE", select(&station("IL", "WILL")), "40m");
    will.restore_journal(&kane.journal_adif(), SUN + 120 - FOUR_DAYS, SUN + 120);
    assert_eq!(calls(&will), ["K9AAA", "W9BBB"], "PREMISE: both came back");
    assert_eq!(
        sent_from(&will),
        ["KANE", "KANE"],
        "each contact keeps the county it was sent from"
    );
    assert_eq!(qso_lines(&will), sent, "and the Cabrillo file says so");

    // A contact the new session logs is sent from Will, and all three survive the next
    // restart, whichever County that session is built from.
    worked(&mut will, "N9CCC", "DUPG", SUN + 180);
    for county in ["WILL", "KANE", "COOK"] {
        let mut back = FieldDayLog::new("W9AWE", select(&station("IL", county)), "40m");
        back.restore_journal(&will.journal_adif(), SUN + 240 - FOUR_DAYS, SUN + 240);
        assert_eq!(
            sent_from(&back),
            ["KANE", "KANE", "WILL"],
            "restored into a session built in {county}"
        );
    }
}

/// This laptop's position id, in the merge identities below.
const POS: &str = "a1b2c3d4";

/// `(written, already there)` of one merge of `log` into `lb`.
fn merged(log: &FieldDayLog, lb: &mut tempo_core::logbook::Logbook) -> (usize, usize) {
    let r = tempo_core::contest::merge_into_general(log, POS, lb);
    (r.written.len(), r.already)
}

/// The journal an earlier build wrote once its restore had let the party's rows go past its four
/// days: none of them, and their numbers in its header — the highest, and when the latest of them
/// was logged.
fn as_an_earlier_build_let_them_go(party: &FieldDayLog) -> String {
    let journal = party.journal_adif();
    let header = &journal[..journal.find("<EOH>").expect("a header")];
    let seq = party.qsos().iter().map(|q| q.seq).max().unwrap_or(0);
    let when = party.qsos().iter().map(|q| q.when_unix).max().unwrap_or(0);
    let spent = format!("{seq}:{when}");
    format!(
        "{header}<APP_NEXUS_QSEQ_SPENT:{}>{spent}\n<EOH>\n",
        spent.len()
    )
}

/// ⭐ **A contact logged five days after the party's first ones gets a number of its own, and
/// is merged, once.** The journal restore reached back four days, and the contact numbers went
/// with the rows it let go: five days on, the next contact was numbered 1 again, so its merge
/// name was the first contact's, and the merge counted it already in the Logbook and wrote
/// nothing. A club host holding `(position, 1)` would have taken it for that contact too.
///
/// Updated to the restore that loads the running's own rows whatever their age: its premise was
/// that five days on neither of the first two was restored, and now both are, so the new contact
/// numbers on from them. The numbers an earlier build's journal kept in its header, once it had
/// let the rows go, still count.
#[test]
fn a_contact_five_days_on_gets_a_number_of_its_own_and_is_merged_once() {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    let mut lb = tempo_core::logbook::Logbook::new();
    assert_eq!(merged(&party, &mut lb), (2, 0));

    // Five days on, the same running of the party: the first two are in its log.
    let later = SUN + 5 * 86_400;
    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&party.journal_adif(), later - FOUR_DAYS, later);
    assert_eq!(
        calls(&back),
        ["K9AAA", "W9BBB"],
        "the party's own, five days on"
    );
    worked(&mut back, "N9CCC", "DUPG", later);
    assert_eq!(merged(&back, &mut lb), (1, 2), "the new contact is merged");
    assert_eq!(merged(&back, &mut lb), (0, 3), "once");
    let in_logbook: Vec<&str> = lb.records().iter().map(|r| r.call.as_str()).collect();
    assert_eq!(in_logbook, ["K9AAA", "W9BBB", "N9CCC"]);
    assert_eq!(
        back.qsos()[2].seq,
        3,
        "1 and 2 are the first contacts' numbers"
    );

    // The journal an earlier build wrote keeps the numbers of the contacts it let go, though it
    // holds none of them: a session started from it numbers on from them, and so does one
    // started a day later from that session's journal. (Another day: this session logged
    // nothing.)
    let mut idle = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    idle.restore_journal(
        &as_an_earlier_build_let_them_go(&party),
        later - FOUR_DAYS,
        later,
    );
    let next_day = later + 86_400;
    let mut again = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    again.restore_journal(&idle.journal_adif(), next_day - FOUR_DAYS, next_day);
    worked(&mut again, "N9DDD", "WILL", next_day);
    assert_eq!(
        again.qsos()[0].seq,
        3,
        "the first contacts' 1 and 2 stay theirs"
    );
}

/// ⭐ **Another contest worked in between keeps the party's numbers too.** A week after the
/// party, CQ WW: the party's contacts are past the four days and another contest's, so its
/// session let them go and numbered its own from 1. Back in the party the next day, CQ WW's
/// contact was all the journal held, and the party's next contact took its first contacts'
/// number 2.
///
/// Updated to the restore that loads the running's own rows whatever their age: back in the
/// party, its two contacts are in its log again (and in the Logbook already), and the new one
/// numbers on from them.
#[test]
fn a_contest_worked_in_between_keeps_the_partys_numbers() {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    let mut lb = tempo_core::logbook::Logbook::new();
    assert_eq!(merged(&party, &mut lb), (2, 0));

    let cqww = ruleset_by_id("cqww_ssb", CURRENT_RULES_YEAR).expect("a shipped ruleset");
    let zone = StationData {
        mycall: "W9AWE".into(),
        contest_cq_zone: "4".into(),
        ..Default::default()
    };
    let week = SUN + 7 * 86_400;
    let mut other = FieldDayLog::new(
        "W9AWE",
        ContestSession::for_ruleset(cqww, &zone).expect("CQ WW"),
        "20m",
    );
    other.restore_journal(&party.journal_adif(), week - FOUR_DAYS, week);
    assert!(other.log_fields_at(
        "DL9XYZ",
        &fields(&[("RST", "59"), ("ZN", "14")]),
        "PH",
        "USB",
        0,
        week
    ));

    let day = week + 86_400;
    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&other.journal_adif(), day - FOUR_DAYS, day);
    assert_eq!(back.held(), (1, 0), "PREMISE: CQ WW's contact is held");
    assert_eq!(calls(&back), ["K9AAA", "W9BBB"], "the party's own, back");
    worked(&mut back, "N9CCC", "DUPG", day);
    assert_eq!(
        merged(&back, &mut lb),
        (1, 2),
        "the party's new contact is merged"
    );
    assert!(
        back.qsos()[2].seq > 2,
        "a party number already given: {}",
        back.qsos()[2].seq
    );
}

/// The control the two above need: **the party's next running numbers from its own rows**, as
/// it always has. Last year's numbers are another running's names, which this one never
/// shares, and carrying them on would leave a club host that starts the new event empty
/// counting them as contacts still to come.
#[test]
fn the_party_next_year_numbers_from_its_own_rows() {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    let next_year = SUN + 364 * 86_400;
    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&party.journal_adif(), next_year - FOUR_DAYS, next_year);
    assert_ne!(
        back.ruleset().running_of(SUN),
        back.ruleset().running_of(next_year),
        "PREMISE: another running"
    );
    worked(&mut back, "N9CCC", "DUPG", next_year);
    assert_eq!(back.qsos()[0].seq, 1);
}

/// ⭐ **The latest contact let go decides, wherever the journal holds it.** A rehearsal is
/// held after the party's own rows (the journal writes what a restore held last), so five days
/// on the rehearsal's contacts are let go after the party's: the party's numbers are kept
/// whatever order the rows are let go in, and four new contacts are each merged. Numbered from
/// 1 again, the fourth was the party's first contact's number.
///
/// Updated to the restore that loads the running's own rows whatever their age: five days on
/// the party's two are in its log and number themselves, and only the rehearsal's are let go.
#[test]
fn the_latest_contact_let_go_decides_wherever_the_journal_holds_it() {
    let mut rehearsal = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    for (i, call) in ["K9AAA", "K9BBB", "K9CCC"].into_iter().enumerate() {
        worked(&mut rehearsal, call, "COOK", WED + 60 * i as u64);
    }
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    party.restore_journal(&rehearsal.journal_adif(), SUN - FOUR_DAYS, SUN);
    worked(&mut party, "N9AAA", "LAKE", SUN);
    worked(&mut party, "N9BBB", "LAKE", SUN + 60);
    assert_eq!(party.held(), (0, 3), "PREMISE: the rehearsal is held");
    let mut lb = tempo_core::logbook::Logbook::new();
    assert_eq!(merged(&party, &mut lb), (2, 0));

    let later = SUN + 5 * 86_400;
    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&party.journal_adif(), later - FOUR_DAYS, later);
    assert_eq!(back.held(), (0, 0), "PREMISE: the rehearsal let go");
    assert_eq!(
        calls(&back),
        ["N9AAA", "N9BBB"],
        "the party's own, five days on"
    );
    for (i, call) in ["W9AAA", "W9BBB", "W9CCC", "W9DDD"].into_iter().enumerate() {
        worked(&mut back, call, "WILL", later + 60 * i as u64);
    }
    assert_eq!(
        merged(&back, &mut lb),
        (4, 2),
        "every new contact is merged"
    );
}

// ---- a contact never merged into the Logbook, past the journal's four days -------------

/// The call of every record in a journal, in the journal's order.
fn journal_calls(journal: &str) -> Vec<String> {
    records(journal)
        .iter()
        .map(|r| {
            let at = r.find("<CALL:").expect("every record has a call") + "<CALL:".len();
            let (len, rest) = r[at..].split_once('>').expect("a CALL tag");
            rest[..len.parse::<usize>().expect("its length")].to_string()
        })
        .collect()
}

/// ⭐ **A contact never merged into the Logbook stays in the journal, however old it is.** The
/// restore reached back four days, and the next rewrite of the journal wrote only what the
/// restore had loaded: a tester who forgot Merge and logged again five days later lost the
/// party's contacts from Nexus entirely. In the party's next running they are another running's:
/// they stay, byte for byte, and are not loaded, no part of that session's log, its score or its
/// files. (In the party's own running its log holds them, whatever their age.)
#[test]
fn a_contact_never_merged_stays_in_the_journal_however_old() {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    let first = records(&party.journal_adif());

    // A year on, the party's next running, a new contact: the journal is written again.
    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&party.journal_adif(), NEXT_YEAR - FOUR_DAYS, NEXT_YEAR);
    assert_eq!(
        calls(&back),
        Vec::<&str>::new(),
        "PREMISE: another running's"
    );
    worked(&mut back, "N9CCC", "DUPG", NEXT_YEAR);
    let journal = back.journal_adif();
    assert_eq!(journal_calls(&journal), ["N9CCC", "K9AAA", "W9BBB"]);
    for record in &first {
        assert!(
            journal.contains(record.as_str()),
            "kept as logged: {record}"
        );
    }
    assert!(
        !back.adif().contains("K9AAA") && back.qsos().len() == 1,
        "and in nothing this session submits"
    );

    // Ten days on in that running, they are still there, after its own two.
    let later = NEXT_YEAR + 10 * 86_400;
    let mut again = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    again.restore_journal(&journal, later - FOUR_DAYS, later);
    worked(&mut again, "N9DDD", "WILL", later);
    assert_eq!(
        journal_calls(&again.journal_adif()),
        ["N9CCC", "N9DDD", "K9AAA", "W9BBB"],
        "every contact never merged, ten days on"
    );
}

/// The merge identities of a merge's written records, with when each was made: what the
/// Logbook holds of them.
fn copies_of(written: &[tempo_core::logbook::QsoRecord]) -> Vec<(String, u64)> {
    written
        .iter()
        .map(|r| {
            let qid = r.contest.as_deref().expect("a contest copy").qid.clone();
            (qid, r.when_unix)
        })
        .collect()
}

/// What a Logbook holds of merged contacts: each copy's merge identity, and when it was made.
fn copies_in(lb: &tempo_core::logbook::Logbook) -> Vec<(String, u64)> {
    lb.records()
        .iter()
        .filter_map(|r| r.contest.as_deref().map(|c| (c.qid.clone(), r.when_unix)))
        .collect()
}

/// The party, a year on: two contacts logged on its Sunday, never merged, past the window in the
/// party's next running, and one logged in that running now.
fn a_year_on() -> (FieldDayLog, FieldDayLog) {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&party.journal_adif(), NEXT_YEAR - FOUR_DAYS, NEXT_YEAR);
    worked(&mut back, "N9CCC", "DUPG", NEXT_YEAR);
    (party, back)
}

/// ⭐ **The contacts never merged are offered, and one press merges them once** — under the
/// names the party's own merge gives them, so neither merge ever writes one twice. They stay in
/// the journal until their copies are on disk, and then age out as a merged contact always did.
#[test]
fn the_contacts_never_merged_are_offered_and_merged_once() {
    use tempo_core::contest::{plan_aged_merge, settle_aged, Unmerged};
    let (party, mut back) = a_year_on();
    assert_eq!(
        settle_aged(&mut back, POS, &[], &[], NEXT_YEAR),
        [Unmerged {
            event: "ilqp",
            count: 2,
            first_unix: SUN,
            last_unix: SUN + 60,
        }],
        "how many, from which contest, and when"
    );

    let session = back.session.clone();
    let report = plan_aged_merge(&back, "ilqp", &session, POS, Vec::new());
    let written: Vec<&str> = report.written.iter().map(|r| r.call.as_str()).collect();
    assert_eq!(written, ["K9AAA", "W9BBB"]);
    assert_eq!((report.already, report.refused), (0, 0));
    let mut lb = tempo_core::logbook::Logbook::new();
    assert_eq!(
        merged(&party, &mut lb),
        (2, 0),
        "PREMISE: the party's own merge"
    );
    assert_eq!(
        copies_of(&report.written),
        copies_in(&lb),
        "the same names, at the same times"
    );

    // Its copies on their way to disk: offered no more, and still kept.
    let copies = copies_of(&report.written);
    assert_eq!(settle_aged(&mut back, POS, &copies, &[], NEXT_YEAR), []);
    let again = plan_aged_merge(&back, "ilqp", &session, POS, copies.clone());
    assert_eq!(
        (again.added(), again.already),
        (0, 0),
        "a second press writes nothing, and answers only for what it offered"
    );
    assert_eq!(
        journal_calls(&back.journal_adif()),
        ["N9CCC", "K9AAA", "W9BBB"]
    );
    // On disk: let go.
    assert_eq!(settle_aged(&mut back, POS, &copies, &copies, NEXT_YEAR), []);
    assert_eq!(journal_calls(&back.journal_adif()), ["N9CCC"]);
}

/// The control: **a contact merged before it aged out goes as it always did**, once the
/// Logbook is asked and its running is over; and the next running numbers from its own rows
/// either way.
#[test]
fn a_contact_merged_before_it_aged_out_still_ages_out() {
    let (party, mut back) = a_year_on();
    let mut lb = tempo_core::logbook::Logbook::new();
    assert_eq!(merged(&party, &mut lb), (2, 0));
    let copies: Vec<(String, u64)> = copies_in(&lb);
    assert_eq!(
        back.qsos()[0].seq,
        1,
        "the next running numbers from its own rows"
    );
    assert_eq!(
        journal_calls(&back.journal_adif()),
        ["N9CCC", "K9AAA", "W9BBB"],
        "kept until the Logbook is asked"
    );
    assert_eq!(
        tempo_core::contest::settle_aged(&mut back, POS, &copies, &copies, NEXT_YEAR),
        []
    );
    assert_eq!(journal_calls(&back.journal_adif()), ["N9CCC"]);
}

/// ⭐ **A copy that names a contact at another time is not its copy**, and the contact stays.
/// An earlier build numbered a contact logged five days after the party's first ones from 1
/// again, so the party's first contact's copy carries its name too. Let go on the name alone,
/// it was gone from the journal and never in the Logbook. It is kept, and not offered: the
/// merge would take it for the contact already there.
#[test]
fn a_copy_naming_a_contact_at_another_time_keeps_it() {
    use tempo_core::contest::settle_aged;
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    let mut lb = tempo_core::logbook::Logbook::new();
    assert_eq!(merged(&party, &mut lb), (1, 0));
    let copies: Vec<(String, u64)> = copies_in(&lb);

    // That build's journal five days on: the party's contact let go, and W9ZZZ numbered 1.
    let mut reused = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut reused, "W9ZZZ", "WILL", SUN + 5 * 86_400);
    assert_eq!(
        reused.qsos()[0].seq,
        1,
        "PREMISE: the number the party's first had"
    );

    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&reused.journal_adif(), NEXT_YEAR - FOUR_DAYS, NEXT_YEAR);
    assert_eq!(settle_aged(&mut back, POS, &copies, &copies, NEXT_YEAR), []);
    assert_eq!(journal_calls(&back.journal_adif()), ["W9ZZZ"], "kept");
}

/// The control: **a contact taken back stays taken back.** In the party's next running it goes
/// with the next rewrite, as it always did past the window, and it is never offered.
#[test]
fn a_contact_taken_back_ages_out_and_is_never_offered() {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    party
        .remove_last("W9BBB", SUN + 60, SUN + 120)
        .expect("the newest contact");
    assert!(
        party.journal_adif().contains("APP_NEXUS_REMOVED"),
        "PREMISE: journaled as taken back"
    );
    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&party.journal_adif(), NEXT_YEAR - FOUR_DAYS, NEXT_YEAR);
    worked(&mut back, "N9CCC", "DUPG", NEXT_YEAR);
    assert_eq!(journal_calls(&back.journal_adif()), ["N9CCC", "K9AAA"]);
    let offered = tempo_core::contest::settle_aged(&mut back, POS, &[], &[], NEXT_YEAR);
    assert_eq!(
        offered.iter().map(|u| u.count).sum::<usize>(),
        1,
        "K9AAA alone"
    );
}

/// ⭐ **Another contest's contacts are offered under that contest, and merged under the names
/// its own merge gives them.** Field Day's, never merged, read by the party's session a week on.
#[test]
fn another_contests_contacts_are_offered_and_merged_as_that_contest() {
    use tempo_core::contest::{plan_aged_merge, settle_aged};
    let fd = || {
        tempo_core::contest::ContestSession::field_day(
            tempo_core::fieldday::FdEvent::ArrlFd,
            "3A",
            "IL",
        )
    };
    let mut field_day = FieldDayLog::new("W9AWE", fd(), "40m");
    assert!(field_day.log_fields_at(
        "K1ABC",
        &fields(&[("CLASS", "2A"), ("SECTION", "EMA")]),
        "CW",
        "",
        0,
        SUN - 7 * 86_400
    ));
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    party.restore_journal(&field_day.journal_adif(), SUN - FOUR_DAYS, SUN);
    let offered = settle_aged(&mut party, POS, &[], &[], SUN);
    assert_eq!(offered.len(), 1);
    assert_eq!((offered[0].event, offered[0].count), ("arrlfd", 1));

    let report = plan_aged_merge(&party, "arrlfd", &fd(), POS, Vec::new());
    assert_eq!(
        plan_aged_merge(&party, "ilqp", &party.session.clone(), POS, Vec::new()).added(),
        0,
        "nothing of the party's own"
    );
    let mut lb = tempo_core::logbook::Logbook::new();
    tempo_core::contest::merge_into_general(&field_day, POS, &mut lb);
    let fd_copy = copies_in(&lb);
    assert_eq!(
        copies_of(&report.written),
        fd_copy,
        "Field Day's own name for it"
    );
    let rec = &report.written[0];
    assert_eq!(rec.call, "K1ABC");
    assert_eq!(
        rec.contest.as_deref().map(|c| c.srx_string.clone()),
        lb.records()[0]
            .contest
            .as_deref()
            .map(|c| c.srx_string.clone()),
        "the exchange it was logged with"
    );
}

/// ⚠️ **A contact with no number is kept and never offered.** A journal no build since club sync
/// has written numbers no row; no merge name can be given one without taking a name another
/// contact of the running may hold.
#[test]
fn a_contact_with_no_number_is_kept_and_never_offered() {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    let journal = party.journal_adif();
    let tag = journal
        .find("<APP_NEXUS_QSEQ:")
        .expect("PREMISE: a numbered row");
    let end = tag + journal[tag..].find('>').expect("the tag") + 2;
    let unnumbered = format!("{}{}", &journal[..tag], &journal[end..]);
    assert!(!unnumbered.contains("APP_NEXUS_QSEQ"), "PREMISE: no number");

    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&unnumbered, NEXT_YEAR - FOUR_DAYS, NEXT_YEAR);
    assert_eq!(
        tempo_core::contest::settle_aged(&mut back, POS, &[], &[], NEXT_YEAR),
        []
    );
    assert_eq!(journal_calls(&back.journal_adif()), ["K9AAA"], "kept");
}

/// The control: **the contacts kept past the window change nothing a session submits or syncs.**
/// The same contact, logged in the party's next running in a session restored from the journal
/// that keeps the party's two and in one restored from the journal the build before wrote (their
/// numbers in its header, and no rows), writes the same Cabrillo file and the same ADIF, scores
/// the same, and holds the same rows — the rows a club host is sent.
#[test]
fn the_contacts_kept_past_the_window_change_nothing_submitted_or_synced() {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    let later = NEXT_YEAR;
    let mut idle = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    idle.restore_journal(&party.journal_adif(), later - FOUR_DAYS, later);
    let keeps = idle.journal_adif();
    let mut before = keeps.clone();
    for record in records(&keeps) {
        before = before.replacen(&format!("{record}\n"), "", 1);
    }
    assert!(
        before.contains("APP_NEXUS_QSEQ_SPENT") && records(&before).is_empty(),
        "PREMISE: the numbers, and no rows"
    );

    let session = |journal: &str| {
        let mut log = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
        log.restore_journal(journal, later - FOUR_DAYS, later);
        worked(&mut log, "N9CCC", "DUPG", later);
        log
    };
    let (kept, then) = (session(&keeps), session(&before));
    assert_eq!(kept.qsos(), then.qsos(), "the rows a club host is sent");
    assert_eq!(kept.adif(), then.adif());
    assert_eq!(kept.cabrillo(7_000), then.cabrillo(7_000));
    assert_eq!(score(&kept), score(&then));
    assert_ne!(
        kept.journal_adif(),
        then.journal_adif(),
        "the journal alone differs"
    );
}

/// ⭐ **The same station worked again days later in the same running is merged both times**, from
/// a journal an earlier build wrote: it had let the first contact go past its four days when the
/// second was logged, so neither was a dupe where it was logged. Read back together, the later
/// one would be refused as the earlier one's dupe and never merged. (This build's session holds
/// the whole running, so working the station again there is a dupe.)
#[test]
fn the_same_station_worked_again_days_later_is_merged_both_times() {
    use tempo_core::contest::{plan_aged_merge, settle_aged};
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    let later = SUN + 5 * 86_400;
    let mut again = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    again.restore_journal(&party.journal_adif(), later - FOUR_DAYS, later);
    assert_eq!(
        again.ruleset().running_of(SUN),
        again.ruleset().running_of(later),
        "PREMISE: one running"
    );
    assert!(
        again.log_fields_at(
            "K9AAA",
            &fields(&[("RST", "599"), ("QTH", "COOK")]),
            "CW",
            "",
            0,
            later
        ) && again.qsos().last().is_some_and(|q| q.dupe),
        "a dupe of the party's own"
    );

    // The earlier build's journal: the second contact, and the first kept after it.
    let mut earlier = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    earlier.restore_journal(
        &as_an_earlier_build_let_them_go(&party),
        later - FOUR_DAYS,
        later,
    );
    worked(&mut earlier, "K9AAA", "COOK", later);
    let journal = format!(
        "{}{}\n",
        earlier.journal_adif(),
        records(&party.journal_adif()).join("\n")
    );
    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&journal, NEXT_YEAR - FOUR_DAYS, NEXT_YEAR);
    let offered = settle_aged(&mut back, POS, &[], &[], NEXT_YEAR);
    assert_eq!(offered.iter().map(|u| u.count).sum::<usize>(), 2);
    let session = back.session.clone();
    let report = plan_aged_merge(&back, "ilqp", &session, POS, Vec::new());
    let when: Vec<u64> = report.written.iter().map(|r| r.when_unix).collect();
    assert_eq!(when, [later, SUN], "both, each once");
}

/// **Each contest's contacts are offered on their own**, in the order the journal keeps them.
#[test]
fn each_contest_is_offered_on_its_own() {
    use tempo_core::contest::{settle_aged, Unmerged};
    let mut field_day = FieldDayLog::new(
        "W9AWE",
        tempo_core::contest::ContestSession::field_day(
            tempo_core::fieldday::FdEvent::ArrlFd,
            "3A",
            "IL",
        ),
        "40m",
    );
    assert!(field_day.log_fields_at(
        "K1ABC",
        &fields(&[("CLASS", "2A"), ("SECTION", "EMA")]),
        "CW",
        "",
        0,
        SUN - 9 * 86_400
    ));
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    party.restore_journal(&field_day.journal_adif(), SUN - FOUR_DAYS, SUN);
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&party.journal_adif(), NEXT_YEAR - FOUR_DAYS, NEXT_YEAR);
    assert_eq!(
        settle_aged(&mut back, POS, &[], &[], NEXT_YEAR),
        [
            Unmerged {
                event: "ilqp",
                count: 2,
                first_unix: SUN,
                last_unix: SUN + 60,
            },
            Unmerged {
                event: "arrlfd",
                count: 1,
                first_unix: SUN - 9 * 86_400,
                last_unix: SUN - 9 * 86_400,
            },
        ]
    );
}

/// **A journal read twice into one log keeps each contact once** — as it holds each record of
/// another contest once.
#[test]
fn a_journal_read_twice_keeps_each_contact_once() {
    let (_, back) = a_year_on();
    let journal = back.journal_adif();
    let later = NEXT_YEAR + 86_400;
    let mut twice = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    twice.restore_journal(&journal, later - FOUR_DAYS, later);
    twice.restore_journal(&journal, later - FOUR_DAYS, later);
    assert_eq!(
        journal_calls(&twice.journal_adif()),
        ["N9CCC", "K9AAA", "W9BBB"]
    );
}

// ---- the party's log holds the whole party, whatever its age -----------------------------

/// ⭐ **The party's log holds every one of its contacts, however long after the party it is
/// opened**: its rows and count, its score, the Cabrillo and ADIF files it submits, and the dupe
/// check. The restore loaded only the journal's last four days, so the party opened five days on
/// held none of its contacts, and a Cabrillo file exported then held none of them; sponsors take
/// logs for weeks. Five, ten and thirty days on, in the party's running, every one is there as
/// the party left it, and working one of them again is a dupe.
#[test]
fn the_partys_log_holds_every_contact_five_ten_and_thirty_days_on() {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    let journal = party.journal_adif();
    for days in [5, 10, 30] {
        let later = SUN + days * 86_400;
        let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
        back.restore_journal(&journal, later - FOUR_DAYS, later);
        assert_eq!(
            back.ruleset().running_of(later),
            back.ruleset().running_of(SUN),
            "PREMISE: the party's running, {days} days on"
        );
        assert_eq!(calls(&back), ["K9AAA", "W9BBB"], "{days} days on");
        assert_eq!(back.qso_count(), 2, "{days} days on");
        assert_eq!(score(&back), score(&party), "{days} days on");
        assert_eq!(qso_lines(&back), qso_lines(&party), "{days} days on");
        assert_eq!(
            back.submission_adif(),
            party.submission_adif(),
            "{days} days on"
        );
        assert!(
            back.log_fields_at(
                "K9AAA",
                &fields(&[("RST", "599"), ("QTH", "COOK")]),
                "CW",
                "",
                0,
                later
            ) && back.qsos().last().is_some_and(|q| q.dupe),
            "{days} days on, K9AAA worked again is a dupe"
        );
    }
}

/// The controls: **the party's next running, and another contest, hold none of its contacts**,
/// and the journal keeps every one. A year on the party starts empty, and CQ WW opened a week
/// after it starts with nothing of the party's.
#[test]
fn the_next_running_and_another_contest_hold_none_of_the_partys_contacts() {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    let journal = party.journal_adif();

    let next_year = SUN + 364 * 86_400;
    let mut next = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    next.restore_journal(&journal, next_year - FOUR_DAYS, next_year);
    assert_ne!(
        next.ruleset().running_of(next_year),
        next.ruleset().running_of(SUN),
        "PREMISE: the next running"
    );
    assert_eq!(calls(&next), Vec::<&str>::new(), "a year on");
    assert_eq!(journal_calls(&next.journal_adif()), ["K9AAA", "W9BBB"]);

    let cqww = ruleset_by_id("cqww_ssb", CURRENT_RULES_YEAR).expect("a shipped ruleset");
    let zone = StationData {
        mycall: "W9AWE".into(),
        contest_cq_zone: "4".into(),
        ..Default::default()
    };
    let week = SUN + 7 * 86_400;
    let mut other = FieldDayLog::new(
        "W9AWE",
        ContestSession::for_ruleset(cqww, &zone).expect("CQ WW"),
        "20m",
    );
    other.restore_journal(&journal, week - FOUR_DAYS, week);
    assert_eq!(calls(&other), Vec::<&str>::new(), "CQ WW a week on");
    assert_eq!(journal_calls(&other.journal_adif()), ["K9AAA", "W9BBB"]);

    // At the window's very edge (the party's first contact four days back to the second), CQ WW
    // holds them both, and reads neither into its log.
    let edge = SUN + FOUR_DAYS;
    let mut at_edge = FieldDayLog::new(
        "W9AWE",
        ContestSession::for_ruleset(cqww, &zone).expect("CQ WW"),
        "20m",
    );
    at_edge.restore_journal(&journal, edge - FOUR_DAYS, edge);
    assert_eq!(calls(&at_edge), Vec::<&str>::new(), "at the window's edge");
    assert_eq!(at_edge.held(), (2, 0), "at the window's edge");
}

/// ⭐ **Another contest's session lets none of the party's contacts go while the party's running
/// lasts**, merged or not, and back in the party its log holds them all. To CQ WW opened a week
/// on they are another contest's, past its four days, and the contest screen asking the Logbook
/// let the merged ones go: the next contact wrote CQ WW's journal without them, and the party's
/// Cabrillo file exported after that held none of them. A year on, the party's running over,
/// they go as a merged contact always did.
#[test]
fn another_contests_session_lets_none_of_the_partys_contacts_go_while_its_running_lasts() {
    use tempo_core::contest::settle_aged;
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    let mut lb = tempo_core::logbook::Logbook::new();
    assert_eq!(merged(&party, &mut lb), (2, 0));
    let copies = copies_in(&lb);

    let cqww = || {
        let zone = StationData {
            mycall: "W9AWE".into(),
            contest_cq_zone: "4".into(),
            ..Default::default()
        };
        let rs = ruleset_by_id("cqww_ssb", CURRENT_RULES_YEAR).expect("a shipped ruleset");
        ContestSession::for_ruleset(rs, &zone).expect("CQ WW")
    };
    let week = SUN + 7 * 86_400;
    let mut other = FieldDayLog::new("W9AWE", cqww(), "20m");
    other.restore_journal(&party.journal_adif(), week - FOUR_DAYS, week);
    assert_eq!(
        other.aged(),
        2,
        "PREMISE: another contest's, past the window"
    );
    assert_eq!(
        settle_aged(&mut other, POS, &copies, &copies, week),
        [],
        "PREMISE: both merged"
    );
    assert!(other.log_fields_at(
        "DL9XYZ",
        &fields(&[("RST", "59"), ("ZN", "14")]),
        "PH",
        "USB",
        0,
        week
    ));
    let journal = other.journal_adif();
    assert_eq!(
        journal_calls(&journal),
        ["DL9XYZ", "K9AAA", "W9BBB"],
        "kept"
    );

    let day = week + 86_400;
    let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    back.restore_journal(&journal, day - FOUR_DAYS, day);
    assert_eq!(calls(&back), ["K9AAA", "W9BBB"]);
    assert_eq!(
        qso_lines(&back),
        qso_lines(&party),
        "the party's Cabrillo file, a week on"
    );

    // CONTROL: a year on, the party's running is over, and they go.
    let mut next = FieldDayLog::new("W9AWE", cqww(), "20m");
    next.restore_journal(&journal, NEXT_YEAR - FOUR_DAYS, NEXT_YEAR);
    assert_eq!(settle_aged(&mut next, POS, &copies, &copies, NEXT_YEAR), []);
    assert_eq!(journal_calls(&next.journal_adif()), ["DL9XYZ"]);
}

/// The control: **a contact taken back in the party stays taken back, however long after it the
/// party is opened** — among the contacts it took back, out of its files, its score and its dupe
/// check, its number still taken.
#[test]
fn a_contact_taken_back_in_the_party_stays_taken_back_however_old() {
    let mut party = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
    worked(&mut party, "K9AAA", "COOK", SUN);
    worked(&mut party, "W9BBB", "LAKE", SUN + 60);
    party
        .remove_last("W9BBB", SUN + 60, SUN + 120)
        .expect("the newest contact");
    let journal = party.journal_adif();
    for days in [5, 30] {
        let later = SUN + days * 86_400;
        let mut back = FieldDayLog::new("W9AWE", select(&station("IL", "KANE")), "40m");
        back.restore_journal(&journal, later - FOUR_DAYS, later);
        assert_eq!(calls(&back), ["K9AAA"], "{days} days on");
        let taken_back: Vec<&str> = back
            .removed()
            .iter()
            .flat_map(|r| r.rows.iter().map(|q| q.call.as_str()))
            .collect();
        assert_eq!(taken_back, ["W9BBB"], "{days} days on");
        assert_eq!(back.qso_count(), 1, "{days} days on");
        assert_eq!(qso_lines(&back), qso_lines(&party), "{days} days on");
        worked(&mut back, "W9BBB", "LAKE", later);
        assert_eq!(
            back.qsos()[1].seq,
            3,
            "{days} days on: its number stays taken"
        );
    }
}
