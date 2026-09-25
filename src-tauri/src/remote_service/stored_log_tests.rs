//! The log as a test reads it — SPEC-2 v3 C19 Part D: tempo-app's [`StoredLog`], which src-tauri's
//! tests reach through the `test-util` feature (a dev-dependency, never in a shipped build). Its
//! header holds the contract: the read waits for every change made before it — a test's budget
//! (`TEST_WAIT`), not a screen's `READ_WAIT` — and refuses rather than answer from a store whose
//! writer is still behind; `caught_up` is the same wait for a harness about to ask a product reader
//! about a change it has just made.
//!
//! The tests below hold it to src-tauri's own fixtures: the store and the 1.13 path, the refusal,
//! and the wait.
pub(crate) use tempo_app::test_util::StoredLog;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote_service::query::log_tests::{
        launch, memory, parse_one, settle, synthetic_log, Dir,
    };
    use std::sync::Arc;
    use std::time::Duration;
    use tempo_app::logstore::{Freshness, LogRows};
    use tempo_core::logbook::sqlite::LogDb;
    use tempo_core::logbook::{adif_record_own_log, Logbook, QsoRecord};

    /// On the store and on the 1.13 path, the read is the log — every record, whole, in log order —
    /// including a contact logged a moment before it is asked (P4): as the database holds it, read
    /// on a connection of its own, and on the 1.13 path as `log.adi`, the log's home there, holds it.
    #[test]
    fn the_stored_log_is_the_log_on_both_paths() {
        let text = synthetic_log(400, 0x0C19_D0A1);
        let d = Dir::new("stored-log");
        std::fs::write(d.log(), &text).unwrap();
        for (arm, e) in [
            ("the store", launch(&d)),
            ("the 1.13 path", memory(&d, &text)),
        ] {
            e.lock().unwrap().log_qso(parse_one(
                "<CALL:5>ZD7AA<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260829<TIME_ON:6>030000<EOR>",
            ));
            let stored = e.lock().unwrap().stored_log();
            assert!(
                stored.iter().any(|r| r.call == "ZD7AA"),
                "{arm}: logged just now"
            );
            settle(&e);
            let on_disk: Vec<QsoRecord> = if e.lock().unwrap().log_on_file() {
                Logbook::load(&d.memory_log())
                    .records()
                    .iter()
                    .map(|r| QsoRecord::clone(r))
                    .collect()
            } else {
                LogDb::open(&d.db())
                    .and_then(|db| db.load_all())
                    .expect("the database reads")
            };
            let own = |rows: &mut dyn Iterator<Item = &QsoRecord>| -> Vec<String> {
                rows.map(adif_record_own_log).collect()
            };
            assert_eq!(
                own(&mut stored.iter().map(|r| r.as_ref())),
                own(&mut on_disk.iter()),
                "{arm}: every record, whole, in log order"
            );
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

    /// ★ [`StoredLog::caught_up`] waits for the writer: held for a moment after a contact is
    /// logged, the wait outlasts the hold, and then a read of the store needs no wait of its own.
    /// The control: the same read before the hold is let go is stale.
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
        let LogRows::Store(reads) = &rows;
        let now = || reads.read(Duration::ZERO, |_| Ok(())).unwrap().1;
        assert!(
            matches!(now(), Freshness::Stale(_)),
            "control: the writer is held"
        );
        let waiter = {
            let e = Arc::clone(&e);
            std::thread::spawn(move || {
                let started = std::time::Instant::now();
                e.caught_up();
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
