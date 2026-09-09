//! ⭐ **One end-to-end run per state QSO party** — the proof the whole batch exists for.
//!
//! Batch 8 shipped four rulesets, four county domains and the station-data settings, and
//! an operator could not SELECT any of them: `ContestSession` had one constructor and it
//! was Field Day's. These tests are the answer to "can somebody actually work this
//! contest", asked the only way it can be answered offline: **select the party, work
//! several stations including a dupe, and read back the four things a sponsor's rules
//! decide** — the entry strip's boxes, the dupe verdict, the score, and the Cabrillo the
//! operator submits.
//!
//! Every expected number in here comes from `tasks/contest-sources-verified.md`, which
//! read each sponsor's current page in full on 2026-09-09; the quotes are reproduced at
//! each assertion so a rules change is a red test with the sentence it contradicts
//! attached, not a silent rescore.
//!
//! ⚠️ These are integration tests, so they see only the public API — which is the point:
//! if an operator cannot reach a contest through it, neither can this file.
use tempo_core::contest::{ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

/// "Select the Tennessee QSO Party from Ohio" — the operator action, as the two calls
/// the engine makes: look the ruleset up by its rules-file id, build a session from the
/// station data.
fn select(event_id: &str, station: &StationData) -> ContestSession {
    let rs = ruleset_by_id(event_id, CURRENT_RULES_YEAR)
        .unwrap_or_else(|| panic!("{event_id} is a shipped ruleset"));
    ContestSession::for_ruleset(rs, station)
        .unwrap_or_else(|e| panic!("{event_id} session refused: {e}"))
}

fn station(state: &str, county: &str) -> StationData {
    StationData {
        contest_qth_state: state.to_string(),
        contest_qth_county: county.to_string(),
        ..Default::default()
    }
}

/// The entry strip's boxes, in order — `(slot id, required)`. This is exactly what the
/// DTO's `receives` is built from, so asserting it here asserts the strip.
fn entry_boxes(s: &ContestSession) -> Vec<(&'static str, bool)> {
    s.role()
        .receives
        .iter()
        .filter_map(|k| s.exchange.field(k))
        .map(|f| (f.key, f.required))
        .collect()
}

/// What the session is composing, as `(slot, value)` pairs — the read-only sent display.
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

/// `(qso_points, powered, multipliers, total)` for a log, through the ruleset it runs.
///
/// The multiplier count is an `Option` because `None` ("this contest has no multiplier
/// concept") and `Some(0)` ("it has one and none has been worked") are different facts.
/// Every party here is in the `Some` arm; Field Day is the `None` arm, asserted in
/// `tempo-app`'s snapshot test.
fn score(log: &FieldDayLog) -> (u32, u32, Option<u32>, u32) {
    log.ruleset().scoring.score(log.score_rows(), 1)
}

// ---------------------------------------------------------------------------
// TNQP — Tennessee QSO Party
// ---------------------------------------------------------------------------

/// ⭐ **TNQP, worked from Tennessee, end to end.**
///
/// Sponsor (tnqp.org 2026 rules PDF, read 2026-09-09):
/// * exchange — *"RS(T) and Tennessee County, U.S. state, Canadian province/territory,
///   or DXCC entity"*;
/// * points — *"3 points per QSO regardless of mode"*;
/// * multipliers — *"Tennessee Stations … Multipliers accumulate on a per band basis"*;
/// * *"Tennessee stations may work anyone"*.
#[test]
fn tnqp_from_tennessee_works_anyone_and_scores_per_band_multipliers() {
    let s = select("tnqp", &station("TN", "WILL"));

    // THE ROLE, derived from where the operator is — never typed.
    assert_eq!(s.role().id, "in_state");
    assert_eq!(s.contest_id, "TN-QSO-PARTY");
    // THE ENTRY STRIP: RST then QTH, both required — not a Class/Section pair.
    assert_eq!(entry_boxes(&s), vec![("RST", true), ("QTH", true)]);
    // THE SENT EXCHANGE: the 3-digit RST the ruleset declares, and MY county.
    assert_eq!(
        composing(&s),
        vec![("RST", "599".into()), ("QTH", "WILL".into())]
    );
    // …and the county carries the arm that matched it, which is what picks its ADIF
    // column and its multiplier bucket.
    assert_eq!(s.my_exchange[1].domain, Some("tn_counties"));

    let mut log = FieldDayLog::new("W4ABC", s, "20m");
    // A Connecticut station sends its state; a Davidson-county station sends a county.
    assert!(log.log_fields_at(
        "K1ABC",
        &fields(&[("RST", "599"), ("QTH", "CT")]),
        "CW",
        "",
        0,
        100
    ));
    assert!(log.log_fields_at(
        "W4XYZ",
        &fields(&[("RST", "599"), ("QTH", "DAVI")]),
        "CW",
        "",
        0,
        110
    ));

    // THE DUPE VERDICT — `(call, band, mode class, QTH)`, so the same station on the
    // same band and mode is refused…
    assert!(
        !log.log_fields_at(
            "K1ABC",
            &fields(&[("RST", "599"), ("QTH", "CT")]),
            "CW",
            "",
            0,
            120
        ),
        "same call, band and mode class is a dupe"
    );
    // …and the POSITIVE CONTROL: a different mode class is a legal second contact, so
    // the refusal above is about the key and not about a log that stopped accepting.
    assert!(log.log_fields_at(
        "K1ABC",
        &fields(&[("RST", "59"), ("QTH", "CT")]),
        "PH",
        "",
        0,
        130
    ));

    // THE SCORE. 3 points per QSO regardless of mode × 3 QSOs = 9. An in-state station
    // counts its `qth` universe per band: CT and DAVI on 20m = 2. (`dxcc` counts nothing
    // — this build resolves no entity, which `LoggedQso::entity` states.)
    let (qso, powered, mults, total) = score(&log);
    assert_eq!((qso, powered), (9, 9), "3 points per QSO, no power tier");
    assert_eq!(mults, Some(2), "CT and DAVI, both on 20m");
    assert_eq!(total, 18);

    // THE CABRILLO the operator submits.
    let cab = log.cabrillo(14_030).expect("one entry");
    // TNQP is on neither side of the Cabrillo token map, so the ADIF id passes through
    // unchanged — which is the other half of the mapping being exercised.
    assert!(cab.contains("CONTEST: TN-QSO-PARTY\n"), "{cab}");
    assert!(cab.contains("LOCATION: TN\n"), "{cab}");
    assert!(cab.contains("CATEGORY-OPERATOR: SINGLE-OP\n"), "{cab}");
    // The sent side is MY county on every line; the received side is theirs.
    assert!(cab.contains(" W4ABC 599 WILL K1ABC 599 CT\n"), "{cab}");
    assert!(cab.contains(" W4ABC 599 WILL W4XYZ 599 DAVI\n"), "{cab}");
    // ⭐ The phone row sends `59`, not `599` — §2.2's own definition of the field, and
    // the one part of a sent exchange that is a property of the row.
    assert!(cab.contains(" W4ABC 59 WILL K1ABC 59 CT\n"), "{cab}");

    // THE SCORE NOTE. TNQP's computed bonuses (100/K4TCG QSO, 500/county with ≥10 QSOs)
    // are deliberately not modelled, so the total shown is honestly short and says so.
    assert_eq!(
        log.ruleset().score_note_key,
        "settings.contestScore.incomplete"
    );
}

/// TNQP worked from OUTSIDE Tennessee: the other role, the other exchange, and the
/// other multiplier universe. *"Stations outside of Tennessee work only Tennessee
/// stations"*, and their multipliers are *"Tennessee counties (95 max/band)"*.
#[test]
fn tnqp_from_out_of_state_sends_its_state_and_counts_tennessee_counties() {
    let s = select("tnqp", &station("CT", ""));
    assert_eq!(s.role().id, "out_of_state");
    assert_eq!(
        composing(&s),
        vec![("RST", "599".into()), ("QTH", "CT".into())]
    );
    // `CT` is in none of the slot's ENUM arms — TNQP's second arm is free text, which
    // is the sponsor's own position (no state list is published) — so it records no
    // domain rather than being forced into the county universe.
    assert_eq!(s.my_exchange[1].domain, None);

    let mut log = FieldDayLog::new("K1ABC", s, "20m");
    assert!(log.log_fields_at(
        "W4ABC",
        &fields(&[("RST", "599"), ("QTH", "WILL")]),
        "CW",
        "",
        0,
        100
    ));
    assert!(log.log_fields_at(
        "W4XYZ",
        &fields(&[("RST", "599"), ("QTH", "DAVI")]),
        "CW",
        "",
        0,
        110
    ));
    // A second Williamson-county station is NOT a dupe (different call) and does not add
    // a second multiplier.
    assert!(log.log_fields_at(
        "W4QRP",
        &fields(&[("RST", "599"), ("QTH", "WILL")]),
        "CW",
        "",
        0,
        120
    ));

    let (qso, _, mults, total) = score(&log);
    assert_eq!(qso, 9);
    assert_eq!(
        mults,
        Some(2),
        "WILL and DAVI — a county counts once per band"
    );
    assert_eq!(total, 18);
    // The out-of-state role counts COUNTIES only: the `qth` and `dxcc` universes are
    // in-state rules, and a row worked under `out_of_state` must not reach them.
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("qth", 0), ("dxcc", 0), ("county", 2)]
    );
}

