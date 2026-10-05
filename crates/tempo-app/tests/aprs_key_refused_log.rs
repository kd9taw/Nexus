//! An APRS frame whose key the radio refuses did not go out, and the station says so. An automatic
//! ack is skipped AND LOGGED, as when the radio has the mic (operator ruling, 2026-10-04: "An
//! auto-ack that can't go out is skipped and logged"); the operator's own beacon is logged and named
//! on the APRS cockpit's status line. Neither is sent later.
//!
//! The ack is the app's only unattended transmission, so the diagnostic log is where an operator
//! asking "why did my station not ack?" finds the answer. This reads the log FILE back, not a
//! stand-in for it.
//!
//! A test binary of its own because the log is one per process (`tempo_core::applog::init` is a
//! one-shot): opening it here can reach no other suite.

use tempo_app::engine::{AprsArm, Engine};

#[test]
fn a_frame_whose_key_the_radio_refuses_is_logged_and_never_sent_later() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("aprs-key-refused-log-{}", std::process::id()))
        .join("nexus-diag.log");
    tempo_core::applog::init(path.clone());
    assert_eq!(
        tempo_core::applog::path(),
        Some(path.as_path()),
        "precondition: the log this test reads is the one the engine writes"
    );

    // Both operator acts behind the ack (Monitor armed by hand, TX on).
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    e.set_aprs_arm(AprsArm::Explicit);
    e.set_tx_enabled(true);

    // An automatic ack, handed to the radio loop, whose key the radio refuses.
    e.aprs_auto_ack("N0CALL-7", "W9XYZ", "042");
    assert!(e.poll_aprs_tx().is_some(), "the ack was handed over");
    e.aprs_key_refused("RPRT -1");
    assert_eq!(
        e.aprs_tx_notice(),
        None,
        "an unattended ack is logged, not put on the cockpit's status line"
    );
    assert!(e.poll_aprs_tx().is_none(), "the ack is not sent later");

    // The operator's own beacon, refused the same way.
    e.aprs_beacon(41.88, -87.63, '/', '>', "", &[])
        .expect("the beacon is queued");
    assert!(e.poll_aprs_tx().is_some(), "the beacon was handed over");
    e.aprs_key_refused("RPRT -1");
    assert!(
        e.aprs_tx_notice()
            .is_some_and(|n| n.starts_with("APRS not sent: the radio did not accept the key")),
        "{:?}",
        e.aprs_tx_notice()
    );
    assert!(e.poll_aprs_tx().is_none(), "the beacon is not sent later");

    // The control: an ack the radio keys is not logged as skipped.
    e.aprs_auto_ack("K1ABC", "W9XYZ", "043");
    assert!(e.poll_aprs_tx().is_some(), "the next ack is handed over");

    tempo_core::applog::flush();
    let log = std::fs::read_to_string(&path).expect("the log was written");
    let skipped: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("APRS auto-ack") && l.contains("skipped"))
        .collect();
    assert_eq!(skipped.len(), 1, "one skip, logged once: {log}");
    assert!(
        skipped[0].contains(
            "APRS auto-ack to N0CALL-7 skipped: the radio did not accept the key (RPRT -1)"
        ),
        "the line names the station and the reason: {}",
        skipped[0]
    );
    let dropped = log
        .lines()
        .filter(|l| l.contains("APRS not keyed: the radio did not accept the key (RPRT -1)"))
        .count();
    assert_eq!(dropped, 1, "the beacon's refusal, logged once: {log}");
}
