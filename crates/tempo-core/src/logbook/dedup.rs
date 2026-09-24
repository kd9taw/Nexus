//! `log_qso`'s duplicate-contact guard: THE predicate, and the scan it has always been asked
//! over, kept as the oracle.
//!
//! ⛔ **FT HARD GATE — operator yes 2026-09-19, scoped to exactly this:** the guard becomes an
//! index lookup, and the log's append goes through the store, with the behaviour IDENTICAL. The
//! predicate below is the one `Engine::log_qso` has always run, moved here verbatim so there is
//! ONE copy of it; the index — since SPEC-2's C13, the station lists of
//! [`super::hot::HotIndex`] — only narrows which rows it is asked about. The old answer — a scan
//! of every row — is kept as [`scan_for_duplicate`], and it is the ORACLE the index's parity test
//! holds it to. Any difference between the two is a behaviour change and a fresh gate.
//!
//! # Why an index can be exact here
//!
//! The predicate's first clause is [`crate::message::same_call`], which is equality of
//! [`crate::message::base_call`] — a pure function of the call. So every row that can satisfy
//! the predicate shares the incoming contact's base call, and a map from base call to the rows
//! holding it hands the predicate exactly those rows: the rest could only have answered no.
//! [`is_recent_duplicate_in_slot`] is the predicate's other clauses, for a caller that has
//! already matched the call that way.
//!
//! # Why it never touches the database
//!
//! The FT sequencer logs from the radio loop, under the engine lock, and the whole storage
//! programme exists to keep the disk off that path. The index is read from memory alone — it has
//! no store in scope, and nothing it does can wait on one.

use super::{Logbook, QsoRecord};

/// How close in time two contacts must be for the second to count as the first logged again.
pub const DEDUP_WINDOW_SECS: u64 = 300;

/// THE guard's predicate: `r`, already in the log, is `rec` logged again — the same station
/// (base call, case-insensitive), band and mode, within [`DEDUP_WINDOW_SECS`]. Verbatim from
/// `Engine::log_qso`; the one copy.
pub fn is_recent_duplicate(r: &QsoRecord, rec: &QsoRecord) -> bool {
    crate::message::same_call(&r.call, &rec.call)
        && is_recent_duplicate_in_slot(&r.band, &r.mode, r.when_unix, rec)
}

/// The predicate's clauses after the call's: a row logged on `band` in `mode` at `when` — whose
/// call is already known to be `rec`'s station — is `rec` logged again. Band and mode compare
/// case-insensitively, and the window is inclusive at both ends.
pub fn is_recent_duplicate_in_slot(band: &str, mode: &str, when: u64, rec: &QsoRecord) -> bool {
    band.eq_ignore_ascii_case(&rec.band)
        && mode.eq_ignore_ascii_case(&rec.mode)
        && rec.when_unix.abs_diff(when) <= DEDUP_WINDOW_SECS
}

/// The guard as it always was: every row in the log, asked in turn. O(n) per contact — kept
/// as the ORACLE the index is tested against, and as the reference for what "identical" means.
pub fn scan_for_duplicate(log: &Logbook, rec: &QsoRecord) -> bool {
    log.records().iter().any(|r| is_recent_duplicate(r, rec))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::hot::{HotIndex, HotKeys};
    use crate::logbook::parse_adif;

    const T0: u64 = 1_788_000_000;

    fn contact(call: &str, band: &str, mode: &str, when: u64) -> QsoRecord {
        let mut r = parse_adif("<CALL:4>W1AW<BAND:3>20m<MODE:3>FT8<EOR>").remove(0);
        r.call = call.to_string();
        r.band = band.to_string();
        r.mode = mode.to_string();
        r.when_unix = when;
        r
    }

    struct NoKeys;
    impl HotKeys for NoKeys {
        fn band_key(&self, band: &str) -> String {
            band.to_ascii_lowercase()
        }
        fn entity(&self, _: &str) -> Option<String> {
            None
        }
    }

    /// The window's edges, pinned by value rather than left to chance: 300 s is a duplicate,
    /// 301 s is not, in both directions — and a portable, a hashed and a lower-case spelling of
    /// one station are one station. The oracle and the index both. (Every other case is the
    /// hot index's parity property.)
    #[test]
    fn the_window_edges_and_the_call_spellings_are_the_old_guards() {
        let mut log = Logbook::new();
        log.add(contact("W1AW/P", "20m", "FT8", T0));
        let index = HotIndex::build(&log, &NoKeys);
        for (call, band, mode, dt, dup) in [
            ("W1AW", "20m", "FT8", 300i64, true),
            ("W1AW", "20m", "FT8", -300, true),
            ("W1AW", "20m", "FT8", 301, false),
            ("W1AW", "20m", "FT8", -301, false),
            ("<W1AW>", "20M", "ft8", 0, true),
            ("w1aw", "20m", "FT8", 0, true),
            ("KH6/W1AW", "20m", "FT8", 0, true),
            ("W1AW", "40m", "FT8", 0, false),
            ("W1AW", "20m", "FT4", 0, false),
            ("K1ABC", "20m", "FT8", 0, false),
        ] {
            let p = contact(call, band, mode, (T0 as i64 + dt) as u64);
            assert_eq!(
                scan_for_duplicate(&log, &p),
                dup,
                "oracle: {call} {band} {mode} {dt}"
            );
            assert_eq!(
                index.is_duplicate(&p),
                dup,
                "index: {call} {band} {mode} {dt}"
            );
        }
    }
}