// ---------------------------------------------------------------------------
// OhQP — three roles, which is the shape the whole RoleSpec model was built for
// ---------------------------------------------------------------------------

/// ⭐ **OhQP's THREE roles, each producing its own exchange.**
///
/// ohqp.org, read 2026-09-09: *"Ohio stations send RST and their Ohio county. … Non Ohio
/// W/VE stations (including KH6/KL7) send RST and their state or province. … DX stations
/// outside of W/VE send RST and "DX"."*
///
/// ⚠️ A two-role model ships a DX entrant an exchange they have no legal value for, which
/// is why this test asserts the third arm rather than assuming it follows from the second.
#[test]
fn ohqp_sends_a_county_a_state_or_dx_by_role() {
    let oh = select("ohqp", &station("OH", "FRAN"));
    assert_eq!(oh.role().id, "in_state");
    assert_eq!(
        composing(&oh),
        vec![("RST", "599".into()), ("QTH", "FRAN".into())]
    );
    assert_eq!(oh.my_exchange[1].domain, Some("oh_counties"));

    let mi = select("ohqp", &station("MI", ""));
    assert_eq!(mi.role().id, "w_ve");
    assert_eq!(composing(&mi)[1], ("QTH", "MI".to_string()));
    assert_eq!(mi.my_exchange[1].domain, Some("oh_mults"));

    // ⭐ Alaska and Hawaii are W/VE here — the sponsor puts KH6/KL7 in that role
    // explicitly. A W/VE list built from the CONUS states would ship a KL7 entrant the
    // DX role and the wrong exchange.
    assert_eq!(select("ohqp", &station("AK", "")).role().id, "w_ve");
    assert_eq!(select("ohqp", &station("HI", "")).role().id, "w_ve");

    // The DX role is DECLARED, never inferred from a blank form.
    let dx = select(
        "ohqp",
        &StationData {
            dxcc: true,
            ..station("", "")
        },
    );
    assert_eq!(dx.role().id, "dx");
    assert_eq!(composing(&dx)[1], ("QTH", "DX".to_string()));

    // ⚠️ THE COUNTY TRAP, as a guard. Ohio and Michigan both have a Wayne county, so a
    // Michigan operator whose county box holds `WAYN` must still send `MI` — the role
    // decides the slot's value, not a probe of the county against Ohio's domain.
    let mi_wayne = select("ohqp", &station("MI", "WAYN"));
    assert_eq!(mi_wayne.role().id, "w_ve");
    assert_eq!(composing(&mi_wayne)[1], ("QTH", "MI".to_string()));
}

