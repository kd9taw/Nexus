//! ⭐ **CQ WW RTTY through the ENGINE** — the wiring half of what
//! `tempo-core/tests/cqww_rtty.rs` pins on the session and the log itself: the dial the rig
//! was on reaches each QSO line, the entrant's NAME and EMAIL come from their own settings at
//! export time, and the snapshot carries what the entry strip shows (the contest's advisory
//! bands, and the warning a W/VE call gets before it sends the DX exchange).
//!
//! Own process: CQ WW RTTY prices every contact by the relation between two stations, so a
//! session needs a country file, and `install_call_resolver` is process-wide. The stub below
//! knows the example calls used here and nothing else; src-tauri installs the real cty.dat
//! resolver and pins its entity spellings.

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

/// ⭐ **The strip's W/VE warning rides the snapshot.** A US call with no contest state set
/// is about to send the DX exchange (no QTH); the session says so, and the snapshot carries
/// it — what was typed, and the listed codes it most likely means — for the strip to word.
#[test]
fn the_snapshot_carries_the_location_warning_and_only_when_it_applies() {
    let mut e = cqww_rtty_engine();
    assert!(
        e.snapshot()
            .field_day
            .expect("in the contest")
            .location_warning
            .is_none(),
        "IL is a listed QTH"
    );
    // A running session keeps the location it started with (it is what is on the air), so
    // a changed state takes effect when the contest is entered again.
    let mut s = e.settings().clone();
    s.contest_qth_state = String::new();
    e.apply_settings(s);
    e.set_mode("chat").expect("leave the contest");
    e.set_mode("fieldday-sp")
        .expect("a warning is not a refusal: the contest still starts");
    let w = e
        .snapshot()
        .field_day
        .expect("in the contest")
        .location_warning
        .expect("a US call with no state is warned");
    assert_eq!(w.typed, "");
    assert!(w.hints.is_empty());
    // An ARRL section gets the state it is in as a hint.
    let mut s = e.settings().clone();
    s.contest_qth_state = "EMA".into();
    e.apply_settings(s);
    e.set_mode("chat").expect("leave the contest");
    e.set_mode("fieldday-sp").expect("still starts");
    let w = e
        .snapshot()
        .field_day
        .expect("in the contest")
        .location_warning
        .expect("a section is warned");
    assert_eq!((w.typed.as_str(), w.hints), ("EMA", vec!["MA".to_string()]));
}

/// ⭐ **The state a station SENT wins over the one its call suggests, all the way to the
/// World Radio League body.** The merge fills a contact's state from the callsign resolver
/// (the FCC licensee's mailing address, or the province a call area names) only where the
/// contact carries none. The QTH in the exchange is where the station says it is operating,
/// so the merge writes it first.
///
/// The record checked is the one the upload queue holds, put through the same DTO round
/// trip the upload worker makes before the WRL leg builds its body.
#[test]
fn a_merged_contact_keeps_the_state_it_sent_over_the_resolvers_guess() {
    let mut e = cqww_rtty_engine();
    // The resolver answers NY for every US call: a mailing address, not a location.
    e.set_state_resolver(|call, _grid| call.starts_with('W').then(|| "NY".to_string()));
    e.set_frequency(14.0842, "20m", "USB");
    let ma = fields(&[("RST", "579"), ("ZN", "5"), ("QTH", "MA")]);
    assert!(e
        .contest_log_manual("W1ABC", &ma, "DIG", Some("RTTY"))
        .unwrap());
    // POSITIVE CONTROL: a US station logged without its QTH. The resolver's answer fills it,
    // so the resolver does run on merged rows and the MA above had to beat it.
    let no_qth = fields(&[("RST", "599"), ("ZN", "5")]);
    assert!(e
        .contest_log_manual("W2DEF", &no_qth, "DIG", Some("RTTY"))
        .unwrap());
    e.fd_set_upload(true, vec!["wrl".into()]).unwrap();
    e.fd_merge_to_general().expect("in the contest");

    let queued = e.take_pending_uploads();
    let rec = |call: &str| {
        queued
            .iter()
            .find(|p| p.rec.call == call)
            .map(|p| p.rec.clone())
            .unwrap_or_else(|| panic!("{call} was queued for WRL"))
    };
    assert_eq!(
        rec("W1ABC").state.as_deref(),
        Some("MA"),
        "the exchange wins"
    );
    assert_eq!(
        rec("W2DEF").state.as_deref(),
        Some("NY"),
        "the resolver fills a gap"
    );

    // The body the WRL leg sends, built from the record as the upload worker hands it over.
    let pushed: tempo_core::logbook::QsoRecord =
        tempo_app::dto::LoggedQso::from(rec("W1ABC")).into();
    let body = tempo_core::wrl::build_contact_json(&pushed, "W9XYZ", None);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["state"], "MA", "{body}");
    assert_eq!(v["rstSent"], "599", "{body}");
    assert_eq!(v["rstRcvd"], "579", "{body}");
}
