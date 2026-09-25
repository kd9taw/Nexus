//! The log as a test reads it — SPEC-2 v3 C19 Part D. Once the copy of the log in memory is gone
//! (`Engine::log_records`, `get_log` and their kin), a test asserts on what the logbook store
//! holds, read through the same door every reader of the log uses (`Engine::log_rows`).
//!
//! ⚠️ **A test that looks at the log before its change is written must say so another way.** This
//! read waits, as every read of the store does, for the changes made before it was asked (P4), and
//! it refuses — panics — rather than answer from a store whose writer is still behind: an answer
//! missing a change would pass a test that should fail. A test that stalls the writer on purpose
//! (C12's `WriteHold`) and then inspects the log is asking a different question, and is rewritten
//! for it, never converted.
use std::ops::ControlFlow;
use std::sync::Arc;

use tempo_app::engine::Engine;
use tempo_app::logstore::{Freshness, READ_WAIT};
use tempo_core::logbook::QsoRecord;

/// Every contact the log holds, whole and in log order: from the store once every change made
/// before this call is written, or on the 1.13 path the log in memory.
pub(crate) trait StoredLog {
    /// In place of `log_records()`.
    fn stored_log(&self) -> Vec<Arc<QsoRecord>>;

    /// The same contacts as values, in place of `get_log()`.
    fn stored_records(&self) -> Vec<QsoRecord> {
        self.stored_log()
            .into_iter()
            .map(Arc::unwrap_or_clone)
            .collect()
    }
}

impl StoredLog for Engine {
    fn stored_log(&self) -> Vec<Arc<QsoRecord>> {
        let rows = self.log_rows();
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_service::query::log_tests::{
        launch, memory, parse_one, settle, synthetic_log, Dir,
    };

    /// On the store and on the 1.13 path, the read is the log in memory — every record, whole,
    /// in log order — including a contact logged a moment before it is asked (P4).
    #[test]
    fn the_stored_log_is_the_log_in_memory_on_both_paths() {
        let text = synthetic_log(400, 0x0C19_D0A1);
        let d = Dir::new("stored-log");
        std::fs::write(d.log(), &text).unwrap();
        for (arm, e) in [("the store", launch(&d)), ("the 1.13 path", memory(&text))] {
            e.lock().unwrap().log_qso(parse_one(
                "<CALL:5>ZD7AA<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260829<TIME_ON:6>030000<EOR>",
            ));
            let eng = e.lock().unwrap();
            let stored = eng.stored_log();
            // The copy this read replaces, as its oracle while the copy still exists.
            let held = eng.log_records().to_vec();
            assert_eq!(stored, held, "{arm}");
            assert!(
                stored.iter().any(|r| r.call == "ZD7AA"),
                "{arm}: logged just now"
            );
            drop(eng);
            settle(&e);
        }
    }

    /// ★ POSITIVE CONTROL for the refusal: with the store's writer held past the read's wait, a
    /// contact just logged is not in the store yet, and the read refuses rather than answer
    /// without it.
    #[test]
    #[should_panic(expected = "the store's writer is still behind")]
    fn a_read_the_writer_is_behind_is_refused() {
        let d = Dir::new("stored-log-stale");
        std::fs::write(d.log(), synthetic_log(20, 0x0C19_D0A2)).unwrap();
        let e = launch(&d);
        let _hold = tempo_core::logbook::sqlite::WriteHold::take(&d.db()).unwrap();
        e.lock().unwrap().log_qso(parse_one(
            "<CALL:5>ZD7AA<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260829<TIME_ON:6>030000<EOR>",
        ));
        let _ = e.lock().unwrap().stored_log();
    }
}