/// OhQP scoring, from Ohio: *"MULTIPLIERS – Multipliers are counted once per mode, i.e.
/// working the same multiplier on both CW and SSB counts as two multipliers."* Points are
/// the sponsor's own CW 2 / phone 1.
#[test]
fn ohqp_counts_multipliers_once_per_mode() {
    let mut log = FieldDayLog::new("W8ABC", select("ohqp", &station("OH", "FRAN")), "40m");
    assert!(log.log_fields_at(
        "W8XYZ",
        &fields(&[("RST", "599"), ("QTH", "CUYA")]),
        "CW",
        "",
        0,
        100
    ));
    assert!(log.log_fields_at(
        "K8DEF",
        &fields(&[("RST", "59"), ("QTH", "CUYA")]),
        "PH",
        "",
        0,
        110
    ));
    assert!(log.log_fields_at(
        "K1ABC",
        &fields(&[("RST", "599"), ("QTH", "CT")]),
        "CW",
        "",
        0,
        120
    ));

    // Points: CW 2 + PH 1 + CW 2 = 5.
    let (qso, _, mults, total) = score(&log);
    assert_eq!(qso, 5, "the sponsor's CW 2 / phone 1");
    // `county` counts CUYA on CW and CUYA on PH — two, per the quoted sentence — and the
    // in-state-only `mult` universe counts CT once.
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("county", 2), ("mult", 1)]
    );
    assert_eq!(mults, Some(3));
    assert_eq!(total, 15);
    // OhQP's score is COMPLETE — no note, unlike TNQP and TXQP.
    assert_eq!(log.ruleset().score_note_key, "");

    let cab = log.cabrillo(7_030).expect("one entry");
    // ⚠️ The Cabrillo TOKEN, not the ADIF id: OhQP is `OH-QSO-PARTY` to ADIF and
    // `MRRC-OHQP` on the Cabrillo master list. The two namespaces meet once, in the
    // header writer, and the session's own id is what reaches it — reading `FdEvent`
    // there would hand every party to the mapper as Field Day and get `ARRL-FD` back.
    assert!(cab.contains("CONTEST: MRRC-OHQP\n"), "{cab}");
    // …and the CONTROL that the two really are different namespaces: the ADIF export
    // keeps the ADIF id on the same log.
    assert!(
        log.adif().contains("OH-QSO-PARTY"),
        "ADIF keeps the ADIF id"
    );
    assert!(cab.contains(" W8ABC 599 FRAN W8XYZ 599 CUYA\n"), "{cab}");
}

