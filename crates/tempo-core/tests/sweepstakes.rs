//! ⭐ **ARRL November Sweepstakes, end to end** — the proof batch 9 exists for.
//!
//! Same shape as `qso_parties.rs`: select the contest, work several stations including
//! a dupe, and read back the things ARRL's rules decide — the entry strip's boxes, the
//! sent exchange, the dupe verdict, the score, and the Cabrillo the operator submits.
//!
//! Every expected value here comes from a source read in full on 2026-09-09, and the
//! quote is reproduced at the assertion so a rules change is a red test with the
//! sentence it contradicts attached:
//!
//! * `https://contests.arrl.org/ContestRules/SS-Rules.pdf`, **Version 2.1**, all 10
//!   pages — the exchange (§4), the scoring (§5), the dupe rule (§2.2), the operating
//!   period, and the Entry Categories table's power thresholds;
//! * `https://contests.arrl.org/contestmultipliers.php?a=wve` — *"These are the names
//!   of the **85** W/VE sections used in ARRL contests"*, which is the multiplier
//!   universe and the `SEC` domain;
//! * `https://www.arrl.org/cabrillo-format-tutorial` — ARRL's own **QSO DATA TEMPLATE**
//!   for Sweepstakes, with the lettered legend that settles the column order;
//! * `https://adif.org/317/ADIF_317.htm` and
//!   `https://www.contestcalendar.com/cabnames.php` — the two registries, which agree
//!   for this contest.
//!
//! ⚠️ Integration tests, so they see only the public API — which is the point: if an
//! operator cannot reach Sweepstakes through it, neither can this file.
use tempo_core::contest::{ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

/// "Pick ARRL November Sweepstakes (CW)" — the operator action, as the two calls the
/// engine makes.
fn select(event_id: &str, station: &StationData) -> ContestSession {
    let rs = ruleset_by_id(event_id, CURRENT_RULES_YEAR)
        .unwrap_or_else(|| panic!("{event_id} is a shipped ruleset"));
    ContestSession::for_ruleset(rs, station)
        .unwrap_or_else(|e| panic!("{event_id} session refused: {e}"))
}

/// A fully declared Sweepstakes entrant: W9XYZ, Wisconsin, first licensed 1974, single
/// operator, low power, unassisted. That is the §6.3 walk's own operator.
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

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn score(log: &FieldDayLog) -> (u32, u32, Option<u32>, u32) {
    log.ruleset().scoring.score(log.score_rows(), 1)
}

/// 2026-11-07T21:04:00Z — four minutes into the CW weekend the ruleset computes
/// (*"CW: First full weekend in November"*, *"Begins 2100 UTC Saturday"*), so the dates
/// on the Cabrillo lines below are dates this contest could actually have been worked.
const SAT: u64 = 1_794_085_440;

/// ⭐ **The whole contest, worked from Wisconsin.**
///
/// SS-Rules v2.1: exchange *"4.1 Serial Number … 4.2 Precedence … 4.3 Your call sign
/// (the call sign must be included during the exchange) 4.4 Check … 4.5 Section"*;
/// *"5.1 QSO points: Each contact counts for two (2) points."*; *"5.2 Multipliers:
/// ARRL/RAC Sections"*; *"2.2 Each station may be contacted only once, regardless of
/// band."*
#[test]
fn sweepstakes_cw_from_wisconsin_end_to_end() {
    let s = select("arrlss_cw", &w9xyz());

    // ONE role — "W/VE stations may only contact other W/VE stations" is one side, so
    // there is no asymmetry to model.
    assert_eq!(s.role().id, "");
    // ⚠️ TWO registries that AGREE for this contest: ADIF 3.1.7 lists ARRL-SS-CW as
    // "ARRL November Sweepstakes (CW)", and the Cabrillo master list carries the same
    // string at id 177.
    assert_eq!(s.contest_id, "ARRL-SS-CW");

    // THE ENTRY STRIP: five boxes, all required — the §9 sizing case, and the only
    // exchange in the shipped set with a callsign among them.
    assert_eq!(
        entry_boxes(&s),
        vec![
            ("NR", true),
            ("PREC", true),
            ("CALL", true),
            ("CK", true),
            ("SEC", true),
        ]
    );

    // THE SENT EXCHANGE. The serial is a PLACEHOLDER until one is issued (§2.6); the
    // precedence is DERIVED from the declared category, never typed — single op, low
    // power, unassisted is "“A” for Single Operator, Low Power".
    assert_eq!(
        composing(&s),
        vec![
            ("NR", "0".into()),
            ("PREC", "A".into()),
            ("CALL", "W9XYZ".into()),
            ("CK", "74".into()),
            ("SEC", "WI".into()),
        ]
    );
    // The SECTION carries the domain that matched it — the sponsor's current 85, not
    // the 83-code Field Day list.
    assert_eq!(s.my_exchange[4].domain, Some("ss_sections"));
    // …and the entry's declared LOCATION is that section, which is what Cabrillo's
    // LOCATION header means here (ARRL's own sample: `LOCATION: CT`).
    assert_eq!(s.my_location.state, "WI");

    let mut log = FieldDayLog::new("W9XYZ", s, "20m");

    // §6.3's walk, rows 1–4. A serial is issued at COMPOSE time, for a peer; K1ABC
    // never answers, so serial 1 is spent and the gap is invisible to a checker.
    assert_eq!(log.session.compose_for("K1ABC", SAT).tx[0].raw, "1");
    assert_eq!(log.session.next_serial, 2);
    log.session.clear_in_flight();
    assert_eq!(log.session.compose_for("K2DEF", SAT + 30).tx[0].raw, "2");

    // K2DEF completes, sending "12 A K2DEF 71 CT".
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
        SAT + 60
    ));

    // ⭐ **THE NO-BAND DUPE KEY.** *"Each station may be contacted only once, regardless
    // of band."* — so the SAME station on a DIFFERENT band is refused, which is the
    // whole difference from every other contest in the shipped set. No exchange is
    // composed for it: the refusal happens before a serial would be spent.
    log.band = "40m".into();
    assert!(
        !log.log_fields_at(
            "K2DEF",
            &fields(&[
                ("NR", "40"),
                ("PREC", "A"),
                ("CALL", "K2DEF"),
                ("CK", "71"),
                ("SEC", "CT"),
            ]),
            "CW",
            "",
            0,
            SAT + 120
        ),
        "SS dupes across the whole contest: a second contact on 40m is not a contact"
    );
    // POSITIVE CONTROL: a DIFFERENT station on that same new band logs, so the refusal
    // above is the callsign component of the key and not a log that stopped accepting.
    log.session.clear_in_flight();
    log.session.compose_for("N0GHI", SAT + 150);
    assert!(log.log_fields_at(
        "N0GHI",
        &fields(&[
            ("NR", "3"),
            ("PREC", "U"),
            ("CALL", "N0GHI"),
            ("CK", "82"),
            ("SEC", "MO"),
        ]),
        "CW",
        "",
        0,
        SAT + 180
    ));
    // ⭐ …and a RECEIVED check that differs from mine is never constrained: N0GHI was
    // licensed in 1982 and I in 1974, which is the contest working correctly.
    assert_eq!(log.constant_sent_warning(), None);

    // A third station, in a section already worked, to prove PerLog counts once.
    log.band = "20m".into();
    log.session.compose_for("W1AW", SAT + 210);
    assert!(log.log_fields_at(
        "W1AW",
        &fields(&[
            ("NR", "9"),
            ("PREC", "M"),
            ("CALL", "W1AW"),
            ("CK", "36"),
            ("SEC", "CT"),
        ]),
        "CW",
        "",
        0,
        SAT + 240
    ));

    // ⭐ **THE SCORE.** "Each contact counts for two (2) points" × 3 logged contacts = 6.
    // "Final score equals the total QSO points times the total multipliers contacted",
    // and the multiplier is the SECTION at PerLog scope: CT, MO and CT again = 2, not 3
    // and not 4 (CT was worked on two bands and counts once).
    let (qso, powered, mults, total) = score(&log);
    assert_eq!((qso, powered), (6, 6), "2 points a contact, no power tier");
    assert_eq!(mults, Some(2), "CT and MO — CT once, not once per band");
    assert_eq!(total, 12, "6 QSO points × 2 sections");
    // Sweepstakes' score is COMPLETE — no bonus term this build cannot compute.
    assert_eq!(log.ruleset().score_note_key, "");

    // ⭐ **THE CABRILLO**, in ARRL's own published column order.
    let cab = log.cabrillo(14_030).expect("a single-mode event exports");
    assert!(cab.contains("CONTEST: ARRL-SS-CW\n"), "{cab}");
    assert!(cab.contains("CALLSIGN: W9XYZ\n"), "{cab}");
    assert!(cab.contains("LOCATION: WI\n"), "{cab}");
    assert!(cab.contains("CATEGORY-OPERATOR: SINGLE-OP\n"), "{cab}");
    // `E= Your call. F= Your QSO #. G= Your precedence. H= Your check. I= Your ARRL
    // Section. J= The call of the station you worked. K … N= theirs.`
    assert!(
        cab.contains("QSO: 14000 CW 2026-11-07 2105 W9XYZ 2 A 74 WI K2DEF 12 A 71 CT\n"),
        "{cab}"
    );
    assert!(
        cab.contains("QSO: 7000 CW 2026-11-07 2107 W9XYZ 3 A 74 WI N0GHI 3 U 82 MO\n"),
        "{cab}"
    );
    assert!(
        cab.contains("QSO: 14000 CW 2026-11-07 2108 W9XYZ 4 A 74 WI W1AW 9 M 36 CT\n"),
        "{cab}"
    );
    // ⭐ §6.2's own control, in its own words: EXACTLY TWO callsign occurrences on a
    // line whose exchange declares a callsign on both sides. Four is unsubmittable.
    for line in cab.lines().filter(|l| l.starts_with("QSO:")) {
        assert_eq!(line.matches("W9XYZ").count(), 1, "{line}");
    }
    assert_eq!(cab.matches("K2DEF").count(), 1, "{cab}");
}

