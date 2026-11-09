//! ⭐ **What a spotted station would add to a contest's multipliers** — `FieldDayLog::spot_mults`,
//! the question a contest band map asks of every spot, answered by the scorer's own keys.
//!
//! ⚠️ **The country file here is a STUB**, the shape `ilqp.rs` established: the real resolver is
//! installed by src-tauri from the vendored cty.dat.
use tempo_core::contest::{install_call_resolver, CallLocation, ContestSession, StationData};
use tempo_core::fd_rules::{ruleset_by_id, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FieldDayLog;

fn place(call: &str) -> Option<CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    let entity = match up.as_str() {
        c if c.starts_with("KH6") => "Hawaii",
        c if c.starts_with("VE") => "Canada",
        c if c.starts_with("DL") => "Germany",
        c if c.starts_with('F') => "France",
        c if c.starts_with("EA") => "Spain",
        c if c.starts_with('I') => "Italy",
        c if c.starts_with("JA") => "Japan",
        c if c.starts_with("PY") => "Brazil",
        c if c.starts_with("ZS") => "South Africa",
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

fn log_of(event: &str, station: StationData, band: &str) -> FieldDayLog {
    resolver();
    let rs = ruleset_by_id(event, CURRENT_RULES_YEAR).expect("a shipped ruleset");
    let session = ContestSession::for_ruleset(rs, &station).expect("the session builds");
    FieldDayLog::new("W9XYZ", session, band)
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn ilqp(state: &str) -> FieldDayLog {
    log_of(
        "ilqp",
        StationData {
            contest_qth_state: state.into(),
            contest_qth_county: if state == "IL" {
                "COOK".into()
            } else {
                String::new()
            },
            ..Default::default()
        },
        "20m",
    )
}

fn work(log: &mut FieldDayLog, call: &str, qth: &str, t: u64) {
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
}

/// The Illinois QSO Party, in-state: a county, a state and a DXCC entity each count once per log.
/// A county read from the station's own earlier contact; an entity from the call; the US, which
/// the sponsor's sentence excludes, never.
#[test]
fn an_in_state_station_sees_each_multiplier_a_spot_would_add() {
    let mut log = ilqp("IL");
    work(&mut log, "K9AAA", "COOK", 100);
    work(&mut log, "W1AAA", "CT", 110);
    let probe = log.spot_mults();
    assert_eq!(
        probe.new_mults("K9AAA", "40m", "CW", None),
        vec![],
        "COOK is counted"
    );
    assert_eq!(
        probe.new_mults("DL1AAA", "20m", "CW", None),
        vec![("dxcc", "Germany".to_string())],
        "a DX entity: from the call, before a word of exchange"
    );
    assert_eq!(
        probe.new_mults("K9ZZZ", "20m", "CW", None),
        vec![],
        "a US station never worked: the US is excluded, and its county is not guessed"
    );
}

/// A received value is read from the station's own earlier contact: the Ohio QSO Party counts a
/// county once per MODE, so a county worked on CW is a new multiplier on phone.
#[test]
fn a_received_multiplier_is_the_one_the_station_sent_before() {
    let mut log = log_of(
        "ohqp",
        StationData {
            contest_qth_state: "WI".into(),
            ..Default::default()
        },
        "20m",
    );
    work(&mut log, "K8AAA", "ADAM", 100);
    let probe = log.spot_mults();
    assert_eq!(
        probe.new_mults("K8AAA", "40m", "PH", None),
        vec![("county", "ADAM".to_string())],
        "ADAM on phone is new"
    );
    assert_eq!(
        probe.new_mults("K8AAA", "40m", "CW", None),
        vec![],
        "ADAM on CW is counted"
    );
    assert_eq!(
        probe.new_mults("K8BBB", "40m", "PH", None),
        vec![],
        "a station never worked: its county is not guessed"
    );
}

/// A capped rule adds nothing past its cap: ILQP's *"DXCC countries (maximum 5)"*.
#[test]
fn a_capped_rule_adds_nothing_once_it_is_full() {
    let mut log = ilqp("IL");
    for (i, call) in ["DL1AAA", "F1AAA", "EA1AAA", "I1AAA"].iter().enumerate() {
        work(&mut log, call, "DX", 100 + i as u64);
    }
    assert_eq!(
        log.spot_mults().new_mults("JA1AAA", "20m", "CW", None),
        vec![("dxcc", "Japan".to_string())],
        "the fifth entity still counts"
    );
    work(&mut log, "JA1AAA", "DX", 200);
    assert_eq!(
        log.spot_mults().new_mults("PY1AAA", "20m", "CW", None),
        vec![],
        "the sixth does not"
    );
}

/// A rule counts only for the roles it names: out of state, ILQP's multiplier is the county alone.
#[test]
fn an_out_of_state_station_counts_only_its_own_roles_rules() {
    let log = ilqp("WI");
    assert_eq!(
        log.spot_mults().new_mults("DL1AAA", "20m", "CW", None),
        vec![]
    );
}

/// A mode the contest bans counts nothing: ILQP gives an FT8 contact no credit.
#[test]
fn a_spot_in_a_banned_mode_adds_nothing() {
    let probe = ilqp("IL").spot_mults();
    assert_eq!(probe.new_mults("DL1AAA", "20m", "DIG", Some("FT8")), vec![]);
    assert_eq!(
        probe.new_mults("DL1AAA", "20m", "DIG", Some("RTTY")),
        vec![("dxcc", "Germany".to_string())],
        "control: RTTY is not banned"
    );
}

/// CQ WPX: the prefix is the whole multiplier, read from the call by the sponsor's own rule.
#[test]
fn a_wpx_spot_adds_its_prefix_until_it_is_worked() {
    let mut log = log_of(
        "cqwpx_cw",
        StationData {
            mycall: "W9XYZ".into(),
            ..Default::default()
        },
        "20m",
    );
    let probe = log.spot_mults();
    assert_eq!(
        probe.new_mults("ZS6ABC", "40m", "CW", None),
        vec![("prefix", "ZS6".to_string())]
    );
    assert!(log.log_fields_at(
        "ZS6XYZ",
        &fields(&[("RST", "599"), ("NR", "1")]),
        "CW",
        "",
        0,
        100
    ));
    assert_eq!(
        log.spot_mults().new_mults("ZS6ABC", "40m", "CW", None),
        vec![],
        "ZS6 is worked, once per log"
    );
}

/// An IL county as the call history would give it.
fn county(code: &'static str) -> Vec<tempo_core::contest::FieldValue> {
    vec![tempo_core::contest::FieldValue {
        key: "QTH",
        raw: code.to_string(),
        domain: Some("il_counties"),
    }]
}

/// ⭐ **A prediction is the scorer's answer to what another source says the station sends.** Out
/// of state, ILQP's multiplier is the county: a station never worked whose county is not yet
/// worked predicts that county; one whose county is worked predicts nothing.
#[test]
fn a_never_worked_station_predicts_the_county_it_would_send() {
    let mut log = ilqp("WI");
    work(&mut log, "K9AAA", "COOK", 100);
    let probe = log.spot_mults();
    assert_eq!(
        probe.predicted_mults("K9NEW", "20m", "CW", None, &[county("LAKE")]),
        vec![("county", "LAKE".to_string())],
        "LAKE is not worked"
    );
    assert_eq!(
        probe.predicted_mults("K9NEW", "20m", "CW", None, &[county("COOK")]),
        vec![],
        "COOK is worked"
    );
    assert_eq!(
        probe.predicted_mults("K9NEW", "20m", "CW", None, &[]),
        vec![],
        "nothing said, nothing predicted"
    );
    assert_eq!(
        probe.predicted_mults("K9NEW", "20m", "DIG", Some("FT8"), &[county("LAKE")]),
        vec![],
        "a banned mode counts nothing"
    );
}

/// The station's own earlier exchange in this contest beats any other source: a station logged
/// from COOK predicts nothing, whatever the history says.
#[test]
fn an_earlier_exchange_beats_the_prediction() {
    let mut log = ilqp("WI");
    work(&mut log, "K9AAA", "COOK", 100);
    let probe = log.spot_mults();
    assert_eq!(
        probe.predicted_mults("K9AAA", "40m", "CW", None, &[county("LAKE")]),
        vec![]
    );
}

/// A county line: every county any of its exchanges would add, each once, and never a
/// multiplier the call alone already gives (that is `new_mults`', confirmed).
#[test]
fn a_county_line_predicts_each_county_it_would_add() {
    let mut log = ilqp("WI");
    work(&mut log, "K9AAA", "COOK", 100);
    let probe = log.spot_mults();
    assert_eq!(
        probe.predicted_mults(
            "K9NEW",
            "20m",
            "CW",
            None,
            &[
                county("COOK"),
                county("DUPG"),
                county("LAKE"),
                county("DUPG")
            ]
        ),
        vec![
            ("county", "DUPG".to_string()),
            ("county", "LAKE".to_string())
        ]
    );
    let probe = ilqp("IL").spot_mults();
    assert_eq!(
        probe.new_mults("DL1AAA", "20m", "CW", None),
        vec![("dxcc", "Germany".to_string())]
    );
    assert_eq!(
        probe.predicted_mults("DL1AAA", "20m", "CW", None, &[county("LAKE")]),
        vec![("county", "LAKE".to_string())],
        "the entity is confirmed already, so only the county is predicted"
    );
}

/// ⭐ **A spot whose mode is only a guess from its frequency still previews its multipliers.** At
/// the RTTY Roundup, which bans CW, a spot that names no mode on US 40 m below 7.070 MHz is
/// guessed CW from where it sits, and that stretch is busy with RTTY during the contest. The
/// preview judges a banned mode by the mode a spot names, never by the guessed class: a guess is
/// not a contact in a banned mode, so the station's country, and the state it is said to send,
/// are still shown. A spot that names a banned mode adds nothing.
#[test]
fn a_spot_guessed_cw_from_its_frequency_still_previews_its_multipliers() {
    let log = log_of(
        "arrlrtty",
        StationData {
            contest_qth_state: "IL".into(),
            ..Default::default()
        },
        "40m",
    );
    let probe = log.spot_mults();
    assert_eq!(
        probe.new_mults("DL1AAA", "40m", "CW", None),
        vec![("dxcc", "Germany".to_string())],
        "a guessed CW class: its country"
    );
    let ma = vec![tempo_core::contest::FieldValue {
        key: "QTH",
        raw: "MA".to_string(),
        domain: Some("arrl_rtty_qth"),
    }];
    assert_eq!(
        probe.predicted_mults("K1NEW", "40m", "CW", None, std::slice::from_ref(&ma)),
        vec![("qth", "MA".to_string())],
        "a guessed CW class: the state it is said to send"
    );
    assert_eq!(
        probe.new_mults("DL1AAA", "40m", "CW", Some("CW")),
        vec![],
        "control: a spot that names CW adds nothing"
    );
    assert_eq!(
        probe.predicted_mults("K1NEW", "40m", "DIG", Some("FT8"), &[ma]),
        vec![],
        "control: a banned mode named"
    );
}
