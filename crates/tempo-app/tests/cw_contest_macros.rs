//! ⭐ **What the CW cockpit's built-in contest F2 and F6 key, contest by contest**, through
//! the engine's own expander and the shipped rules. The sets are N1MM's contest layout, which
//! carries the exchange on F2 (his call and the exchange) and F6 (the exchange in S&P). The
//! templates are read out of `features/esmRoles.ts`, which `CwCockpit.tsx` sends, and the set is
//! chosen the way its `builtIn` chooses it: the one with `{RST}` when the session's received
//! slots carry an `rst` one, the one without when they do not. `{EXCH}` is the exchange WITHOUT
//! the report, which `{RST}` keys.
//!
//! The contest set once had no `{RST}`, so the Illinois QSO Party's exchange keyed
//! `K9AAA DE W9XYZ COOK COOK K` where the sponsor's exchange is RS(T) and county. And a 5NN
//! where the exchange has no report is a wrong exchange: Sweepstakes would copy it as the
//! serial.
//!
//! Own process: CQ WW and CQ WPX price every contact by the relation between two stations,
//! so their sessions need a country file, and `install_call_resolver` is process-wide. The
//! stub below places the two calls used here and nothing else.

use tempo_app::engine::Engine;
use tempo_core::contest::{install_call_resolver, CallLocation};

const LAYOUTS: &str = include_str!("../../../ui/src/features/esmRoles.ts");

fn place(call: &str) -> Option<CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    (up.starts_with('W') || up.starts_with('K')).then_some(CallLocation {
        entity: "United States",
        continent: "NA",
        cq_zone: None,
    })
}

/// Install the stub country file once for this test binary.
fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(place).expect("the first install in this test binary");
    });
}

/// One built-in macro's text, by set and key, as `esmRoles.ts` declares it.
fn macro_text(set: &str, key: &str) -> String {
    let decl = format!("export const {set}: BuiltinMacro[] = [");
    let at = LAYOUTS
        .find(&decl)
        .unwrap_or_else(|| panic!("esmRoles.ts declares no {set}"));
    let body = &LAYOUTS[at..];
    let body = &body[..body.find("\n]").expect("the set closes")];
    let row = body
        .lines()
        .find(|l| l.contains(&format!("key: '{key}'")))
        .unwrap_or_else(|| panic!("{set} has no {key}"));
    let text = row.split("text: '").nth(1).expect("a one-line row");
    text[..text.find('\'').expect("the text closes")].to_string()
}

/// (contest, what F2 keys to K9AAA, what F6 keys) for W9XYZ in Cook County, Illinois, CQ
/// zone 4, at EN52, check 05, section IL, single operator at low power.
const CASES: [(&str, &str, &str); 16] = [
    ("ilqp", "K9AAA 5NN COOK", "TU 5NN COOK"),
    ("tnqp", "K9AAA 5NN IL", "TU 5NN IL"),
    ("ohqp", "K9AAA 5NN IL", "TU 5NN IL"),
    ("txqp", "K9AAA 5NN IL", "TU 5NN IL"),
    ("nyqp", "K9AAA 5NN IL", "TU 5NN IL"),
    ("cqww_cw", "K9AAA 5NN 4", "TU 5NN 4"),
    ("cqww_ssb", "K9AAA 5NN 4", "TU 5NN 4"),
    ("cqww_rtty", "K9AAA 5NN 4 IL", "TU 5NN 4 IL"),
    ("cqwpx_cw", "K9AAA 5NN 1", "TU 5NN 1"),
    ("cqwpx_ssb", "K9AAA 5NN 1", "TU 5NN 1"),
    ("arrlss_cw", "K9AAA 1 A W9XYZ 05 IL", "TU 1 A W9XYZ 05 IL"),
    ("arrlss_ssb", "K9AAA 1 A W9XYZ 05 IL", "TU 1 A W9XYZ 05 IL"),
    ("cqp", "K9AAA 1 IL", "TU 1 IL"),
    ("arrlvhf_jan", "K9AAA EN52", "TU EN52"),
    ("arrlvhf_jun", "K9AAA EN52", "TU EN52"),
    ("arrlvhf_sep", "K9AAA EN52", "TU EN52"),
];

