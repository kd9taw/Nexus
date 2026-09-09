//! ⭐ **CQ WW DX and CQ WPX, end to end** — the proof batch 10 exists for.
//!
//! These are the first two contests whose multipliers and QSO points are functions of the
//! CALLSIGN rather than of the exchange, so they exercise machinery nothing before them
//! reached: [`MultSource::DxccEntity`], [`MultSource::Prefix`], [`PointsRule::ByRelation`]
//! with CQ WPX's band groups, two independent per-band multiplier sets, and the trailing
//! Cabrillo transmitter column.
//!
//! Every expected number comes from the sponsor's own current page, read 2026-09-09:
//! <https://www.cqww.com/rules/>, <https://cqwpx.com/rules/>, <https://cqwpx.com/rules_faq.htm>,
//! <https://cqww.com/cabrillo.htm> and <https://cqwpx.com/cabrillo.htm>. The sentence each
//! assertion rests on is quoted at it, so a rules change is a red test with the sponsor's
//! own words attached rather than a silent rescore.
//!
//! ⚠️ **The country file here is a STUB, and that is deliberate.** The real resolver is
//! AD1C's `cty.dat`, vendored in `crates/propagation` — a crate that depends on
//! `tempo-core`, so this crate can never call it. The composition root installs the real
//! one; `place` below installs a hand-written table for the two dozen callsigns these
//! tests work, which is what lets the relation arms be driven deterministically instead of
//! depending on a country file that is refreshed weekly. The binding to the REAL resolver
//! is asserted where both crates are in scope, in `src-tauri`.
use tempo_core::contest::{
    install_call_resolver, wpx_prefix, CallLocation, ContestSession, StationData,
};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

/// The stub country file: prefix → (entity, continent), longest prefix first.
///
/// The entity spellings are `cty.dat`'s own, because
/// [`MultiplierRule::excluding`](tempo_core::contest::MultiplierRule::excluding) compares
/// entity names by string and a stub that spelt them differently would prove nothing about
/// the shipped one. ⚠️ **Hawaii is `OC`, not `NA`** — that is the country file's answer and
/// CQ WW's *"continental boundaries"*, and it is what makes a US-to-Hawaii contact a
/// three-point contact.
const TABLE: &[(&str, &str, &str)] = &[
    ("KL7", "Alaska", "NA"),
    ("KH6", "Hawaii", "OC"),
    ("VE", "Canada", "NA"),
    ("XE", "Mexico", "NA"),
    ("HC8", "Galapagos Is.", "SA"),
    ("PY", "Brazil", "SA"),
    ("DL", "Fed. Rep. of Germany", "EU"),
    ("OE", "Austria", "EU"),
    ("PA", "Netherlands", "EU"),
    ("S5", "Slovenia", "EU"),
    ("G", "England", "EU"),
    ("4U1", "ITU HQ", "EU"),
    ("JA", "Japan", "AS"),
    ("ZS", "South Africa", "AF"),
    ("VK", "Australia", "OC"),
    ("W", "United States", "NA"),
    ("K", "United States", "NA"),
    ("N", "United States", "NA"),
    ("AA", "United States", "NA"),
];

fn place(call: &str) -> Option<CallLocation> {
    // The base call, portable designators resolved the way the country file does: the
    // designator names where the station IS.
    let up = call.trim().to_ascii_uppercase();
    let parts: Vec<&str> = up
        .split('/')
        .filter(|p| !p.is_empty() && !matches!(*p, "MM" | "AM" | "M" | "P" | "A" | "E" | "J"))
        .collect();
    let base = match parts.as_slice() {
        [] => return None,
        [one] => *one,
        [a, b, ..] => {
            // A bare-digit designator does not move the station out of its country.
            if b.chars().all(|c| c.is_ascii_digit()) {
                *a
            } else if b.len() < a.len() {
                *b
            } else {
                *a
            }
        }
    };
    let mut best: Option<(usize, &str, &str)> = None;
    for (p, entity, cont) in TABLE {
        if base.starts_with(p) && best.is_none_or(|(n, _, _)| p.len() > n) {
            best = Some((p.len(), entity, cont));
        }
    }
    best.map(|(_, entity, continent)| CallLocation { entity, continent })
}

