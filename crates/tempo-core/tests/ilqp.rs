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
        !log.log_fields_at("K9AAA", &cook, "DIG", "RTTY", 0, 110),
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
