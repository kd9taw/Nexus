//! Capture the §8(a) Field Day migration goldens from the CURRENT tree.
//!
//! ARRL Field Day and Winter Field Day are SHIPPED SOFTWARE WITH USERS. The contest
//! programme refactors the exchange model out from under them, so the only honest proof
//! that nothing moved is a byte comparison against output captured BEFORE the first line
//! of the refactor. This example writes those bytes; `tests/fd_goldens.rs` asserts them
//! after every batch.
//!
//! Run (from the repo root, once, at the pre-batch-0 commit):
//!     cargo run -p tempo-core --example capture_fd_goldens
//!
//! It is deliberately NOT a test: a test that rewrites its own expectations proves
//! nothing. Re-running it after a behaviour change is how a migration bug gets
//! blessed — if the goldens move, the change is what is wrong, not the fixture.
use std::path::Path;
use tempo_core::fd_rules::{ruleset, CURRENT_RULES_YEAR};
use tempo_core::fieldday::{Exchange, FdEvent, FieldDayLog};

/// The fixture rows, in log order. Every timestamp is a literal; nothing here reads a
/// clock, so the bytes are reproducible on any machine at any time.
///
/// `(call, class, section, mode, submode, when_unix, band)`
const ROWS: &[(&str, &str, &str, &str, &str, u64, &str)] = &[
    ("K1ABC", "2A", "EMA", "CW", "", 1_782_000_000, "20m"),
    ("N0XYZ", "4A", "MN", "CW", "", 1_782_000_060, "20m"),
    ("W1AW", "1D", "CT", "PH", "", 1_782_000_120, "40m"),
    ("K5ABC", "3A", "STX", "PH", "", 1_782_000_180, "40m"),
    ("VE3XYZ", "2A", "ONE", "DIG", "RTTY", 1_782_000_240, "15m"),
    ("JA1ABC", "1E", "DX", "DIG", "FT8", 1_782_000_300, "15m"),
    // The legacy row: no timestamp. Cabrillo keeps its `----------`/`----`
    // placeholder and ADIF omits QSO_DATE/TIME_ON rather than inventing a date.
    ("W9LEG", "2A", "WI", "PH", "", 0, "15m"),
];

/// The fixed synthetic log, identical for both events (only `event` differs).
pub fn golden_log(event: FdEvent) -> FieldDayLog {
    golden_log_n(event, ROWS.len())
}

/// The first `rows` rows of the fixture. `golden_log` is this with the whole table;
/// `tests/fd_goldens.rs`'s positive control is this with one row fewer, which is how a
/// vacuous byte comparison (empty golden, wrong fixture) is made to show itself.
/// It exists here rather than as a `FieldDayLog::truncate` because production code does
/// not grow a row-removal API to serve a test.
pub fn golden_log_n(event: FdEvent, rows: usize) -> FieldDayLog {
    let mut log = FieldDayLog::new("W9XYZ", Exchange::new("3A", "WI"), "20m");
    log.event = event;
    for (i, (call, class, sect, mode, submode, when, band)) in ROWS.iter().take(rows).enumerate() {
        log.band = (*band).to_string();
        assert!(
            log.log_submode_at(call, class, sect, mode, submode, i as u64, *when),
            "fixture row {call} must log — a refused row would silently shorten the golden"
        );
    }
    assert_eq!(log.qso_count(), rows, "every fixture row landed");
    log
}

fn main() {
    let dir = Path::new("crates/tempo-core/tests/fixtures/fd-goldens");
    std::fs::create_dir_all(dir).expect("fixture dir");
    for (event, stem) in [(FdEvent::ArrlFd, "arrlfd"), (FdEvent::WinterFd, "wfd")] {
        let log = golden_log(event);
        std::fs::write(dir.join(format!("{stem}.cbr")), log.cabrillo(14_074)).unwrap();
        std::fs::write(dir.join(format!("{stem}.adi")), log.adif()).unwrap();
        let rs = ruleset(event, CURRENT_RULES_YEAR);
        let (qso, powered) = rs.scoring.qso_and_powered(&log, 5);
        let bonus = rs.bonus_points(&["w1aw-bulletin".to_string(), "web-submission".to_string()]);
        // Printed, not written: the score golden is PINNED AS LITERALS in fd_goldens.rs,
        // so a later batch cannot regenerate its own expectation by re-running this.
        println!("{stem}: qso={qso} powered={powered} bonus={bonus}");
    }
}