#[test]
fn every_shipped_contest_but_field_day_is_listed() {
    // A ruleset added to the seed is a red here until somebody writes down what its F2 keys.
    let seed: serde_json::Value =
        serde_json::from_str(include_str!("../../tempo-core/src/fd_rules.seed.json"))
            .expect("the seed parses");
    let mut shipped: Vec<&str> = seed["rulesets"]
        .as_array()
        .expect("rulesets")
        .iter()
        .filter_map(|r| r["event"].as_str())
        .filter(|id| !matches!(*id, "arrlfd" | "wfd"))
        .collect();
    shipped.sort_unstable();
    let mut listed: Vec<&str> = CASES.iter().map(|c| c.0).collect();
    listed.sort_unstable();
    assert_eq!(listed, shipped);
}

#[test]
fn f2_and_f6_key_each_contests_exchange_with_the_report_where_it_has_one() {
    resolver();
    for (event, f2, f6) in CASES {
        let mut e = Engine::new("W9XYZ", "EN52", 0);
        let mut s = e.settings().clone();
        s.fd_active = true;
        s.fd_event = event.into();
        s.contest_qth_state = "IL".into();
        s.contest_qth_county = "COOK".into();
        s.contest_cq_zone = 4;
        s.contest_check = "05".into();
        s.fd_section = "IL".into();
        s.contest_category_operator = "SINGLE-OP".into();
        s.contest_category_power = "LOW".into();
        s.contest_category_assisted = "NON-ASSISTED".into();
        e.apply_settings(s);
        e.set_mode("fieldday-sp")
            .unwrap_or_else(|err| panic!("{event} builds: {err}"));
        e.select_peer("K9AAA");
        let fd = e.snapshot().field_day.expect("the contest runs");
        let set = if fd.receives.iter().any(|f| f.kind == "rst") {
            "CW_CONTEST_LAYOUT"
        } else {
            "CW_CONTEST_NO_REPORT_LAYOUT"
        };
        assert_eq!(
            (
                e.preview_cw(&macro_text(set, "F2")),
                e.preview_cw(&macro_text(set, "F6"))
            ),
            (f2.to_string(), f6.to_string()),
            "{event} keys {set}"
        );
    }
}

/// ⭐ **The signed table, key by key, through the engine's own expander**: what each built-in
/// key sends in the Illinois QSO Party from Cook County, and at ARRL Field Day as 3A WI,
/// working K9AAA — the signature's "What they key", with the two Field Day texts beside it.
#[test]
fn every_key_sends_the_signed_text_in_the_illinois_qso_party_and_at_field_day() {
    resolver();
    let keys = ["F1", "F2", "F3", "F4", "F5", "F6", "F7", "F8"];
    for (event, set, want) in [
        (
            "ilqp",
            "CW_CONTEST_LAYOUT",
            [
                "CQ TEST DE W9XYZ W9XYZ K",
                "K9AAA 5NN COOK",
                "TU W9XYZ",
                "W9XYZ",
                "K9AAA",
                "TU 5NN COOK",
                "AGN AGN",
                "K9AAA QSO B4",
            ],
        ),
        (
            "arrlfd",
            "CW_FIELD_DAY_LAYOUT",
            [
                "CQ FD DE W9XYZ W9XYZ K",
                "K9AAA 3A WI",
                "TU W9XYZ",
                "W9XYZ",
                "K9AAA",
                "TU 3A WI",
                "AGN AGN",
                "K9AAA QSO B4",
            ],
        ),
    ] {
        let mut e = Engine::new("W9XYZ", "EN52", 0);
        let mut s = e.settings().clone();
        s.fd_active = true;
        s.fd_event = event.into();
        s.contest_qth_state = "IL".into();
        s.contest_qth_county = "COOK".into();
        s.fd_class = "3A".into();
        s.fd_section = "WI".into();
        e.apply_settings(s);
        e.set_mode("fieldday-sp")
            .unwrap_or_else(|err| panic!("{event} builds: {err}"));
        e.select_peer("K9AAA");
        let keyed: Vec<String> = keys
            .iter()
            .map(|k| e.preview_cw(&macro_text(set, k)))
            .collect();
        assert_eq!(keyed, want, "{event} keys {set}");
    }
}
