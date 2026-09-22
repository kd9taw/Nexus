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
            Error::Db(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Copy { source, .. } => Some(source),
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
        db.set_meta(DONE, 1)?;
        return Ok(Outcome::Empty);
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

    fn unresolved(_: &QsoRecord) -> Resolved<'static> {
        Resolved::default()
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
