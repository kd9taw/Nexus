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
//!
//! ⚠️ **It waits as long as a loaded box takes ([`TEST_WAIT`]), not `READ_WAIT`.** `READ_WAIT` is a
//! screen's budget: past it a Remote read answers busy and a fold answers from the store as it
//! stands, both by design, and each is tested with the writer held. A test held to it fails on a
//! slow disk (WSL2's fsync beside two builds), not on a wrong answer — the intermittent busy and
//! stale parity failures of 2026-09-25, reproduced by a writer slowed past it. [`caught_up`] is
//! the same wait for a harness about to ask one of those readers about a change it just made.
use std::ops::ControlFlow;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempo_app::engine::Engine;
use tempo_app::logstore::{Freshness, LogRows};
use tempo_core::logbook::QsoRecord;

/// How long a test waits for the store's writer to hold every change made before it asks.
pub(crate) const TEST_WAIT: Duration = Duration::from_secs(120);

/// Every contact the log holds, whole and in log order: from the store once every change made
/// before this call is written, or on the 1.13 path the log in memory.
pub(crate) trait StoredLog {
    /// In place of `log_records()`.
    fn stored_log(&self) -> Vec<Arc<QsoRecord>> {
        self.stored_log_within(TEST_WAIT)
    }

    /// [`Self::stored_log`], refused unless the writer holds every change within `wait`.
    fn stored_log_within(&self, wait: Duration) -> Vec<Arc<QsoRecord>>;

    /// The same contacts as values, in place of `get_log()`.
    fn stored_records(&self) -> Vec<QsoRecord> {
        self.stored_log()
            .into_iter()
            .map(Arc::unwrap_or_clone)
            .collect()
    }
}

impl StoredLog for Engine {
    fn stored_log_within(&self, wait: Duration) -> Vec<Arc<QsoRecord>> {
        let rows = self.log_rows();
        let mut log = Vec::new();
        let fresh = rows
            .each_record(wait, &mut |r| {
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

/// Wait until the store's writer holds every change made to `engine` so far — at once on the
/// 1.13 path. What a harness calls before it asks a reader of the store about a change it has just
/// made: the reader's own wait is `READ_WAIT`.
pub(crate) fn caught_up(engine: &Mutex<Engine>) {
    let rows = engine.lock().unwrap().log_rows();
    if let LogRows::Store(reads) = rows {
        let ((), fresh) = reads
            .read(TEST_WAIT, |_| Ok(()))
            .expect("the logbook store reads");
        assert_eq!(
            fresh,
            Freshness::Current,
            "the store's writer took every change within {TEST_WAIT:?}"
        );
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
        let _ = e
            .lock()
            .unwrap()
            .stored_log_within(Duration::from_millis(200));
    }

    /// ★ `caught_up` waits for the writer: held for a moment after a contact is logged, the wait
    /// outlasts the hold, and then a read of the store needs no wait of its own. The control: the
    /// same read before the hold is let go is stale.
    #[test]
    fn caught_up_waits_until_the_writer_holds_every_change() {
        let d = Dir::new("stored-log-caught-up");
        std::fs::write(d.log(), synthetic_log(20, 0x0C19_D0A3)).unwrap();
        let e = launch(&d);
        let hold = tempo_core::logbook::sqlite::WriteHold::take(&d.db()).unwrap();
        e.lock().unwrap().log_qso(parse_one(
            "<CALL:5>ZD7AA<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260829<TIME_ON:6>030000<EOR>",
        ));
        let rows = e.lock().unwrap().log_rows();
        let now = || match &rows {
            LogRows::Store(reads) => reads.read(Duration::ZERO, |_| Ok(())).unwrap().1,
            LogRows::Memory(_) => panic!("premise: the store's rows"),
        };
        assert!(
            matches!(now(), Freshness::Stale(_)),
            "control: the writer is held"
        );
        let waiter = {
            let e = Arc::clone(&e);
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                caught_up(&e);
                started.elapsed()
            })
        };
        std::thread::sleep(Duration::from_millis(300));
        drop(hold);
        let waited = waiter.join().unwrap();
        assert!(
            waited >= Duration::from_millis(250),
            "it waited for the writer: {waited:?}"
        );
        assert_eq!(
            now(),
            Freshness::Current,
            "a read straight after needs no wait"
        );
        settle(&e);
    }
}
