//! Converting an operator's existing `log.adi` into the database — SPEC-1 v3's **C5**.
//!
//! This is the riskiest step in the whole programme, and the reason is not technical: it is the
//! one piece where a bug destroys a real 150,000-QSO log rather than failing a test. Everything
//! below is shaped by that.
//!
//! # The order, which is the safety
//!
//! 1. **The permanent copy is taken before the database exists.** `log.adi.pre-sqlite`, beside
//!    the log, written from the same bytes this conversion goes on to read. So it is there and
//!    correct after a conversion that fails at any later point — including one that never got
//!    as far as creating a database.
//! 2. Only then is the database opened or created.
//! 3. The records go in in file order, one transaction per chunk.
//! 4. The conversion is marked done as its LAST act, so an interrupted run is never mistaken
//!    for a finished one.
//!
//! # Why the copy is not in `backups/`, and not `log.adi.bak`
//!
//! The backup ring **evicts** (`BACKUP_KEEP` snapshots within `BACKUP_TOTAL_BYTES`), membership
//! is by filename glob, and this copy is by construction the oldest file there — so it would be
//! first out, on a big log within a fortnight of ordinary saves, and no "special" name protects
//! it. The anchor (`log.adi.bak`) is a different artifact — the pre-first-load bytes — and
//! [`super::Logbook::backup_once`] **refuses to overwrite an existing one**, so reusing that
//! name on any machine that has ever opened a non-empty log would be a silent no-op. The worst
//! possible outcome for a safety copy is one that quietly did not happen.
//!
//! Living beside the log, outside the ring, is the precedent the anchor already set: "never
//! eligible for rotation" holds by construction rather than by a check someone could delete.
//!
//! # Resumability, and why the row count is the watermark
//!
//! A resume must not duplicate a record and must not drop one. Both follow from two properties
//! this module does not have to add:
//!
//! - **Every record's id is a function of the file alone.** A row carrying `APP_NEXUS_ID` keeps
//!   it; a row without one gets a PROVISIONAL id hashed from its own raw span
//!   ([`super::id::settle_file_ids`]). So reading the same file twice yields the same ids in
//!   the same order, and "which records are already in" is answerable.
//! - **A chunk is one transaction.** A crash therefore leaves whole chunks, never a torn one.
//!
//! So `COUNT(*)` on the store IS how far the last attempt got, and the resume inserts from
//! there. A separately stored counter would be a second transaction that could disagree with
//! the rows it counts, and a watermark running ahead of its data is precisely how a resume
//! drops records.
//!
//! The one thing that can invalidate the prefix is the SOURCE changing between attempts, so its
//! length is recorded before the first insert and checked on resume; a source that has moved
//! starts the conversion over rather than stitching a new file onto an old prefix.

use super::sqlite::{self, Batch, LogDb, Resolved};
use super::{Logbook, QsoRecord};
use std::path::{Path, PathBuf};

/// Records per transaction. The same value the writer thread chunks at, and for the same
/// reason: a transaction spanning a whole import holds SQLite's write lock for its length.
/// Here it doubles as the resume granularity — an interrupted conversion loses at most this
/// many records' worth of work, never a record.
const CHUNK_ROWS: usize = 256;

/// `log_meta` key: the byte length of the source this conversion is reading. Written before the
/// first insert, so a resume can tell "carry on" from "that was a different file".
const SOURCE_LEN: &str = "migration_source_len";
/// `log_meta` key: set as the conversion's last act. Its absence means "not finished", which an
/// interrupted run and a never-started one share — and both want the same treatment.
const DONE: &str = "migration_done";

/// What a conversion did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// There was no log to convert — a missing file, or one with no records in it. The store is
    /// marked done: an operator starting fresh has nothing to migrate, now or later.
    Empty,
    /// This store has already been converted. Doing it again would be the duplicate-import bug.
    AlreadyDone,
    /// Records were converted.
    Converted {
        /// How many this attempt wrote.
        written: usize,
        /// How many an earlier, interrupted attempt had already written — 0 on a first run.
        resumed_at: usize,
        /// How many the source holds in total.
        total: usize,
    },
}

/// Why a conversion could not finish.
#[derive(Debug)]
pub enum Error {
    /// The safety copy could not be written. **This refuses the whole conversion**, and that is
    /// deliberate: converting a log whose original is not safely copied is the one outcome this
    /// module exists to prevent. Every other failure here can be retried; that one cannot.
    Copy {
        /// Where the copy was to go.
        path: PathBuf,
        /// What went wrong.
        source: std::io::Error,
    },
    /// ⛔ The log has content, and NOT ONE record could be read out of it.
    ///
    /// A corrupt, truncated-to-nothing or wrong-format `log.adi` is the case that must never be
    /// mistaken for an empty one: "empty" marks the store converted, and a store marked
    /// converted is never looked at again — so the operator's whole log would be silently
    /// written off as "nothing to migrate". The parser resyncs at the next `<` and is happy to
    /// read a partly damaged file, so reaching zero records out of a non-empty body means the
    /// file is not a logbook this build can read. The conversion refuses, marks nothing, and
    /// leaves the operator's file exactly where it is.
    Unreadable {
        /// How much there was to read, so the refusal can say it plainly.
        bytes: usize,
    },
    /// The store refused, or SQLite did.
    Db(sqlite::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Copy { path, source } => write!(
                f,
                "could not copy the logbook to {} before converting it: {source}",
                path.display()
            ),
            Error::Unreadable { bytes } => write!(
                f,
                "the logbook file holds {bytes} bytes but no readable contacts — it has not \
                 been converted, and it has not been changed"
            ),
            Error::Db(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Copy { source, .. } => Some(source),
            Error::Unreadable { .. } => None,
            Error::Db(e) => Some(e),
        }
    }
}

impl From<sqlite::Error> for Error {
    fn from(e: sqlite::Error) -> Self {
        Error::Db(e)
    }
}

/// This module's result.
pub type Result<T> = std::result::Result<T, Error>;