/// ⭐ **The Phone weekend is a SEPARATE contest**, with its own token and its own
/// weekend — not one ruleset with a mode split. *"CW: First full weekend in November /
/// Phone: Third full weekend in November"*, and *"2.4 Use only CW during the CW weekend
/// and Phone during the Phone weekend."*
#[test]
fn the_phone_weekend_is_a_second_ruleset_with_its_own_token_and_window() {
    let cw = ruleset_by_id("arrlss_cw", CURRENT_RULES_YEAR).expect("shipped");
    let ph = ruleset_by_id("arrlss_ssb", CURRENT_RULES_YEAR).expect("shipped");
    assert_eq!(cw.contest_id, "ARRL-SS-CW");
    assert_eq!(ph.contest_id, "ARRL-SS-SSB");
    // "Contest Period: Begins 2100 UTC Saturday and runs through 0259 UTC Monday" —
    // 30 hours from 2100Z — on two different weekends of November 2026. Nov 1 2026 is a
    // Sunday, so the first full weekend is Sat 7 November and the third is Sat 21st.
    let (cw_w, ph_w) = (cw.event_window(2026), ph.event_window(2026));
    let utc = |m: u32, d: u32, h: u64| {
        // Days from 1970-01-01 to 2026-11-01 is a constant this test does not need to
        // compute: 2026-11-07T21:00Z and 2026-11-21T21:00Z, 14 days apart.
        let _ = (m, d);
        h
    };
    let _ = utc;
    assert_eq!(
        ph_w.start_unix - cw_w.start_unix,
        14 * 24 * 3600,
        "the first full weekend and the third are 14 days apart"
    );
    for w in [cw_w, ph_w] {
        assert_eq!(
            w.end_unix - w.start_unix,
            30 * 3600,
            "2100Z Sat → 0300Z Mon"
        );
        assert_eq!(
            (w.start_unix % 86_400) / 3600,
            21,
            "\"Begins 2100 UTC Saturday\""
        );
        // Unix day 0 was a Thursday, so a Saturday is day ≡ 2 (mod 7).
        assert_eq!((w.start_unix / 86_400) % 7, 2, "a Saturday");
    }
    // Everything else about them is the same contest.
    assert_eq!(cw.exchange.name, ph.exchange.name);
    assert_eq!(cw.dupe_rule, ph.dupe_rule);

    // The phone entry sends the same exchange and exports under the other token.
    let s = select("arrlss_ssb", &w9xyz());
    let mut log = FieldDayLog::new("W9XYZ", s, "20m");
    log.session.compose_for("K2DEF", 100);
    assert!(log.log_fields_at(
        "K2DEF",
        &fields(&[
            ("NR", "12"),
            ("PREC", "A"),
            ("CALL", "K2DEF"),
            ("CK", "71"),
            ("SEC", "CT"),
        ]),
        "PH",
        "",
        0,
        110
    ));
    let cab = log.cabrillo(14_250).expect("one entry");
    assert!(cab.contains("CONTEST: ARRL-SS-SSB\n"), "{cab}");
    // ⚠️ No RST column either way — SS's exchange has no signal report, and a writer
    // with a fixed RST column is wrong for this contest.
    assert!(
        cab.contains("QSO: 14000 PH 1970-01-01 0001 W9XYZ 1 A 74 WI K2DEF 12 A 71 CT\n"),
        "{cab}"
    );
    assert!(!cab.contains(" 59 "), "SS sends no signal report:\n{cab}");
}

