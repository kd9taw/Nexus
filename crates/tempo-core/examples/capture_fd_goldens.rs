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
use tempo_core::contest::{field_day, FieldKind};
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

/// A SECOND Winter Field Day fixture, and the reason it exists: every row in [`ROWS`]
/// carries an ARRL class (`2A`/`4A`/`1D`/`3A`/`1E`), for BOTH events, so `wfd.cbr` and
/// `wfd.adi` hold no legal Winter class at all. They therefore cannot catch a
/// regression on the class path — the 2026-09-07 `ABCDEF`-for-WFD fix moving neither
/// of them was not the evidence it looked like.
///
/// These rows are legal Winter Field Day end to end: all four sponsor classes
/// (`H` home, `I` indoor, `O` outdoor, `M` mobile — `downloads/2026-rules-v3.pdf`,
/// "V3 9.8.25"), real ARRL/RAC sections, and a legal `2O` on the SENT side too, which
/// [`ROWS`] cannot have without moving frozen bytes.
///
/// `(call, class, section, mode, submode, when_unix, band)`
const WFD_CLASS_ROWS: &[(&str, &str, &str, &str, &str, u64, &str)] = &[
    ("W0WFD", "1H", "MO", "PH", "", 1_782_000_360, "80m"),
    ("K8IND", "3I", "OH", "CW", "", 1_782_000_420, "80m"),
    ("N7OUT", "2O", "AZ", "DIG", "FT8", 1_782_000_480, "40m"),
    ("VA3MOB", "1M", "ONS", "PH", "", 1_782_000_540, "20m"),
];

/// The class letters above are the SHIPPED spec's, not a second copy of the rules.
///
/// `contest::field_day(WinterFd)`'s CLASS pattern is the one place Nexus decides what
/// a Winter class may be; if it ever goes back to ARRL's `ABCDEF` — the shipped bug —
/// none of H/I/O/M appears in it and this panics, so the fixture cannot quietly become
/// a log of classes the sequencer would refuse. (A plain `contains` is enough because
/// none of the four letters occurs anywhere else in either pattern.)
fn assert_classes_are_the_shipped_winter_set() {
    let class = field_day(FdEvent::WinterFd).field("CLASS").expect("CLASS");
    let FieldKind::Pattern { re } = class.kind else {
        panic!("the Winter Field Day CLASS slot is no longer a pattern: {class:?}");
    };
    for (_, c, ..) in WFD_CLASS_ROWS {
        let letter = c.chars().next_back().expect("a class letter");
        assert!(
            re.contains(letter),
            "fixture class {c} is not legal under the shipped Winter pattern {re}"
        );
    }
}

/// The all-four-classes Winter log. `rows` is `WFD_CLASS_ROWS.len()` for the golden
/// itself and one fewer for the discrimination control, exactly like [`golden_log_n`].
pub fn wfd_class_log_n(rows: usize) -> FieldDayLog {
    assert_classes_are_the_shipped_winter_set();
    let mut log = FieldDayLog::new("W9XYZ", Exchange::new("2O", "WI"), "80m");
    log.event = FdEvent::WinterFd;
    for (i, (call, class, sect, mode, submode, when, band)) in
        WFD_CLASS_ROWS.iter().take(rows).enumerate()
    {
        log.band = (*band).to_string();
        assert!(
            log.log_submode_at(call, class, sect, mode, submode, i as u64, *when),
            "fixture row {call} must log — a refused row would silently shorten the golden"
        );
    }
    assert_eq!(log.qso_count(), rows, "every fixture row landed");
    log
}

/// The whole [`WFD_CLASS_ROWS`] table as a log.
pub fn wfd_class_log() -> FieldDayLog {
    wfd_class_log_n(WFD_CLASS_ROWS.len())
}

/// How many rows [`wfd_class_log`] holds — the discrimination control subtracts one.
pub fn wfd_class_row_count() -> usize {
    WFD_CLASS_ROWS.len()
}

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
    // The Winter class fixture (2026-09-07). Captured LATER than the four above, from
    // a tree whose §8(a) goldens were already green — which is what makes it a
    // capture and not a re-bless: writing it moved none of their bytes.
    let wfd = wfd_class_log();
    std::fs::write(dir.join("wfd-classes.cbr"), wfd.cabrillo(3_570)).unwrap();
    std::fs::write(dir.join("wfd-classes.adi"), wfd.adif()).unwrap();
}