/// Install the stub once per test binary. Integration tests share one process, so this is
/// `Once`-guarded and the FIRST install is asserted — an install that silently lost a race
/// would leave the tests scoring against whatever won.
fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(place).expect("the first install in this test binary");
    });
}

fn select(event_id: &str, station: &StationData) -> ContestSession {
    resolver();
    let rs = ruleset_by_id(event_id, CURRENT_RULES_YEAR)
        .unwrap_or_else(|| panic!("{event_id} is a shipped ruleset"));
    ContestSession::for_ruleset(rs, station)
        .unwrap_or_else(|e| panic!("{event_id} session refused: {e}"))
}

/// A station: my callsign and my CQ zone, which is the whole of CQ WW's station data.
fn station(mycall: &str, zone: &str) -> StationData {
    StationData {
        mycall: mycall.to_string(),
        contest_cq_zone: zone.to_string(),
        ..Default::default()
    }
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn entry_boxes(s: &ContestSession) -> Vec<(&'static str, bool)> {
    s.role()
        .receives
        .iter()
        .filter_map(|k| s.exchange.field(k))
        .map(|f| (f.key, f.required))
        .collect()
}

fn composing(s: &ContestSession) -> Vec<(&'static str, String)> {
    s.my_exchange
        .iter()
        .map(|v| (v.key, v.raw.clone()))
        .collect()
}

fn score(log: &FieldDayLog) -> (u32, u32, Option<u32>, u32) {
    log.ruleset().scoring.score(log.score_rows(), 1)
}

/// Log one contact on a named band — CQ WW and CQ WPX are both six-band contests, and the
/// per-band multiplier and the WPX band groups are the whole point.
///
/// It composes for the peer first, which is what the engine does when a QSO starts: CQ
/// WPX's exchange is a SERIAL, and a serial is issued onto the in-flight exchange rather
/// than read off the session's template. Skipping that step would put the template's `0`
/// on every QSO line of the submitted log.
fn work(log: &mut FieldDayLog, band: &str, call: &str, ex: &[(&str, &str)], at: u64) -> bool {
    log.band = band.to_string();
    log.session.compose_for(call, at);
    log.log_fields_at(call, &fields(ex), "CW", "", 0, at)
}

// ---------------------------------------------------------------------------
// CQ WW DX
// ---------------------------------------------------------------------------

/// ⭐ **CQ WW CW, worked from the United States, end to end.**
///
/// cqww.com/rules read 2026-09-09:
/// * exchange — *"CW: RST report plus CQ Zone of the station transmitter location
///   (e.g., 599 05)"*;
/// * dupe — *"Stations may be contacted once on each band."*;
/// * points — *"Contacts between stations on different continents count three (3) points.
///   Contacts between stations on the same continent but in different countries count one
///   (1) point. Exception: Contacts between stations in different countries within the
///   North American boundaries count two (2) points. Contacts between stations in the same
///   country have zero (0) QSO point value, but count for zone and country multiplier
///   credit."*;
/// * multipliers — *"Zone: A multiplier of one (1) for each different CQ Zone contacted on
///   each band. … Country: A multiplier of one (1) for each different country contacted on
///   each band."*
#[test]
fn cq_ww_cw_from_the_united_states_end_to_end() {
    let s = select("cqww_cw", &station("W9XYZ", "4"));

    assert_eq!(s.contest_id, "CQ-WW-CW");
    assert_eq!(
        s.role().id,
        "",
        "CQ WW is symmetric — one role, no location test"
    );
    // THE ENTRY STRIP: a signal report and a zone. Not a section, not a county, not a
    // serial — CQ WW is the one contest in the shipped set whose exchange is a NUMBER.
    assert_eq!(entry_boxes(&s), vec![("RST", true), ("ZN", true)]);
    // THE SENT EXCHANGE: the 3-digit CW report the ruleset declares, and MY zone, read
    // from the station data rather than typed per contact.
    assert_eq!(
        composing(&s),
        vec![("RST", "599".into()), ("ZN", "4".into())]
    );
    // ⭐ MY OWN LOCATION, resolved once from my callsign — the `me` half of every
    // relation this log prices a contact by.
    assert_eq!(
        s.my_call_location.map(|l| (l.entity, l.continent)),
        Some(("United States", "NA"))
    );

    let mut log = FieldDayLog::new("W9XYZ", s, "20m");
    // ⭐ ONE CONTACT PER RELATION ARM, all on 20 m.
    assert!(work(
        &mut log,
        "20m",
        "K2DEF",
        &[("RST", "599"), ("ZN", "5")],
        100
    ));
    assert!(work(
        &mut log,
        "20m",
        "DL1ABC",
        &[("RST", "599"), ("ZN", "14")],
        110
    ));
    assert!(work(
        &mut log,
        "20m",
        "VE3XYZ",
        &[("RST", "599"), ("ZN", "5")],
        120
    ));
    // …the same German station again on 40 m: a new band, so a new zone bucket and a new
    // country bucket, and three more points.
    assert!(work(
        &mut log,
        "40m",
        "DL1ABC",
        &[("RST", "599"), ("ZN", "14")],
        130
    ));

    // THE DUPE VERDICT — *"Stations may be contacted once on each band."* The same
    // station on the same band is refused…
    assert!(
        !work(
            &mut log,
            "20m",
            "DL1ABC",
            &[("RST", "599"), ("ZN", "14")],
            140
        ),
        "same call, same band is a dupe"
    );
    // …and the POSITIVE CONTROL: a DIFFERENT station on that same 20 m is accepted, so
    // the refusal is the dupe key and not a log that stopped taking contacts.
    assert!(work(
        &mut log,
        "20m",
        "G3ABC",
        &[("RST", "599"), ("ZN", "14")],
        150
    ));

    // ⭐ THE SCORE, arm by arm.
    // K2DEF   United States → SAME COUNTRY        → 0
    // DL1ABC  Germany, EU   → DIFFERENT CONTINENT → 3   (20 m)
    // VE3XYZ  Canada, NA    → WITHIN NORTH AMERICA→ 2
    // DL1ABC  Germany, EU   → DIFFERENT CONTINENT → 3   (40 m)
    // G3ABC   England, EU   → DIFFERENT CONTINENT → 3
    let (qso, powered, mults, total) = score(&log);
    assert_eq!(qso, 11, "0 + 3 + 2 + 3 + 3");
    assert_eq!(powered, 11, "CQ WW applies no power multiplier");

    // ⭐ TWO INDEPENDENT PER-BAND MULTIPLIER SETS, counted separately and SUMMED —
    // *"the sum of zone and country multipliers"*.
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("zone", 3), ("country", 5)],
        "zones {{5,14}} on 20 m + {{14}} on 40 m; countries {{US,DE,CA,GB}} on 20 m + {{DE}} on 40 m"
    );
    assert_eq!(mults, Some(8));
    assert_eq!(total, 88, "11 QSO points × (3 zones + 5 countries)");

    // ⭐ THE CABRILLO the operator submits — the sponsor's own template
    // `QSO: freq mo date time call rst exch call rst exch t`, sample
    // `QSO: 3799 PH 2000-11-26 0711 N6TW 59 03 JT1Z 59 23 0`.
    let cab = log.cabrillo(14_030).expect("one entry");
    assert!(cab.contains("CONTEST: CQ-WW-CW\n"), "{cab}");
    assert!(cab.contains("CALLSIGN: W9XYZ\n"), "{cab}");
    assert!(
        cab.contains("QSO: 14000 CW 1970-01-01 0001 W9XYZ 599 4 K2DEF 599 5 0\n"),
        "{cab}"
    );
    assert!(
        cab.contains("QSO: 7000 CW 1970-01-01 0002 W9XYZ 599 4 DL1ABC 599 14 0\n"),
        "{cab}"
    );
    // ⭐ THE TRAILING TRANSMITTER COLUMN — *"Note for Column 81 (transmitter number) …
    // It must be a 0 or a 1"* — on EVERY QSO line, and it is the last thing on each.
    let qso_lines: Vec<&str> = cab.lines().filter(|l| l.starts_with("QSO:")).collect();
    assert_eq!(qso_lines.len(), 5);
    assert!(
        qso_lines.iter().all(|l| l.ends_with(" 0")),
        "every CQ WW QSO line ends with the transmitter column: {qso_lines:?}"
    );
}

