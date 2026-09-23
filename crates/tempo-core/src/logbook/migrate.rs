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
//! 2. Only then is the database opened or created — its tables, not yet its indexes.
//! 3. The records go in in file order, one transaction per chunk.
//! 4. The indexes are built over the finished table, one transaction each.
//! 5. The conversion is marked done as its LAST act, so an interrupted run is never mistaken
//!    for a finished one — including one interrupted while it built the indexes.
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

/// Records per transaction. It is also the resume granularity: an interrupted conversion loses
/// at most this many records' worth of WORK — about a second's, measured at 150,000 contacts —
/// and never a record.
///
/// ⭐ **Chosen from measurements; after deferring the indexes it is the biggest lever left.** A
/// record's id is a hash, so its rows land at random places in the primary keys of `qso` and of
/// its six-odd `qso_extra` rows, and every commit rewrites every key page it touched. At the
/// writer thread's 256, with the nine indexes already deferred, a 150,000-contact conversion
/// still wrote 3.8 GB and spent 12 s in the store. The store's time and bytes, measured at
/// 150,000 / 300,000 contacts: 16,384 → 2.9 s, 0.68 GB / 6.4 s, 2.0 GB; **65,536 → 2.5 s,
/// 0.39 GB / 5.4 s, 0.96 GB**; the whole log in one transaction → no faster (5.7 s at 300,000,
/// the transaction outgrowing the page cache), a write-ahead log as big as the database, and all
/// the work lost to a crash. On an SD card or a spinning disk it is the bytes that cost, so the
/// gap there is wider than on the machine these were measured on.
///
/// The writer thread's reason for small chunks — a long transaction holds the write lock a
/// contest keystroke may be waiting on — does not arise here: the conversion runs before the
/// session starts, and a second window waits on the conversion lock, not on the store.
const CHUNK_ROWS: usize = 65_536;

/// Contacts between two [`Progress::Convert`] reports inside one chunk. A chunk is one
/// transaction and now a big one, so reports at commits alone would move a progress bar in a
/// few large jumps.
const PROGRESS_EVERY: usize = 4_096;

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

/// How far a conversion has got, for a caller that shows it. A first launch on a lifetime log is
/// the one long wait an operator sees, and a count that moves is the difference between a wait
/// and what looks like a hang.
///
/// Reported in this order: [`Progress::Copy`] once; [`Progress::Convert`] as the contacts go in —
/// every few thousand, and whenever a chunk commits; then [`Progress::Index`] as each index is
/// built. [`Progress::Attach`] is not reported from here — it belongs to the caller that reads the
/// finished store back (`tempo_app::logstore::open_with_progress`). A store that is already
/// converted reports nothing at all from here, and neither does a second window waiting for the
/// first one's conversion.
///
/// The callback is called on the converting thread, some of the time inside a transaction: a
/// slow callback slows the conversion and nothing else, since nothing else writes to the store
/// while it converts. Keep it quick — hand the value to the thread that draws it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Progress {
    /// Reading `log.adi`, keeping it as `log.adi.pre-sqlite`, and reading the contacts out of it.
    /// No count yet: until the file is read there is nothing to count.
    Copy,
    /// `done` of the log's `total` contacts are written to the store. They commit in chunks, so a
    /// crash goes back to the last chunk that committed — never further. A resumed conversion
    /// starts at what an earlier attempt committed, not at zero.
    Convert {
        /// Contacts written so far.
        done: usize,
        /// Contacts the log holds.
        total: usize,
    },
    /// `done` of the store's `total` indexes are built. Every contact is in by now.
    Index {
        /// Indexes built so far.
        done: usize,
        /// Indexes the store has.
        total: usize,
    },
    /// The converted store is being read back for the session.
    Attach,
}

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
    migrate_log_with_progress(log_path, db_path, resolve, &mut |_| {})
}