/// ⭐ **`constant_sent`, both directions** (§6.3: *"one direction is half a test, and
/// read the wrong way round this flag refuses legal contacts"*).
///
/// SS-Rules §4.4.2: *"The same Check must be sent throughout the contest."* It
/// constrains the check I SEND. It says nothing about the checks I receive.
#[test]
fn the_constant_check_guard_fires_on_my_own_check_and_never_on_theirs() {
    let mut log = FieldDayLog::new("W9XYZ", select("arrlss_cw", &w9xyz()), "20m");
    let work = |log: &mut FieldDayLog, call: &str, nr: &str, ck: &str, sec: &str, t: u64| {
        log.session.compose_for(call, t);
        assert!(log.log_fields_at(
            call,
            &fields(&[
                ("NR", nr),
                ("PREC", "A"),
                ("CALL", call),
                ("CK", ck),
                ("SEC", sec),
            ]),
            "CW",
            "",
            0,
            t
        ));
    };

    // DIRECTION 1 — three stations, three DIFFERENT received checks. Every one of them
    // was licensed in a different year, and the guard must stay silent.
    work(&mut log, "K2DEF", "12", "71", "CT", 100);
    work(&mut log, "N0GHI", "3", "82", "MO", 110);
    work(&mut log, "W1AW", "9", "36", "EMA", 120);
    assert_eq!(
        log.constant_sent_warning(),
        None,
        "a RECEIVED check differs per station and is never constrained"
    );
    assert_eq!(log.constant_sent_scan(), None);
    assert!(log.cabrillo(14_030).is_ok(), "and the entry exports");

    // DIRECTION 2 — the operator corrects their OWN check mid-contest. Rows 1–3 sent
    // 74; this one sends 47.
    log.session
        .move_to(&[("CK", "47")])
        .expect("CK is a slot this role sends");
    work(&mut log, "K5AF", "60", "60", "STX", 130);

    // FIRING SITE 1 — at log time, and it WARNED: the contact above was logged.
    let m = log
        .constant_sent_warning()
        .expect("the sent check changed part-way through the log");
    assert_eq!(m.key, "CK");
    assert_eq!((m.first.as_str(), m.then.as_str()), ("74", "47"));
    assert_eq!(
        log.qso_count(),
        4,
        "the guard WARNS; it never refuses a QSO"
    );

    // FIRING SITE 2 — at export, the whole log is scanned and the entry is refused by
    // name, with both values and the row counts. A log sending two checks is not one
    // entry under §4.4.2.
    let err = log
        .cabrillo(14_030)
        .expect_err("two checks in one log is not one entry");
    assert!(err.contains("\"74\""), "{err}");
    assert!(err.contains("\"47\""), "{err}");
    assert!(err.contains("3 contact(s)"), "{err}");
    assert!(err.contains("CK"), "{err}");
}

