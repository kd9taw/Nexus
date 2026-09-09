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
const WFD_CLASSES_CBR: &str = include_str!("fixtures/fd-goldens/wfd-classes.cbr");
const WFD_CLASSES_ADI: &str = include_str!("fixtures/fd-goldens/wfd-classes.adi");

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

/// The Winter class path, which `wfd.cbr`/`wfd.adi` cannot cover: every row in the
/// §8(a) fixture carries an ARRL class (`2A`/`4A`/`1D`/`3A`/`1E`) for BOTH events, so
/// those two files hold no legal Winter class and could not have moved when the
/// `ABCDEF`-for-WFD bug was fixed on 2026-09-07. This log is legal Winter Field Day on
/// both sides — `2O` sent, all four sponsor classes received — and its bytes are what
/// a regression in the class path would move.
#[test]
fn the_winter_class_log_exports_byte_identically() {
    let log = capture::wfd_class_log();
    assert_eq!(
        log.cabrillo(3_570),
        WFD_CLASSES_CBR,
        "WFD class Cabrillo moved"
    );
    assert_eq!(log.adif(), WFD_CLASSES_ADI, "WFD class ADIF moved");
}

/// …and the classes in it are really there. A golden whose class column had been
/// blanked or normalised would still be byte-identical to a golden captured from the
/// same broken code, so the letters are asserted against the file directly.
#[test]
fn the_winter_class_goldens_carry_all_four_sponsor_classes() {
    for (cls, sect) in [("1H", "MO"), ("3I", "OH"), ("2O", "AZ"), ("1M", "ONS")] {
        assert!(
            WFD_CLASSES_CBR.contains(&format!(" {cls} {sect}")),
            "Cabrillo golden is missing the {cls} row"
        );
        assert!(
            WFD_CLASSES_ADI.contains(&format!("<CLASS:2>{cls} ")),
            "ADIF golden is missing the {cls} row"
        );
    }
    // The sent side is a legal Winter class too, which the §8(a) fixture's frozen
    // `3A` header cannot be.
    assert!(
        WFD_CLASSES_CBR.contains("W9XYZ 2O WI"),
        "sent class is not Winter-legal"
    );
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
        let (qso, powered) = rs.scoring.qso_and_powered(log.score_rows(), 5);
        let bonus = rs.bonus_points(&["w1aw-bulletin".to_string(), "web-submission".to_string()]);
        assert_eq!(
            (qso, powered, bonus),
            (want_qso, want_powered, want_bonus),
            "{event:?}"
        );
    }
}

/// POSITIVE CONTROL — a byte comparison that cannot fail is not evidence. Dropping the
/// last fixture row MUST change all six artifacts; if it does not, the assertions above
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
    let short = capture::wfd_class_log_n(capture::wfd_class_row_count() - 1);
    assert_ne!(
        short.cabrillo(3_570),
        WFD_CLASSES_CBR,
        "WFD class Cabrillo golden is vacuous"
    );
    assert_ne!(
        short.adif(),
        WFD_CLASSES_ADI,
        "WFD class ADIF golden is vacuous"
    );
    assert!(
        !WFD_CLASSES_CBR.is_empty() && !WFD_CLASSES_ADI.is_empty(),
        "empty WFD class golden file"
    );
}

/// §8(b) — **a real operator's 1.x log still loads.**
///
/// `arrlfd.adi` is not a round-trip of this build's own output: it was written by the
/// SHIPPED exporter and checked in before batch 0 moved a line, so it carries `CLASS`
/// and `ARRL_SECT` and no `APP_NEXUS_EX`/`MYEX` at all. Restoring it exercises exactly
/// the two fallbacks §8(b) specifies — the standard columns for the received side, the
/// session's current sent exchange for the sent side — and both are exact for Field
/// Day, which is why the Cabrillo comes back byte for byte.
#[test]
fn a_1x_journal_restores_to_an_identical_cabrillo_and_score() {
    let head = capture::golden_log(FdEvent::ArrlFd);
    let mut restored = capture::empty_log(FdEvent::ArrlFd);
    restored.merge_adif(ARRLFD_ADI, 0);
    assert_eq!(
        restored.qso_count(),
        head.qso_count(),
        "a 1.x journal lost rows on restore"
    );
    assert_eq!(
        restored.cabrillo(14_074),
        ARRLFD_CBR,
        "a restored 1.x log exports different bytes"
    );
    let rs = ruleset(FdEvent::ArrlFd, CURRENT_RULES_YEAR);
    assert_eq!(
        rs.scoring.qso_and_powered(restored.score_rows(), 5),
        rs.scoring.qso_and_powered(head.score_rows(), 5),
        "a restored 1.x log scores differently"
    );
}

/// POSITIVE CONTROL for the test above — a restore that silently dropped rows would
/// pass it only if the assertions could not tell. Strip one row's `CALL` (the one field
/// `restore_row` refuses on) and the count MUST come back one short and the bytes MUST
/// differ; if they do not, the green above is not evidence.
#[test]
fn the_1x_journal_restore_discriminates() {
    let damaged = ARRLFD_ADI.replacen("<CALL:5>K1ABC", "<CALL:0>", 1);
    assert_ne!(damaged, ARRLFD_ADI, "the fixture was not actually damaged");
    let mut restored = capture::empty_log(FdEvent::ArrlFd);
    restored.merge_adif(&damaged, 0);
    assert_eq!(
        restored.qso_count(),
        capture::golden_log(FdEvent::ArrlFd).qso_count() - 1,
        "a row with no callsign was restored anyway"
    );
    assert_ne!(
        restored.cabrillo(14_074),
        ARRLFD_CBR,
        "the Cabrillo comparison cannot see a missing row"
    );
}

/// ⭐ **A Field Day journal carries NO private carrier**, and that is what keeps the
/// golden above byte-identical rather than a coincidence.
///
/// The writer emits `APP_NEXUS_EX`/`MYEX` exactly when the declared fallback would not
/// give the row back — for Field Day it always would, because class + section ARE the
/// received exchange and the sent exchange does not move. Asserted directly, because
/// "the golden did not move" and "no tag is written" are the same fact and a reader
/// should not have to infer one from the other.
#[test]
fn field_days_journal_carries_no_private_carrier() {
    for event in [FdEvent::ArrlFd, FdEvent::WinterFd] {
        let adi = capture::golden_log(event).adif();
        for tag in ["APP_NEXUS_EX", "APP_NEXUS_MYEX", "APP_NEXUS_ROLE"] {
            assert!(
                !adi.contains(tag),
                "{event:?}: {tag} is written for a row the standard columns already carry"
            );
        }
        // POSITIVE CONTROL: the matcher can see a tag that IS there.
        assert!(adi.contains("APP_NEXUS_QSEQ"), "{event:?}: no tag at all?");
    }
}
