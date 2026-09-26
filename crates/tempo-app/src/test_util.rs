//! The log as a test reads it — SPEC-2 v3 C19 Part D. Once the copy of the log in memory is gone
//! (`Engine::log_records`, `Engine::get_log`, `StationCore::logbook` and their kin), a test
//! asserts on what the logbook store holds, read through the same door every reader of the log
//! uses (`log_rows`).
//!
//! Built for tempo-app's tests, and — through the `test-util` feature, which only a
//! dev-dependency turns on — for src-tauri's, so both halves of the migration read the log one
//! way. It is never in a shipped build: a whole log gathered into memory is what C19 retires.
//!
//! ⚠️ **A test that looks at the log before its change is written must say so another way.** This
//! read waits, as every read of the store does, for the changes made before it was asked (P4), and
//! it refuses — panics — rather than answer from a store whose writer is still behind: an answer
//! missing a change would pass a test that should fail. A test that stalls the writer on purpose
//! (C12's `WriteHold`) and then inspects the log is asking a different question, and is rewritten
//! for it, never converted.
//!
//! ⚠️ **It is a read of the store, so never under the Engine lock** (a debug build panics): on an
//! [`Engine`] a test owns, call it with no guard held; on the engine a command shares
//! (`Mutex<Engine>`) it takes the lock for the handles alone and reads with it released.
//!
//! ⚠️ **It is not the oracle.** A test comparing the store with this read compares the store with
//! itself. What a change should have left is Stage 1's log (`stage1_tests`, the write path before
//! C19 B, run in lockstep), or a fact the test states about the contacts it made.

use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempo_core::logbook::QsoRecord;

use crate::engine::{engine_lock, Engine};
use crate::logstore::{Freshness, LogRows};
use crate::station::StationCore;

/// How long a test's read waits for the store's writer: a test's budget, not a screen's.
/// `READ_WAIT` is what a screen waits before it shows the store as it stands; a loaded machine can
/// take longer than that to write a change, and a test that failed then would be failing the
/// machine, not the code. As long as the harnesses' `flush_log_store` waits.
pub const TEST_WAIT: Duration = Duration::from_secs(120);

/// Every contact the log holds, whole and in log order: from the store once every change made
/// before the call is written, or on the no-store path the log in memory.
pub trait StoredLog {
    /// The log's handles, taken as every reader takes them — for the engine a command shares,
    /// under the Engine lock for that moment alone — and read by the methods below with the lock
    /// released.
    fn log_handles(&self) -> LogRows;

    /// The log, once the store holds every change made before this call — waiting up to `wait`
    /// for the writer, and refusing (a panic) past it rather than answer without a change.
    fn stored_log_within(&self, wait: Duration) -> Vec<Arc<QsoRecord>> {
        let mut log = Vec::new();
        let fresh = self
            .log_handles()
            .each_record(wait, &mut |r| {
                log.push(Arc::new(r.clone()));
                ControlFlow::Continue(())
            })
            .expect("the logbook store reads");
        behind_is_refused(fresh, wait);
        log
    }

    /// [`Self::stored_log_within`] a test's budget ([`TEST_WAIT`]).
    fn stored_log(&self) -> Vec<Arc<QsoRecord>> {
        self.stored_log_within(TEST_WAIT)
    }

    /// [`Self::stored_log`], each record owned — for a test whose values meet a `Vec<QsoRecord>`
    /// (an expected list, a `Logbook` built from them).
    fn stored_records(&self) -> Vec<QsoRecord> {
        self.stored_log()
            .into_iter()
            .map(Arc::unwrap_or_clone)
            .collect()
    }

    /// The same wait, with no read of the rows: once this returns, the store holds every change
    /// made before it was called — for a harness about to ask a product reader, which waits only a
    /// screen's `READ_WAIT`, about a change it has just made.
    fn caught_up(&self) {
        caught_up_within(self.log_handles(), TEST_WAIT);
    }
}

/// [`StoredLog::caught_up`], waiting up to `wait`: a pass that stops at the first row, so all it
/// costs is the wait for the writer.
fn caught_up_within(rows: LogRows, wait: Duration) {
    let fresh = rows
        .each_record(wait, &mut |_| ControlFlow::Break(()))
        .expect("the logbook store reads");
    behind_is_refused(fresh, wait);
}

