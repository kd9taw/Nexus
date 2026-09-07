//! §8(a): ARRL FD and Winter FD keep working, byte for byte, through the contest
//! refactor. These bytes were captured from HEAD by `examples/capture_fd_goldens.rs`
//! BEFORE batch 0 moved anything; every batch re-runs this test unchanged.
//!
//! If one of these goes red, the batch that reddened it changed shipped Field Day
//! behaviour. Re-capturing the golden is never the fix.
// The fixture builder lives in the capture arm so the bytes and the builder can never
// drift apart. `main` — the capture arm's own entry point — is dead here by
// construction, and re-exporting it would be worse than allowing it.
#[allow(dead_code)]
#[path = "../examples/capture_fd_goldens.rs"]
mod capture;

use tempo_core::fd_rules::{ruleset, CURRENT_RULES_YEAR};
use tempo_core::fieldday::FdEvent;

const ARRLFD_CBR: &str = include_str!("fixtures/fd-goldens/arrlfd.cbr");
const ARRLFD_ADI: &str = include_str!("fixtures/fd-goldens/arrlfd.adi");
const WFD_CBR: &str = include_str!("fixtures/fd-goldens/wfd.cbr");
const WFD_ADI: &str = include_str!("fixtures/fd-goldens/wfd.adi");

#[test]
fn arrl_field_day_cabrillo_and_adif_are_byte_identical() {
    let log = capture::golden_log(FdEvent::ArrlFd);
    assert_eq!(log.cabrillo(14_074), ARRLFD_CBR, "ARRL FD Cabrillo moved");
    assert_eq!(log.adif(), ARRLFD_ADI, "ARRL FD ADIF moved");
}

#[test]
fn winter_field_day_cabrillo_and_adif_are_byte_identical() {
    let log = capture::golden_log(FdEvent::WinterFd);
    assert_eq!(log.cabrillo(14_074), WFD_CBR, "Winter FD Cabrillo moved");
    assert_eq!(log.adif(), WFD_ADI, "Winter FD ADIF moved");
}

#[test]
fn both_events_score_exactly_what_head_scored() {
    // The six numbers below are what `examples/capture_fd_goldens.rs` PRINTED at
    // 82eb3112, pinned as literals on purpose: a golden a later batch can regenerate
    // is not a golden. Hand-checked against the fixture: 2 CW rows x 2 pts + 3 phone
    // rows x 1 pt + 2 digital rows x 2 pts = 11 QSO points. ARRL FD multiplies by the
    // legal 5x power tier (55); WFD's Objectives model applies no on-air power
    // multiplier, so powered == qso. Bonus = w1aw-bulletin + web-submission.
    for (event, want_qso, want_powered, want_bonus) in [
        (FdEvent::ArrlFd, 11u32, 55u32, 150u32),
        (FdEvent::WinterFd, 11u32, 11u32, 150u32),
    ] {
        let log = capture::golden_log(event);
        let rs = ruleset(event, CURRENT_RULES_YEAR);
        let (qso, powered) = rs.scoring.qso_and_powered(&log, 5);
        let bonus = rs.bonus_points(&["w1aw-bulletin".to_string(), "web-submission".to_string()]);
        assert_eq!(
            (qso, powered, bonus),
            (want_qso, want_powered, want_bonus),
            "{event:?}"
        );
    }
}

/// POSITIVE CONTROL — a byte comparison that cannot fail is not evidence. Dropping the
/// last fixture row MUST change all four artifacts; if it does not, the assertions above
/// are passing vacuously (wrong fixture, empty file, a golden of the empty string).
#[test]
fn the_goldens_discriminate() {
    for (event, cbr, adi) in [
        (FdEvent::ArrlFd, ARRLFD_CBR, ARRLFD_ADI),
        (FdEvent::WinterFd, WFD_CBR, WFD_ADI),
    ] {
        let short = capture::golden_log_n(event, 6);
        assert_ne!(
            short.cabrillo(14_074),
            cbr,
            "{event:?}: Cabrillo golden is vacuous"
        );
        assert_ne!(short.adif(), adi, "{event:?}: ADIF golden is vacuous");
        assert!(
            !cbr.is_empty() && !adi.is_empty(),
            "{event:?}: empty golden file"
        );
    }
}