/// ⭐ **The two multiplier universes are INDEPENDENT**, which is the shape a single
/// combined count cannot express: four stations, four countries, but only two zones.
///
/// It also pins the zone bucket's normalisation. *"599 05"* is the sponsor's own worked
/// exchange, so an operator who copies `05` and an operator who copies `5` have copied the
/// same zone — and two buckets there is a multiplier the log did not work.
#[test]
fn zone_and_country_are_counted_separately_and_a_padded_zone_is_one_zone() {
    let mut log = FieldDayLog::new("W9XYZ", select("cqww_cw", &station("W9XYZ", "4")), "20m");
    // Four different countries, two of which share CQ zone 14.
    assert!(work(
        &mut log,
        "20m",
        "DL1ABC",
        &[("RST", "599"), ("ZN", "14")],
        100
    ));
    assert!(work(
        &mut log,
        "20m",
        "G3ABC",
        &[("RST", "599"), ("ZN", "14")],
        110
    ));
    assert!(work(
        &mut log,
        "20m",
        "PA0XYZ",
        &[("RST", "599"), ("ZN", "14")],
        120
    ));
    assert!(work(
        &mut log,
        "20m",
        "JA1ABC",
        &[("RST", "599"), ("ZN", "25")],
        130
    ));
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("zone", 2), ("country", 4)],
        "three EU countries share zone 14; the country universe still counts three of them"
    );

    // ⭐ The padded form the sponsor's own example uses. A fifth station in zone 5, copied
    // as `05`, and a sixth copied as `5`: ONE new zone, two new countries.
    assert!(work(
        &mut log,
        "20m",
        "K2DEF",
        &[("RST", "599"), ("ZN", "05")],
        140
    ));
    assert!(work(
        &mut log,
        "20m",
        "VE3XYZ",
        &[("RST", "599"), ("ZN", "5")],
        150
    ));
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("zone", 3), ("country", 6)],
        "`05` and `5` are the same CQ zone"
    );
    // …and the Cabrillo still carries what each operator ACTUALLY sent. The bucket key is
    // normalised; the log is not.
    let cab = log.cabrillo(14_030).expect("one entry");
    assert!(cab.contains(" K2DEF 599 05 0\n"), "{cab}");
    assert!(cab.contains(" VE3XYZ 599 5 0\n"), "{cab}");
}