// ---------------------------------------------------------------------------
// CQP — the serial party, and the dupe rule read from the sponsor
// ---------------------------------------------------------------------------

/// ⭐ **CQP end to end: a SERIAL exchange, and the sponsor's own dupe sentence.**
///
/// cqp.org/Rules.html (*"Last Update: 19-July-2026"*, read 2026-09-09):
/// * *"California stations send QSO number and 4-letter county abbreviation. Stations
///   outside of California send QSO number and 2-letter State, Canadian
///   province/territory, or "DX"."* — **no RST**;
/// * *"Stations may be worked once on CW and once on Phone on each of the 6 bands"*;
/// * *"Each complete non-duplicate Phone contact is worth 3 points. NEW IN 2026"*;
/// * *"The final score is the total number of QSO Points multiplied by the total number
///   of scored multipliers"* — per LOG, not per band.
#[test]
fn cqp_issues_serials_dupes_once_per_mode_per_band_and_scores_per_log() {
    let s = select("cqp", &station("CA", "ALAM"));
    assert_eq!(s.role().id, "in_state");
    // NO RST — the strip is serial + QTH, which is what the sponsor's exchange says.
    assert_eq!(entry_boxes(&s), vec![("NR", true), ("QTH", true)]);
    assert_eq!(
        composing(&s),
        vec![("NR", "0".into()), ("QTH", "ALAM".into())]
    );

    let mut log = FieldDayLog::new("W6ABC", s, "20m");
    // ⭐ THE SERIAL IS ISSUED AT COMPOSE, onto `in_flight`, and the row copies it from
    // there — batch 3's mechanism, used rather than duplicated.
    log.session.compose_for("K1ABC", 100);
    assert!(log.log_fields_at(
        "K1ABC",
        &fields(&[("NR", "17"), ("QTH", "CT")]),
        "CW",
        "",
        0,
        100
    ));
    log.session.compose_for("W7XYZ", 110);
    assert!(log.log_fields_at(
        "W7XYZ",
        &fields(&[("NR", "4"), ("QTH", "AZ")]),
        "CW",
        "",
        0,
        110
    ));
    assert_eq!(
        log.qsos()
            .iter()
            .map(|q| q.sent("NR").to_string())
            .collect::<Vec<_>>(),
        vec!["1", "2"],
        "one number per contact, in order, from the issue"
    );

    // THE DUPE VERDICT, from THIS ruleset's own rule: once per mode per band.
    assert!(
        !log.log_fields_at(
            "K1ABC",
            &fields(&[("NR", "18"), ("QTH", "CT")]),
            "CW",
            "",
            0,
            120
        ),
        "CW on 20m again is a dupe"
    );
    assert!(
        log.log_fields_at(
            "K1ABC",
            &fields(&[("NR", "19"), ("QTH", "CT")]),
            "PH",
            "",
            0,
            130
        ),
        "phone on the same band is a second legal contact"
    );
    log.band = "40m".to_string();
    assert!(
        log.log_fields_at(
            "K1ABC",
            &fields(&[("NR", "20"), ("QTH", "CT")]),
            "CW",
            "",
            0,
            140
        ),
        "CW on another band is a third"
    );

    // THE SCORE: 3 points every mode in 2026 × 4 QSOs = 12, multiplied by the mults
    // counted ONCE FOR THE LOG — CT and AZ, however many bands and modes they appear on.
    let (qso, _, mults, total) = score(&log);
    assert_eq!(qso, 12, "phone is 3 points too, NEW IN 2026");
    assert_eq!(mults, Some(2), "per LOG: CT and AZ, not once per band");
    assert_eq!(total, 24);

    let cab = log.cabrillo(14_030).expect("one entry");
    assert!(cab.contains("CONTEST: CA-QSO-PARTY\n"), "{cab}");
    assert!(cab.contains(" W6ABC 1 ALAM K1ABC 17 CT\n"), "{cab}");
}

