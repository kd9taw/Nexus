//! Reading the logbook store — SPEC-2 v3's read path, laid in C12.
//!
//! The database has been the log's durable home since C9, and until now nothing read it but the
//! launch: every question about the log was answered from the whole copy of it held in memory.
//! Stage 2 moves those questions into the store, one family at a time, and they all come
//! through here: a [`LogReader`] hands a caller a read connection, runs what it asks inside ONE
//! read transaction, and takes the connection back.
//!
//! # Three promises
//!
//! 1. **A read is one picture of the log.** Everything a closure passed to [`LogReader::read`]
//!    runs sees the store as it stood when the read's first statement ran, whatever commits
//!    meanwhile — the writer's, or another window's. A record comes back whole or not at all
//!    (`LogDb::in_one_snapshot`). In WAL mode a reader never waits for the writer, and the
//!    writer never waits for a reader.
//! 2. **A read never writes.** Its connections are opened with [`LogDb::open_reader`]:
//!    `query_only`, and unable to create a database that is not there.
//! 3. **A read never runs under the Engine lock.** It is disk I/O, and the radio loop needs that
//!    lock every 20 ms; [`super::io_fence::off_engine_lock`] asserts it in a debug build, at
//!    the one door every read comes through.
//!
//! What it does NOT promise is that a change made a moment ago is in the picture: the writer
//! commits on its own thread, a little after the change. A caller that must see its own
//! changes waits for them first ([`super::writer::LogWriter::wait_committed`]) — which is the
//! app's `StoreReads`, not this module.
//!
//! # The pool
//!
//! A few connections, opened on first use and kept for the next read: opening one parses the
//! schema, and a lifetime log's hottest pages stay in its cache. Nothing is opened until a
//! read asks, so a session that never reads the store costs nothing here.

use super::io_fence;
use super::sqlite::{LogDb, Result};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

/// How many idle read connections the pool keeps. More readers than this at once each open
/// their own, and the extra ones are closed when they finish.
const IDLE_READERS: usize = 2;

/// Read connections to one logbook store. Share it as an `Arc`; a read borrows a connection for
/// its duration only.
#[derive(Debug)]
pub struct LogReader {
    path: PathBuf,
    idle: Mutex<Vec<LogDb>>,
}

impl LogReader {
    /// A reader of the store at `path`. Opens nothing: the first read does.
    pub fn new(path: &Path) -> LogReader {
        LogReader {
            path: path.to_path_buf(),
            idle: Mutex::new(Vec::new()),
        }
    }