/// Where the logbook database for `log_path` lives: beside it, named for the log's own stem —
/// `log.adi` → `log.sqlite3`. The ONE place the name is decided; every caller that needs it
/// (the conversion, the store, the data-folder copy) asks here, so the name cannot drift between
/// the file that is written and the file that is carried.
///
/// SQLite also keeps `log.sqlite3-wal` and `log.sqlite3-shm` beside it while it is open. They
/// are part of the database, not separate files: a committed contact can live in the `-wal`
/// until a checkpoint, which is why the database is only ever copied through SQLite
/// ([`super::sqlite::copy_database`]) and never file by file.
pub fn database_path(log_path: &Path) -> PathBuf {
    let stem = log_path.file_stem().unwrap_or_default().to_os_string();
    let mut name = if stem.is_empty() {
        std::ffi::OsString::from("log")
    } else {
        stem
    };
    name.push(".sqlite3");
    log_path.with_file_name(name)
}

/// Where the permanent pre-conversion copy of `log_path` lives: beside it, never rotated.
pub fn pre_sqlite_path(log_path: &Path) -> PathBuf {
    let mut name = log_path.file_name().unwrap_or_default().to_os_string();
    name.push(".pre-sqlite");
    log_path.with_file_name(name)
}

/// Take the permanent copy, from `bytes`, unless one is already there.
///
/// **An existing copy is kept, not replaced, and that is not a failure.** A second attempt
/// follows a first that may have got further, so the earlier copy is the one taken from the
/// operator's log before anything touched it. Replacing it with a later one is how a safety
/// copy comes to record the damage instead of the original.
///
/// Written with [`super::replace_private`] — 0600 from the first byte, per-PID tmp, atomic
/// rename — because the bytes may carry a credential a pre-1.11 log recorded, and a copy the
/// ring never sweeps would keep it at rest forever.
pub fn take_pre_sqlite_copy(log_path: &Path, bytes: &[u8]) -> Result<PathBuf> {
    let path = pre_sqlite_path(log_path);
    if path.exists() {
        return Ok(path);
    }
    super::replace_private(&path, bytes).map_err(|source| Error::Copy {
        path: path.clone(),
        source,
    })?;
    Ok(path)
}

/// Convert the log at `log_path` into the database at `db_path`, resuming an interrupted
/// attempt if there is one.
///
/// `resolve` supplies the two columns tempo-core cannot derive — cty.dat's entity name and CQ
/// zone (see [`Resolved`]). tempo-core does not depend on `propagation`, so the caller that
/// owns that table hands them in; a caller with no resolver passes one returning
/// [`Resolved::default`], which stores NULL rather than guessing from the `COUNTRY` text.
///
/// ⚠️ This does NOT make the database canonical and does not change what owns the log. It
/// converts, and stops.
pub fn migrate_log<R>(log_path: &Path, db_path: &Path, resolve: R) -> Result<Outcome>
where
    R: Fn(&QsoRecord) -> Resolved<'static>,
{
    // The ordinary launch: a store that is already converted. Answered from the store alone,
    // BEFORE the log is read — a lifetime log is tens of megabytes, and reading it on every
    // launch to learn that there is nothing to do is the launch cost the operator ruled out.
    // Only a store that EXISTS is asked: opening one that does not would create it, and the
    // copy below has to be taken before the store is created.
    if db_path.is_file() && is_converted(db_path)? {
        return Ok(Outcome::AlreadyDone);
    }
    // ⛔ ONE conversion at a time per store. Two Nexus windows share one data folder, and the
    // second can start while the first is still converting: without this it would read the
    // half-converted store's row count as a resume point and convert alongside it. It waits
    // here instead, then finds the store converted and does nothing.
    let lock = ConversionLock::take(db_path);
    if db_path.is_file() && is_converted(db_path)? {
        return Ok(Outcome::AlreadyDone);
    }
    let out = convert(log_path, db_path, resolve)?;
    // Converted: nobody will need the lock file again — a later caller answers from the store
    // on the fast path above and never reaches it. Kept after a FAILURE, because a caller
    // already waiting on it will try again and a newcomer must wait on the same file.
    lock.finished();
    Ok(out)
}

/// Whether the store at `db_path` is marked converted. The store must exist.
pub fn is_converted(db_path: &Path) -> Result<bool> {
    Ok(LogDb::open(db_path)?.meta(DONE)? == Some(1))
}

/// How far a conversion has got, for a screen that shows it: `done` of `total` records are in the
/// store. `total` is 0 until the log has been read and counted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    /// Records in the store so far, counting any an interrupted attempt had already written.
    pub done: usize,
    /// Records in the log; 0 while that is not known yet.
    pub total: usize,
}

/// [`migrate_log`], telling `progress` how far it has got.
///
/// ⚠️ A HOOK POINT, NOT YET WIRED. This reports nothing, so a start-up screen that shows it stays
/// indeterminate. The conversion's own loop is where `done` and `total` are known, and that is
/// where the calls belong.
pub fn migrate_log_reporting<R>(
    log_path: &Path,
    db_path: &Path,
    resolve: R,
    progress: &mut dyn FnMut(Progress),
) -> Result<Outcome>
where
    R: Fn(&QsoRecord) -> Resolved<'static>,
{
    let _ = progress;
    migrate_log(log_path, db_path, resolve)
}

/// Whether a conversion into `db_path` is running right now: some process holds its lock.
///
/// Asked without waiting, and without creating the lock file. A launch asks it before it does
/// anything of its own, so a second double-click while the first launch converts stands down
/// instead of queueing invisibly behind the conversion and then opening a second copy of Nexus.
/// A lock file nobody holds is a conversion that was interrupted, not one that is running; and a
/// lock that cannot be asked about reads as "no", as [`ConversionLock`] itself treats one.
pub fn conversion_in_progress(db_path: &Path) -> bool {
    let Ok(file) = std::fs::File::open(ConversionLock::path_for(db_path)) else {
        return false;
    };
    matches!(file.try_lock(), Err(std::fs::TryLockError::WouldBlock))
}