// ---------------------------------------------------------------------------
// TXQP — the fourth party, and the score-note it must show
// ---------------------------------------------------------------------------

/// ⭐ **TXQP end to end**, plus the two facts batch 8 recorded and this batch must surface.
///
/// txqp.net (`?page_id=23` / `?page_id=90`, read 2026-09-09):
/// * *"Texas stations use RS/T and Texas County using the TQP standard abbreviations"*;
/// * *"Count two (2) points per phone QSO … three (3) points per CW and other digital"*;
/// * *"Multipliers are counted once for the QSO Party"* — per log;
/// * the county list is **254 entries, 252 four-letter plus `BEE` and `LEE`** — a domain
///   written to the sponsor's *prose* (`len == 4`) rejects both.
#[test]
fn txqp_from_texas_scores_per_log_and_shows_its_score_note() {
    let s = select("txqp", &station("TX", "BEXA"));
    assert_eq!(s.role().id, "in_state");
    assert_eq!(
        composing(&s),
        vec![("RST", "599".into()), ("QTH", "BEXA".into())]
    );

    let mut log = FieldDayLog::new("W5ABC", s, "20m");
    assert!(log.log_fields_at(
        "K1ABC",
        &fields(&[("RST", "599"), ("QTH", "CT")]),
        "CW",
        "",
        0,
        100
    ));
    assert!(log.log_fields_at(
        "K2DEF",
        &fields(&[("RST", "59"), ("QTH", "NY")]),
        "PH",
        "",
        0,
        110
    ));
    // ⭐ `BEE` — one of the two three-letter codes the sponsor's own list carries and
    // the sponsor's own prose would reject. A Texas mobile in Bee county is a legal
    // contact, and it is worked here so the domain is proved to hold it.
    assert!(log.log_fields_at(
        "W5MOB",
        &fields(&[("RST", "599"), ("QTH", "BEE")]),
        "CW",
        "",
        0,
        120
    ));

    let (qso, _, mults, total) = score(&log);
    assert_eq!(qso, 3 + 2 + 3, "CW/digital 3, phone 2");
    assert_eq!(
        mults,
        Some(3),
        "CT, NY and BEE — counted once for the party"
    );
    assert_eq!(total, 24);

    // ⭐ THE SCORE NOTE. TXQP's mobile bonuses are computed from the log and are
    // deliberately not modelled (the sponsor's own paragraph is garbled and was not
    // guessed at), so the number shown is short of the sponsor's and says so.
    assert_eq!(
        log.ruleset().score_note_key,
        "settings.contestScore.incomplete"
    );

    let cab = log.cabrillo(14_030).expect("one entry");
    // The Cabrillo token again — `TX-QSO-PARTY` to ADIF, `TXQP` to Cabrillo.
    assert!(cab.contains("CONTEST: TXQP\n"), "{cab}");
    assert!(
        log.adif().contains("TX-QSO-PARTY"),
        "ADIF keeps the ADIF id"
    );
    assert!(cab.contains(" W5ABC 599 BEXA W5MOB 599 BEE\n"), "{cab}");
}

