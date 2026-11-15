//! ⭐ **An operator's own contest, end to end through tempo-core**: compiled from its definition,
//! installed in the user layer, found by id and by `CONTEST_ID`, duped, scored and exported like a
//! shipped contest — and FT8/FT4 still send standard messages for it.
//!
//! ⚠️ The user layer is process-global and these tests run in parallel, so each one installs
//! under its OWN id and Cabrillo token.
use tempo_core::contest::{ContestSession, StationData};
use tempo_core::fd_rules::custom::{self, CustomDef, CustomField, CustomWindow, FtActivity};
use tempo_core::fd_rules::{
    install_custom, remove_custom, ruleset_by_contest_id, ruleset_by_id, ruleset_ids,
    CURRENT_RULES_YEAR,
};
use tempo_core::fieldday::{FieldDayLog, FtMessages};

/// 2026-11-14T00:00Z.
const NOW: u64 = 1_794_614_400;

fn def(id: &str, token: &str, fields: &[&str]) -> CustomDef {
    CustomDef {
        format: custom::FORMAT.into(),
        version: custom::VERSION,
        id: id.into(),
        name: format!("Test {id}"),
        short: String::new(),
        cabrillo_contest: token.into(),
        modes: vec!["CW".into(), "SSB".into(), "FT8".into()],
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
        ft_roundup: false,
        dupe: "band".into(),
        points: Default::default(),
    }
}

fn station() -> StationData {
    StationData {
        mycall: "W9XYZ".into(),
        mygrid: "EN52".into(),
        contest_qth_state: "IL".into(),
        contest_category_operator: "SINGLE-OP".into(),
        ..Default::default()
    }
}

fn install(d: &CustomDef) -> &'static tempo_core::fd_rules::FdRuleset {
    let refused = install_custom(std::slice::from_ref(d), NOW);
    assert!(refused.is_empty(), "{refused:?}");
    ruleset_by_id(&d.id, CURRENT_RULES_YEAR).expect("installed")
}

fn log_of(rs: &'static tempo_core::fd_rules::FdRuleset, band: &str) -> FieldDayLog {
    let session = ContestSession::for_ruleset(rs, &station()).expect("a session starts");
    FieldDayLog::new("W9XYZ", session, band)
}

/// The WAE shape — RST and a serial, once per band — compiles, installs, is found by its id and
/// its `CONTEST_ID`, and exports the structural three-row Cabrillo file.
#[test]
fn a_wae_shaped_contest_installs_and_exports_its_cabrillo_file() {
    let mut d = def(
        "custom_cabrillo_golden",
        "CLUB-WAE-PRACTICE",
        &["rst", "serial"],
    );
    d.modes = vec!["CW".into()];
    let rs = install(&d);
    assert!(rs.custom);
    assert!(ruleset_ids().contains(&"custom_cabrillo_golden"));
    assert!(std::ptr::eq(
        ruleset_by_contest_id("club-wae-practice", CURRENT_RULES_YEAR).expect("by CONTEST_ID"),
        rs
    ));
    let mut log = log_of(rs, "20m");
    for (i, (call, band, nr)) in [
        ("K1ABC", "20m", "1"),
        ("DL1ABC", "20m", "2"),
        ("JA1ABC", "40m", "3"),
    ]
    .iter()
    .enumerate()
    {
        log.band = band.to_string();
        let fields = [
            ("RST".to_string(), "599".to_string()),
            ("NR".to_string(), nr.to_string()),
        ];
        assert!(log.log_fields_at(call, &fields, "CW", "", 0, 1_790_380_800 + 60 * i as u64));
    }
    let got = log.cabrillo(14_000).expect("exports");
    let want = std::fs::read_to_string("tests/fixtures/cabrillo-goldens/custom_wae_shaped.cbr")
        .expect("golden");
    assert_eq!(got, want);
}

