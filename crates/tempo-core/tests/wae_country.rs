//! ⭐ **A WAE area is a country of its own in CQ WW, and its DXCC entity everywhere else.**
//!
//! CQ WW's rules, IV.C.2 (cqww.com/rules, read 2026-10-25): *"The DXCC entity list, Worked All
//! Europe (WAE) multiplier list plus IG9/IH9, and continental boundaries are the standards for
//! defining country multipliers."* So IT9 (Sicily) and I (Italy) are two country multipliers
//! there. TNQP, TXQP and ILQP count DXCC countries for their DX multiplier, and on that list
//! Sicily is Italy: one multiplier.
//!
//! The country file is a STUB, as in `cqww.rs`: the crate that vendors cty.dat depends on this
//! one. The binding to the real file — that IT9 places as Sicily and folds into Italy — is
//! asserted in `crates/propagation` and `src-tauri`.
use tempo_core::contest::{
    install_call_resolver, install_wae_parent, CabrilloEntrant, CallLocation, ContestSession,
    StationData,
};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

/// prefix → (entity, continent), longest prefix first; cty.dat's own spellings.
const TABLE: &[(&str, &str, &str)] = &[
    ("IT9", "Sicily", "EU"),
    ("I", "Italy", "EU"),
    ("DL", "Fed. Rep. of Germany", "EU"),
    ("W", "United States", "NA"),
];

fn place(call: &str) -> Option<CallLocation> {
    let call = call.trim().to_ascii_uppercase();
    TABLE
        .iter()
        .find(|(p, _, _)| call.starts_with(p))
        .map(|&(_, entity, continent)| CallLocation {
            entity,
            continent,
            cq_zone: None,
        })
}

fn wae_parent(entity: &str) -> Option<&'static str> {
    (entity == "Sicily").then_some("Italy")
}

fn installed() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(place).expect("the first install in this test binary");
        install_wae_parent(wae_parent).expect("the first install in this test binary");
    });
}

fn log_for(event: &str, station: StationData) -> FieldDayLog {
    installed();
    let rs = ruleset_by_id(event, CURRENT_RULES_YEAR)
        .unwrap_or_else(|| panic!("{event} is a shipped ruleset"));
    let s = ContestSession::for_ruleset(rs, &station)
        .unwrap_or_else(|e| panic!("{event} session refused: {e}"));
    FieldDayLog::new(&station.mycall, s, "20m")
}

fn work(log: &mut FieldDayLog, call: &str, ex: &[(&str, &str)], at: u64) {
    let fields: Vec<(String, String)> = ex
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    log.session.compose_for(call, at);
    assert!(log.log_fields_at(call, &fields, "CW", "", 0, at), "{call}");
}

/// The count the named multiplier rule claims, by the scorer's own count.
fn mults(log: &FieldDayLog, id: &str) -> usize {
    log.ruleset()
        .scoring
        .mult_counts(log.score_rows())
        .into_iter()
        .find(|(rule, _)| *rule == id)
        .unwrap_or_else(|| panic!("{id} is one of this contest's multipliers"))
        .1
}

fn total(log: &FieldDayLog) -> u32 {
    log.ruleset().scoring.score(log.score_rows(), 1).3
}

/// The Cabrillo file's CLAIMED-SCORE line, where the contest's file carries one.
fn claimed(log: &FieldDayLog) -> Option<String> {
    let all = CabrilloEntrant {
        band: "ALL".into(),
        ..Default::default()
    };
    let cab = log.cabrillo_with(14_030, &all).expect("one entry");
    cab.lines()
        .find(|l| l.starts_with("CLAIMED-SCORE:"))
        .map(str::to_string)
}

fn cq_ww() -> FieldDayLog {
    log_for(
        "cqww_cw",
        StationData {
            mycall: "W9XYZ".into(),
            contest_cq_zone: "4".into(),
            ..Default::default()
        },
    )
}

/// The three shipped contests whose DX multiplier counts DXCC countries, each from inside its
/// state (the role that counts it).
fn dxcc_parties() -> Vec<(&'static str, FieldDayLog)> {
    [
        ("tnqp", "TN", "WILL"),
        ("txqp", "TX", "BEE"),
        ("ilqp", "IL", "KANE"),
    ]
    .into_iter()
    .map(|(event, state, county)| {
        let log = log_for(
            event,
            StationData {
                mycall: "W9XYZ".into(),
                contest_qth_state: state.into(),
                contest_qth_county: county.into(),
                ..Default::default()
            },
        );
        (event, log)
    })
    .collect()
}