    /// The store this reads.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run `f` against the store inside ONE read transaction, and hand back what it returned.
    ///
    /// ⚠️ **Never call it holding the Engine lock** — it is disk I/O; a debug build panics.
    /// Take what the read needs under the lock (the app's `StoreReads`), release it, then read.
    pub fn read<T>(&self, f: impl FnOnce(&LogDb) -> Result<T>) -> Result<T> {
        io_fence::off_engine_lock("a read of the logbook store");
        // Taken in its own statement so the pool's lock is released before a connection is
        // opened: opening one is disk work, and no other read should wait behind it.
        let idle = self.lock().pop();
        let db = match idle {
            Some(db) => db,
            None => LogDb::open_reader(&self.path)?,
        };
        let out = db.in_one_snapshot(f);
        // A connection that has just failed is not trusted with the next read.
        if out.is_ok() {
            let mut idle = self.lock();
            if idle.len() < IDLE_READERS {
                idle.push(db);
            }
        }
        out
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<LogDb>> {
        self.idle.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::sqlite::Resolved;
    use crate::logbook::{parse_adif, QsoRecord, RecordId};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// A folder of this test's own, gone with the value.
    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            static N: AtomicU64 = AtomicU64::new(0);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let p = std::env::temp_dir().join(format!(
                "nexus-logreader-{tag}-{}-{}-{nanos}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        fn db(&self) -> PathBuf {
            self.0.join("log.sqlite3")
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// `n` contacts with ids, each carrying a foreign tag.
    fn rows(n: usize, base: u64) -> Vec<QsoRecord> {
        (0..n)
            .map(|i| {
                let mut r = parse_adif(&format!(
                    "<CALL:6>K{}READ<QSO_DATE:8>20260901<TIME_ON:6>{:02}{:02}00<BAND:3>20m\
                     <MODE:3>FT8<APP_OTHER_LOGGER:3>xyz<EOR>",
                    i % 10,
                    i / 60 % 24,
                    i % 60
                ))
                .remove(0);
                r.id = Some(RecordId::Provisional {
                    hash: base + i as u64,
                    ordinal: 0,
                });
                r
            })
            .collect()
    }

    /// A store holding `n` contacts, and the connection that wrote them, kept open.
    fn store(d: &Dir, n: usize) -> (LogDb, Vec<QsoRecord>) {
        let rows = rows(n, 1_000);
        let mut db = LogDb::open(&d.db()).unwrap();
        db.insert_all(rows.iter().map(|r| (r, Resolved::default())))
            .unwrap();
        (db, rows)
    }

    /// A read hands back what the store holds, through the one decoder `load_all` uses, and
    /// the connection it opened is kept for the next read.
    #[test]
    fn a_read_sees_the_store_and_keeps_its_connection_for_the_next() {
        let d = Dir::new("read");
        let (_writer, rows) = store(&d, 12);
        let reader = LogReader::new(&d.db());
        assert_eq!(
            reader.lock().len(),
            0,
            "nothing is opened until a read asks"
        );
        assert_eq!(reader.read(|db| db.load_all()).unwrap(), rows);
        assert_eq!(
            reader.lock().len(),
            1,
            "the connection waits for the next read"
        );
        assert_eq!(reader.read(|db| db.row_count()).unwrap(), 12);
        assert_eq!(reader.lock().len(), 1, "and the next read reused it");
    }

    /// ★ A READ IS ONE PICTURE: two statements inside one read see the same store, though a
    /// contact is committed between them by another connection.
    ///
    /// The control is the same two statements, each a read of its own: they DO see the commit —
    /// so the first result is the transaction's doing, not a commit that did not land.
    #[test]
    fn a_read_is_one_picture_whatever_commits_during_it() {
        let d = Dir::new("picture");
        let (mut writer, _) = store(&d, 5);
        let late = rows(1, 9_000);
        let reader = LogReader::new(&d.db());
        let (first, second) = reader
            .read(|db| {
                let first = db.row_count()?;
                writer
                    .insert_all(late.iter().map(|r| (r, Resolved::default())))
                    .expect("the other connection commits");
                Ok((first, db.row_count()?))
            })
            .unwrap();
        assert_eq!((first, second), (5, 5), "one read, one picture");

        // Control: separate reads are separate pictures.
        let late = rows(1, 9_500);
        let first = reader.read(|db| db.row_count()).unwrap();
        writer
            .insert_all(late.iter().map(|r| (r, Resolved::default())))
            .unwrap();
        let second = reader.read(|db| db.row_count()).unwrap();
        assert_eq!(
            (first, second),
            (6, 7),
            "control: a read begun after a commit sees it"
        );
    }

    /// A reader cannot write — SQLite refuses it, not this module's good manners. The control
    /// is the same write through an ordinary connection, which succeeds.
    #[test]
    fn a_reader_cannot_write_to_the_store() {
        let d = Dir::new("readonly");
        let (_writer, _) = store(&d, 3);
        let reader = LogReader::new(&d.db());
        let refused = reader.read(|db| db.set_meta("reader_wrote", 1));
        assert!(refused.is_err(), "a reader's write must be refused");
        assert_eq!(
            LogDb::open(&d.db()).unwrap().meta("reader_wrote").unwrap(),
            None,
            "and nothing reached the file"
        );
        // Control: the refusal is the reader's, not the store's.
        LogDb::open(&d.db())
            .unwrap()
            .set_meta("reader_wrote", 1)
            .expect("control: an ordinary connection writes");
    }

    /// A reader never creates a database: a store that is not there is an error, and stays
    /// not there.
    #[test]
    fn a_reader_never_creates_a_database() {
        let d = Dir::new("missing");
        let reader = LogReader::new(&d.db());
        assert!(reader.read(|db| db.row_count()).is_err());
        assert!(!d.db().exists(), "nothing was created by reading");
    }

    /// A store written by another schema is refused, as the writer's open refuses it — and the
    /// refusal writes nothing, unlike an ordinary open, which stamps a version on a database
    /// that has none.
    #[test]
    fn a_reader_refuses_a_store_of_another_schema() {
        let d = Dir::new("schema");
        LogDb::open(&d.db())
            .unwrap()
            .set_meta("schema_version", 99)
            .unwrap();
        let before = std::fs::read(d.db()).unwrap();
        let err = LogReader::new(&d.db())
            .read(|db| db.row_count())
            .unwrap_err();
        assert!(err.to_string().contains("99"), "{err}");
        assert_eq!(std::fs::read(d.db()).unwrap(), before, "untouched");
    }

    /// ★ POSITIVE CONTROL for the fence: a read under an Engine guard is a panic in a debug
    /// build. Every other test here reads with no guard held, and stays quiet.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "io_fence: a read of the logbook store ran while this thread holds")]
    fn a_read_under_the_engine_lock_is_refused() {
        let d = Dir::new("fence");
        let (_writer, _) = store(&d, 1);
        let reader = LogReader::new(&d.db());
        let _held = io_fence::EngineHeld::acquired();
        let _ = reader.read(|db| db.row_count());
    }
}