/// Hawaii is Oceania in the country file, so a US station works KH6 for THREE points, not
/// two — the continental-boundary fact CQ WW names as a standard of its own. Alaska is
/// North America and is worth two.
#[test]
fn hawaii_is_a_different_continent_and_alaska_is_not() {
    let mut log = FieldDayLog::new("W9XYZ", select("cqww_cw", &station("W9XYZ", "4")), "20m");
    assert!(work(
        &mut log,
        "20m",
        "KH6ABC",
        &[("RST", "599"), ("ZN", "31")],
        100
    ));
    assert_eq!(score(&log).0, 3, "KH6 is OC — a different continent");
    assert!(work(
        &mut log,
        "20m",
        "KL7ABC",
        &[("RST", "599"), ("ZN", "1")],
        110
    ));
    assert_eq!(score(&log).0, 5, "KL7 is NA — the two-point exception");
}

// ---------------------------------------------------------------------------
// CQ WPX
// ---------------------------------------------------------------------------

/// ⭐ **CQ WPX CW, worked from the United States, end to end** — the band groups and the
/// prefix multiplier.
///
/// cqwpx.com/rules §V read 2026-09-09:
/// * *"A station may be worked once on each band for QSO point credit"*;
/// * *"Contacts between stations on different continents are worth three (3) points on 28,
///   21, and 14 MHz and six (6) points on 7, 3.5, and 1.8 MHz."*;
/// * *"Contacts between stations on the same continent, but different countries, are worth
///   one (1) point on 28, 21, and 14 MHz and two (2) points on 7, 3.5, and 1.8 MHz.
///   Exception: For North American stations only—contacts between stations within the
///   North American boundaries … are worth two (2) points on 28, 21, and 14 MHz and four
///   (4) points on 7, 3.5, and 1.8 MHz."*;
/// * ⭐ *"Contacts between stations in the same country are worth 1 point regardless of
///   band."* — the arm the source verification found missing from the design spec, and the
///   one place CQ WPX and CQ WW price the same relation differently;
/// * *"Each PREFIX is counted only once regardless of the band or number of times the same
///   prefix is worked."*
#[test]
fn cq_wpx_cw_from_the_united_states_end_to_end() {
    let s = select("cqwpx_cw", &station("W9XYZ", ""));

    assert_eq!(s.contest_id, "CQ-WPX-CW");
    // *"RS(T) report plus a progressive contact serial number starting with 001"* — and
    // NO zone: WPX needs no station data beyond the callsign, which is why the session
    // above is built with an empty one.
    assert_eq!(entry_boxes(&s), vec![("RST", true), ("NR", true)]);
    assert_eq!(
        composing(&s),
        vec![("RST", "599".into()), ("NR", "0".into())],
        "the serial is a template placeholder until one is issued to a peer"
    );

    let mut log = FieldDayLog::new("W9XYZ", s, "20m");
    // DIFFERENT CONTINENT, both band groups.
    assert!(work(
        &mut log,
        "20m",
        "DL1ABC",
        &[("RST", "599"), ("NR", "1")],
        100
    ));
    assert!(work(
        &mut log,
        "40m",
        "DL1ABC",
        &[("RST", "599"), ("NR", "2")],
        110
    ));
    // WITHIN NORTH AMERICA, both band groups.
    assert!(work(
        &mut log,
        "20m",
        "VE3XYZ",
        &[("RST", "599"), ("NR", "3")],
        120
    ));
    assert!(work(
        &mut log,
        "80m",
        "VE3XYZ",
        &[("RST", "599"), ("NR", "4")],
        130
    ));
    // SAME COUNTRY, one on each band group — 1 point on both.
    assert!(work(
        &mut log,
        "20m",
        "K2DEF",
        &[("RST", "599"), ("NR", "5")],
        140
    ));
    assert!(work(
        &mut log,
        "160m",
        "K2DEF",
        &[("RST", "599"), ("NR", "6")],
        150
    ));

    let (qso, _, mults, total) = score(&log);
    assert_eq!(
        qso,
        3 + 6 + 2 + 4 + 1 + 1,
        "3/6 different continent, 2/4 within NA, 1/1 same country"
    );
    // ⭐ THE PREFIX MULTIPLIER, once per log: DL1, VE3, K2 — *"regardless of the band or
    // number of times the same prefix is worked"*, so the six contacts are three
    // multipliers.
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("prefix", 3)]
    );
    assert_eq!(mults, Some(3));
    assert_eq!(total, 17 * 3);

    let cab = log.cabrillo(14_030).expect("one entry");
    assert!(cab.contains("CONTEST: CQ-WPX-CW\n"), "{cab}");
    // The sponsor's own sample is `QSO: 7005 CW 2009-05-30 0002 AA1ZZZ 599 1 S50A 599 4`;
    // the trailing transmitter column is column 81 of the same template.
    assert!(
        cab.contains("QSO: 14000 CW 1970-01-01 0001 W9XYZ 599 1 DL1ABC 599 1 0\n"),
        "{cab}"
    );
    assert!(
        cab.lines()
            .filter(|l| l.starts_with("QSO:"))
            .all(|l| l.ends_with(" 0")),
        "{cab}"
    );
}