/// ⭐ **PRECEDENCE is derived from the entry category, and an undeclared axis REFUSES.**
///
/// A default here would transmit a category the operator never claimed on every contact
/// of a 24-hour contest — the same defect as the hardcoded `CATEGORY-OPERATOR: MULTI-OP`
/// this model replaced.
#[test]
fn precedence_comes_from_the_declared_category_and_is_never_guessed() {
    let rs = ruleset_by_id("arrlss_cw", CURRENT_RULES_YEAR).expect("shipped");
    let prec = |power: &str, assisted: &str, station: &str, operator: &str| {
        ContestSession::for_ruleset(
            rs,
            &StationData {
                contest_category_power: power.into(),
                contest_category_assisted: assisted.into(),
                contest_category_station: station.into(),
                contest_category_operator: operator.into(),
                ..w9xyz()
            },
        )
        .map(|s| s.field("PREC").to_string())
    };

    // SS-Rules v2.1 §4.2, through the whole public path.
    assert_eq!(prec("QRP", "NON-ASSISTED", "", "SINGLE-OP"), Ok("Q".into()));
    assert_eq!(prec("LOW", "NON-ASSISTED", "", "SINGLE-OP"), Ok("A".into()));
    assert_eq!(
        prec("HIGH", "NON-ASSISTED", "", "SINGLE-OP"),
        Ok("B".into())
    );
    assert_eq!(prec("HIGH", "ASSISTED", "", "SINGLE-OP"), Ok("U".into()));
    assert_eq!(prec("LOW", "NON-ASSISTED", "", "MULTI-OP"), Ok("M".into()));
    assert_eq!(
        prec("LOW", "NON-ASSISTED", "SCHOOL", "SINGLE-OP"),
        Ok("S".into())
    );

    // …and the entry category reaches the Cabrillo header from the same declaration,
    // so the letter and the header cannot disagree.
    let multi = ContestSession::for_ruleset(
        rs,
        &StationData {
            contest_category_operator: "MULTI-OP".into(),
            ..w9xyz()
        },
    )
    .expect("a declared multi-op entry");
    assert_eq!(multi.field("PREC"), "M");
    assert_eq!(multi.entry_category.token(), "MULTI-OP");

    // AN UNDECLARED AXIS REFUSES, with a sentence naming what to do.
    let err = prec("", "NON-ASSISTED", "", "SINGLE-OP").expect_err("no power category");
    assert!(err.contains("PRECEDENCE"), "{err}");
    assert!(err.contains("5 W"), "{err}");
    let err = prec("LOW", "", "", "SINGLE-OP").expect_err("no assisted declaration");
    assert!(err.contains("Unlimited"), "{err}");
}