/// [`migrate_log`], telling `progress` how far it has got (see [`Progress`]).
pub fn migrate_log_with_progress<R>(
    log_path: &Path,
    db_path: &Path,
    resolve: R,
    progress: &mut dyn FnMut(Progress),
) -> Result<Outcome>
where
    R: Fn(&QsoRecord) -> Resolved<'static>,
{
    migrate_in_chunks(log_path, db_path, resolve, progress, CHUNK_ROWS)
}

/// [`migrate_log_with_progress`] with the transaction size named. Only the tests name another:
/// the properties they check — whole chunks, a resume from the row count — hold for any size,
/// but a fixture has to SPAN several chunks to show them, and at [`CHUNK_ROWS`] that would be a
/// fixture of hundreds of thousands of contacts.
fn migrate_in_chunks<R>(
    log_path: &Path,
    db_path: &Path,
    resolve: R,
    progress: &mut dyn FnMut(Progress),
    chunk_rows: usize,
) -> Result<Outcome>
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
    let out = convert(log_path, db_path, resolve, progress, chunk_rows)?;
    // Converted: nobody will need the lock file again — a later caller answers from the store
    // on the fast path above and never reaches it. Kept after a FAILURE, because a caller
    // already waiting on it will try again and a newcomer must wait on the same file.
    lock.finished();
    Ok(out)
}