#[test]
fn cq_ww_counts_sicily_and_italy_as_two_countries() {
    let mut log = cq_ww();
    work(&mut log, "I1ABC", &[("RST", "599"), ("ZN", "15")], 100);
    work(&mut log, "IT9XYZ", &[("RST", "599"), ("ZN", "15")], 110);
    assert_eq!(mults(&log, "country"), 2);
    assert_eq!(mults(&log, "zone"), 1);
    // 3 + 3 points (Europe from North America) × (1 zone + 2 countries).
    assert_eq!(total(&log), 18);
    assert_eq!(claimed(&log).as_deref(), Some("CLAIMED-SCORE: 18"));
}

#[test]
fn the_dxcc_contests_count_sicily_as_italy() {
    for (event, mut log) in dxcc_parties() {
        work(&mut log, "I1ABC", &[("RST", "599"), ("QTH", "DX")], 100);
        let before = total(&log);
        work(&mut log, "IT9XYZ", &[("RST", "599"), ("QTH", "DX")], 110);
        assert_eq!(mults(&log, "dxcc"), 1, "{event}: Sicily is Italy");
        // The second contact doubles the points and adds no multiplier, so the total doubles;
        // counting Sicily as a country would have added a multiplier on top.
        assert!(before > 0, "{event}: premise, the first contact scores");
        assert_eq!(total(&log), before * 2, "{event}");
        // A party's file that claims a score (ILQP's) claims the one shown; TNQP's and TXQP's
        // claim none (the sponsor scores the log).
        let want = match event {
            "ilqp" => Some(format!("CLAIMED-SCORE: {}", total(&log))),
            _ => None,
        };
        assert_eq!(claimed(&log), want, "{event}");
    }
}

/// CONTROL: a log with no WAE area scores the same on either list — Germany and Italy are two
/// countries in CQ WW and two DXCC countries in every party.
#[test]
fn a_log_without_a_wae_area_scores_as_it_did() {
    let mut ww = cq_ww();
    work(&mut ww, "I1ABC", &[("RST", "599"), ("ZN", "15")], 100);
    work(&mut ww, "DL1ABC", &[("RST", "599"), ("ZN", "14")], 110);
    assert_eq!(mults(&ww, "country"), 2);
    assert_eq!(total(&ww), 6 * 4);
    for (event, mut log) in dxcc_parties() {
        work(&mut log, "I1ABC", &[("RST", "599"), ("QTH", "DX")], 100);
        work(&mut log, "DL1ABC", &[("RST", "599"), ("QTH", "DX")], 110);
        assert_eq!(mults(&log, "dxcc"), 2, "{event}");
    }
}

/// The entry strip asks which list a contest counts; only CQ WW's three answer WAE.
#[test]
fn only_cq_ww_counts_wae_areas() {
    let wae: Vec<&str> = [
        "arrlfd",
        "wfd",
        "tnqp",
        "ohqp",
        "cqp",
        "txqp",
        "ilqp",
        "nyqp",
        "arrlss_cw",
        "arrlss_ssb",
        "cqww_cw",
        "cqww_ssb",
        "cqww_rtty",
        "cqwpx_cw",
        "cqwpx_ssb",
        "arrlvhf_jan",
        "arrlvhf_jun",
        "arrlvhf_sep",
    ]
    .into_iter()
    .filter(|e| {
        ruleset_by_id(e, CURRENT_RULES_YEAR)
            .unwrap_or_else(|| panic!("{e} is shipped"))
            .scoring
            .counts_wae_areas()
    })
    .collect();
    assert_eq!(wae, ["cqww_cw", "cqww_ssb", "cqww_rtty"]);
    assert_eq!(
        tempo_core::contest::counted_country("Sicily", true),
        "Sicily"
    );
    installed();
    assert_eq!(
        tempo_core::contest::counted_country("Sicily", false),
        "Italy"
    );
    assert_eq!(
        tempo_core::contest::counted_country("Italy", false),
        "Italy"
    );
}