/// The `SEC` domain is the SPONSOR'S CURRENT LIST — 85 sections, Territories as `TER`.
///
/// *"These are the names of the 85 W/VE sections used in ARRL contests, along with their
/// standard abbreviations."* (`contestmultipliers.php?a=wve`, read 2026-09-09.) §5.2's
/// prose *"plus Northern Territories"* is a NAME, not an abbreviation: `NT` is not on
/// the sponsor's list, and a `SEC` domain seeded from that phrase is wrong for one
/// section.
#[test]
fn the_section_domain_is_the_sponsors_current_85_with_ter_not_nt() {
    let rs = ruleset_by_id("arrlss_cw", CURRENT_RULES_YEAR).expect("shipped");
    let sec = rs.exchange.field("SEC").expect("SEC");
    let tempo_core::contest::FieldKind::Enum { domain } = sec.kind else {
        panic!("SEC is an enum over the section list")
    };
    assert_eq!(domain.id, "ss_sections");
    assert_eq!(domain.values.len(), 85, "the sponsor states 85");
    assert!(domain.contains("TER"), "Territories is TER");
    assert!(
        !domain.contains("NT"),
        "NT is not an ARRL section abbreviation — §5.2's \"Northern Territories\" is prose"
    );
    // The three the older ARRL era had and this list does not, plus the ones that
    // replaced them — a section list carried from a pre-2017 era is wrong on all six.
    for gone in ["MAR", "GTA"] {
        assert!(!domain.contains(gone), "{gone} is not a current section");
    }
    for now in ["NB", "NS", "PE", "GH", "ONE", "ONN", "ONS"] {
        assert!(domain.contains(now), "{now} is a current section");
    }
    // An out-of-domain section is refused BEFORE it goes on the air.
    let err = ContestSession::for_ruleset(
        rs,
        &StationData {
            fd_section: "NT".into(),
            ..w9xyz()
        },
    )
    .expect_err("NT is not a section this contest accepts");
    assert!(err.contains("NT"), "{err}");
}
