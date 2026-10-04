//! An automatic APRS ack that can't go out while the radio has the mic is skipped AND LOGGED
//! (operator ruling, 2026-10-04: "An auto-ack that can't go out is skipped and logged").
//!
//! The ack is the app's only unattended transmission, so nobody is watching the APRS cockpit when
//! it is skipped: the diagnostic log is where an operator asking "why did my station not ack?"
//! finds the answer, beside the APRS lines the transmit path already writes there. This reads the
//! log FILE back, not a stand-in for it.
//!
//! A test binary of its own because the log is one per process (`tempo_core::applog::init` is a
//! one-shot): opening it here can reach no other suite.

use tempo_app::engine::{AprsArm, Engine};

#[test]
fn an_auto_ack_skipped_while_the_radio_has_the_mic_is_logged() {
    let path = std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join(format!("aprs-ack-skip-log-{}", std::process::id()))
        .join("nexus-diag.log");
    tempo_core::applog::init(path.clone());
    assert_eq!(
        tempo_core::applog::path(),
        Some(path.as_path()),
        "precondition: the log this test reads is the one the engine writes"
    );

    // Both operator acts behind the ack (Monitor armed by hand, TX on), on a radio with the mic.
    let mut e = Engine::new("W9XYZ", "EN61", 0);
    e.set_aprs_arm(AprsArm::Explicit);
    e.set_tx_enabled(true);
    e.observe_flex_radio_has_mic(true);
    e.aprs_auto_ack("N0CALL-7", "W9XYZ", "042");
    assert!(e.poll_aprs_tx().is_none(), "nothing was queued");

    // The control: off the mic, the same ack is queued, and no skip is logged for it.
    e.observe_flex_radio_has_mic(false);
    e.aprs_auto_ack("K1ABC", "W9XYZ", "043");
    assert!(e.poll_aprs_tx().is_some(), "off the mic, the ack is queued");

    tempo_core::applog::flush();
    let log = std::fs::read_to_string(&path).expect("the log was written");
    let skipped: Vec<&str> = log
        .lines()
        .filter(|l| l.contains("APRS auto-ack") && l.contains("skipped"))
        .collect();
    assert_eq!(skipped.len(), 1, "one skip, logged once: {log}");
    assert!(
        skipped[0].contains("APRS auto-ack to N0CALL-7 skipped: the radio has the mic"),
        "the line names the station and the reason: {}",
        skipped[0]
    );
}
