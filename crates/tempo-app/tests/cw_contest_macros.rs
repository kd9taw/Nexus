//! ⭐ **What the CW cockpit's built-in contest F3 and F4 key, contest by contest**, through
//! the engine's own expander and the shipped rules. The templates are read out of
//! `CwCockpit.tsx` itself, and the set is chosen the way its `builtIn` chooses it: the one
//! with `{RST}` when the session's received slots carry an `rst` one, the one without when
//! they do not. `{EXCH}` is the exchange WITHOUT the report, which `{RST}` keys.
//!
//! The contest set had no `{RST}`, so the Illinois QSO Party's F3 keyed
//! `K9AAA DE W9XYZ COOK COOK K` where the sponsor's exchange is RS(T) and county. And a 5NN
//! where the exchange has no report is a wrong exchange: Sweepstakes would copy it as the
//! serial.
//!
//! Own process: CQ WW and CQ WPX price every contact by the relation between two stations,
//! so their sessions need a country file, and `install_call_resolver` is process-wide. The
//! stub below places the two calls used here and nothing else.

use tempo_app::engine::Engine;
use tempo_core::contest::{install_call_resolver, CallLocation};

const COCKPIT: &str = include_str!("../../../ui/src/components/CwCockpit.tsx");

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

/// One built-in macro's text, by set and key, as `CwCockpit.tsx` declares it.
fn macro_text(set: &str, key: &str) -> String {
    let decl = format!("export const {set}: CwMacro[] = [");
    let at = COCKPIT
        .find(&decl)
        .unwrap_or_else(|| panic!("CwCockpit.tsx declares no {set}"));
    let body = &COCKPIT[at..];
    let body = &body[..body.find("\n]").expect("the set closes")];
    let row = body
        .lines()
        .find(|l| l.contains(&format!("key: '{key}'")))
        .unwrap_or_else(|| panic!("{set} has no {key}"));
    let text = row.split("text: '").nth(1).expect("a one-line row");
    text[..text.find('\'').expect("the text closes")].to_string()
}

/// (contest, what F3 keys to K9AAA, what F4 keys) for W9XYZ in Cook County, Illinois, CQ
/// zone 4, at EN52, check 05, section IL, single operator at low power.
const CASES: [(&str, &str, &str); 16] = [
    (
        "ilqp",
        "K9AAA DE W9XYZ 5NN COOK COOK K",
        "K9AAA TU 5NN COOK DE W9XYZ K",
    ),
    (
        "tnqp",
        "K9AAA DE W9XYZ 5NN IL IL K",
        "K9AAA TU 5NN IL DE W9XYZ K",
    ),
    (
        "ohqp",
        "K9AAA DE W9XYZ 5NN IL IL K",
        "K9AAA TU 5NN IL DE W9XYZ K",
    ),
    (
        "txqp",
        "K9AAA DE W9XYZ 5NN IL IL K",
        "K9AAA TU 5NN IL DE W9XYZ K",
    ),
    (
        "nyqp",
        "K9AAA DE W9XYZ 5NN IL IL K",
        "K9AAA TU 5NN IL DE W9XYZ K",
    ),
    (
        "cqww_cw",
        "K9AAA DE W9XYZ 5NN 4 4 K",
        "K9AAA TU 5NN 4 DE W9XYZ K",
    ),
    (
        "cqww_ssb",
        "K9AAA DE W9XYZ 5NN 4 4 K",
        "K9AAA TU 5NN 4 DE W9XYZ K",
    ),
    (
        "cqww_rtty",
        "K9AAA DE W9XYZ 5NN 4 IL 4 IL K",
        "K9AAA TU 5NN 4 IL DE W9XYZ K",
    ),
    (
        "cqwpx_cw",
        "K9AAA DE W9XYZ 5NN 1 1 K",
        "K9AAA TU 5NN 1 DE W9XYZ K",
    ),
    (
        "cqwpx_ssb",
        "K9AAA DE W9XYZ 5NN 1 1 K",
        "K9AAA TU 5NN 1 DE W9XYZ K",
    ),
    (
        "arrlss_cw",
        "K9AAA DE W9XYZ 1 A W9XYZ 05 IL 1 A W9XYZ 05 IL K",
        "K9AAA TU 1 A W9XYZ 05 IL DE W9XYZ K",
    ),
    (
        "arrlss_ssb",
        "K9AAA DE W9XYZ 1 A W9XYZ 05 IL 1 A W9XYZ 05 IL K",
        "K9AAA TU 1 A W9XYZ 05 IL DE W9XYZ K",
    ),
    (
        "cqp",
        "K9AAA DE W9XYZ 1 IL 1 IL K",
        "K9AAA TU 1 IL DE W9XYZ K",
    ),
    (
        "arrlvhf_jan",
        "K9AAA DE W9XYZ EN52 EN52 K",
        "K9AAA TU EN52 DE W9XYZ K",
    ),
    (
        "arrlvhf_jun",
        "K9AAA DE W9XYZ EN52 EN52 K",
        "K9AAA TU EN52 DE W9XYZ K",
    ),
    (
        "arrlvhf_sep",
        "K9AAA DE W9XYZ EN52 EN52 K",
        "K9AAA TU EN52 DE W9XYZ K",
    ),
];

#[test]
fn every_shipped_contest_but_field_day_is_listed() {
    // A ruleset added to the seed is a red here until somebody writes down what its F3 keys.
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
fn f3_and_f4_key_each_contests_exchange_with_the_report_where_it_has_one() {
    resolver();
    for (event, f3, f4) in CASES {
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
            "DEFAULT_CONTEST_MACROS"
        } else {
            "DEFAULT_CONTEST_NO_REPORT_MACROS"
        };
        assert_eq!(
            (
                e.preview_cw(&macro_text(set, "F3")),
                e.preview_cw(&macro_text(set, "F4"))
            ),
            (f3.to_string(), f4.to_string()),
            "{event} keys {set}"
        );
    }
}