/// A read that did not see every change made before it was asked is refused, never answered.
fn behind_is_refused(fresh: Freshness, wait: Duration) {
    assert!(
        matches!(fresh, Freshness::Current),
        "the store's writer is still behind the changes made before this read after {wait:?}: a \
         test that looks at the log before its change is written must say so another way"
    );
}

impl StoredLog for Engine {
    fn log_handles(&self) -> LogRows {
        self.log_rows()
    }
}

impl StoredLog for Mutex<Engine> {
    fn log_handles(&self) -> LogRows {
        engine_lock(self).log_rows()
    }
}

impl StoredLog for StationCore {
    fn log_handles(&self) -> LogRows {
        self.log_rows()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logstore::tests::{engine_on_store, legacy_log, qso, Dir};

    /// On the store and on the 1.13 path, the read is what the store holds — every record, whole,
    /// in log order, as a connection of its own loads it — including a contact logged a moment
    /// before it is asked (P4); and on the engine a command shares, it is read with the lock
    /// released.
    #[test]
    fn the_stored_log_is_what_the_store_holds_on_both_paths() {
        let (on_store, on_file) = (Dir::new("stored-log"), Dir::new("stored-log-1-13"));
        for d in [&on_store, &on_file] {
            std::fs::write(d.log(), legacy_log(40)).unwrap();
        }
        let mut file = Engine::new("K2DEF", "FN31", 0);
        file.set_log_path(on_file.log());
        for (arm, mut e) in [
            ("the store", engine_on_store(&on_store)),
            ("the 1.13 path", file),
        ] {
            e.log_qso(qso("ZD7AA", 1_788_000_000));
            let stored = e.stored_log();
            // The oracle: the store's rows as a connection of its own loads them — a read apart
            // from the one under test, once the writer has taken the contact (the read above
            // waited for it).
            let path = e
                .station()
                .store
                .as_ref()
                .expect("a store")
                .db_path()
                .to_path_buf();
            let held: Vec<Arc<QsoRecord>> = tempo_core::logbook::sqlite::LogDb::open_reader(&path)
                .expect("the store opens")
                .load_all()
                .expect("the store loads")
                .into_iter()
                .map(Arc::new)
                .collect();
            assert_eq!(stored, held, "{arm}");
            assert_eq!(stored.len(), 41, "{arm}");
            assert!(
                stored.iter().any(|r| r.call == "ZD7AA"),
                "{arm}: logged just now"
            );
            assert_eq!(e.station().stored_log(), held, "{arm}: the station's own");
            let owned: Vec<QsoRecord> = held.iter().map(|r| QsoRecord::clone(r)).collect();
            assert_eq!(e.stored_records(), owned, "{arm}: each record owned");
            e.caught_up();
            let shared = Mutex::new(e);
            assert_eq!(
                shared.stored_log(),
                held,
                "{arm}: the engine a command shares"
            );
        }
    }

    /// ★ POSITIVE CONTROL for the refusal, and the proof the store's arm reads the store: with the
    /// store's writer held past the read's wait, a contact just logged is not in the store yet,
    /// and the read refuses rather than answer without it (the log in memory holds it at once).
    #[test]
    #[should_panic(expected = "the store's writer is still behind")]
    fn a_read_the_writer_is_behind_is_refused() {
        let d = Dir::new("stored-log-stale");
        std::fs::write(d.log(), legacy_log(20)).unwrap();
        let mut e = engine_on_store(&d);
        let _hold = tempo_core::logbook::sqlite::WriteHold::take(&d.db()).unwrap();
        e.log_qso(qso("ZD7AA", 1_788_000_000));
        let _ = e.stored_log_within(Duration::from_millis(200));
    }

    /// The same control for the wait with no read.
    #[test]
    #[should_panic(expected = "the store's writer is still behind")]
    fn a_wait_the_writer_is_behind_is_refused() {
        let d = Dir::new("stored-log-behind");
        std::fs::write(d.log(), legacy_log(20)).unwrap();
        let mut e = engine_on_store(&d);
        let _hold = tempo_core::logbook::sqlite::WriteHold::take(&d.db()).unwrap();
        e.log_qso(qso("ZD7AA", 1_788_000_000));
        caught_up_within(e.log_handles(), Duration::from_millis(200));
    }
}