/// The plain SAME-CONTINENT arm, which a US station can never reach: from Germany, another
/// European country is one point high and two points low, and the North American exception
/// does not apply.
#[test]
fn cq_wpx_from_europe_reaches_the_plain_same_continent_arm() {
    let mut log = FieldDayLog::new("DL9XYZ", select("cqwpx_cw", &station("DL9XYZ", "")), "20m");
    assert!(work(
        &mut log,
        "20m",
        "G3ABC",
        &[("RST", "599"), ("NR", "1")],
        100
    ));
    assert_eq!(
        score(&log).0,
        1,
        "same continent, different country, high band"
    );
    assert!(work(
        &mut log,
        "80m",
        "G3ABC",
        &[("RST", "599"), ("NR", "2")],
        110
    ));
    assert_eq!(score(&log).0, 3, "…and two points on the low bands");
    // POSITIVE CONTROL that this really is the same-continent arm and not the NA one: a
    // Canadian worked from Germany is a DIFFERENT-continent contact, three points.
    assert!(work(
        &mut log,
        "20m",
        "VE3XYZ",
        &[("RST", "599"), ("NR", "3")],
        120
    ));
    assert_eq!(score(&log).0, 6);
}

/// ⭐ **The prefix rule's edge cases, counted as MULTIPLIERS in a real log** — the unit
/// tests in `contest::callsign` pin the strings; this pins that the strings are what the
/// score is built on.
///
/// The six calls carry five distinct prefixes: `4U1ITU` → `4U1`, `PA/N8BJQ` → `PA0`,
/// `W1AW/4` → `W4`, `OE/K5ZD` → `OE0`, `KL7RA/WK9` → `WK9`, and `N8BJQ/P` → `N8` —
/// with `4U1UN` sharing `4U1` with `4U1ITU`.
#[test]
fn the_wpx_prefix_edge_cases_are_the_multipliers_the_log_counts() {
    let calls = [
        ("4U1ITU", "4U1"),
        ("4U1UN", "4U1"),
        ("PA/N8BJQ", "PA0"),
        ("W1AW/4", "W4"),
        ("OE/K5ZD", "OE0"),
        ("KL7RA/WK9", "WK9"),
        ("N8BJQ/P", "N8"),
    ];
    // The strings first, so a failure below is a scoring failure and not a parsing one.
    for (call, want) in calls {
        assert_eq!(wpx_prefix(call).as_deref(), Some(want), "{call}");
    }

    let mut log = FieldDayLog::new("W9XYZ", select("cqwpx_cw", &station("W9XYZ", "")), "20m");
    for (i, (call, _)) in calls.iter().enumerate() {
        assert!(
            work(
                &mut log,
                "20m",
                call,
                &[("RST", "599"), ("NR", &(i + 1).to_string())],
                100 + i as u64
            ),
            "{call} logs"
        );
    }
    // Seven contacts, SIX distinct prefixes — `4U1ITU` and `4U1UN` are one.
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("prefix", 6)]
    );
    // ⚠️ NEGATIVE CONTROL: seven would mean the two `4U1` calls counted separately, and
    // five would mean a portable designator was dropped.
    assert_ne!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("prefix", 7)]
    );
}