/// Re-installing an id replaces the entry, and a `&'static` taken before is still the ruleset it
/// was; removing it makes the id unknown.
#[test]
fn a_reinstall_replaces_the_entry_and_an_earlier_borrow_is_unchanged() {
    let d = def("custom_reinstall", "CLUB-REINSTALL", &["rst", "serial"]);
    let before = install(&d);
    let mut d2 = d.clone();
    d2.exchange.push(CustomField {
        field: "cq_zone".into(),
        label: String::new(),
    });
    let after = install(&d2);
    assert!(!std::ptr::eq(before, after));
    assert_eq!(
        before.exchange.fields.len(),
        2,
        "the old borrow is the old ruleset"
    );
    assert_eq!(after.exchange.fields.len(), 3);
    assert_eq!(
        ruleset_ids()
            .iter()
            .filter(|id| **id == "custom_reinstall")
            .count(),
        1,
        "one entry, replaced"
    );
    assert!(remove_custom("custom_reinstall"));
    assert!(ruleset_by_id("custom_reinstall", CURRENT_RULES_YEAR).is_none());
    assert!(!remove_custom("custom_reinstall"));
}

/// Two of the operator's contests cannot share a Cabrillo token: a journal finds its contest by it.
#[test]
fn a_second_custom_contest_may_not_reuse_a_token() {
    install(&def("custom_token_a", "CLUB-SHARED", &["rst"]));
    let refused = install_custom(&[def("custom_token_b", "CLUB-SHARED", &["rst"])], NOW);
    assert_eq!(refused.len(), 1, "{refused:?}");
    assert!(
        refused[0]
            .1
            .contains("already used by your contest \"custom_token_a\""),
        "{refused:?}"
    );
    assert!(ruleset_by_id("custom_token_b", CURRENT_RULES_YEAR).is_none());
}

/// `once`, `band` and `band_mode` dupe as named.
#[test]
fn the_three_dupe_rules_behave_as_named() {
    for (dupe, same_band, other_band, other_mode) in [
        ("once", true, true, true),
        ("band", true, false, true),
        ("band_mode", true, false, false),
    ] {
        let id = format!("custom_dupe_{dupe}");
        let mut d = def(
            &id,
            &format!("CLUB-DUPE-{}", dupe.to_uppercase().replace('_', "-")),
            &["rst"],
        );
        d.dupe = dupe.into();
        let rs = install(&d);
        let mut log = log_of(rs, "20m");
        let f = [("RST".to_string(), "599".to_string())];
        assert!(log.log_fields_at("K1ABC", &f, "CW", "", 0, 1_790_380_800));
        assert_eq!(
            log.is_dupe_mode("K1ABC", "CW"),
            same_band,
            "{dupe}: same band"
        );
        assert_eq!(
            log.is_dupe_mode("K1ABC", "PH"),
            other_mode,
            "{dupe}: other mode"
        );
        log.band = "40m".into();
        assert_eq!(
            log.is_dupe_mode("K1ABC", "CW"),
            other_band,
            "{dupe}: other band"
        );
    }
}

/// Points per mode class and nothing else: 2 CW + 1 SSB at `{CW: 2, PH: 1}` is 5.
#[test]
fn points_are_per_mode_class_with_no_multiplier() {
    let mut d = def("custom_points", "CLUB-POINTS", &["rst"]);
    d.points = custom::CustomPoints {
        cw: 2,
        ph: 1,
        dig: 1,
    };
    let rs = install(&d);
    let mut log = log_of(rs, "20m");
    for (i, (call, mode, rst)) in [
        ("K1ABC", "CW", "599"),
        ("K2ABC", "CW", "599"),
        ("K3ABC", "PH", "59"),
    ]
    .iter()
    .enumerate()
    {
        let f = [("RST".to_string(), rst.to_string())];
        assert!(log.log_fields_at(call, &f, mode, "", 0, 1_790_380_800 + i as u64));
    }
    let (qso_points, total) = rs.scoring.qso_and_powered(log.score_rows(), 1);
    assert_eq!((qso_points, total), (5, 5));
    assert!(rs.scoring.multipliers.is_empty());
}