/// Whether the store at `db_path` is marked converted. The store must exist.
///
/// Asked through [`LogDb::open_for_conversion`], which writes nothing to a store that has its
/// tables. An ordinary open would build the indexes a conversion still in progress has not built
/// yet — taking the write lock out from under that conversion, which is exactly the store a
/// second window asks about while the first one converts.
pub fn is_converted(db_path: &Path) -> Result<bool> {
    Ok(LogDb::open_for_conversion(db_path)?.meta(DONE)? == Some(1))
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
fn convert<R>(
    log_path: &Path,
    db_path: &Path,
    resolve: R,
    progress: &mut dyn FnMut(Progress),
    chunk_rows: usize,
) -> Result<Outcome>
where
    R: Fn(&QsoRecord) -> Resolved<'static>,
{
    progress(Progress::Copy);
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

    let mut db = LogDb::open_for_conversion(db_path)?;
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

    let mut done = resumed_at;
    progress(Progress::Convert { done, total });
    for chunk in log.records()[resumed_at..].chunks(chunk_rows) {
        let start = done;
        // Reported as the rows go in, not only when the chunk commits: see `PROGRESS_EVERY`.
        let rows = chunk.iter().enumerate().map(|(i, r)| {
            if i > 0 && (start + i) % PROGRESS_EVERY == 0 {
                progress(Progress::Convert {
                    done: start + i,
                    total,
                });
            }
            (&**r, resolve(r))
        });
        db.insert_all(rows)?;
        done += chunk.len();
        progress(Progress::Convert { done, total });
    }

    // The indexes, over the finished table: one sorted pass each instead of nine kept up row by
    // row. BEFORE the conversion is marked done, so a crash while they are built leaves a store
    // that is not done — and the resume, finding every row in, builds the ones still missing.
    db.build_indexes(&mut |done, total| progress(Progress::Index { done, total }))?;

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

    /// The transaction size for a test whose fixture has to span several chunks (see
    /// [`migrate_in_chunks`]).
    const TEST_CHUNK_ROWS: usize = 256;

    /// `migration_done` as the store holds it, read on a read-only connection of its own. An
    /// ordinary [`LogDb::open`] would build the indexes an interrupted conversion had not reached
    /// yet, and so change the very state a resume is about to start from.
    fn done_mark(db_path: &Path) -> Option<i64> {
        rusqlite::Connection::open_with_flags(db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .expect("the store opens read-only")
            .query_row("SELECT v FROM log_meta WHERE k = ?1", [DONE], |r| r.get(0))
            .ok()
    }

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

        // Every index, read before anything could build a missing one: the conversion defers
        // them to the end, so a store it finished without them would look complete to every
        // other check here and only be slow.
        assert_eq!(
            schema(&d.db()),
            ordinary_schema(),
            "the converted store has every table and index an ordinary store has"
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
    /// **Fixture: 6,000 contacts in chunks of [`TEST_CHUNK_ROWS`] = 256, so 24 chunks.** Well
    /// above the batch boundary, which matters twice over: a handful of contacts could not tell a
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
        const _: () = assert!(TOTAL > TEST_CHUNK_ROWS * 4);

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
            done_mark(&d.db()),
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
            TEST_CHUNK_ROWS - 1,
            TEST_CHUNK_ROWS,
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
        let _ = migrate_in_chunks(
            &d.join("log.adi"),
            &d.join("log.sqlite3"),
            unresolved,
            &mut |_| {},
            TEST_CHUNK_ROWS,
        );
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

    /// The store's schema as SQLite records it: every table and index, by name and definition.
    /// Read on a connection of its own, READ-ONLY — an ordinary [`LogDb::open`] builds any index
    /// that is missing, which is exactly the state some of these tests have to look at.
    fn schema(db_path: &Path) -> Vec<(String, String, Option<String>)> {
        let conn = rusqlite::Connection::open_with_flags(
            db_path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .expect("the store opens read-only");
        let mut stmt = conn
            .prepare("SELECT type, name, sql FROM sqlite_master ORDER BY type, name")
            .unwrap();
        let rows = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
            .unwrap();
        rows.map(|r| r.unwrap()).collect()
    }

    /// The schema an ordinary open gives a new store — every table, every index.
    fn ordinary_schema() -> Vec<(String, String, Option<String>)> {
        let d = Dir::new("ordinary-schema");
        drop(LogDb::open(&d.db()).unwrap());
        schema(&d.db())
    }

    /// The names of the store's secondary indexes (the ones SQLite does not make for a key).
    fn index_names(db_path: &Path) -> Vec<String> {
        schema(db_path)
            .into_iter()
            .filter(|(kind, _, sql)| kind == "index" && sql.is_some())
            .map(|(_, name, _)| name)
            .collect()
    }

    /// ⛔ **A conversion killed while it builds the indexes resumes, builds the rest, and only
    /// then says it is done.** The indexes are built over the finished table, after the last
    /// contact and BEFORE `migration_done` — so the crash this test makes leaves every contact
    /// in, some indexes built and some not, and the store NOT marked converted. The resume must
    /// then write no contact twice, build only what is missing (building one that exists would
    /// fail the whole resume), and finish with the same schema an ordinary store has.
    ///
    /// The kill is real and lands where it is aimed: the child stops itself inside its progress
    /// callback once half the indexes are built — between two index builds, the store open, the
    /// write-ahead log unflushed — and this process kills it there.
    #[test]
    fn a_conversion_killed_while_it_builds_the_indexes_resumes_and_builds_the_rest() {
        const TOTAL: usize = 600;
        let d = Dir::new("killindex");
        std::fs::write(d.log(), legacy_log(TOTAL)).unwrap();
        let before = std::fs::read(d.log()).unwrap();

        let mut child = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "--ignored",
                "--nocapture",
                "logbook::migrate::tests::convert_until_half_the_indexes_are_built",
            ])
            .env(KILL_DIR, &d.0)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("the child conversion starts");
        let marker = d.0.join("stopped-while-indexing");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while !marker.exists() && std::time::Instant::now() < deadline {
            if child.try_wait().ok().flatten().is_some() {
                break; // it ran to the end without ever stopping in the index phase
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let _ = child.kill();
        let _ = child.wait();
        let stopped_at = std::fs::read_to_string(&marker)
            .expect("the child must be caught while it builds the indexes, not after");

        // What the crash left.
        let all = ordinary_schema();
        let all_indexes = all
            .iter()
            .filter(|(kind, _, sql)| kind == "index" && sql.is_some())
            .count();
        let built = index_names(&d.db()).len();
        assert!(
            built > 0 && built < all_indexes,
            "the kill landed part-way through the indexes ({stopped_at}): {built} of \
             {all_indexes} built"
        );
        {
            let probe = rusqlite::Connection::open_with_flags(
                d.db(),
                rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let rows: i64 = probe
                .query_row("SELECT COUNT(*) FROM qso", [], |r| r.get(0))
                .unwrap();
            assert_eq!(
                rows, TOTAL as i64,
                "every contact was in before the indexes"
            );
        }
        assert_eq!(
            done_mark(&d.db()),
            None,
            "a conversion killed mid-index is NOT marked done"
        );
        assert_eq!(
            std::fs::read(d.log()).unwrap(),
            before,
            "log.adi is untouched"
        );

        // The resume.
        let out = migrate_log(&d.log(), &d.db(), unresolved).expect("the resume completes");
        assert_eq!(
            out,
            Outcome::Converted {
                written: 0,
                resumed_at: TOTAL,
                total: TOTAL
            },
            "nothing left to insert — only the indexes to finish"
        );
        assert_eq!(
            schema(&d.db()),
            all,
            "every index is built, as an ordinary store has"
        );
        let db = LogDb::open(&d.db()).unwrap();
        assert_eq!(db.meta(DONE).unwrap(), Some(1), "and it is marked done now");
        let stored = db.load_all().unwrap();
        let source = Logbook::load(&d.log());
        assert_eq!(
            stored.len(),
            TOTAL,
            "no contact dropped, none written twice"
        );
        for (a, b) in stored.iter().zip(source.records()) {
            assert_eq!(a, &**b);
        }
        assert_eq!(
            std::fs::read(pre_sqlite_path(&d.log())).unwrap(),
            before,
            "the copy is the operator's log"
        );
    }

    /// ⛔ **Asking whether a store is converted writes nothing to it.** A second window asks
    /// while the first may still be converting, and that store has no indexes yet: an ordinary
    /// open would build them — holding the write lock for as long as that takes, out from under
    /// the first window's next chunk, whose wait is bounded by `busy_timeout`.
    #[test]
    fn asking_whether_a_store_is_converted_writes_nothing_to_it() {
        let d = Dir::new("askonly");
        std::fs::write(d.log(), legacy_log(300)).unwrap();
        let source = Logbook::load(&d.log());
        {
            // A store part-way through its conversion: rows in, no indexes, not done.
            let mut db = LogDb::open_for_conversion(&d.db()).unwrap();
            let rows: Vec<_> = source.records()[..200]
                .iter()
                .map(|r| (&**r, Resolved::default()))
                .collect();
            db.insert_all(rows).unwrap();
        }
        let before = schema(&d.db());
        assert!(index_names(&d.db()).is_empty(), "premise: no index yet");

        assert!(!is_converted(&d.db()).unwrap(), "it is not converted");
        assert_eq!(schema(&d.db()), before, "and asking built nothing");

        // Control: an ordinary open, on the same store, does build them — so the check above
        // could have seen a write had there been one.
        drop(LogDb::open(&d.db()).unwrap());
        assert_eq!(
            schema(&d.db()),
            ordinary_schema(),
            "control: an ordinary open builds every missing index"
        );
    }

    /// What a conversion reports is the contract a launch screen is built on: the copy; then the
    /// contacts, from where an earlier attempt got to, never going back and never more than
    /// [`PROGRESS_EVERY`] apart — inside a big chunk as well as across small ones — up to the
    /// last; then each index. Nothing else, and from a store already converted, nothing at all.
    #[test]
    fn a_conversion_reports_how_far_it_has_got() {
        let indexes = ordinary_schema()
            .iter()
            .filter(|(kind, _, sql)| kind == "index" && sql.is_some())
            .count();
        let check = |seen: &[Progress], from: usize, total: usize, what: &str| {
            assert_eq!(seen.first(), Some(&Progress::Copy), "{what}: {seen:?}");
            let counts: Vec<usize> = seen[1..]
                .iter()
                .map_while(|p| match *p {
                    Progress::Convert { done, total: t } if t == total => Some(done),
                    _ => None,
                })
                .collect();
            assert_eq!(
                counts.first(),
                Some(&from),
                "{what}: starts where it resumes"
            );
            assert_eq!(counts.last(), Some(&total), "{what}: ends at every contact");
            for w in counts.windows(2) {
                assert!(
                    w[0] < w[1] && w[1] - w[0] <= PROGRESS_EVERY,
                    "{what}: moves forward, never more than {PROGRESS_EVERY} at a time: {counts:?}"
                );
            }
            let rest: Vec<Progress> = seen[1 + counts.len()..].to_vec();
            let built: Vec<Progress> = (0..=indexes)
                .map(|done| Progress::Index {
                    done,
                    total: indexes,
                })
                .collect();
            assert_eq!(rest, built, "{what}: then each index, and nothing else");
        };

        // The shipped chunk size, with a log inside one chunk: the count still moves as the
        // rows go in, not only when the chunk commits.
        const ONE_CHUNK: usize = PROGRESS_EVERY * 2 + 11;
        const _: () = assert!(ONE_CHUNK < CHUNK_ROWS);
        let d = Dir::new("progress");
        std::fs::write(d.log(), legacy_log(ONE_CHUNK)).unwrap();
        let mut seen = Vec::new();
        migrate_log_with_progress(&d.log(), &d.db(), unresolved, &mut |p| seen.push(p))
            .expect("converts");
        check(&seen, 0, ONE_CHUNK, "one big chunk");
        assert!(
            seen.contains(&Progress::Convert {
                done: PROGRESS_EVERY,
                total: ONE_CHUNK
            }),
            "reported from inside the chunk: {seen:?}"
        );

        seen.clear();
        let again = migrate_log_with_progress(&d.log(), &d.db(), unresolved, &mut |p| seen.push(p))
            .expect("opens");
        assert_eq!(again, Outcome::AlreadyDone);
        assert!(
            seen.is_empty(),
            "a converted store reports nothing: {seen:?}"
        );

        // Many small chunks, and a resume: the count starts at what the earlier attempt
        // committed.
        const MANY: usize = TEST_CHUNK_ROWS * 3 + 7;
        let d = Dir::new("progress-resume");
        std::fs::write(d.log(), legacy_log(MANY)).unwrap();
        let source = Logbook::load(&d.log());
        {
            let mut db = LogDb::open_for_conversion(&d.db()).unwrap();
            let len = std::fs::read(d.log()).unwrap().len() as i64;
            db.set_meta(SOURCE_LEN, len).unwrap();
            let rows: Vec<_> = source.records()[..TEST_CHUNK_ROWS]
                .iter()
                .map(|r| (&**r, Resolved::default()))
                .collect();
            db.insert_all(rows).unwrap();
        }
        seen.clear();
        migrate_in_chunks(
            &d.log(),
            &d.db(),
            unresolved,
            &mut |p| seen.push(p),
            TEST_CHUNK_ROWS,
        )
        .expect("resumes");
        check(&seen, TEST_CHUNK_ROWS, MANY, "a resume in small chunks");
    }

    /// The child half of the index-phase kill test: convert, and once half the indexes are
    /// built, say so and wait to be killed right there.
    #[test]
    #[ignore = "invoked as a child process by the index-phase kill test"]
    fn convert_until_half_the_indexes_are_built() {
        let Ok(dir) = std::env::var(KILL_DIR) else {
            return;
        };
        let d = PathBuf::from(dir);
        let marker = d.join("stopped-while-indexing");
        let _ = migrate_log_with_progress(
            &d.join("log.adi"),
            &d.join("log.sqlite3"),
            unresolved,
            &mut |p| {
                if let Progress::Index { done, total } = p {
                    if done > 0 && done * 2 >= total {
                        let _ = std::fs::write(&marker, format!("{done} of {total}"));
                        loop {
                            std::thread::sleep(std::time::Duration::from_secs(1));
                        }
                    }
                }
            },
        );
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
        let first = convert(&d.log(), &d.db(), unresolved, &mut |_| {}, CHUNK_ROWS)
            .expect("the first converts");
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
            done_mark(&d.db()),
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