// ---------------------------------------------------------------------------
// The two contests side by side, and what they do NOT change
// ---------------------------------------------------------------------------

/// ⭐ **The same relation, priced differently by two contests** — the whole reason
/// [`PointsRule::ByRelation`] is DATA and not a function.
///
/// CQ WW: *"Contacts between stations in the same country have zero (0) QSO point value"*.
/// CQ WPX: *"Contacts between stations in the same country are worth 1 point regardless of
/// band."*
#[test]
fn the_same_country_arm_is_zero_in_cq_ww_and_one_in_cq_wpx() {
    let mut ww = FieldDayLog::new("W9XYZ", select("cqww_cw", &station("W9XYZ", "4")), "20m");
    assert!(work(
        &mut ww,
        "20m",
        "K2DEF",
        &[("RST", "599"), ("ZN", "5")],
        100
    ));
    assert!(work(
        &mut ww,
        "160m",
        "K2DEF",
        &[("RST", "599"), ("ZN", "5")],
        110
    ));
    assert_eq!(ww.ruleset().scoring.score(ww.score_rows(), 1).0, 0);

    let mut wpx = FieldDayLog::new("W9XYZ", select("cqwpx_cw", &station("W9XYZ", "")), "20m");
    assert!(work(
        &mut wpx,
        "20m",
        "K2DEF",
        &[("RST", "599"), ("NR", "1")],
        100
    ));
    assert!(work(
        &mut wpx,
        "160m",
        "K2DEF",
        &[("RST", "599"), ("NR", "2")],
        110
    ));
    assert_eq!(
        wpx.ruleset().scoring.score(wpx.score_rows(), 1).0,
        2,
        "1 point regardless of band, on both"
    );
    // …and CQ WW still counts the same-country contact for MULTIPLIER credit —
    // *"but count for zone and country multiplier credit"*.
    assert_eq!(
        ww.ruleset().scoring.mult_counts(ww.score_rows()),
        vec![("zone", 2), ("country", 2)],
        "zone 5 and the United States, on each of two bands"
    );
}

