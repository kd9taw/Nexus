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
//! ⚠️ **It is not the oracle.** While the copy exists, a test comparing the store with the log in
//! memory (P6: `same_log(&stored(..), e.log_records(), ..)`) compares two things; the same test on
//! this read would compare the store with itself. Those stay on the copy until the cut hands them
//! the Stage-1 engine.

use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};

use tempo_core::logbook::QsoRecord;

use crate::engine::{engine_lock, Engine};
use crate::logstore::{Freshness, LogRows, READ_WAIT};
use crate::station::StationCore;

/// Every contact the log holds, whole and in log order: from the store once every change made
/// before this call is written, or on the 1.13 path the log in memory.
pub trait StoredLog {
    fn stored_log(&self) -> Vec<Arc<QsoRecord>>;
}

/// The pass behind [`StoredLog::stored_log`], over handles taken already.
fn read(rows: LogRows) -> Vec<Arc<QsoRecord>> {
    let mut log = Vec::new();
    let fresh = rows
        .each_record(READ_WAIT, &mut |r| {
            log.push(Arc::new(r.clone()));
            ControlFlow::Continue(())
        })
        .expect("the logbook store reads");
    assert!(
        matches!(fresh, Freshness::Current),
        "the store's writer is still behind the changes made before this read: a test that \
         looks at the log before its change is written must say so another way"
    );
    log
}

impl StoredLog for Engine {
    fn stored_log(&self) -> Vec<Arc<QsoRecord>> {
        read(self.log_rows())
    }
}

impl StoredLog for Mutex<Engine> {
    fn stored_log(&self) -> Vec<Arc<QsoRecord>> {
        let rows = engine_lock(self).log_rows();
        read(rows)
    }
}

impl StoredLog for StationCore {
    fn stored_log(&self) -> Vec<Arc<QsoRecord>> {
        read(self.log_rows())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logstore::tests::{engine_on_store, legacy_log, qso, Dir};

    /// On the store and on the 1.13 path, the read is the log in memory — every record, whole,
    /// in log order — including a contact logged a moment before it is asked (P4); and on the
    /// engine a command shares, it is read with the lock released.
    #[test]
    fn the_stored_log_is_the_log_in_memory_on_both_paths() {
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
            // The copy this read replaces, as its oracle while the copy still exists.
            let held = e.log_records().to_vec();
            assert_eq!(stored, held, "{arm}");
            assert_eq!(stored.len(), 41, "{arm}");
            assert!(
                stored.iter().any(|r| r.call == "ZD7AA"),
                "{arm}: logged just now"
            );
            assert_eq!(e.station().stored_log(), held, "{arm}: the station's own");
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
        let _ = e.stored_log();
    }
}