// ---------------------------------------------------------------------------
// The refusals — how a wrong exchange is PREVENTED rather than detected later
// ---------------------------------------------------------------------------

/// A session is refused, by name, rather than transmitting a blank or an out-of-domain
/// value for the whole contest. Each arm has a positive control on the same ruleset.
#[test]
fn a_session_is_refused_when_the_exchange_it_would_send_is_not_legal() {
    let tnqp = ruleset_by_id("tnqp", CURRENT_RULES_YEAR).expect("shipped");
    let ohqp = ruleset_by_id("ohqp", CURRENT_RULES_YEAR).expect("shipped");

    // (a) An in-state operator who has not said which county they are in. The role is
    //     Tennessee's, the exchange is a Tennessee county, and there is none.
    let e = ContestSession::for_ruleset(tnqp, &station("TN", "")).unwrap_err();
    assert!(e.contains("QTH"), "names the slot: {e}");
    // POSITIVE CONTROL — the same ruleset and the same state, with the county filled in.
    assert!(ContestSession::for_ruleset(tnqp, &station("TN", "WILL")).is_ok());

    // (b) A county that is not one of THIS party's counties. `CUYA` is Ohio's Cuyahoga
    //     and Tennessee has no such code, so it is refused before it goes on the air
    //     rather than after it is in a submitted log.
    //
    //     ⚠️ TNQP's `QTH` slot would accept `CUYA`: its second arm is free text, which
    //     is the sponsor's own position for the out-of-state role's state/province/DXCC
    //     value. The IN-STATE role's value is a county and the sponsor publishes the
    //     list, so the catch-all arm is not available to it.
    let e = ContestSession::for_ruleset(tnqp, &station("TN", "CUYA")).unwrap_err();
    assert!(e.contains("CUYA"), "names the value: {e}");
    // …and the mirror image on the other party, which is the control that proves the
    // check is per-ruleset and not a spelling rule.
    assert!(ContestSession::for_ruleset(ohqp, &station("OH", "CUYA")).is_ok());
    let e = ContestSession::for_ruleset(ohqp, &station("OH", "SEQU")).unwrap_err();
    assert!(e.contains("SEQU"), "names the value: {e}");

    // (c) An operator who has filled in nothing at all. OhQP's `w_ve` role does not
    //     match an empty state, so they fall to `dx` — and `dx` sends the literal `DX`,
    //     which is a legal value. That is the one case a blank form does NOT refuse, so
    //     it is asserted rather than left to be discovered: `dxcc` is a DECLARATION, and
    //     an operator who has declared nothing gets the catch-all role by design
    //     (§3's "the LAST role is the catch-all").
    let blank = ContestSession::for_ruleset(ohqp, &station("", "")).expect("dx is reachable");
    assert_eq!(blank.role().id, "dx");
    assert_eq!(blank.field("QTH"), "DX");
}

/// ⭐ **Field Day is not moved by any of this.** `for_ruleset` and `field_day` are two
/// entry points to one contest object, so this pins them together: the session an
/// operator gets by picking ARRL Field Day out of the new picker is the session Field
/// Day has always built.
#[test]
fn for_ruleset_and_field_day_build_the_same_session() {
    let rs = ruleset_by_id("arrlfd", CURRENT_RULES_YEAR).expect("shipped");
    let built = ContestSession::for_ruleset(
        rs,
        &StationData {
            fd_class: "3A".into(),
            fd_section: "WI".into(),
            ..Default::default()
        },
    )
    .expect("Field Day is a shipped ruleset like any other");
    let shipped = ContestSession::field_day(tempo_core::fieldday::FdEvent::ArrlFd, "3A", "WI");

    assert_eq!(built.event_id, shipped.event_id);
    assert_eq!(built.contest_id, shipped.contest_id);
    assert_eq!(built.rules_year, shipped.rules_year);
    assert_eq!(built.role().id, shipped.role().id);
    assert_eq!(built.my_exchange, shipped.my_exchange);
    assert_eq!(built.my_location.state, shipped.my_location.state);
    assert_eq!(built.entry_category, shipped.entry_category);
    assert_eq!(built.upload, shipped.upload);
}
