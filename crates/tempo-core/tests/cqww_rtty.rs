//! ⭐ **CQ WW RTTY DX, end to end** — the contest this build ships for on 2026-09-26.
//!
//! Every expected number comes from the sponsor's own current pages, read 2026-09-17:
//! <https://cqwwrtty.com/rules.htm> and <https://cqwwrtty.com/cabrillo.htm>. The sentence
//! each assertion rests on is quoted at it, so a rules change is a red test with the
//! sponsor's own words attached rather than a silent rescore.
//!
//! What it has that CQ WW CW/SSB (`tests/cqww.rs`) does not: a SECOND ROLE (W/VE stations
//! send a QTH, everyone else does not), a THIRD multiplier (the W/VE QTH), and a point
//! table with no North American exception.
//!
//! ⚠️ **The country file here is a STUB**, for the reason `tests/cqww.rs` gives: the real
//! resolver lives in a crate that depends on this one. src-tauri pins the real entity
//! spellings. Every callsign is an example call.
use tempo_core::contest::{install_call_resolver, CallLocation, ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

/// The stub country file: prefix → (entity, continent), longest prefix wins. Entity
/// spellings are cty.dat's own; Sicily is a WAE entity (`*IT9`) and a country of its own.
const TABLE: &[(&str, &str, &str)] = &[
    ("KL7", "Alaska", "NA"),
    ("KH6", "Hawaii", "OC"),
    ("IT9", "Sicily", "EU"),
    ("VE", "Canada", "NA"),
    ("VO", "Canada", "NA"),
    ("VY", "Canada", "NA"),
    ("DL", "Fed. Rep. of Germany", "EU"),
    ("G", "England", "EU"),
    ("I", "Italy", "EU"),
    ("JA", "Japan", "AS"),
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

/// A station: my call, my zone, and my QTH as Settings holds it.
fn station(mycall: &str, zone: &str, qth: &str) -> StationData {
    StationData {
        mycall: mycall.to_string(),
        contest_cq_zone: zone.to_string(),
        contest_qth_state: qth.to_string(),
        contest_category_power: "LOW".to_string(),
        contest_category_assisted: "NON-ASSISTED".to_string(),
        ..Default::default()
    }
}

fn select(st: &StationData) -> ContestSession {
    resolver();
    let rs =
        ruleset_by_id("cqww_rtty", CURRENT_RULES_YEAR).expect("cqww_rtty is a shipped ruleset");
    ContestSession::for_ruleset(rs, st).unwrap_or_else(|e| panic!("cqww_rtty session refused: {e}"))
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

/// 2026-09-26 00:00:00Z, the contest's first second.
const START: u64 = 1_790_380_800;

/// Log one RTTY contact on a band, `min` minutes into the contest.
fn work(log: &mut FieldDayLog, band: &str, call: &str, ex: &[(&str, &str)], min: u64) -> bool {
    log.band = band.to_string();
    log.log_fields_at(call, &fields(ex), "DIG", "RTTY", 0, START + min * 60)
}

// ---------------------------------------------------------------------------
// Roles and the exchange
// ---------------------------------------------------------------------------

/// ⭐ **Who sends what.** III: *"RST report plus CQ zone number of the station location (e.g.,
/// 599 05). Stations in the continental USA and Canada also send QTH (e.g., 599 05 MA)."*
#[test]
fn a_w_ve_station_sends_its_qth_and_everyone_else_sends_rst_and_zone() {
    let il = select(&station("W9XYZ", "4", "IL"));
    assert_eq!(il.contest_id, "CQ-WW-RTTY");
    assert_eq!(il.role().id, "w_ve");
    assert_eq!(
        composing(&il),
        vec![
            ("RST", "599".into()),
            ("ZN", "4".into()),
            ("QTH", "IL".into())
        ]
    );
    // IV.C.3's Canadian call areas are the sponsor's own codes, three letters included.
    for (call, zone, qth) in [
        ("VE8XYZ", "1", "NWT"),
        ("VY2XYZ", "5", "PEI"),
        ("VO1XYZ", "5", "NF"),
        ("VO2XYZ", "2", "LB"),
        ("VE3XYZ", "4", "ON"),
    ] {
        let s = select(&station(call, zone, qth));
        assert_eq!(s.role().id, "w_ve", "{qth}");
        assert_eq!(s.field("QTH"), qth);
    }
    // "The District of Columbia = DC" — a QTH of its own.
    assert_eq!(select(&station("W3XYZ", "5", "DC")).role().id, "w_ve");

    // A DX station sends the report and its zone, and nothing else.
    let dx = select(&station("DL9XYZ", "14", ""));
    assert_eq!(dx.role().id, "dx");
    assert_eq!(
        composing(&dx),
        vec![("RST", "599".into()), ("ZN", "14".into())]
    );
    // ⭐ IV.C.3: "Alaska (KL7) and Hawaii (KH6) are counted as country multipliers only and
    // not as state multipliers" — neither is continental, so neither sends a QTH.
    for (call, qth) in [("KL7XYZ", "AK"), ("KH6XYZ", "HI")] {
        let s = select(&station(call, "1", qth));
        assert_eq!(s.role().id, "dx", "{qth} is not a W/VE QTH");
        assert_eq!(s.field("QTH"), "");
    }
    // ⚠️ A Canadian spelling the sponsor does not print is not a QTH either — the role is
    // selected on the sponsor's own codes (NF, not NL), and this pins that it is.
    assert_eq!(select(&station("VO1XYZ", "5", "NL")).role().id, "dx");

    // Both roles RECEIVE the QTH, and it is optional: a DX station sends none.
    for s in [&il, &dx] {
        let boxes: Vec<(&str, bool)> = s
            .role()
            .receives
            .iter()
            .filter_map(|k| s.exchange.field(k))
            .map(|f| (f.key, f.required))
            .collect();
        assert_eq!(boxes, vec![("RST", true), ("ZN", true), ("QTH", false)]);
    }
}

/// The window: *"September 26-27 / Starts 00:00:00 UTC Saturday -- Ends 23:59:59 UTC
/// Sunday"* — the last full weekend of September, which also reproduces every date on the
/// sponsor's archived rules, 2023's included (September 23-24, when September 30 was a
/// Saturday whose Sunday was October 1).
#[test]
fn the_window_is_the_last_full_weekend_of_september() {
    let rs = ruleset_by_id("cqww_rtty", CURRENT_RULES_YEAR).expect("shipped");
    for (year, start) in [
        (2022, 1_663_977_600_u64),
        (2023, 1_695_427_200),
        (2024, 1_727_481_600),
        (2025, 1_758_931_200),
        (2026, 1_790_380_800),
        (2027, 1_821_830_400),
    ] {
        let w = rs.event_window(year);
        assert_eq!(w.start_unix, start, "{year} starts 0000Z Saturday");
        assert_eq!(w.end_unix - w.start_unix, 48 * 3600, "{year} runs 48 hours");
    }
}

// ---------------------------------------------------------------------------
// Points, multipliers and dupes
// ---------------------------------------------------------------------------

/// ⭐ **The score, from Illinois.**
///
/// IV.B: *"Contacts between stations on different continents count three (3) points.
/// Contacts between stations on the same continent but in different countries count two (2)
/// points. Contacts between stations in the same country count one (1) point."* — with no
/// North American exception, so W to VE is the plain same-continent two.
///
/// IV.A: *"The final score is the result of the total QSO points multiplied by the sum of
/// zone, country and QTH multipliers."* Each of the three is counted *"on each band"*.
#[test]
fn points_and_three_per_band_multipliers_from_the_united_states() {
    let mut log = FieldDayLog::new("W9XYZ", select(&station("W9XYZ", "4", "IL")), "20m");
    let score = |log: &FieldDayLog| log.ruleset().scoring.score(log.score_rows(), 1);
    assert!(work(
        &mut log,
        "20m",
        "VE3XYZ",
        &[("RST", "599"), ("ZN", "4"), ("QTH", "ON")],
        1
    ));
    assert_eq!(score(&log).0, 2, "W→VE: same continent, different country");
    assert!(work(
        &mut log,
        "20m",
        "K2DEF",
        &[("RST", "599"), ("ZN", "5"), ("QTH", "NY")],
        2
    ));
    assert_eq!(score(&log).0, 2 + 1, "W→W: same country");
    assert!(work(
        &mut log,
        "20m",
        "JA1ABC",
        &[("RST", "599"), ("ZN", "25")],
        3
    ));
    assert_eq!(score(&log).0, 2 + 1 + 3, "W→JA: different continents");

    // THE DUPE — IV.B "Stations may be contacted once on each band." The same station on
    // the same band is refused; the same station on another band is a new contact.
    assert!(
        !work(
            &mut log,
            "20m",
            "K2DEF",
            &[("RST", "599"), ("ZN", "5"), ("QTH", "NY")],
            4
        ),
        "same call, same band"
    );
    assert!(work(
        &mut log,
        "40m",
        "K2DEF",
        &[("RST", "599"), ("ZN", "5"), ("QTH", "NY")],
        5
    ));
    assert!(work(
        &mut log,
        "40m",
        "DL1ABC",
        &[("RST", "599"), ("ZN", "14")],
        6
    ));
    assert_eq!(score(&log).0, 2 + 1 + 3 + 1 + 3);

    // THE THREE MULTIPLIER SETS, per band:
    //   zone:    20 m {4, 5, 25}                40 m {5, 14}                  = 5
    //   country: 20 m {Canada, United States,   40 m {United States, Germany} = 5
    //                  Japan}
    //   qth:     20 m {ON, NY}                  40 m {NY}                     = 3
    // ⚠️ The United States and Canada COUNT as countries: IV.C names no exclusion (the
    // seed's provenance records that this is the rules read as written).
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows()),
        vec![("zone", 5), ("country", 5), ("qth", 3)]
    );
    let (qso, _, mults, total) = score(&log);
    assert_eq!(mults, Some(13));
    assert_eq!(total, qso * 13);
}

/// A WAE entity is a country of its own — IV.C *"The DXCC entity list, Worked All Europe
/// (WAE) multiplier list plus IG9/IH9"* — and neither a blank QTH nor the `DX` Cabrillo
/// placeholder is ever a QTH multiplier.
#[test]
fn a_wae_entity_counts_and_a_blank_or_dx_qth_does_not() {
    let mut log = FieldDayLog::new("DL9XYZ", select(&station("DL9XYZ", "14", "")), "20m");
    assert!(work(
        &mut log,
        "20m",
        "I1ABC",
        &[("RST", "599"), ("ZN", "15")],
        1
    ));
    assert!(work(
        &mut log,
        "20m",
        "IT9ABC",
        &[("RST", "599"), ("ZN", "15")],
        2
    ));
    assert!(work(
        &mut log,
        "20m",
        "G3ABC",
        &[("RST", "599"), ("ZN", "14"), ("QTH", "DX")],
        3
    ));
    let counts = log.ruleset().scoring.mult_counts(log.score_rows());
    assert_eq!(counts[1], ("country", 3), "Italy, Sicily and England");
    assert_eq!(counts[2], ("qth", 0), "no W/VE QTH was worked");
    // POSITIVE CONTROL: a real QTH on the same log does count.
    assert!(work(
        &mut log,
        "20m",
        "W1ABC",
        &[("RST", "599"), ("ZN", "5"), ("QTH", "MA")],
        4
    ));
    assert_eq!(
        log.ruleset().scoring.mult_counts(log.score_rows())[2],
        ("qth", 1)
    );
    // Same continent, different country: two points each, then EU→NA three — the DX role
    // is priced exactly as the W/VE role is.
    assert_eq!(
        log.ruleset().scoring.score(log.score_rows(), 1).0,
        2 + 2 + 2 + 3
    );
}