/// ⚠️ **FT REGRESSION PIN — FT8/FT4 send standard messages for EVERY custom contest**, the
/// FT-shaped ones included, and whichever FT mode is or is not ticked. Changing what goes on the
/// air for a custom contest is a separately signed-off change, and it changes THIS test on purpose.
#[test]
fn every_custom_contest_sends_standard_messages_on_ft8_and_ft4() {
    let shapes: &[(&str, &[&str], Option<&str>, bool)] = &[
        ("custom_ft_fd", &["fd_class", "arrl_section"], None, false),
        ("custom_ft_navhf", &["grid4"], Some("TEST"), false),
        ("custom_ft_ww", &["grid4"], Some("WW"), false),
        ("custom_ft_ru_serial", &["rst", "serial"], None, true),
        (
            "custom_ft_ru_state",
            &["rst", "state_province"],
            None,
            false,
        ),
        ("custom_ft_euvhf", &["rst", "serial", "grid6"], None, false),
        ("custom_ft_none", &["rst", "cq_zone"], None, false),
    ];
    for (i, (id, fields, cq, roundup)) in shapes.iter().enumerate() {
        for modes in [&["FT8", "FT4"][..], &["CW"][..]] {
            let mut d = def(id, &format!("CLUB-FT-{i}"), fields);
            d.ft_cq = cq.map(str::to_string);
            d.ft_roundup = *roundup;
            d.modes = modes.iter().map(|m| m.to_string()).collect();
            let rs = install(&d);
            for mode in ["FT8", "FT4"] {
                assert_eq!(
                    FtMessages::for_contest(rs, mode),
                    FtMessages::Standard,
                    "{id} on {mode}, ticked {modes:?}"
                );
            }
        }
    }
    // The shape IS computed — it is only not read on the air.
    let mut d = def("custom_ft_navhf", "CLUB-FT-1", &["grid4"]);
    d.ft_cq = Some("TEST".into());
    d.modes = vec!["FT8".into()];
    assert_eq!(install(&d).ft_activity, Some(FtActivity::NaVhf));
}

/// A one-off contest's window is its one date in every year: past it reads as past, never as next
/// year's running.
#[test]
fn a_one_off_contest_never_runs_again() {
    let mut d = def("custom_once", "CLUB-ONCE", &["rst"]);
    d.window = CustomWindow::Once {
        start_utc: "2026-12-05T14:00Z".into(),
        duration_hours: 4,
    };
    let rs = install(&d);
    let w = rs.event_window(2026);
    assert_eq!(
        (w.start_unix, w.end_unix),
        (1_796_479_200, 1_796_479_200 + 4 * 3600)
    );
    assert_eq!(rs.event_window(2027), w);
    assert_eq!(rs.next_or_running(w.end_unix + 86_400 * 400), w);
}

/// ⭐ **The values an operator types reach the exchange they send** — and a blank one, a number
/// out of range or a text too long refuses the session, so nothing goes on the air.
#[test]
fn typed_values_are_sent_and_a_blank_or_bad_one_refuses_the_session() {
    let d = def(
        "custom_typed",
        "CLUB-TYPED",
        &["rst", "age", "name", "text", "text"],
    );
    let rs = install(&d);
    assert_eq!(rs.entered_slots(), ["AGE", "NAME", "TXT1", "TXT2"]);
    assert!(
        rs.sent_settings().is_empty(),
        "a typed value is not a station setting"
    );
    let with = |pairs: &[(&str, &str)]| StationData {
        entered: pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        ..station()
    };
    let full = [
        ("AGE", "42"),
        ("NAME", "alex"),
        ("TXT1", "w9 club"),
        ("TXT2", "7"),
    ];
    let s = ContestSession::for_ruleset(rs, &with(&full)).expect("starts");
    let sent: Vec<&str> = s.my_exchange.iter().map(|v| v.raw.as_str()).collect();
    assert_eq!(sent, ["599", "42", "ALEX", "W9 CLUB", "7"]);
    for (pairs, want) in [
        (&full[1..], "Your AGE is empty"),
        (
            &[("AGE", "142"), full[1], full[2], full[3]][..],
            "AGE is \"142\" — it must be a number from 0 to 99",
        ),
        (
            &[("AGE", "4x"), full[1], full[2], full[3]][..],
            "must be a number from 0 to 99",
        ),
        (
            &[full[0], ("NAME", "ALEXANDRA-JANE"), full[2], full[3]][..],
            "Your NAME is longer than 12 characters",
        ),
    ] {
        let err = ContestSession::for_ruleset(rs, &with(pairs)).expect_err("refused");
        assert!(err.contains(want), "{err}");
    }
    assert!(ContestSession::for_ruleset(rs, &with(&full[..3]))
        .expect_err("a blank text field refuses")
        .contains("Fill it in when you set up the contest"),);
}
