//! A contest's Cabrillo EXPORT through the engine — the wiring half of what
//! `tempo-core/tests/cqww_rtty.rs` pins on the log itself: the dial the rig was on when
//! each contact was logged reaches its QSO line, and the entrant's NAME and EMAIL come
//! from their own settings at export time. Also the snapshot's advisory band list, which
//! needs the same relation-priced session to exist.
//!
//! Own process: CQ WW RTTY prices every contact by the relation between two stations, so
//! a session needs a country file, and `install_call_resolver` is process-wide. The stub
//! below knows the example calls used here and nothing else; src-tauri installs the real
//! cty.dat resolver.

use tempo_app::engine::Engine;
use tempo_core::contest::{install_call_resolver, CallLocation};

fn place(call: &str) -> Option<CallLocation> {
    let up = call.trim().to_ascii_uppercase();
    let (entity, continent) = if up.starts_with("VE") {
        ("Canada", "NA")
    } else if up.starts_with("JA") {
        ("Japan", "AS")
    } else if up.starts_with('W') || up.starts_with('K') {
        ("United States", "NA")
    } else {
        return None;
    };
    Some(CallLocation {
        entity,
        continent,
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

/// An engine running CQ WW RTTY from Illinois.
fn cqww_rtty_engine() -> Engine {
    resolver();
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    let mut s = e.settings().clone();
    s.fd_active = true;
    s.fd_event = "cqww_rtty".into();
    s.contest_cq_zone = 4;
    s.contest_qth_state = "IL".into();
    s.contest_category_power = "LOW".into();
    s.contest_category_assisted = "NON-ASSISTED".into();
    s.op_name = "EXAMPLE OPERATOR".into();
    s.contest_email = "op@example.com".into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp")
        .expect("a CQ WW RTTY session builds");
    e
}

fn fields(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn qso_lines(cab: &str) -> Vec<&str> {
    cab.lines().filter(|l| l.starts_with("QSO:")).collect()
}

#[test]
fn the_export_writes_the_dial_and_the_entrants_name_and_email() {
    let mut e = cqww_rtty_engine();

    e.set_frequency(14.0842, "20m", "USB");
    let ve3 = fields(&[("RST", "599"), ("ZN", "4"), ("QTH", "ON")]);
    assert!(e
        .contest_log_manual("VE3XYZ", &ve3, "DIG", Some("RTTY"))
        .unwrap());
    // A QSY between contacts: the next row carries the NEW dial.
    e.set_frequency(7.0812, "40m", "USB");
    let ja = fields(&[("RST", "599"), ("ZN", "25")]);
    assert!(e
        .contest_log_manual("JA1ABC", &ja, "DIG", Some("RTTY"))
        .unwrap());

    let cab = e.export_log("cabrillo").expect("one entry");
    let lines = qso_lines(&cab);
    assert_eq!(lines.len(), 2, "{cab}");
    assert!(lines[0].starts_with("QSO: 14084 RY "), "{}", lines[0]);
    assert!(lines[1].starts_with("QSO: 7081 RY "), "{}", lines[1]);
    for line in ["NAME: EXAMPLE OPERATOR", "EMAIL: op@example.com"] {
        assert!(
            cab.lines().any(|l| l == line),
            "missing {line:?} in:\n{cab}"
        );
    }

    // Read at EXPORT: a corrected address reaches the next file with no new session.
    let mut s = e.settings().clone();
    s.contest_email = "fixed@example.com".into();
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("the session comes back");
    let cab = e.export_log("cabrillo").expect("one entry");
    assert!(
        cab.lines().any(|l| l == "EMAIL: fixed@example.com"),
        "{cab}"
    );

    // CONTROL: an empty address is not a header.
    let mut s = e.settings().clone();
    s.contest_email = String::new();
    e.apply_settings(s);
    e.set_mode("fieldday-sp").expect("the session comes back");
    let cab = e.export_log("cabrillo").expect("one entry");
    assert!(!cab.contains("EMAIL:"), "{cab}");
}

/// ⭐ **The contest's bands reach the entry strip, as advice.** CQ WW RTTY II: *"Five bands
/// only: 3.5, 7, 14, 21 and 28 MHz."* The snapshot carries the ruleset's list so the strip
/// can say the rig is elsewhere; nothing refuses a contact over it.
#[test]
fn the_snapshot_carries_the_contests_advisory_bands() {
    let mut e = cqww_rtty_engine();
    let fd = e.snapshot().field_day.expect("in the contest");
    assert_eq!(fd.bands, vec!["80m", "40m", "20m", "15m", "10m"]);
    // Advice, not a gate: a contact on 30 m still logs.
    e.set_frequency(10.142, "30m", "USB");
    let ja = fields(&[("RST", "599"), ("ZN", "25")]);
    assert!(e
        .contest_log_manual("JA1ABC", &ja, "DIG", Some("RTTY"))
        .unwrap());

    // CONTROL: Field Day names no band list, so its snapshot carries none.
    let mut f = Engine::new("W9XYZ", "EN61", 0);
    let mut s = f.settings().clone();
    s.fd_active = true;
    s.fd_class = "3A".into();
    s.fd_section = "WI".into();
    f.apply_settings(s);
    f.set_mode("fieldday-sp").expect("Field Day builds");
    assert!(f
        .snapshot()
        .field_day
        .expect("in Field Day")
        .bands
        .is_empty());
}