/// The advisory lock that keeps two processes from converting one log into one store at once.
/// Best-effort by construction: a lock that cannot be taken (a filesystem without locks, a
/// read-only folder) is no lock, and the conversion's own checks — a resume reads the store's
/// own row count, a chunk is one transaction, and a duplicate id is refused by the primary key —
/// still keep a concurrent conversion from duplicating or dropping a contact.
struct ConversionLock {
    path: PathBuf,
    file: Option<std::fs::File>,
}

impl ConversionLock {
    /// Where the lock lives: beside the store, named for it.
    fn path_for(db_path: &Path) -> PathBuf {
        let mut name = db_path.file_name().unwrap_or_default().to_os_string();
        name.push(".convert-lock");
        db_path.with_file_name(name)
    }

    /// Take the lock, WAITING while another process holds it.
    fn take(db_path: &Path) -> ConversionLock {
        let path = Self::path_for(db_path);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&path)
            .ok()
            .filter(|f| f.lock().is_ok());
        ConversionLock { path, file }
    }

    /// The conversion finished: remove the file while still holding it.
    fn finished(mut self) {
        if self.file.is_some() {
            let _ = std::fs::remove_file(&self.path);
        }
        self.file = None;
    }
}

/// The conversion itself — [`migrate_log`] is the gatekeeping around it.
fn convert<R>(log_path: &Path, db_path: &Path, resolve: R) -> Result<Outcome>
where
    R: Fn(&QsoRecord) -> Resolved<'static>,
{
    // The bytes, scrubbed of upload stamps exactly as the anchor's are. The copy is taken from
    // the CLEAN bytes on purpose: a pre-1.11 log can carry a service's echoed API key, and a
    // new file is not a place to write one. The scrub touches only `APP_TEMPO_UL_*` tails, so
    // the copy still holds the operator's original encoding byte for byte, which after the
    // conversion is the only surviving record of it (a lossy decode is what reaches the store).
    let bytes = std::fs::read(log_path).unwrap_or_default();
    let clean = super::scrub_upload_stamps(&bytes);
    let source = clean.as_deref().unwrap_or(&bytes);
    let has_records = !super::body_after_eoh(source)
        .iter()
        .all(|b| b.is_ascii_whitespace());

    // ⛔ BEFORE the database is created, so a conversion that fails at any later point — or
    // that never gets a database at all — still leaves the operator their original.
    if has_records {
        take_pre_sqlite_copy(log_path, source)?;
    }

    let mut db = LogDb::open(db_path)?;
    if db.meta(DONE)? == Some(1) {
        return Ok(Outcome::AlreadyDone);
    }
    if !has_records {
        db.set_meta(DONE, 1)?;
        return Ok(Outcome::Empty);
    }

    // Reading through `Logbook::load` rather than a parser of this module's own: it settles the
    // ids (the property the resume rests on), takes the anchor, sweeps the copies beside the log
    // and scrubs the log itself. One reader, so the store can never hold rows a load would not
    // produce.
    let log = Logbook::load(log_path);
    let total = log.len();
    if total == 0 {
        // ⛔ NOT `Empty`. The body was not blank, so there was something here to read and
        // nothing came out of it. Marking this store done would write the operator's whole log
        // off as "nothing to migrate", permanently and without a word.
        return Err(Error::Unreadable {
            bytes: source.len(),
        });
    }

    let len = i64::try_from(source.len()).unwrap_or(i64::MAX);
    let resumed_at = match db.meta(SOURCE_LEN)? {
        // The same source an earlier attempt was reading: its rows are a prefix of these.
        Some(prev) if prev == len => usize::try_from(db.row_count()?).unwrap_or(usize::MAX),
        // A DIFFERENT source. The rows in the store are a prefix of a file that no longer
        // exists, and stitching this one onto them would interleave two logs. Start over —
        // safe precisely because the store is not canonical yet and the copy is already taken.
        Some(_) => {
            db.apply(Batch {
                clear: true,
                ..Batch::default()
            })?;
            db.set_meta(SOURCE_LEN, len)?;
            0
        }
        None => {
            db.set_meta(SOURCE_LEN, len)?;
            0
        }
    };
    // A store holding more rows than the source has records cannot be a prefix of it. Rather
    // than silently writing nothing, clear and convert the whole file.
    let resumed_at = if resumed_at > total {
        db.apply(Batch {
            clear: true,
            ..Batch::default()
        })?;
        0
    } else {
        resumed_at
    };

    for chunk in log.records()[resumed_at..].chunks(CHUNK_ROWS) {
        let rows: Vec<(&QsoRecord, Resolved<'static>)> =
            chunk.iter().map(|r| (&**r, resolve(r))).collect();
        db.insert_all(rows)?;
    }

    // LAST, so an interrupted run is never read as a finished one.
    db.set_meta(DONE, 1)?;
    Ok(Outcome::Converted {
        written: total - resumed_at,
        resumed_at,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::sqlite::LogDb;

    /// A log of `n` contacts, none of which carries an id — a pre-1.14 file, which is the case
    /// the migration exists for.
    fn legacy_log(n: usize) -> String {
        let mut s = super::super::adif_header();
        for i in 0..n {
            s.push_str(&format!(
                "<CALL:{}>{}<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8\
                 <QSO_DATE:8>20260101<TIME_ON:6>{:02}{:02}{:02}<EOR>\n",
                format!("K{i}ABC").len(),
                format_args!("K{i}ABC"),
                i / 3600 % 24,
                i / 60 % 60,
                i % 60,
            ));
        }
        s
    }

    struct Dir(std::path::PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            let p = std::env::temp_dir().join(format!(
                "nexus-migrate-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).expect("temp dir");
            Dir(p)
        }
        fn log(&self) -> PathBuf {
            self.0.join("log.adi")
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

    /// Carries the scratch directory to the child process of the kill test.
    const KILL_DIR: &str = "NEXUS_MIGRATE_KILL_DIR";

    fn unresolved(_: &QsoRecord) -> Resolved<'static> {
        Resolved::default()
    }

    /// The database sits beside the log under the log's own stem, and `Logbook::data_files`
    /// names it apart from the files that may be byte-copied — with its `-wal`/`-shm` in NEITHER
    /// list, because they are not files a copy may carry on their own.
    #[test]
    fn the_database_is_named_beside_the_log_and_listed_apart_from_the_plain_files() {
        let d = Dir::new("names");
        assert_eq!(database_path(&d.log()), d.0.join("log.sqlite3"));
        assert_eq!(
            database_path(Path::new("/x/other.adi")),
            Path::new("/x/other.sqlite3")
        );
        for name in [
            "log.adi",
            "log.adi.bak",
            "log.adi.pre-sqlite",
            "log.sqlite3",
            "log.sqlite3-wal",
            "log.sqlite3-shm",
            "backups/log-20260901-120000.adi",
            "backups/unrelated.txt",
        ] {
            let p = d.0.join(name);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"x").unwrap();
        }
        let files = Logbook::data_files(&d.log());
        let mut plain: Vec<_> = files
            .files
            .iter()
            .map(|p| p.strip_prefix(&d.0).unwrap().to_string_lossy().into_owned())
            .collect();
        plain.sort();
        assert_eq!(
            plain,
            [
                "backups/log-20260901-120000.adi",
                "log.adi",
                "log.adi.bak",
                "log.adi.pre-sqlite",
            ],
            "the log and its three kinds of safety copy — and nothing of the database's"
        );
        assert_eq!(files.database, Some(d.0.join("log.sqlite3")));

        // Only what exists: a folder with no database names none.
        std::fs::remove_file(d.0.join("log.sqlite3")).unwrap();
        assert_eq!(Logbook::data_files(&d.log()).database, None);
    }

    /// The whole point, and the assertion is on the RECORDS in the store — not on an export of
    /// them, which is blind to whatever an export may legitimately leave out.
    #[test]
    fn every_record_reaches_the_store() {
        let d = Dir::new("all");
        std::fs::write(d.log(), legacy_log(700)).unwrap();

        let out = migrate_log(&d.log(), &d.db(), unresolved).expect("converts");
        assert_eq!(
            out,
            Outcome::Converted {
                written: 700,
                resumed_at: 0,
                total: 700
            }
        );

        let db = LogDb::open(&d.db()).unwrap();
        assert_eq!(db.row_count().unwrap(), 700, "every record is in");
        assert_eq!(db.meta(DONE).unwrap(), Some(1), "and the store says so");
        let stored = db.load_all().unwrap();
        let source = Logbook::load(&d.log());
        assert_eq!(stored.len(), 700, "the store holds every record");
        // Deep equality per record, not a count and not a round-tripped export: a count alone
        // cannot tell 700 correct records from 700 copies of the first one.
        for (a, b) in stored.iter().zip(source.records()) {
            assert_eq!(a, &**b, "a stored record differs from the one in the log");
        }
    }

    /// The disaster case: a conversion that stops part way. It must resume, and the result must
    /// be the same log — no record written twice, none skipped.
    ///
    /// The interruption is real rather than simulated: the first attempt is run against a source
    /// whose records are inserted and then abandoned before `migration_done` is set, which is
    /// exactly the state a kill leaves (a chunk is one transaction, so a crash lands between
    /// chunks, never inside one).
    #[test]
    fn an_interrupted_conversion_resumes_without_duplicating_or_dropping() {
        let d = Dir::new("resume");
        std::fs::write(d.log(), legacy_log(700)).unwrap();
        let source = Logbook::load(&d.log());

        // Attempt 1, stopped after two chunks: open the store, record the source length, write
        // 512 of the 700 records, and never mark it done.
        {
            let mut db = LogDb::open(&d.db()).unwrap();
            let len = std::fs::read(d.log()).unwrap().len() as i64;
            db.set_meta(SOURCE_LEN, len).unwrap();
            for chunk in source.records()[..512].chunks(CHUNK_ROWS) {
                let rows: Vec<_> = chunk.iter().map(|r| (&**r, Resolved::default())).collect();
                db.insert_all(rows).unwrap();
            }
            assert_eq!(db.meta(DONE).unwrap(), None, "the attempt did not finish");
        }

        let out = migrate_log(&d.log(), &d.db(), unresolved).expect("resumes");
        assert_eq!(
            out,
            Outcome::Converted {
                written: 188,
                resumed_at: 512,
                total: 700
            },
            "the resume picks up at the row count, not at zero and not past the end"
        );

        let stored = LogDb::open(&d.db()).unwrap().load_all().unwrap();
        assert_eq!(stored.len(), 700, "no record written twice, none dropped");
        for (a, b) in stored.iter().zip(source.records()) {
            assert_eq!(a, &**b);
        }
        let ids: std::collections::HashSet<_> = stored.iter().filter_map(|r| r.id).collect();
        assert_eq!(ids.len(), 700, "every stored id is distinct");
    }

    /// The copy has to be there after a conversion that FAILED, which is the only time it
    /// matters. Proved by pointing the conversion at a database path that cannot be created.
    #[test]
    fn the_copy_survives_a_conversion_that_fails() {
        let d = Dir::new("failcopy");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        // A directory where the database file should go: `Connection::open` cannot create it.
        let db_path = d.0.join("blocked.sqlite3");
        std::fs::create_dir(&db_path).unwrap();

        let err = migrate_log(&d.log(), &db_path, unresolved).expect_err("cannot open the store");
        assert!(matches!(err, Error::Db(_)), "failed at the database: {err}");

        let copy = pre_sqlite_path(&d.log());
        assert!(copy.is_file(), "the copy was taken before the store");
        assert_eq!(
            std::fs::read(&copy).unwrap(),
            std::fs::read(d.log()).unwrap(),
            "and it is the operator's log, byte for byte"
        );
    }

    /// A second attempt keeps the FIRST copy. Replacing it is how a safety copy comes to record
    /// the damage instead of the original.
    #[test]
    fn a_second_attempt_never_clobbers_the_first_copy() {
        let d = Dir::new("copyonce");
        std::fs::write(d.log(), legacy_log(3)).unwrap();
        let original = std::fs::read(d.log()).unwrap();
        migrate_log(&d.log(), &d.db(), unresolved).expect("converts");

        // The log grows, as it would from ordinary logging after the conversion.
        let mut grown = String::from_utf8(original.clone()).unwrap();
        grown.push_str("<CALL:5>W9ZZZ<BAND:3>40m<FREQ:5>7.074<MODE:2>CW<EOR>\n");
        std::fs::write(d.log(), &grown).unwrap();
        take_pre_sqlite_copy(&d.log(), grown.as_bytes()).expect("second attempt");

        assert_eq!(
            std::fs::read(pre_sqlite_path(&d.log())).unwrap(),
            original,
            "the copy still holds the log as it was before the first conversion"
        );
    }

    /// Running it twice must not import the log twice — the duplicate-import bug is the one an
    /// operator would notice as every contact appearing in their log two or three times.
    #[test]
    fn converting_an_already_converted_store_does_nothing() {
        let d = Dir::new("twice");
        std::fs::write(d.log(), legacy_log(300)).unwrap();
        migrate_log(&d.log(), &d.db(), unresolved).expect("converts");
        assert_eq!(
            migrate_log(&d.log(), &d.db(), unresolved).expect("second run"),
            Outcome::AlreadyDone
        );
        assert_eq!(LogDb::open(&d.db()).unwrap().row_count().unwrap(), 300);
    }

    /// An interrupted attempt whose SOURCE then changed cannot be resumed — the rows in the
    /// store are a prefix of a file that no longer exists. Converting the new file on top of
    /// them would interleave two logs, so the store is cleared and converted whole.
    #[test]
    fn a_changed_source_restarts_instead_of_stitching_two_logs() {
        let d = Dir::new("changed");
        std::fs::write(d.log(), legacy_log(400)).unwrap();
        let first = Logbook::load(&d.log());
        {
            let mut db = LogDb::open(&d.db()).unwrap();
            db.set_meta(SOURCE_LEN, 999_999).unwrap(); // a length this file does not have
            for chunk in first.records()[..256].chunks(CHUNK_ROWS) {
                let rows: Vec<_> = chunk.iter().map(|r| (&**r, Resolved::default())).collect();
                db.insert_all(rows).unwrap();
            }
        }

        let out = migrate_log(&d.log(), &d.db(), unresolved).expect("restarts");
        assert_eq!(
            out,
            Outcome::Converted {
                written: 400,
                resumed_at: 0,
                total: 400
            },
            "the whole file is converted, not the tail of it"
        );
        assert_eq!(
            LogDb::open(&d.db()).unwrap().row_count().unwrap(),
            400,
            "and the prefix of the OLD source is gone rather than left interleaved"
        );
    }

    /// ⛔ **`migration_done` must be written AFTER the last record, never before the first.**
    /// Set early, a conversion that dies half way comes back as "already converted" and the
    /// operator silently loses every contact the interrupted run had not reached — the worst
    /// outcome this module can produce, and one no success-path test can see.
    ///
    /// The failure is made real rather than simulated: the store is seeded so that a row the
    /// conversion is going to insert is ALREADY there under its own id, which makes
    /// `insert_all` fail on a duplicate primary key part way through the loop.
    #[test]
    fn a_conversion_that_fails_part_way_is_not_marked_done() {
        let d = Dir::new("notdone");
        std::fs::write(d.log(), legacy_log(700)).unwrap();
        let source = Logbook::load(&d.log());
        let len = std::fs::read(d.log()).unwrap().len() as i64;
        {
            let mut db = LogDb::open(&d.db()).unwrap();
            db.set_meta(SOURCE_LEN, len).unwrap();
            // A prefix, as an interrupted run leaves...
            let rows: Vec<_> = source.records()[..256]
                .iter()
                .map(|r| (&**r, Resolved::default()))
                .collect();
            db.insert_all(rows).unwrap();
            // ...plus one row from far beyond it, so the resume walks into its own id.
            db.insert(&source.records()[600], Resolved::default())
                .unwrap();
        }

        let err = migrate_log(&d.log(), &d.db(), unresolved).expect_err("fails in the loop");
        assert!(matches!(err, Error::Db(_)), "failed at the store: {err}");
        assert_eq!(
            LogDb::open(&d.db()).unwrap().meta(DONE).unwrap(),
            None,
            "a conversion that did not finish must not be marked done"
        );
    }

    /// A store written by a LATER build is refused, and refusing it must touch nothing.
    #[test]
    fn a_store_from_a_newer_build_is_refused() {
        let d = Dir::new("newer");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        LogDb::open(&d.db())
            .unwrap()
            .set_meta("schema_version", 9_999)
            .unwrap();

        let err = migrate_log(&d.log(), &d.db(), unresolved).expect_err("refused");
        assert!(
            matches!(err, Error::Db(sqlite::Error::SchemaVersion { .. })),
            "refused for the schema version, not something else: {err}"
        );
        assert!(
            LogDb::open(&d.db()).is_err(),
            "the store is still the newer one"
        );
    }

    /// Nothing to convert is not an error, and it is marked done so a later run does not go
    /// looking again.
    #[test]
    fn an_absent_or_empty_log_converts_to_nothing() {
        let d = Dir::new("empty");
        assert_eq!(
            migrate_log(&d.log(), &d.db(), unresolved).expect("missing file"),
            Outcome::Empty
        );
        assert!(
            !pre_sqlite_path(&d.log()).exists(),
            "and no copy is taken of a log that does not exist"
        );

        let d2 = Dir::new("headeronly");
        std::fs::write(d2.log(), super::super::adif_header()).unwrap();
        assert_eq!(
            migrate_log(&d2.log(), &d2.db(), unresolved).expect("header only"),
            Outcome::Empty
        );
        assert!(!pre_sqlite_path(&d2.log()).exists());
    }

    /// A poisoned log's key must not reach the copy — and the copy must still be swept if a
    /// poisoned one ARRIVES some other way.
    ///
    /// Two separate facts, and only the second needs the sweep. The copy is taken from the
    /// SCRUBBED bytes, so this module cannot create a poisoned one; but a copy can arrive from
    /// a restore, a profile sync or a laptop migration, and it is the one file beside the log
    /// that is never rotated — so nothing else would ever revisit it.
    #[test]
    fn a_credential_never_reaches_the_copy_and_is_swept_out_of_one_that_arrives() {
        const KEY: &str = "qrz10gb00kk3y0123456789ABCDEFxyz";
        let stamp = format!("rejected|1700000500|Unable to add QSO: bad key={KEY}");
        let poisoned = format!(
            "{}<CALL:5>W9XYZ<QSO_DATE:8>20231114<TIME_ON:6>221320<BAND:3>20m<MODE:3>FT8\
             <APP_TEMPO_UL_QRZ:{}>{}<eor>\n",
            super::super::adif_header(),
            stamp.len(),
            stamp
        );

        // (1) A poisoned log converts, and its key is not in the copy.
        let d = Dir::new("poison");
        std::fs::write(d.log(), &poisoned).unwrap();
        assert!(
            std::fs::read_to_string(d.log()).unwrap().contains(KEY),
            "control: the log really does hold the key before conversion"
        );
        migrate_log(&d.log(), &d.db(), unresolved).expect("converts");
        let copy = std::fs::read_to_string(pre_sqlite_path(&d.log())).unwrap();
        assert!(
            !copy.contains(KEY),
            "the copy is taken from the scrubbed bytes, so it never holds the key"
        );
        assert!(copy.contains("W9XYZ"), "control: it is still the contact");

        // (2) A copy that ARRIVES poisoned is swept, because it is in `backup_copy_paths`.
        let d2 = Dir::new("arrived");
        std::fs::write(
            d2.log(),
            super::super::adif_header() + "<CALL:5>W9XYZ<eor>\n",
        )
        .unwrap();
        let arrived = pre_sqlite_path(&d2.log());
        std::fs::write(&arrived, &poisoned).unwrap();
        assert!(
            std::fs::read_to_string(&arrived).unwrap().contains(KEY),
            "control: the arrived copy really does hold the key"
        );

        let _ = Logbook::load(&d2.log()); // the load is what sweeps the copies beside the log
        assert!(
            !std::fs::read_to_string(&arrived).unwrap().contains(KEY),
            "a copy that arrived poisoned must be swept — nothing else ever revisits it"
        );
    }

    /// ⛔ **The interrupted path, interrupted for real.** The conversion runs in a CHILD
    /// PROCESS which is `kill`ed part way through; this process then resumes it. Nothing is
    /// simulated — the child dies with the store open, mid-conversion, exactly as it would in a
    /// power cut or a force-quit.
    ///
    /// **Fixture: 6,000 contacts against `CHUNK_ROWS` = 256, so 24 chunks.** Well above the
    /// batch boundary, which matters twice over: a handful of contacts could not tell a
    /// conversion that migrated everything from one that migrated only the first chunk, and a
    /// conversion that finished before the kill landed would leave nothing to resume. The test
    /// ASSERTS that it caught a genuine partial state rather than passing quietly if it did not.
    #[test]
    fn a_conversion_killed_mid_flight_resumes_without_duplicating_or_dropping() {
        const TOTAL: usize = 6_000;
        let d = Dir::new("kill");
        std::fs::write(d.log(), legacy_log(TOTAL)).unwrap();
        let source = Logbook::load(&d.log());
        assert_eq!(source.len(), TOTAL, "fixture: the log holds what we think");
        // Checked at COMPILE time, so shrinking the fixture below the batch boundary fails
        // the build rather than quietly turning this into a first-page test.
        const _: () = assert!(TOTAL > CHUNK_ROWS * 4);

        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "--ignored",
                "--nocapture",
                "logbook::migrate::tests::convert_for_the_kill_test",
            ])
            .env(KILL_DIR, &d.0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the child conversion starts");

        // Wait until the child has COMMITTED at least one chunk, then kill it there. Read
        // through a second, read-only connection: WAL lets a reader see committed
        // transactions while the writer is still working.
        let partial = wait_for_a_committed_prefix(&d.db(), TOTAL);
        let _ = child.kill();
        let _ = child.wait();

        let partial = partial.expect("the child must be caught mid-conversion, not after it");
        assert!(
            partial > 0 && partial < TOTAL as u64,
            "the kill landed on a genuine partial state: {partial} of {TOTAL}"
        );
        assert_eq!(
            LogDb::open(&d.db()).unwrap().meta(DONE).unwrap(),
            None,
            "a killed conversion is not marked done"
        );

        // …and the resume finishes it.
        let out = migrate_log(&d.log(), &d.db(), unresolved).expect("the resume completes");
        let Outcome::Converted {
            resumed_at, total, ..
        } = out
        else {
            panic!("expected a resumed conversion, got {out:?}");
        };
        assert_eq!(total, TOTAL);
        assert!(resumed_at > 0, "it resumed rather than starting over");

        let db = LogDb::open(&d.db()).unwrap();
        assert_eq!(
            db.row_count().unwrap(),
            TOTAL as u64,
            "no record dropped and none written twice"
        );
        let stored = db.load_all().unwrap();
        let ids: std::collections::HashSet<_> = stored.iter().filter_map(|r| r.id).collect();
        assert_eq!(ids.len(), TOTAL, "every stored id is distinct");
        // Sampled deep equality, on the RECORDS — including rows either side of the kill.
        for i in [
            0,
            1,
            CHUNK_ROWS - 1,
            CHUNK_ROWS,
            partial as usize - 1,
            partial as usize,
            TOTAL - 1,
        ] {
            assert_eq!(
                &stored[i],
                &*source.records()[i],
                "stored record {i} differs from the one in the log"
            );
        }
    }

    /// The child half of the kill test. Ignored, so it never runs on its own; the parent
    /// invokes it by name with the directory in the environment.
    #[test]
    #[ignore = "invoked as a child process by the kill test"]
    fn convert_for_the_kill_test() {
        let Ok(dir) = std::env::var(KILL_DIR) else {
            return;
        };
        let d = PathBuf::from(dir);
        let _ = migrate_log(&d.join("log.adi"), &d.join("log.sqlite3"), unresolved);
    }

    /// How many records the store holds once it holds SOME but not all, read without disturbing
    /// the writer. `None` if that state never appeared before the deadline.
    fn wait_for_a_committed_prefix(db_path: &Path, total: usize) -> Option<u64> {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while std::time::Instant::now() < deadline {
            if let Ok(conn) = rusqlite::Connection::open_with_flags(
                db_path,
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            ) {
                if let Ok(n) =
                    conn.query_row("SELECT COUNT(*) FROM qso", [], |r| r.get::<_, i64>(0))
                {
                    if n > 0 && (n as usize) < total {
                        return Some(n as u64);
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        None
    }

    /// ⛔ **A second instance starting while the first is still converting WAITS, and then
    /// does nothing.** Two Nexus windows share one data folder. The first holds the conversion
    /// lock while it works; the second must neither read the half-made store's row count as a
    /// place to resume from nor convert alongside it — it waits, finds the store converted, and
    /// answers `AlreadyDone`. Every contact is in the store exactly once.
    ///
    /// Driven for real: this thread takes the lock as the first instance would, a second thread
    /// runs `migrate_log`, and it is shown NOT to have finished — or written a row — while the
    /// lock is held.
    #[test]
    fn a_second_instance_waits_for_the_first_to_finish_converting() {
        let d = Dir::new("twoinstances");
        std::fs::write(d.log(), legacy_log(600)).unwrap();
        let lock = ConversionLock::take(&d.db());
        assert!(
            lock.file.is_some(),
            "premise: this filesystem takes the lock"
        );

        let (log, db) = (d.log(), d.db());
        let second = std::thread::spawn(move || migrate_log(&log, &db, unresolved));
        std::thread::sleep(std::time::Duration::from_millis(400));
        assert!(
            !second.is_finished(),
            "the second instance is waiting on the lock"
        );
        assert!(
            !d.db().exists() || LogDb::open(&d.db()).unwrap().row_count().unwrap() == 0,
            "and has written nothing"
        );

        // The first instance converts, then lets go.
        let first = convert(&d.log(), &d.db(), unresolved).expect("the first converts");
        assert!(matches!(first, Outcome::Converted { total: 600, .. }));
        lock.finished();

        let second = second.join().unwrap().expect("the second completes");
        assert_eq!(second, Outcome::AlreadyDone, "and finds nothing left to do");
        let stored = LogDb::open(&d.db()).unwrap().load_all().unwrap();
        assert_eq!(stored.len(), 600, "every contact, once");
        assert!(
            !ConversionLock::path_for(&d.db()).exists(),
            "the lock file is gone once the conversion is done"
        );
    }

    /// ⛔ **A launch can see another window's conversion without joining its queue.** The answer
    /// comes from the lock a conversion really holds — not from the lock FILE, which an
    /// interrupted conversion leaves behind — and asking must neither wait nor create the file.
    #[test]
    fn a_conversion_in_progress_is_seen_from_its_lock_and_only_while_it_is_held() {
        let d = Dir::new("inprogress");
        let file = ConversionLock::path_for(&d.db());
        assert!(
            !conversion_in_progress(&d.db()),
            "no lock file: nothing is converting"
        );
        assert!(!file.exists(), "and asking did not create one");

        let lock = ConversionLock::take(&d.db());
        assert!(
            lock.file.is_some(),
            "premise: this filesystem takes the lock"
        );
        // Asked from another thread with a deadline, so a check that WAITS on the lock fails
        // here instead of hanging the suite.
        let db = d.db();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(conversion_in_progress(&db));
        });
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(5)),
            Ok(true),
            "held: a conversion is running, and asking did not wait for it"
        );

        drop(lock); // what a crash or a kill leaves: the file, and nobody holding it
        assert!(
            file.exists(),
            "premise: an interrupted conversion leaves its file"
        );
        assert!(
            !conversion_in_progress(&d.db()),
            "a lock file nobody holds is an interrupted conversion, not a running one"
        );

        ConversionLock::take(&d.db()).finished();
        assert!(
            !conversion_in_progress(&d.db()),
            "finished: nothing is converting"
        );
    }

    /// ⛔ **A FULL DISK, for real.** The conversion runs in a child process whose file-size
    /// limit (`ulimit -f`, with `SIGXFSZ` ignored so a write past it fails with `EFBIG` rather
    /// than killing the process) lets the safety copy through and stops the database part way —
    /// which SQLite reports exactly as it reports a full disk. Afterwards:
    /// - the conversion is NOT marked done,
    /// - `log.adi` is exactly as it was,
    /// - `log.adi.pre-sqlite` exists and is the operator's log, byte for byte,
    /// - and the next attempt, with room, converts every contact exactly once.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_full_disk_mid_conversion_leaves_the_log_and_its_copy_and_the_retry_completes() {
        const TOTAL: usize = 6_000;
        let d = Dir::new("fulldisk");
        std::fs::write(d.log(), legacy_log(TOTAL)).unwrap();
        let before = std::fs::read(d.log()).unwrap();
        // 2048 blocks is 1 MiB (512-byte blocks) or 2 MiB (1 KiB) depending on the shell's
        // unit — above the log (~0.8 MiB) either way, and far below the store (~6 MiB).
        assert!(
            before.len() < 1024 * 1024,
            "premise: the copy fits under the limit"
        );
        let status = std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("ulimit -f 2048 && trap '' XFSZ && exec \"$0\" \"$@\"")
            .arg(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "--ignored",
                "--nocapture",
                "logbook::migrate::tests::convert_for_the_full_disk_test",
            ])
            .env(KILL_DIR, &d.0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .expect("the child runs");
        assert!(
            status.success(),
            "the child ran to the end (its conversion failed inside it)"
        );
        let verdict = std::fs::read_to_string(d.0.join("verdict")).expect("the child's verdict");
        assert!(
            verdict.starts_with("err:"),
            "the conversion must have FAILED on the limit, not finished: {verdict}"
        );
        assert!(
            verdict.contains("full") || verdict.contains("large") || verdict.contains("I/O"),
            "and failed on the disk, not on something else: {verdict}"
        );

        assert_eq!(
            std::fs::read(d.log()).unwrap(),
            before,
            "log.adi is untouched"
        );
        assert_eq!(
            std::fs::read(pre_sqlite_path(&d.log())).unwrap(),
            before,
            "the permanent copy exists and is the operator's log"
        );
        assert_ne!(
            LogDb::open(&d.db()).unwrap().meta(DONE).unwrap(),
            Some(1),
            "a conversion the disk stopped is not marked done"
        );

        // With room again, the next attempt finishes the job — once per contact.
        let out = migrate_log(&d.log(), &d.db(), unresolved).expect("completes with room");
        assert!(
            matches!(out, Outcome::Converted { total: TOTAL, .. }),
            "{out:?}"
        );
        let db = LogDb::open(&d.db()).unwrap();
        assert_eq!(db.row_count().unwrap(), TOTAL as u64);
        let ids: std::collections::HashSet<_> =
            db.load_all().unwrap().iter().filter_map(|r| r.id).collect();
        assert_eq!(ids.len(), TOTAL, "no contact twice");
    }

    /// The child half of the full-disk test: convert, and write what happened where the parent
    /// can read it (stdout is the test harness's).
    #[test]
    #[ignore = "invoked as a child process by the full-disk test"]
    fn convert_for_the_full_disk_test() {
        let Ok(dir) = std::env::var(KILL_DIR) else {
            return;
        };
        let d = PathBuf::from(dir);
        let verdict = match migrate_log(&d.join("log.adi"), &d.join("log.sqlite3"), unresolved) {
            Ok(o) => format!("ok: {o:?}"),
            Err(e) => format!("err: {e}"),
        };
        // The limit applies to this write too; the verdict is tiny.
        let _ = std::fs::write(d.join("verdict"), verdict);
    }

    /// A log this build cannot read a single contact out of must NOT be mistaken for an empty
    /// one. "Empty" marks the store converted, and a converted store is never looked at again —
    /// so a corrupt `log.adi` would write the operator's whole history off without a word.
    #[test]
    fn a_log_with_content_but_no_readable_contacts_is_refused_not_called_empty() {
        let d = Dir::new("garbage");
        // Content after `<EOH>`, none of it a record this build can read.
        let junk = format!(
            "{}\u{0}\u{1}not an adif file at all\n",
            super::super::adif_header()
        );
        std::fs::write(d.log(), &junk).unwrap();
        let before = std::fs::read(d.log()).unwrap();

        let err = migrate_log(&d.log(), &d.db(), unresolved).expect_err("refused");
        assert!(
            matches!(err, Error::Unreadable { .. }),
            "refused as unreadable, not something else: {err}"
        );
        assert!(
            err.to_string().contains("has not been changed"),
            "the refusal says plainly what it did and did not do: {err}"
        );
        assert_eq!(
            std::fs::read(d.log()).unwrap(),
            before,
            "the operator's file is untouched"
        );
        assert_eq!(
            LogDb::open(&d.db()).unwrap().meta(DONE).unwrap(),
            None,
            "and NOTHING is marked done, so a later run still looks at it"
        );
        assert_eq!(
            LogDb::open(&d.db()).unwrap().row_count().unwrap(),
            0,
            "no half-populated store is left behind"
        );
    }

    /// A log truncated mid-record converts the contacts that survive it, leaves the operator's
    /// file alone, and marks only what it actually did. The parser resyncs at the next `<`, so a
    /// torn tail costs its own record and nothing else.
    #[test]
    fn a_truncated_log_converts_what_survives_and_leaves_the_file_alone() {
        let d = Dir::new("truncated");
        let whole = legacy_log(600);
        // Cut in the middle of the last record, as a half-written file ends.
        let cut = whole.len() - 40;
        std::fs::write(d.log(), &whole[..cut]).unwrap();
        let before = std::fs::read(d.log()).unwrap();
        let readable = Logbook::load(&d.log()).len();
        assert!(
            (598..600).contains(&readable),
            "control: the truncation really did cost a record, and only about one: {readable}"
        );

        let out = migrate_log(&d.log(), &d.db(), unresolved).expect("converts what it can");
        assert_eq!(
            out,
            Outcome::Converted {
                written: readable,
                resumed_at: 0,
                total: readable
            }
        );
        assert_eq!(
            std::fs::read(d.log()).unwrap(),
            before,
            "the operator's file is untouched by the conversion"
        );
        let db = LogDb::open(&d.db()).unwrap();
        assert_eq!(db.row_count().unwrap(), readable as u64);
        // On the RECORDS: a torn tail must not have produced a mangled row anywhere.
        let stored = db.load_all().unwrap();
        let source = Logbook::load(&d.log());
        for (a, b) in stored.iter().zip(source.records()) {
            assert_eq!(a, &**b);
        }
    }

    /// The copy must not be written world-readable: an operator's log holds their whole
    /// history, and a pre-1.11 one can hold a service's echoed API key.
    #[cfg(unix)]
    #[test]
    fn the_copy_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("mode");
        std::fs::write(d.log(), legacy_log(4)).unwrap();
        migrate_log(&d.log(), &d.db(), unresolved).expect("converts");
        let mode = std::fs::metadata(pre_sqlite_path(&d.log()))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600, "the copy is owner-only");
    }

    /// A conversion refuses outright if the copy cannot be written. Every other failure here
    /// can be retried; converting a log whose original is not safely copied cannot be undone.
    #[cfg(unix)]
    #[test]
    fn a_copy_that_cannot_be_written_refuses_the_conversion() {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("readonly");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let db_path = d.0.join("sub").join("log.sqlite3");
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        // Make the log's own directory read-only, so the copy beside it cannot be created.
        let mut perms = std::fs::metadata(&d.0).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&d.0, perms).unwrap();

        let result = migrate_log(&d.log(), &db_path, unresolved);

        let mut perms = std::fs::metadata(&d.0).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&d.0, perms).unwrap();

        let err = result.expect_err("a conversion without a safety copy is refused");
        assert!(
            matches!(err, Error::Copy { .. }),
            "refused at the copy: {err}"
        );
        assert!(
            !db_path.exists(),
            "and no database was created, so a retry starts clean"
        );
    }
}