/// ⭐ **The transmitter column is a per-CONTEST declaration**, read off the sponsor's own
/// template — so it must be absent from every contest whose sponsor does not publish one.
///
/// OhQP is the strongest case: *"OhQP does not have a multi category that requires knowing
/// transmitter number. Therefore, this field is not permitted in OhQP entries."*
#[test]
fn only_the_two_cq_contests_declare_the_transmitter_column() {
    for (event, want) in [
        ("cqww_cw", true),
        ("cqww_ssb", true),
        ("cqwpx_cw", true),
        ("cqwpx_ssb", true),
        ("arrlfd", false),
        ("wfd", false),
        ("tnqp", false),
        ("ohqp", false),
        ("cqp", false),
        ("txqp", false),
        ("arrlss_cw", false),
        ("arrlss_ssb", false),
    ] {
        let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("shipped");
        assert_eq!(rs.transmitter_column, want, "{event}");
    }

    // And the writer honours it BOTH WAYS, in one breath: a Sweepstakes line carries no
    // trailing column while a CQ WPX line does.
    let mut ss = FieldDayLog::new(
        "W9XYZ",
        select(
            "arrlss_cw",
            &StationData {
                mycall: "W9XYZ".into(),
                fd_section: "WI".into(),
                contest_check: "74".into(),
                contest_category_operator: "SINGLE-OP".into(),
                contest_category_power: "LOW".into(),
                contest_category_assisted: "NON-ASSISTED".into(),
                ..Default::default()
            },
        ),
        "20m",
    );
    assert!(ss.log_fields_at(
        "K2DEF",
        &fields(&[
            ("NR", "12"),
            ("PREC", "A"),
            ("CALL", "K2DEF"),
            ("CK", "71"),
            ("SEC", "CT")
        ]),
        "CW",
        "",
        0,
        100
    ));
    let ss_cab = ss.cabrillo(14_030).expect("one entry");
    let ss_line = ss_cab
        .lines()
        .find(|l| l.starts_with("QSO:"))
        .expect("a QSO line");
    assert!(
        ss_line.ends_with(" CT"),
        "Sweepstakes' line ends with the section, with no transmitter column: {ss_line}"
    );
}

/// The four windows, from the sponsor's own dates for 2026 — each 48 hours from 0000 UTC
/// on the last full weekend's Saturday.
///
/// *"The 2026 CQ World-Wide DX Contest — SSB: October 24-25 / CW: November 28-29. Starts
/// 00:00:00 UTC Saturday, Ends 23:59:59 UTC Sunday"* and *"2026 CQ World-Wide WPX Contest
/// — SSB: March 28-29 / CW: May 30-31. Starts: 0000 UTC Saturday Ends: 2359 UTC Sunday"*.
///
/// ⚠️ The WPX page prints *"SSB: March 28-29, **2025**"* while its own log deadline says
/// *"SSB logs no later than 2359 UTC 31 March 2026"* — a sponsor typo in the year, and
/// March 28–29 IS the last full weekend of March 2026, so the rule below reproduces the
/// dates the sponsor means.
#[test]
fn the_four_windows_are_the_sponsors_own_2026_dates() {
    for (event, start, label) in [
        ("cqww_ssb", 1_792_800_000_u64, "24 Oct 2026"),
        ("cqww_cw", 1_795_824_000, "28 Nov 2026"),
        ("cqwpx_ssb", 1_774_656_000, "28 Mar 2026"),
        ("cqwpx_cw", 1_780_099_200, "30 May 2026"),
    ] {
        let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("shipped");
        let w = rs.event_window(2026);
        assert_eq!(w.start_unix, start, "{event} starts 0000Z {label}");
        assert_eq!(
            w.end_unix - w.start_unix,
            48 * 3600,
            "{event} runs 48 hours"
        );
    }
}

