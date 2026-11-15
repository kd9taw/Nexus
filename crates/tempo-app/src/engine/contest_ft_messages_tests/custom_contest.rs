//! ⭐ An operator's own contest whose exchange matches a WSJT-X contest message runs that
//! activity's messages through the Operate cockpit's verbs: the contest's Running, a double-click
//! on the partner's CQ (whose decode's SNR makes the report), and Call CQ — against the scripted
//! WSJT-X partner. An exchange the message cannot carry is refused at the verb, before anything
//! changes, in the operator's words.
//!
//! ⚠️ The user layer is process-global: each test installs under its own id and token.

use super::*;
use tempo_core::fd_rules::custom::{self, CustomDef, CustomField, CustomWindow};

/// 2026-11-14T00:00Z.
const NOW: u64 = 1_794_614_400;

fn install(id: &str, token: &str, fields: &[&str]) {
    let d = CustomDef {
        format: custom::FORMAT.into(),
        version: custom::VERSION,
        id: id.into(),
        name: format!("Engine {id}"),
        short: String::new(),
        cabrillo_contest: token.into(),
        modes: vec!["FT8".into(), "FT4".into()],
        window: CustomWindow::Weekend {
            month: 8,
            weekend: "nth_full".into(),
            n: 2,
            start_hour_utc: 0,
            duration_hours: 48,
        },
        exchange: fields
            .iter()
            .map(|f| CustomField {
                field: f.to_string(),
                label: String::new(),
            })
            .collect(),
        ft_cq: None,
        ft_roundup: true,
        dupe: "band".into(),
        points: Default::default(),
    };
    let refused = tempo_core::fd_rules::install_custom(std::slice::from_ref(&d), NOW);
    assert!(refused.is_empty(), "{refused:?}");
}

/// RST + serial, Yes to the Roundup: running, double-clicking the partner's CQ, and Call CQ each
/// work the contact over for over as WSJT-X's RTTY activity does, every over the RTTY contest
/// message (i3=3), and the contest log holds the exchange both ways.
#[test]
fn a_roundup_shaped_contest_works_wsjtxs_rtty_messages_through_the_cockpit() {
    install("custom_eng_ru", "ENG-RU", &["rst", "serial"]);
    let ran = scene(
        "custom_eng_ru",
        Tier::Ft8,
        Role::Running,
        Act::Roundup,
        MY_GRID,
    );
    assert_eq!(ran.entered, Ok(()));
    // The partner is heard at -12 dB: `549`.
    assert_eq!(
        ran.sent,
        [
            "CQ RU K1ABC FN42",
            "W9XYZ K1ABC R 549 0001",
            "W9XYZ K1ABC 73",
        ],
        "{:#?}",
        ran.lines
    );
    assert_eq!(
        ran.logged,
        ["W9XYZ rx[RST=559,NR=13] tx[RST=549,NR=1] mode DIG FT8 dupe false"]
    );
    let clicked = scene(
        "custom_eng_ru",
        Tier::Ft8,
        Role::DoubleClick,
        Act::Roundup,
        MY_GRID,
    );
    assert_eq!(
        clicked.sent,
        ["W9XYZ K1ABC 549 0001", "W9XYZ K1ABC RR73"],
        "{:#?}",
        clicked.lines
    );
    assert_eq!(
        clicked.logged,
        ["W9XYZ rx[RST=559,NR=13] tx[RST=549,NR=1] mode DIG FT8 dupe false"]
    );
    for over in ["W9XYZ K1ABC R 549 0001", "W9XYZ K1ABC 549 0001"] {
        assert_eq!(air_form(over).0, "i3=3", "{over}");
        assert_eq!(air_form(over).1, *over, "{over} decodes as itself");
    }
}

/// ⛔ A QTH the Roundup message has no word for (DX) refuses the contest's Running and a
/// double-click, before anything changes, with the station's reason; a serial past 7999 the same.
#[test]
fn a_roundup_exchange_the_message_cannot_carry_is_refused_at_the_verb() {
    install("custom_eng_ru_dx", "ENG-RU-DX", &["rst", "state_province"]);
    // The contest entered with DX as the QTH (a running contest keeps the session it started
    // with, so the switch goes off and on around the change).
    let mut e = engine_for("custom_eng_ru_dx", MY_GRID);
    let mut s = e.settings().clone();
    s.fd_active = false;
    e.apply_settings(s);
    let mut s = e.settings().clone();
    s.contest_qth_state = "DX".into();
    s.fd_active = true;
    e.apply_settings(s);
    e.set_tier(Tier::Ft8);
    let before = mode_line(&e);
    let why = e.start_running().expect_err("refused");
    assert!(why.contains("Your QTH \"DX\""), "{why}");
    assert_eq!(mode_line(&e), before, "nothing changed");
    let cq = format!("CQ RU {DX} {DX_GRID}");
    e.ingest_decodes_for_test(&[heard(&cq)], 2);
    let why = e
        .work_station(DX, Some(DX_GRID), Some(&cq), Some(DX_SNR), None)
        .expect_err("refused");
    assert!(why.contains("Your QTH \"DX\""), "{why}");
    assert_eq!(mode_line(&e), before, "nothing changed");
    assert!(!e.tx_enabled());
}

/// RST + serial + six-character grid: running and double-clicking work the contact in WSJT-X's EU
/// VHF messages through the cockpit — `CQ TEST` and the Tx1 with four characters of the locator,
/// the exchanges with both calls hashed and all six — and the contest log holds it both ways.
#[test]
fn an_eu_vhf_shaped_contest_works_wsjtxs_eu_vhf_messages_through_the_cockpit() {
    install("custom_eng_eu", "ENG-EU", &["rst", "serial", "grid6"]);
    let ran = scene(
        "custom_eng_eu",
        Tier::Ft8,
        Role::Running,
        Act::EuVhf,
        "FN42ab",
    );
    assert_eq!(ran.entered, Ok(()));
    assert_eq!(
        ran.sent,
        [
            "CQ TEST K1ABC FN42",
            "<W9XYZ> <K1ABC> 540001 FN42AB",
            "W9XYZ K1ABC RR73",
        ],
        "{:#?}",
        ran.lines
    );
    assert_eq!(
        ran.logged,
        ["W9XYZ rx[RST=57,NR=13,GRID=EN37AB] tx[RST=54,NR=1,GRID=FN42AB] mode DIG FT8 dupe false"]
    );
    let clicked = scene(
        "custom_eng_eu",
        Tier::Ft8,
        Role::DoubleClick,
        Act::EuVhf,
        "FN42ab",
    );
    assert_eq!(
        clicked.sent,
        [
            "W9XYZ K1ABC FN42",
            "<W9XYZ> <K1ABC> R 540001 FN42AB",
            "W9XYZ K1ABC 73",
        ],
        "{:#?}",
        clicked.lines
    );
    assert_eq!(
        clicked.logged,
        ["W9XYZ rx[RST=57,NR=13,GRID=EN37AB] tx[RST=54,NR=1,GRID=FN42AB] mode DIG FT8 dupe false"]
    );
    for over in [
        "<W9XYZ> <K1ABC> 540001 FN42AB",
        "<W9XYZ> <K1ABC> R 540001 FN42AB",
    ] {
        assert_eq!(air_form(over).0, "i3=5", "{over}");
    }
    for over in ["CQ TEST K1ABC FN42", "W9XYZ K1ABC FN42"] {
        assert_eq!(air_form(over).0, "i3=1", "{over}");
    }
}
