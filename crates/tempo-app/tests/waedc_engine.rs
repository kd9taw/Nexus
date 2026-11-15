//! ⭐ **The WAE DX Contest through the ENGINE** — what the CW and RTTY F-keys put on the air,
//! and what the entry strip takes.
//!
//! DARC's rules, §5: "The exchange consists of RS/RST and a progressive serial number starting
//! with 001. If the station worked does not send a serial number, log the contact with number
//! 000." So the keys send the report and this contact's serial, the serial moves on with every
//! contact logged, and a copied 000 is a contact like any other.
//!
//! The macro texts are read out of the shipped layouts (`features/esmRoles.ts` for the CW
//! cockpit, `features/rttyMacros.ts` for RTTY), so this tests what the cockpits send, through
//! the engine's own expander for CW and its `sent_exchange` for RTTY.
//!
//! Own process: `install_call_resolver` is process-wide. Every callsign is an example call.

use tempo_app::engine::Engine;
use tempo_core::contest::{install_call_resolver, CallLocation};

const CW_LAYOUTS: &str = include_str!("../../../ui/src/features/esmRoles.ts");
const RTTY_LAYOUTS: &str = include_str!("../../../ui/src/features/rttyMacros.ts");

fn resolver() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        install_call_resolver(|call| {
            let up = call.trim().to_ascii_uppercase();
            let (entity, continent) = if up.starts_with("DL") {
                ("Fed. Rep. of Germany", "EU")
            } else {
                ("United States", "NA")
            };
            Some(CallLocation {
                entity,
                continent,
                cq_zone: None,
            })
        })
        .expect("the first install in this test binary");
    });
}

/// One built-in macro's text, by the set's declaration and key.
fn macro_text(layouts: &str, decl: &str, key: &str) -> String {
    let at = layouts.find(decl).unwrap_or_else(|| panic!("no {decl}"));
    let body = &layouts[at..];
    let body = &body[..body.find("\n]").expect("the set closes")];
    let row = body
        .lines()
        .find(|l| l.contains(&format!("key: '{key}'")))
        .unwrap_or_else(|| panic!("{decl} has no {key}"));
    let text = row.split("text: '").nth(1).expect("a one-line row");
    text[..text.find('\'').expect("the text closes")].to_string()
}

fn engine(event: &str) -> Engine {
    resolver();
    let mut e = Engine::new("W9XYZ", "EN52", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = event.into();
    s.contest_category_operator = "SINGLE-OP".into();
    s.contest_category_power = "LOW".into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp")
        .unwrap_or_else(|err| panic!("{event} builds: {err}"));
    e
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

/// ⭐ **CW: F2 and F6 send the report and this contact's serial, and the serial moves on.**
#[test]
fn the_cw_keys_send_the_report_and_a_serial_that_counts_up() {
    let mut e = engine("waedc_cw");
    e.set_frequency(14.025, "20m", "CW");
    let f2 = macro_text(
        CW_LAYOUTS,
        "export const CW_CONTEST_LAYOUT: BuiltinMacro[] = [",
        "F2",
    );
    let f6 = macro_text(
        CW_LAYOUTS,
        "export const CW_CONTEST_LAYOUT: BuiltinMacro[] = [",
        "F6",
    );
    for (n, call) in ["DL1ABC", "K9AAA", "DL2ABC"].iter().enumerate() {
        e.select_peer(call);
        let nr = (n + 1).to_string();
        assert_eq!(
            e.preview_cw(&f2),
            format!("{call} 5NN {nr}"),
            "F2, contact {nr}"
        );
        assert_eq!(
            e.preview_cw(&f6),
            format!("TU 5NN {nr}"),
            "F6, contact {nr}"
        );
        assert!(e
            .contest_log_manual(call, &fields(&[("RST", "599"), ("NR", "5")]), "CW", None)
            .unwrap());
    }
    // The file carries the numbers that went out, three digits wide.
    let cab = e.export_log("cabrillo").expect("one entry");
    let sent: Vec<&str> = cab
        .lines()
        .filter(|l| l.starts_with("QSO:"))
        .map(|l| l.split(' ').nth(7).expect("the sent serial column"))
        .collect();
    assert_eq!(sent, ["001", "002", "003"], "{cab}");
}

/// ⭐ **RTTY: F2 sends `599` and the serial twice, and the serial moves on.** The RTTY cockpit
/// fills `{EXCH}` from the snapshot's `sent_exchange`; `{CALL}` and `{EXCH}` are filled here
/// as `expandRttyMacro` fills them.
#[test]
fn the_rtty_exchange_key_sends_599_and_a_serial_that_counts_up() {
    let mut e = engine("waedc_rtty");
    e.set_frequency(14.085, "20m", "USB");
    let f2 = macro_text(RTTY_LAYOUTS, "const CONTEST: BuiltinMacro[] = [", "F2");
    for (n, call) in ["DL1ABC", "K9AAA"].iter().enumerate() {
        e.contest_working(call).expect("a running session");
        let exch = e
            .snapshot()
            .field_day
            .expect("in the contest")
            .sent_exchange;
        let nr = (n + 1).to_string();
        assert_eq!(exch, nr, "contact {nr}");
        let keyed = f2.replace("{CALL}", call).replace("{EXCH}", &exch);
        assert_eq!(keyed, format!("{call} 599 {nr} {nr}"));
        assert!(e
            .contest_log_manual(
                call,
                &fields(&[("RST", "599"), ("NR", "12")]),
                "DIG",
                Some("RTTY")
            )
            .unwrap());
    }
}

/// ⭐ **A copied 000 is logged.** §5: "If the station worked does not send a serial number, log
/// the contact with number 000." The entry strip's write takes it, and the file writes it.
#[test]
fn a_copied_serial_of_000_is_logged_from_the_entry_strip() {
    for (event, mode, rst) in [("waedc_cw", "CW", "599"), ("waedc_ssb", "PH", "59")] {
        let mut e = engine(event);
        e.set_frequency(14.025, "20m", "CW");
        assert!(
            e.contest_log_manual(
                "DL1ABC",
                &fields(&[("RST", rst), ("NR", "000")]),
                mode,
                None
            )
            .unwrap(),
            "{event}"
        );
        let cab = e.export_log("cabrillo").expect("one entry");
        let line = cab
            .lines()
            .find(|l| l.starts_with("QSO:"))
            .expect("a QSO line");
        assert!(
            line.ends_with(&format!("DL1ABC {rst} 000")),
            "{event}: {line}"
        );
    }
}