/// ⭐ **Both registries, per contest, pinned in one breath** — the ADIF `Contest_ID`
/// enumeration value and the Cabrillo `CONTEST:` token, which are separate registries and
/// for three shipped contests carry different strings.
///
/// For these four they AGREE, verified 2026-09-09 against each registry's own page, so
/// `cabrillo_contest_token` gets no arm — and this test is what stops one being added.
#[test]
fn the_cq_contests_carry_the_same_string_in_both_registries() {
    for (event, token) in [
        ("cqww_cw", "CQ-WW-CW"),
        ("cqww_ssb", "CQ-WW-SSB"),
        ("cqwpx_cw", "CQ-WPX-CW"),
        ("cqwpx_ssb", "CQ-WPX-SSB"),
    ] {
        let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("shipped");
        assert_eq!(rs.contest_id, token, "{event} ADIF CONTEST_ID");
        assert_eq!(
            tempo_core::contest::cabrillo_contest_token(rs.contest_id),
            token,
            "{event} Cabrillo CONTEST: token — the two registries agree here"
        );
    }
    // POSITIVE CONTROL that the map is a MAP and not a pass-through: Field Day's two
    // registries do disagree, and the same call answers with the Cabrillo string.
    assert_eq!(
        tempo_core::contest::cabrillo_contest_token("ARRL-FIELD-DAY"),
        "ARRL-FD"
    );
}

/// A callsign the country file cannot place scores nothing and counts no country — and it
/// does not stop the log taking the contact, because a station that will not resolve is
/// still a station that was worked.
#[test]
fn an_unplaceable_callsign_is_logged_and_scores_nothing() {
    let mut log = FieldDayLog::new("W9XYZ", select("cqww_cw", &station("W9XYZ", "4")), "20m");
    assert!(work(
        &mut log,
        "20m",
        "QQ9ZZZ",
        &[("RST", "599"), ("ZN", "20")],
        100
    ));
    let (qso, _, mults, _) = score(&log);
    assert_eq!(qso, 0, "no relation, so no priced arm");
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("zone", 1), ("country", 0)],
        "the zone came off the exchange and counts; the country came off the call and does not"
    );
    assert_eq!(mults, Some(1));
    // POSITIVE CONTROL: a placeable station in the same log does score.
    assert!(work(
        &mut log,
        "20m",
        "DL1ABC",
        &[("RST", "599"), ("ZN", "14")],
        110
    ));
    assert_eq!(score(&log).0, 3);
}

/// ⭐ **The session refuses when the country file cannot place MY OWN callsign** — the
/// half of the guard that can be reached with a resolver installed. (The other half, "no
/// resolver at all", is asserted in `contest::session`'s own unit tests, which run in a
/// binary where none is installed.)
#[test]
fn a_relation_priced_contest_refuses_a_callsign_the_country_file_cannot_place() {
    resolver();
    let rs = ruleset_by_id("cqww_cw", CURRENT_RULES_YEAR).expect("shipped");
    let err = ContestSession::for_ruleset(rs, &station("QQ9ZZZ", "4"))
        .expect_err("an unplaceable operator callsign cannot score a relation-priced contest");
    assert!(err.contains("QQ9ZZZ"), "{err}");
    assert!(err.contains("CQ-WW-CW"), "{err}");
    // POSITIVE CONTROL: the same ruleset with a placeable callsign builds.
    assert!(ContestSession::for_ruleset(rs, &station("W9XYZ", "4")).is_ok());
    // …and a contest that does NOT price by relation is not refused for the same
    // callsign, because nothing about it needs the country file.
    assert!(ruleset_by_id("ohqp", CURRENT_RULES_YEAR)
        .map(|oh| ContestSession::for_ruleset(
            oh,
            &StationData {
                mycall: "QQ9ZZZ".into(),
                contest_qth_state: "OH".into(),
                contest_qth_county: "FRAN".into(),
                ..Default::default()
            }
        ))
        .expect("shipped")
        .is_ok());
}
