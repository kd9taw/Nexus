//! The logbook's store, as the app holds it — SPEC-1 v3's **C9**, the switchover.
//!
//! The SQLite database beside `log.adi` becomes the DURABLE OWNER of the operator's log. The
//! whole log still lives in memory, exactly as before, inside [`crate::station::StationCore`]
//! behind the engine lock; what changes is how a change reaches the disk. It used to be a
//! whole-file rewrite of `log.adi` — tens of megabytes, under the lock the radio loop needs
//! every 20 ms — for every stamp, mark and edit. Now:
//!
//! 1. the change is made in memory (unchanged code, unchanged rules);
//! 2. the rows it touched are handed to the writer thread
//!    ([`tempo_core::logbook::writer::LogWriter`]) — a channel send, **no I/O**;
//! 3. the mirror lane is told the change's revision, and rewrites `log.adi` from the store once
//!    the store holds it — debounced, on its own thread, a chunk of the log at a time (SPEC-2 v3
//!    C15) — **no I/O** here;
//! 4. an operator command, having released every lock, waits for its own change to commit
//!    ([`Durability::wait`]). The FT auto-log never waits.
//!
//! # What this module owns
//!
//! - [`open`]: conversion of an existing `log.adi` (C5), the load from the store, the check that
//!   `log.adi` holds nothing the store lacks, and the two threads. All of it BEFORE the engine
//!   lock is taken, and all of it answers with a reason when it cannot — the caller then runs
//!   the session on `log.adi` exactly as 1.13 did, which is the one outcome that can never lose
//!   a contact.
//! - [`LogStore`]: the handles, the tickets a command is collecting, and this process's own
//!   changes still on their way to disk — which is what lets a reload after ANOTHER process's
//!   write keep them (see [`LogStore::reload`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tempo_core::logbook::hot::HotIndex;
use tempo_core::logbook::logfile::LogFileWriter;
use tempo_core::logbook::mirror::{
    self, FileStamp, MirrorOptions, MirrorState, MirrorWriter, StoreSource,
};
use tempo_core::logbook::reader::LogReader;
use tempo_core::logbook::sqlite::{self, LogDb, Resolved};
use tempo_core::logbook::writer::{self, Change, LogWriter, Refusal, Ticket, Touched};
use tempo_core::logbook::{migrate, Logbook, QsoRecord, RecordId, Watermarks};

use crate::station::{DxccResolve, StationKeys};

/// cty.dat's answer for a record — the entity NAME and CQ zone the store writes beside it.
/// Injected by the shell, which owns the table; tempo-app does not depend on `propagation`.
pub type StoreResolve = Arc<dyn Fn(&QsoRecord) -> Resolved<'static> + Send + Sync>;

/// How long an operator command waits for its change to reach the disk before it says so.
pub const DURABLE_WAIT: Duration = Duration::from_secs(60);

/// Rows per transaction when a session on the 1.13 path loads `log.adi` into its store in memory
/// — the conversion's size ([`LogStore::fallback`]).
const FALLBACK_LOAD_CHUNK: usize = 65_536;

/// What a change asks of `log.adi` on the 1.13 path ([`LogStore::fallback`]'s lane).
enum ToFile {
    /// Anything but an append: the file is rewritten from the store.
    Rewrite,
    /// These rows were appended at the end of the log, and nothing else changed: they are
    /// appended to the file.
    Append(Vec<Arc<QsoRecord>>),
    /// The rows came from the file: nothing to write.
    Nothing,
}

/// The store, open and owning the log.
pub struct LogStore {
    writer: Arc<LogWriter>,
    /// Read connections to the store, opened on first use — see [`StoreReads`].
    reader: Arc<LogReader>,
    /// Shared with a quit, which writes it with the engine lock released (see [`Unsaved`]).
    /// `None` for a store with no `log.adi` beside it — see [`LogStore::in_memory`] — and for a
    /// session on the 1.13 path, whose `log.adi` is kept by [`Self::lane`] instead.
    mirror: Option<Arc<MirrorWriter>>,
    /// A session on the 1.13 path ([`LogStore::fallback`]): the lane that keeps `log.adi` with
    /// 1.13's rules, where every change is saved once it is in the file. `None` otherwise.
    lane: Option<Arc<LogFileWriter>>,
    log_path: Option<PathBuf>,
    db_path: PathBuf,
    resolve: StoreResolve,
    /// This process's changes the writer has not finished with, and the rows each touched.
    inflight: Vec<InFlight>,
    /// This process's changes the writer GAVE UP ON: not in the store, their rows held here —
    /// sent again when the refusal can pass, held for the quit when it cannot. See
    /// [`LogStore::resend`].
    dropped: Vec<Dropped>,
    /// The latest refusal, in the store's words — what the screen says while any is held.
    last_refusal: Option<String>,
    /// The writer's count of ANOTHER process's commits when memory last matched the store.
    synced_foreign: u64,
    /// Tickets being collected for a command that will wait on them (see
    /// [`crate::engine::Engine::with_log_tickets`]).
    collector: Option<Vec<Ticket>>,
    /// The launch's placeholder: the empty store in memory its engine holds until the attach
    /// replaces it with the operator's log ([`Self::placeholder_until_attached`]). No change may
    /// reach it.
    placeholder: bool,
}

impl std::fmt::Debug for LogStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogStore")
            .field("db_path", &self.db_path)
            .field("log_path", &self.log_path)
            .field("inflight", &self.inflight.len())
            .field("dropped", &self.dropped.len())
            .field("on_log_adi", &self.lane.is_some())
            .finish()
    }
}

/// What [`open`] hands the station: the store, the records to hold, and — when `log.adi`
/// held something the store did not account for — that file, to be taken in.
pub struct Opened {
    pub(crate) store: LogStore,
    pub(crate) records: Vec<QsoRecord>,
    pub(crate) foreign: Option<ForeignLog>,
    /// The hot index of those records, built off the lock, when the caller asked for it
    /// ([`HotBuild`]).
    pub(crate) hot: Option<Prebuilt>,
    /// What the conversion did.
    pub outcome: migrate::Outcome,
}

/// How the launch has the open build the hot index ([`tempo_core::logbook::hot`]) from the rows
/// it loads, before the engine is locked (SPEC-2 v3 C19): with the DXCC resolver the station
/// will hold. The SAME resolver — the same `Arc` — is how the station knows the index is keyed
/// as it keys its own; handed any other, it builds its own from the log instead
/// ([`crate::station::StationCore::attach_store`]). The open also sets aside the rows a contest
/// session restored at launch is swept from ([`SessionRows`]).
#[derive(Clone)]
pub struct HotBuild {
    entity: Option<Arc<DxccResolve>>,
}

impl HotBuild {
    /// Build with `entity` as the DXCC resolver — `None` for a station that has none.
    pub fn keyed_by(entity: Option<Arc<DxccResolve>>) -> Self {
        Self { entity }
    }
}

/// A hot index the open built, the resolver it was keyed by, and the rows a contest session
/// can be swept from: every loaded row from `bound` on.
pub(crate) struct Prebuilt {
    pub(crate) index: HotIndex,
    pub(crate) entity: Option<Arc<DxccResolve>>,
    pub(crate) recent: Vec<QsoRecord>,
    pub(crate) bound: u64,
}

/// The log's rows from `bound` on — what a contest session's dupe sweep is built from (SPEC-2
/// v3 C19) — read before the Engine lock is taken, and what they were read against, so that
/// they are used only while they are still the log's rows
/// ([`crate::engine::Engine::open_session_from`]).
pub struct SessionRows {
    pub(crate) rows: Vec<QsoRecord>,
    /// Every row the log holds from here on is among `rows`.
    pub(crate) bound: u64,
    /// The log as the station held it when the rows were read.
    pub(crate) marks: Watermarks,
    /// Whether the rows are exactly the station's log from `bound` on: read in one picture of
    /// the store that held every change the station had made, and no other window's the station
    /// had not taken in.
    pub(crate) exact: bool,
    /// Whether the store answered in time ([`SESSION_READ_WAIT`]). One that did not — a big
    /// import being written, say — is not asked again: the session's first snapshot sweeps the
    /// log itself, as it always has.
    pub(crate) ready: bool,
}

/// How long a read for a contest session ([`SessionRead::read`]) waits — for the store to hold
/// the station's own changes, and for the writer's look at other windows' commits — before it
/// gives the read up. The switch waits for the read, so a store busy with a big write costs the
/// switch this, then the old way.
pub const SESSION_READ_WAIT: Duration = Duration::from_millis(500);

/// A read of [`SessionRows`] from the store, taken under the Engine lock
/// ([`crate::engine::Engine::session_read`]) and made after it is released ([`Self::read`]).
pub struct SessionRead {
    reads: StoreReads,
    writer: Arc<LogWriter>,
    /// The count of other windows' commits the station's log had taken in when this was taken.
    synced: u64,
    bound: u64,
    marks: Watermarks,
}

// Reads of the store's session rows, DEBUG BUILDS ONLY — per thread, like `LOG_SWEEPS`. The test
// that pins "a switch that opens no session reads nothing" reads it. (A plain comment: doc
// comments cannot attach through `thread_local!`.)
#[cfg(debug_assertions)]
thread_local! {
    pub static SESSION_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Every row the store holds logged at or after `bound`, whole — each as a whole-record load
/// decodes it, the contest exchange included — in log order: the `qso_recent` index names them,
/// and they are decoded by id.
fn rows_since(db: &LogDb, bound: u64) -> sqlite::Result<Vec<QsoRecord>> {
    let mut ids = Vec::new();
    db.each_narrow(
        sqlite::Narrow {
            columns: &[],
            uploads: false,
        },
        sqlite::Scope::Since(bound),
        sqlite::Order::Log,
        &mut |r| {
            ids.extend(r.id);
            std::ops::ControlFlow::Continue(())
        },
    )?;
    db.rows_by_ids(&ids)
}

impl SessionRead {
    /// Read the rows: every row the store holds from the bound on, whole, in log order, waited
    /// for (P4) and read in one transaction. Then count other windows' commits NOW
    /// ([`LogWriter::foreign_commits_now`]): one the read saw is counted by then, so a count
    /// still at the station's says the read holds nothing the station has not taken in. Each
    /// wait is [`SESSION_READ_WAIT`] at most.
    ///
    /// ⚠️ Disk I/O and two waits: never under the Engine lock (a debug build panics).
    pub fn read(self) -> SessionRows {
        #[cfg(debug_assertions)]
        SESSION_READS.with(|c| c.set(c.get() + 1));
        let bound = self.bound;
        let read = self
            .reads
            .read(SESSION_READ_WAIT, |db| rows_since(db, bound));
        let foreign = self.writer.foreign_commits_now(SESSION_READ_WAIT);
        let (rows, ready) = match read {
            Ok((rows, fresh)) => (rows, fresh == Freshness::Current && foreign.is_some()),
            Err(_) => (Vec::new(), false),
        };
        SessionRows {
            rows,
            bound,
            marks: self.marks,
            exact: ready && foreign == Some(self.synced),
            ready,
        }
    }
}

/// A `log.adi` written by something other than this store's mirror — a 1.13 instance, the
/// operator, a restore — read at open so its contacts can be taken in before anything may
/// replace the file.
pub(crate) struct ForeignLog {
    pub(crate) text: String,
    pub(crate) stamp: Option<FileStamp>,
}

/// Why the store could not be made the log's owner. The caller runs the session on `log.adi`
/// instead and says so; nothing here has changed the operator's file.
#[derive(Debug)]
pub enum OpenError {
    /// The data folder is on network storage, where a database can be corrupted by the way
    /// file locking works across a network. `log.adi` degrades gracefully there; SQLite's worst
    /// case is a database that will not open.
    NetworkFolder(String),
    /// Converting `log.adi` did not finish (see [`migrate::Error`]). The pre-conversion copy
    /// is in place if one could be taken, and the conversion resumes at the next start.
    Conversion(migrate::Error),
    /// The store would not open, or could not be read.
    Store(sqlite::Error),
    /// `log.adi` could not be read to check it against the store.
    Unreadable(std::io::Error),
}

impl std::fmt::Display for OpenError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OpenError::NetworkFolder(why) => write!(
                f,
                "the data folder is on {why}, so the logbook stays in log.adi for this session"
            ),
            OpenError::Conversion(e) => write!(f, "the logbook was not converted: {e}"),
            OpenError::Store(e) => write!(f, "the logbook database could not be opened: {e}"),
            OpenError::Unreadable(e) => write!(f, "log.adi could not be read: {e}"),
        }
    }
}

impl std::error::Error for OpenError {}

/// Open the store for the log at `log_path`, converting the log first if it has never been.
///
/// `network` is the shell's verdict on the data folder (`data_folder_location`): certain network
/// storage refuses the store for this session. The mirror runs on its shipped timings.
pub fn open(
    log_path: &Path,
    resolve: StoreResolve,
    network: Option<String>,
) -> Result<Opened, OpenError> {
    open_with(log_path, resolve, network, MirrorOptions::default())
}

/// [`open`], telling `progress` how far a first launch's conversion of `log.adi` has got — what
/// the start-up screen shows while it works (see [`migrate::migrate_log_reporting`]) — and
/// building the hot index from the store as it opens, when `hot` asks for it.
pub fn open_reporting(
    log_path: &Path,
    resolve: StoreResolve,
    network: Option<String>,
    hot: Option<HotBuild>,
    progress: &mut dyn FnMut(migrate::Progress),
) -> Result<Opened, OpenError> {
    open_reporting_with(
        log_path,
        resolve,
        network,
        MirrorOptions::default(),
        hot,
        progress,
    )
}

/// [`open`], with the mirror's timings named — what a test uses so it does not wait a real
/// second for a real write. `mirror.accepted` is decided here and overwritten.
pub fn open_with(
    log_path: &Path,
    resolve: StoreResolve,
    network: Option<String>,
    mirror_options: MirrorOptions,
) -> Result<Opened, OpenError> {
    open_reporting_with(
        log_path,
        resolve,
        network,
        mirror_options,
        None,
        &mut |_| {},
    )
}

/// [`open_with`] and [`open_reporting`] in one: the mirror's timings, the hot index, and the
/// conversion's progress.
fn open_reporting_with(
    log_path: &Path,
    resolve: StoreResolve,
    network: Option<String>,
    mut mirror_options: MirrorOptions,
    hot: Option<HotBuild>,
    progress: &mut dyn FnMut(migrate::Progress),
) -> Result<Opened, OpenError> {
    // All of it — the conversion, the load, the hot index, the read of a foreign `log.adi`, the
    // sweep — before the engine is locked.
    tempo_core::logbook::io_fence::off_engine_lock("opening the logbook store");
    if let Some(why) = network {
        return Err(OpenError::NetworkFolder(why));
    }
    let db_path = migrate::database_path(log_path);
    let outcome = migrate::migrate_log_reporting(log_path, &db_path, |r| resolve(r), progress)
        .map_err(OpenError::Conversion)?;
    let db = LogDb::open(&db_path).map_err(OpenError::Store)?;
    let records = db.load_all().map_err(OpenError::Store)?;
    // The hot index of exactly those rows, and the rows a session restored at launch is swept
    // from, made here where the rows already are. (Once the log in memory goes, SPEC-2 v3 C19,
    // the index comes from the store itself: `HotIndex::from_store`.)
    let hot = hot.map(|HotBuild { entity }| {
        let bound =
            crate::engine::now_unix_secs().saturating_sub(crate::engine::SESSION_READ_WINDOW);
        Prebuilt {
            index: HotIndex::from_rows(&records, &StationKeys(entity.as_deref())),
            entity,
            recent: records
                .iter()
                .filter(|r| r.when_unix >= bound)
                .cloned()
                .collect(),
            bound,
        }
    });

    // Does `log.adi` hold anything the store does not? A pristine mirror (some Nexus mirror's
    // own picture, unchanged since) cannot, and neither can the file this very open just
    // converted. Anything else — an operator's log from before the store existed, a 1.13
    // instance's appends, a restore — is read now and taken in before the mirror may touch it.
    let converted_now = matches!(outcome, migrate::Outcome::Converted { .. });
    let foreign = match mirror::mirror_state(log_path) {
        MirrorState::Absent | MirrorState::Pristine => None,
        MirrorState::Foreign if converted_now => None,
        MirrorState::Foreign => {
            let stamp = mirror::file_stamp(log_path);
            let bytes = std::fs::read(log_path).map_err(OpenError::Unreadable)?;
            Some(ForeignLog {
                text: String::from_utf8_lossy(&bytes).into_owned(),
                stamp,
            })
        }
    };
    // What the mirror may replace though it is not a pristine picture: the file just converted,
    // or the one about to be taken in — for exactly as long as it keeps this stamp.
    mirror_options.accepted = match mirror::mirror_state(log_path) {
        MirrorState::Foreign => mirror::file_stamp(log_path),
        _ => None,
    };
    // The copies beside the log are swept as every launch has always swept them — `stat`-cheap
    // unless one has changed — now that the launch no longer loads `log.adi` itself.
    Logbook::sweep_safety_copies(log_path);

    let writer = Arc::new(LogWriter::start(db));
    let synced_foreign = writer.foreign_commits();
    // Opens nothing yet: a session that never reads the store costs nothing for it.
    let reader = Arc::new(LogReader::new(&db_path));
    // The mirror pictures the store itself, streamed off a read connection (SPEC-2 v3 C15).
    let mirror = Arc::new(MirrorWriter::with_options(
        log_path.to_path_buf(),
        Arc::new(StoreSource::new(Arc::clone(&reader), Arc::clone(&writer))),
        mirror_options,
    ));
    Ok(Opened {
        store: LogStore {
            writer,
            reader,
            mirror: Some(mirror),
            lane: None,
            log_path: Some(log_path.to_path_buf()),
            db_path,
            resolve,
            inflight: Vec::new(),
            dropped: Vec::new(),
            last_refusal: None,
            synced_foreign,
            collector: None,
            placeholder: false,
        },
        records,
        foreign,
        hot,
        outcome,
    })
}

impl LogStore {
    /// An empty store in this process's memory ([`LogDb::memory_name`]), with its writer and
    /// its reads and no `log.adi` beside it — the log of an engine no launch has given one.
    pub(crate) fn in_memory(resolve: StoreResolve) -> Result<LogStore, sqlite::Error> {
        let db_path = LogDb::memory_name();
        let writer = Arc::new(LogWriter::start(LogDb::open(&db_path)?));
        let synced_foreign = writer.foreign_commits();
        Ok(LogStore {
            writer,
            reader: Arc::new(LogReader::new(&db_path)),
            mirror: None,
            lane: None,
            log_path: None,
            db_path,
            resolve,
            inflight: Vec::new(),
            dropped: Vec::new(),
            last_refusal: None,
            synced_foreign,
            collector: None,
            placeholder: false,
        })
    }

    /// The store of a session on the 1.13 path — SPEC-2 v3 C19, D1-A, the operator's "Same code,
    /// in-memory database": `log.adi` is the log's durable home, as it was in 1.13, and `records`
    /// (the log as [`Logbook::load`] read it from `log_path`) are loaded into a store in this
    /// process's memory, so every reader of the log reads a store whichever path the session is
    /// on. `log.adi` is kept by a lane of its own, with 1.13's rules
    /// ([`tempo_core::logbook::logfile`]); the store's C15 mirror is not started.
    ///
    /// The load is the conversion's: the rows in big transactions with the indexes built after
    /// them, on a connection of its own, before the writer's connection takes over. Measured in
    /// a release build: 1.0 s and 174 MiB for 150,000 contacts, 3.9 s and 404 MiB for 500,000, on
    /// top of the 0.7 s / 2.5 s the 1.13 load of `log.adi` itself takes.
    ///
    /// `read` is `log.adi`'s stamp as it stood before the load read it: while the file still
    /// has it, it holds the rows loaded, and the lane may replace it without the station reading
    /// it again. Under the engine lock at launch, as 1.13's load was (see
    /// [`tempo_core::logbook::io_fence`]): the launch holds it across the attach.
    pub(crate) fn fallback(
        log_path: &Path,
        read: Option<FileStamp>,
        records: &[Arc<QsoRecord>],
        resolve: StoreResolve,
    ) -> Result<LogStore, sqlite::Error> {
        let db_path = LogDb::memory_name();
        let mut loader = LogDb::open_for_conversion(&db_path)?;
        loader.lift_memory_cap()?;
        for chunk in records.chunks(FALLBACK_LOAD_CHUNK) {
            loader.insert_all(chunk.iter().map(|r| (&**r, resolve(r))))?;
        }
        loader.build_indexes(&mut |_, _| {})?;
        // The writer's own connection, opened while the loader's still holds the database: an
        // in-memory database lasts as long as some connection to it does.
        let writer = Arc::new(LogWriter::start(LogDb::open(&db_path)?));
        drop(loader);
        let synced_foreign = writer.foreign_commits();
        let reader = Arc::new(LogReader::new(&db_path));
        let lane = Arc::new(LogFileWriter::start(
            log_path.to_path_buf(),
            Arc::new(StoreSource::new(Arc::clone(&reader), Arc::clone(&writer))),
        ));
        if let Some(stamp) = read {
            lane.accept(stamp, records.iter().filter_map(|r| r.id).collect());
        }
        Ok(LogStore {
            writer,
            reader,
            mirror: None,
            lane: Some(lane),
            log_path: Some(log_path.to_path_buf()),
            db_path,
            resolve,
            inflight: Vec::new(),
            dropped: Vec::new(),
            last_refusal: None,
            synced_foreign,
            collector: None,
            placeholder: false,
        })
    }

    /// Make this the launch's placeholder: the store the launch's engine holds until the attach
    /// replaces it with the operator's log (the database, or on the 1.13 path `log.adi` loaded
    /// into memory). A change sent to it would be lost with it, so in a debug build none may be:
    /// one is a panic that names it. A release build checks nothing.
    pub(crate) fn placeholder_until_attached(&mut self) {
        self.placeholder = true;
    }

    /// The lane that keeps `log.adi` on the 1.13 path, if this store is that session's.
    pub(crate) fn lane(&self) -> Option<&Arc<LogFileWriter>> {
        self.lane.as_ref()
    }

    /// The writer, for a command that waits on tickets after releasing every lock.
    pub fn writer(&self) -> Arc<LogWriter> {
        Arc::clone(&self.writer)
    }

    /// A read of the store that will see every change this process has made to it so far —
    /// taken here, under the engine lock, and used after it is released. Handles and one
    /// atomic read: no I/O.
    pub fn reads(&self) -> StoreReads {
        StoreReads {
            reader: Arc::clone(&self.reader),
            writer: Arc::clone(&self.writer),
            after: self.writer.submitted_rev(),
        }
    }

    /// A read of the log's rows from `bound` on, for a contest session's sweep ([`SessionRead`]),
    /// taken here under the Engine lock against `marks` — the log as the station holds it. No
    /// I/O. `None` while the store is missing a change the station holds (one it refused): its
    /// rows could not be the log's.
    pub(crate) fn session_read(&self, bound: u64, marks: Watermarks) -> Option<SessionRead> {
        if !self.dropped.is_empty() {
            return None;
        }
        Some(SessionRead {
            reads: self.reads(),
            writer: Arc::clone(&self.writer),
            synced: self.synced_foreign,
            bound,
            marks,
        })
    }

    /// The database's path.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// The `log.adi` this store mirrors to, if it has one.
    pub fn log_path(&self) -> Option<&Path> {
        self.log_path.as_deref()
    }

    /// The mirror's state, for a caller that surfaces a refusal — `None` with no mirror.
    pub fn mirror_status(&self) -> Option<mirror::Status> {
        self.mirror.as_ref().map(|m| m.status())
    }

    /// cty.dat's answer as the store writes it.
    pub(crate) fn resolved(&self) -> impl Fn(&QsoRecord) -> (Option<String>, Option<u8>) + '_ {
        |r| {
            let x = (self.resolve)(r);
            (x.entity.map(str::to_string), x.cq_zone)
        }
    }

    /// Hand one change to the writer, and its revision to the mirror — or, on the 1.13 path, a
    /// rewrite of `log.adi` to its lane. Never touches the disk. A change that writes nothing is
    /// not sent, and has no ticket.
    pub(crate) fn submit(&mut self, change: Change) -> Option<Ticket> {
        self.submit_as(change, ToFile::Rewrite)
    }

    /// [`Self::submit`] for a change that appended rows at the end of the log and changed
    /// nothing else: on the 1.13 path, the lane appends them to `log.adi`, as 1.13's append did,
    /// instead of rewriting the file.
    pub(crate) fn submit_appended(&mut self, change: Change) -> Option<Ticket> {
        let rows = change.upsert.iter().map(|w| Arc::clone(&w.rec)).collect();
        self.submit_as(change, ToFile::Append(rows))
    }

    /// [`Self::submit`] for a change whose rows came FROM `log.adi` — the 1.13 path taking in
    /// what another program wrote to it: the lane writes nothing for it, and counts it saved once
    /// every change before it is.
    pub(crate) fn submit_quietly(&mut self, change: Change) -> Option<Ticket> {
        self.submit_as(change, ToFile::Nothing)
    }

    fn submit_as(&mut self, change: Change, to_file: ToFile) -> Option<Ticket> {
        if change.is_empty() {
            return None;
        }
        self.collect(Instant::now());
        self.supersede(&change);
        let ticket = self.send_as(change, 0, to_file);
        if let Some(c) = &mut self.collector {
            c.push(ticket.clone());
        }
        Some(ticket)
    }

    /// Hand a change to the writer and its revision to the mirror, and keep the change in flight
    /// with its rows — `resends` times sent again already. No I/O, and no copy of the log: the
    /// mirror reads the store once it holds the change.
    fn send(&mut self, change: Change, resends: u32) -> Ticket {
        self.send_as(change, resends, ToFile::Rewrite)
    }

    /// [`Self::send`], telling the 1.13 path's lane what the change asks of `log.adi`.
    fn send_as(&mut self, change: Change, resends: u32, to_file: ToFile) -> Ticket {
        debug_assert!(
            !self.placeholder,
            "a change reached the launch's placeholder log before the launch attached the \
             operator's: the attach replaces it, and the change would be lost with it"
        );
        let held = Arc::new(Held::of(&change));
        let ticket = self.writer.submit(change);
        self.inflight.push(InFlight {
            ticket: ticket.clone(),
            held,
            resends,
        });
        if let Some(m) = &self.mirror {
            m.dirty(ticket.revision());
        }
        if let Some(lane) = &self.lane {
            // Its number, which a wait reads back as the lane's count once every change the
            // command made has been handed over ([`Self::durability`]).
            let _ = match to_file {
                ToFile::Rewrite => lane.rewrite(ticket.revision()),
                ToFile::Append(rows) => lane.append(ticket.revision(), rows),
                ToFile::Nothing => lane.noted(ticket.revision()),
            };
        }
        ticket
    }

    /// A new change to rows this process still holds for an earlier one — in flight, or given
    /// up on — carries them as they now stand, so the earlier one lets them go: sent again, it
    /// must not write back a row older than the new change (SPEC-2 v3 C19: the retry holds its
    /// own refused change, and a later change to the same row wins, as it always has). A purge
    /// lets every held row go.
    fn supersede(&mut self, change: &Change) {
        let Touched::Rows(ids) = Touched::of(change) else {
            for held in self.held_mut() {
                *Arc::make_mut(held) = Held::default();
            }
            return;
        };
        let ids: std::collections::HashSet<RecordId> = ids.into_iter().collect();
        for held in self.held_mut() {
            if held.touches(&ids) {
                Arc::make_mut(held).release(&ids);
            }
        }
    }

    /// Every change's held rows, in flight and given up on.
    fn held_mut(&mut self) -> impl Iterator<Item = &mut Arc<Held>> {
        self.inflight
            .iter_mut()
            .map(|f| &mut f.held)
            .chain(self.dropped.iter_mut().map(|d| &mut d.held))
    }

    /// This process's changes the store may not hold yet — in flight, or given up on — for a plan
    /// to read in place of the store's rows (see [`Pending`]). Pointers, no copy of a row; taken
    /// under the engine lock, read after it is released.
    pub(crate) fn pending(&self) -> Pending {
        let mut held: Vec<(u64, Arc<Held>)> = self
            .inflight
            .iter()
            .map(|f| (f.ticket.revision(), Arc::clone(&f.held)))
            .chain(self.dropped.iter().map(|d| (d.rev, Arc::clone(&d.held))))
            .collect();
        // In the order the changes were made, which a plan asking for the newest row reads.
        held.sort_by_key(|(rev, _)| *rev);
        Pending(held)
    }

    /// Take in what the writer has finished with since the last look. A change that landed is
    /// forgotten; one it gave up on becomes [`Dropped`] — still in memory, not in the store — with
    /// its next automatic re-send scheduled when the refusal can pass ([`RESEND_AFTER`]). No I/O;
    /// never waits.
    fn collect(&mut self, now: Instant) {
        let mut kept = Vec::with_capacity(self.inflight.len());
        for f in std::mem::take(&mut self.inflight) {
            if !f.ticket.is_resolved() {
                kept.push(f);
                continue;
            }
            let Some(refusal) = f.ticket.refusal() else {
                if f.resends > 0 {
                    tempo_core::applog::info(
                        "logbook",
                        "a logbook change the database had refused is saved now",
                    );
                }
                continue;
            };
            let wait = resend_after(f.resends);
            if refusal.retryable {
                tempo_core::applog::warn(
                    "logbook",
                    &format!(
                        "the logbook database did not take a change ({}); Nexus keeps it in \
                         memory and sends it again in {} s",
                        refusal.reason,
                        wait.as_secs()
                    ),
                );
            } else {
                tempo_core::applog::error(
                    "logbook",
                    &format!(
                        "the logbook database refused a change for good ({}); Nexus keeps it in \
                         memory for this session, and quitting asks about it",
                        refusal.reason
                    ),
                );
            }
            self.last_refusal = Some(refusal.reason.clone());
            self.dropped.push(Dropped {
                held: f.held,
                rev: f.ticket.revision(),
                refusal,
                resends: f.resends,
                due: now + wait,
            });
        }
        self.inflight = kept;
    }

    /// Send again the changes the writer gave up on for a reason that can pass — every one when
    /// `all` (a quit, and its Keep trying), otherwise only those whose wait is up — each with the
    /// rows it holds, under the revision it first went out with and the watermarks the log
    /// stands at now (`marks`). How many went out. A change refused for what it is is never sent
    /// again: it would be refused every time, and a loop is not a save. No I/O.
    ///
    /// ⚠️ **Why this cannot write a change twice, or out of order** (SPEC-2 v3 C19: the retry
    /// holds its own refused change, where it used to read its rows back out of the log in
    /// memory). Each change carries STATE, not a delta — every row as it stood after the change
    /// — so applied twice, or after the first attempt landed despite its error, it writes what
    /// is already there. And a later change to one of its rows took that row out of it when it
    /// was submitted ([`Self::supersede`]), carrying the row as it then stood: a re-send never
    /// writes back a row older than one the store already has. What is left may be nothing at
    /// all, and it is sent anyway: its landing is what tells the writer the revision it lost is
    /// no longer missing.
    pub(crate) fn resend(&mut self, marks: Watermarks, all: bool, now: Instant) -> usize {
        self.collect(now);
        let (go, stay): (Vec<Dropped>, Vec<Dropped>) = std::mem::take(&mut self.dropped)
            .into_iter()
            .partition(|d| d.refusal.retryable && (all || d.due <= now));
        self.dropped = stay;
        if go.is_empty() {
            return 0;
        }
        let sent = go.len();
        for d in go {
            let change = d.held.change(d.rev, marks);
            self.send(change, d.resends + 1);
        }
        tempo_core::applog::info(
            "logbook",
            &format!("sending {sent} logbook change(s) the database refused again"),
        );
        sent
    }

    /// What the screen says while the database has refused a change: how many are being sent
    /// again, how many are held for the quit, and the latest reason. `None` while every change
    /// is in the store or on its first way there. Never waits.
    pub(crate) fn save_trouble(&self) -> Option<crate::dto::LogSaveTrouble> {
        let held = self.held();
        // A re-send on its way is still a change the database has not taken.
        let resending = self
            .inflight
            .iter()
            .filter(|f| f.resends > 0 && !f.ticket.is_resolved())
            .count();
        let retrying = held.retryable + resending;
        (retrying + held.refused > 0).then(|| crate::dto::LogSaveTrouble {
            retrying: retrying as u32,
            held: held.refused as u32,
            reason: held
                .retry_reason
                .or(held.reason)
                .or_else(|| self.last_refusal.clone())
                .unwrap_or_default(),
        })
    }

    /// The changes the writer gave up on, counted as a quit counts them: those that sending
    /// again can save, and those it cannot — the ones already taken in ([`Self::collect`]) and
    /// any refused since. Never waits.
    fn held(&self) -> Standing {
        let mut s = Standing::default();
        for d in &self.dropped {
            s.count_refusal(&d.refusal);
        }
        for f in &self.inflight {
            if let Some(r) = f.ticket.refusal() {
                s.count_refusal(&r);
            }
        }
        s
    }

    /// Start collecting the tickets of the changes that follow.
    pub(crate) fn begin_collecting(&mut self) {
        self.collector = Some(Vec::new());
    }

    /// Stop collecting, and hand back what was collected.
    pub(crate) fn take_collected(&mut self) -> Vec<Ticket> {
        self.collector.take().unwrap_or_default()
    }

    /// Whether ANOTHER process has committed to the store since memory last matched it. An
    /// atomic read — no I/O — so it is safe to ask under any lock.
    pub(crate) fn foreign_changed(&self) -> bool {
        self.writer.foreign_commits() != self.synced_foreign
    }

    /// How many times ANOTHER process has committed to the store since this one opened it — the
    /// count a plan takes, and its commit compares ([`crate::station::StationCore::unchanged_since`]).
    /// An atomic read, no I/O.
    pub(crate) fn foreign_commits(&self) -> u64 {
        self.writer.foreign_commits()
    }

    /// The log after another process's commits: every row as the store holds it now, with this
    /// process's own changes still in flight laid over it. `in_place` keeps every row where
    /// memory has it ([`writer::merge_reloaded_in_place`]) — for a change about to be made BY
    /// POSITION — and a row another process deleted is then kept and reported; otherwise the
    /// log becomes the store's, in the store's order ([`writer::merge_reloaded`]). Reads the
    /// store on a connection of its own.
    ///
    /// ⚠️ This READS the database, and the caller holds the engine lock. It runs only when
    /// another process has written — two radio windows sharing one data folder — at exactly
    /// the points the old two-instance recovery re-read the whole of `log.adi` under the same
    /// lock, so it costs nothing a shared log did not already cost. A stalled WRITE does not
    /// stall it: in WAL mode a reader never waits for a writer.
    pub(crate) fn reload(
        &mut self,
        held: &[Arc<QsoRecord>],
        in_place: bool,
    ) -> Result<(Vec<Arc<QsoRecord>>, bool), String> {
        // Read the count BEFORE the load: a commit that lands during it is counted after, and
        // costs another reload rather than being missed.
        let seen = self.writer.foreign_commits();
        self.collect(Instant::now());
        let pending: Vec<Touched> = self
            .inflight
            .iter()
            .map(|f| f.held.touched())
            .chain(self.dropped.iter().map(|d| d.held.touched()))
            .collect();
        let stored = LogDb::open(&self.db_path)
            .and_then(|db| db.load_all())
            .map_err(|e| e.to_string())?;
        self.synced_foreign = seen;
        Ok(if in_place {
            writer::merge_reloaded_in_place(stored, held, &pending)
        } else {
            (writer::merge_reloaded(stored, held, &pending), false)
        })
    }

    /// The store has taken in the `log.adi` with `stamp`: the mirror may replace it now.
    pub(crate) fn accept_log_file(&self, stamp: FileStamp) {
        if let Some(m) = &self.mirror {
            m.accept(stamp);
        }
    }

    /// Have the mirror picture the store as it stands, changing nothing in it — for a `log.adi`
    /// that is not the store's own picture yet (the file a conversion read, one just taken in),
    /// so the next launch finds a mirror and has nothing to read or take in.
    pub(crate) fn refresh_mirror(&self) {
        if let Some(m) = &self.mirror {
            m.dirty(self.writer.submitted_rev());
        }
    }

    /// What a quit still has to wait for (see [`Unsaved`]): this process's changes the writer
    /// has not finished with, and the mirror. Handles only — never touches the disk.
    pub(crate) fn unsaved(&self) -> Unsaved {
        let tickets = self
            .inflight
            .iter()
            .filter(|f| !f.ticket.is_resolved())
            .map(|f| f.ticket.clone())
            .collect();
        Unsaved {
            changes: self.durability(tickets),
            held: self.held(),
            mirror: self.mirror.clone(),
            lane: self.lane_so_far(),
        }
    }

    /// What a command that made the changes `tickets` name waits on: the writer, and on the
    /// 1.13 path the lane that puts them in `log.adi` — every change it has been handed so far,
    /// which is every one the command made.
    pub(crate) fn durability(&self, tickets: Vec<Ticket>) -> Durability {
        Durability {
            writer: Some(self.writer()),
            tickets,
            lane: self.lane_so_far(),
        }
    }

    /// The 1.13 path's lane, with how many changes it has been handed so far.
    fn lane_so_far(&self) -> Option<(Arc<LogFileWriter>, u64)> {
        self.lane.as_ref().map(|l| (Arc::clone(l), l.sent()))
    }

    /// The receipt for the change `ticket` names, just submitted — an append a Remote log
    /// redeems once it is safe: committed, and on the 1.13 path in `log.adi` too.
    pub(crate) fn receipt(&self, ticket: Ticket) -> tempo_core::logbook::LogAppendReceipt {
        match self.lane_so_far() {
            Some((lane, n)) => tempo_core::logbook::LogAppendReceipt::durable_in_file(
                self.writer(),
                ticket,
                lane,
                n,
                DURABLE_WAIT,
            ),
            None => {
                tempo_core::logbook::LogAppendReceipt::durable(self.writer(), ticket, DURABLE_WAIT)
            }
        }
    }

    /// Write everything submitted so far, and the mirror, waiting up to `deadline` for each.
    /// The exit path.
    pub fn flush(&self, deadline: Duration) -> Result<(), String> {
        let logged = self
            .writer
            .flush(deadline)
            .map(|_| ())
            .map_err(|e| e.to_string());
        let mirrored = self.mirror.as_ref().map(|m| m.flush(deadline));
        let filed = self.lane.as_ref().map(|l| l.flush(deadline));
        logged?;
        if let Some(st) = filed.filter(|st| st.pending()) {
            return Err(format!(
                "log.adi does not hold every change yet{}",
                if st.foreign_write {
                    ": another program or computer changed it".to_string()
                } else {
                    st.last_error.map(|e| format!(": {e}")).unwrap_or_default()
                }
            ));
        }
        match mirrored {
            Some(m) if m.pending => match m.last_error {
                Some(e) => Err(format!("log.adi was not brought up to date: {e}")),
                None => Ok(()),
            },
            _ => Ok(()),
        }
    }
}

/// What an operator command waits on after it has released every lock: the writer, and the
/// tickets of the changes it made. Empty when the log has no store (the 1.13 path writes
/// `log.adi` inline and has nothing to wait for).
#[derive(Debug, Default)]
pub struct Durability {
    writer: Option<Arc<LogWriter>>,
    tickets: Vec<Ticket>,
    /// On the 1.13 path, the lane that puts the changes in `log.adi` — where they are saved —
    /// and how many it had been handed when the command was done.
    lane: Option<(Arc<LogFileWriter>, u64)>,
}

impl Durability {
    /// Whether there is anything to wait for.
    pub fn is_empty(&self) -> bool {
        self.tickets.is_empty()
    }

    /// How many changes this covers.
    pub fn len(&self) -> usize {
        self.tickets.len()
    }

    /// Wait until every change is on disk, or say why not. ⚠️ Never call it holding a lock —
    /// and from an async command, only inside `spawn_blocking`: a wait of up to a minute must
    /// not pin a runtime worker the rest of the app needs.
    pub fn wait(&self, deadline: Duration) -> Result<(), String> {
        let Some(writer) = &self.writer else {
            return Ok(());
        };
        let start = std::time::Instant::now();
        for t in &self.tickets {
            let left = deadline.saturating_sub(start.elapsed());
            writer.wait_durable(t, left).map_err(|e| e.to_string())?;
        }
        // On the 1.13 path a change is saved once it is in `log.adi`, as 1.13's command returned
        // once its write had happened.
        if let Some((lane, n)) = self.lane.as_ref().filter(|_| !self.tickets.is_empty()) {
            lane.wait_saved(*n, deadline.saturating_sub(start.elapsed()))?;
        }
        Ok(())
    }

    /// Each change as a receipt the Remote service redeems the same way it redeems an append
    /// to `log.adi` ([`tempo_core::logbook::LogAppendReceipt::sync`]).
    pub fn into_receipts(self) -> Vec<tempo_core::logbook::LogAppendReceipt> {
        let Some(writer) = self.writer else {
            return Vec::new();
        };
        let lane = self.lane;
        self.tickets
            .into_iter()
            .map(|t| match &lane {
                Some((lane, n)) => tempo_core::logbook::LogAppendReceipt::durable_in_file(
                    Arc::clone(&writer),
                    t,
                    Arc::clone(lane),
                    *n,
                    DURABLE_WAIT,
                ),
                None => tempo_core::logbook::LogAppendReceipt::durable(
                    Arc::clone(&writer),
                    t,
                    DURABLE_WAIT,
                ),
            })
            .collect()
    }
}

/// A read of the logbook store — SPEC-2 v3's read path, laid in C12.
///
/// Taken under the engine lock ([`LogStore::reads`], [`crate::engine::Engine::log_store_reads`])
/// and used after it is released: it carries the reader, the writer, and the revision of the
/// latest change this process had submitted when it was taken. A read through it first waits for
/// every change up to that revision to be committed, then reads the store in ONE read
/// transaction — so it sees every change made before the question was asked, the way a read of
/// the log in memory always has (memory is written first), and one consistent picture of them.
///
/// **It never waits under a lock and never reads under one**: the wait and the read are fenced
/// off the Engine lock ([`tempo_core::logbook::io_fence`]), and a debug build panics if either
/// runs under it.
///
/// When the store has not taken every change within the wait — a stalled write, a slow disk —
/// or has refused one, the read still happens, against the store as it stands, and says so
/// ([`Freshness::Stale`]). An answer that is a moment old and says so beats no answer.
#[derive(Debug, Clone)]
pub struct StoreReads {
    reader: Arc<LogReader>,
    writer: Arc<LogWriter>,
    /// Every change up to this revision was submitted before the read was asked for.
    after: u64,
}

/// Whether a read through [`StoreReads`] saw every change made before it was asked for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Freshness {
    /// Every change submitted before the read was asked for is in what it saw.
    Current,
    /// The store had not taken them all when the wait ran out, or had refused one: the read saw
    /// the store as it stood. Why, in the writer's words.
    Stale(String),
}

impl StoreReads {
    /// Wait up to `wait_up_to` for the store to hold every change made before this read was
    /// asked for, then run `f` against it in one read transaction.
    ///
    /// ⚠️ Never call it holding the Engine lock; a debug build panics.
    pub fn read<T>(
        &self,
        wait_up_to: Duration,
        f: impl FnOnce(&LogDb) -> sqlite::Result<T>,
    ) -> Result<(T, Freshness), sqlite::Error> {
        let fresh = match self.writer.wait_committed(self.after, wait_up_to) {
            Ok(_) => Freshness::Current,
            Err(why) => Freshness::Stale(why.to_string()),
        };
        Ok((self.reader.read(f)?, fresh))
    }

    /// Every record the store holds, in log order, each as [`LogDb::load_all`] decodes it —
    /// the log as a reader of the store sees it. What tests read in place of the copy held in
    /// memory as that copy goes away (SPEC-2 v3, C19).
    pub fn rows(&self, wait_up_to: Duration) -> Result<(Vec<QsoRecord>, Freshness), sqlite::Error> {
        self.read(wait_up_to, |db| db.load_all())
    }
}

/// How long a pass over the log ([`LogRows`]) waits for the writer to take the changes made
/// before it was asked for, before it reads the store as it stands and says so
/// ([`Freshness::Stale`]). The writer commits a change within one disk flush; this is for the
/// moment a bulk write is ahead of it.
pub const READ_WAIT: Duration = Duration::from_secs(2);

/// The log's rows, for a pass over them — SPEC-2 v3's **C14**. Taken under the Engine lock
/// ([`crate::engine::Engine::log_rows`]: handles, or a copy of pointers) and read after it is
/// released.
///
/// ONE way for every fold, lookup and sweep to reach the log, whichever holds it this session,
/// so each of them is written once.
#[derive(Debug, Clone)]
pub enum LogRows {
    /// The logbook store, which owns the log. A pass first waits for every change made before
    /// this was taken to be committed (P4: a question asked straight after a contact is logged
    /// sees it), then reads in ONE read transaction.
    Store(StoreReads),
    /// The 1.13 path, when the store was refused and the session runs on `log.adi`: the log in
    /// memory, as a copy of its pointers. SPEC-2 v3 D1 moves that path onto an in-memory
    /// database of the store's own shape (C19), and this arm goes with it.
    Memory(Vec<Arc<QsoRecord>>),
}

impl LogRows {
    /// Hand `each` every row `scope` names, in `order`, with (at least) `narrow`'s fields
    /// filled; `each` answering [`std::ops::ControlFlow::Break`] ends the pass. Whether it saw
    /// every change made before the rows were taken is the answer.
    ///
    /// On the 1.13 path the rows are whole records, and `scope` is applied to them by the
    /// store's own definition of it — the same rows the store would name.
    ///
    /// ⚠️ **Never call it holding the Engine lock** — it reads the disk, or passes over the
    /// whole log; a debug build panics ([`tempo_core::logbook::io_fence`]).
    pub fn each(
        &self,
        narrow: sqlite::Narrow,
        scope: sqlite::Scope<'_>,
        order: sqlite::Order,
        each: &mut dyn FnMut(&QsoRecord) -> std::ops::ControlFlow<()>,
    ) -> Result<Freshness, sqlite::Error> {
        tempo_core::logbook::io_fence::whole_log_off_engine_lock("a pass over the log's rows");
        match self {
            LogRows::Store(reads) => reads
                .read(READ_WAIT, |db| db.each_narrow(narrow, scope, order, each))
                .map(|((), fresh)| fresh),
            LogRows::Memory(rows) => {
                let named = |r: &QsoRecord| match scope {
                    sqlite::Scope::All => true,
                    sqlite::Scope::Since(t) => r.when_unix >= t,
                    sqlite::Scope::CallNorm(call) => sqlite::call_norm_of(&r.call) == call,
                };
                let mut visit = |r: &Arc<QsoRecord>| named(r) && each(r).is_break();
                match order {
                    sqlite::Order::Log => {
                        let _ = rows.iter().any(&mut visit);
                    }
                    sqlite::Order::NewestFirst => {
                        let _ = rows.iter().rev().any(&mut visit);
                    }
                }
                Ok(Freshness::Current)
            }
        }
    }

    /// Hand `each` every record of the log, WHOLE, in log order — the store streamed a chunk at a
    /// time ([`LogDb::each_record`]), so a pass holds one chunk of the log and never the whole of
    /// it; on the 1.13 path, the log in memory. `each` answering
    /// [`std::ops::ControlFlow::Break`] ends the pass. What the exports read (SPEC-2 v3 C15).
    ///
    /// The store's pass first waits up to `wait` for every change made before the rows were
    /// taken, as [`StoreReads::read`] does.
    ///
    /// ⚠️ Never under the Engine lock, as [`Self::each`].
    pub fn each_record(
        &self,
        wait: Duration,
        each: &mut dyn FnMut(&QsoRecord) -> std::ops::ControlFlow<()>,
    ) -> Result<Freshness, sqlite::Error> {
        tempo_core::logbook::io_fence::whole_log_off_engine_lock("a pass over the log's rows");
        match self {
            LogRows::Store(reads) => reads
                .read(wait, |db| {
                    db.each_record(sqlite::RECORD_CHUNK, &mut |r| each(&r))
                })
                .map(|((), fresh)| fresh),
            LogRows::Memory(rows) => {
                let _ = rows.iter().any(|r| each(r).is_break());
                Ok(Freshness::Current)
            }
        }
    }

    /// The whole records these ids name, in log order — [`LogDb::rows_by_ids`] on the store.
    /// An id the log does not hold is simply absent.
    ///
    /// ⚠️ Never under the Engine lock, as [`Self::each`].
    pub fn rows_by_ids(
        &self,
        ids: &[tempo_core::logbook::RecordId],
    ) -> Result<(Vec<QsoRecord>, Freshness), sqlite::Error> {
        match self {
            LogRows::Store(reads) => reads.read(READ_WAIT, |db| db.rows_by_ids(ids)),
            LogRows::Memory(rows) => {
                tempo_core::logbook::io_fence::whole_log_off_engine_lock(
                    "a pass over the log's rows",
                );
                let wanted: std::collections::HashSet<_> = ids.iter().collect();
                let found = rows
                    .iter()
                    .filter(|r| r.id.as_ref().is_some_and(|id| wanted.contains(id)))
                    .map(|r| QsoRecord::clone(r))
                    .collect();
                Ok((found, Freshness::Current))
            }
        }
    }

    /// How many contacts the log holds.
    ///
    /// ⚠️ Never under the Engine lock, as [`Self::each`].
    pub fn count(&self) -> Result<(u64, Freshness), sqlite::Error> {
        match self {
            LogRows::Store(reads) => reads.read(READ_WAIT, |db| db.row_count()),
            LogRows::Memory(rows) => Ok((rows.len() as u64, Freshness::Current)),
        }
    }
}

/// What a quit still has to wait for — SPEC-1's C10, the quit that says it is saving.
///
/// This process's changes the writer has not finished with, taken as their tickets, and the
/// `log.adi` copy beside the logbook. Taken under the engine lock ([`LogStore::unsaved`] is
/// pointer copies, no I/O) and waited on with EVERY lock released, from the quit's own thread.
/// Empty on the 1.13 path, which wrote `log.adi` inline.
///
/// Why tickets rather than the writer's own count ([`writer::Status::pending`]): the count goes
/// down both when a change lands and when the writer GIVES UP on one, and a quit must tell
/// those apart. A change still on its way may land if the operator waits; a refused one will
/// not by waiting — its ticket says so, and whether sending it again could
/// ([`Ticket::refusal`]).
#[derive(Default)]
pub struct Unsaved {
    changes: Durability,
    /// The changes the writer had already given up on when this was taken — in memory, not in
    /// the store ([`LogStore::resend`] sends the ones it can again).
    held: Standing,
    mirror: Option<Arc<MirrorWriter>>,
    /// On the 1.13 path, the lane that puts every change in `log.adi`, where it is saved, and
    /// how many changes it had been handed when this was taken.
    lane: Option<(Arc<LogFileWriter>, u64)>,
}

/// How long a change the database refused for a reason that can pass waits before it is sent
/// again by itself, by how many times it has been sent again already — then once a minute for as
/// long as the refusal lasts. Each attempt costs a channel send and, while the disk still
/// refuses, one failed transaction.
const RESEND_AFTER: [Duration; 4] = [
    Duration::from_secs(5),
    Duration::from_secs(15),
    Duration::from_secs(30),
    Duration::from_secs(60),
];

fn resend_after(resends: u32) -> Duration {
    RESEND_AFTER[(resends as usize).min(RESEND_AFTER.len() - 1)]
}

/// A change of this process's the writer has not finished with.
struct InFlight {
    ticket: Ticket,
    /// The rows it writes or removes — less any a later change took over ([`LogStore::supersede`]).
    held: Arc<Held>,
    /// How many times these rows have been sent again after the writer gave up on them — 0 for a
    /// change on its first way to disk.
    resends: u32,
}

/// A change of this process's the writer gave up on ([`Refusal`]): not in the store, its rows
/// held here.
struct Dropped {
    /// The rows it wrote or removed, as it carried them — what a re-send carries, less any a
    /// later change took over.
    held: Arc<Held>,
    /// The revision it first went out with, which a re-send keeps.
    rev: u64,
    refusal: Refusal,
    resends: u32,
    /// When it is next sent again by itself (a refusal that can pass).
    due: Instant,
}

/// One change's rows, as this process holds them until the store does: what it purged, removed
/// and wrote. Each written row is the row AS IT STOOD after the change — state, never a delta —
/// which is what lets it be read in the store's place, and sent again, safely.
#[derive(Debug, Clone, Default)]
pub(crate) struct Held {
    clear: bool,
    remove: Vec<RecordId>,
    upsert: Vec<sqlite::RowWrite>,
}

impl Held {
    fn of(change: &Change) -> Held {
        Held {
            clear: change.clear,
            remove: change.remove.clone(),
            upsert: change.upsert.clone(),
        }
    }

    /// The rows it touches, as a reload of the store must keep them ([`writer::merge_reloaded`]).
    fn touched(&self) -> Touched {
        if self.clear {
            return Touched::All;
        }
        let mut ids = self.remove.clone();
        ids.extend(self.upsert.iter().filter_map(|w| w.rec.id));
        Touched::Rows(ids)
    }

    fn touches(&self, ids: &std::collections::HashSet<RecordId>) -> bool {
        self.remove.iter().any(|id| ids.contains(id))
            || self
                .upsert
                .iter()
                .any(|w| w.rec.id.is_some_and(|id| ids.contains(&id)))
    }

    /// Let go of `ids`: a later change carries them.
    fn release(&mut self, ids: &std::collections::HashSet<RecordId>) {
        self.remove.retain(|id| !ids.contains(id));
        self.upsert
            .retain(|w| w.rec.id.is_none_or(|id| !ids.contains(&id)));
    }

    /// The change again, as it now stands: under revision `rev`, with `marks`.
    fn change(&self, rev: u64, marks: Watermarks) -> Change {
        Change {
            rev,
            priority: if self.clear || self.remove.len() + self.upsert.len() > writer::CHUNK_ROWS {
                writer::Priority::Bulk
            } else {
                writer::Priority::Interactive
            },
            clear: self.clear,
            remove: self.remove.clone(),
            upsert: self.upsert.clone(),
            marks,
            meta: Vec::new(),
        }
    }

    /// What this change says of the row `id`: `Some(Some(row))` it wrote it so, `Some(None)` it
    /// removed it (or purged the log), `None` it says nothing of it.
    fn says(&self, id: RecordId) -> Option<Option<Arc<QsoRecord>>> {
        if let Some(w) = self.upsert.iter().find(|w| w.rec.id == Some(id)) {
            return Some(Some(Arc::clone(&w.rec)));
        }
        (self.clear || self.remove.contains(&id)).then_some(None)
    }
}

/// This process's changes the store may not hold yet — on their way to it, or refused and held
/// for sending again — as a plan reads them ([`LogStore::pending`], SPEC-2 v3 C19). A plan reads
/// a row from the store; if one of these says something of it, that is the row as this process
/// knows it, whatever the store holds, because the store has not taken it yet (or never will
/// on its own). Each row is held by one change at most — a later change takes it over — so
/// the answer does not depend on the order they are asked in.
#[derive(Debug, Clone, Default)]
pub struct Pending(Vec<(u64, Arc<Held>)>);

impl Pending {
    /// What this process's changes the store may not hold say of the row `id`: `Some(Some(row))`
    /// the row as they left it, `Some(None)` gone, `None` nothing — the store's row stands.
    pub fn row(&self, id: RecordId) -> Option<Option<Arc<QsoRecord>>> {
        self.0.iter().find_map(|(_, h)| h.says(id))
    }

    /// Every row these changes wrote that `keep` accepts, in the order the changes were made.
    pub fn rows_matching(&self, keep: impl Fn(&QsoRecord) -> bool) -> Vec<Arc<QsoRecord>> {
        self.0
            .iter()
            .flat_map(|(_, h)| h.upsert.iter().map(|w| &w.rec))
            .filter(|r| keep(r))
            .cloned()
            .collect()
    }

    /// Whether nothing is held: every change this process made is in the store, or on its way to
    /// it with nothing a plan must read in its place.
    pub fn is_empty(&self) -> bool {
        self.0
            .iter()
            .all(|(_, h)| !h.clear && h.remove.is_empty() && h.upsert.is_empty())
    }
}

/// Where the changes a quit is waiting for stand.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Standing {
    /// Changes the writer has not finished with. Waiting may still put them on disk.
    pub pending: usize,
    /// Changes the writer gave up on for a reason that can pass: not in the logbook database,
    /// and sending them again from memory may put them there.
    pub retryable: usize,
    /// Changes the writer gave up on for what they are. They are not in the logbook database,
    /// and neither waiting nor sending them again will put them there; the log in memory still
    /// has them until the process goes.
    pub refused: usize,
    /// Why the first change in `refused` was refused, in the writer's words — for the
    /// diagnostic log and for the operator, untranslated.
    pub reason: Option<String>,
    /// Why the first change in `retryable` was refused, likewise.
    pub retry_reason: Option<String>,
}

impl Standing {
    /// Nothing left on its way, and nothing refused.
    pub fn saved(&self) -> bool {
        self.pending == 0 && self.retryable == 0 && self.refused == 0
    }

    fn count_refusal(&mut self, r: &Refusal) {
        let (count, reason) = if r.retryable {
            (&mut self.retryable, &mut self.retry_reason)
        } else {
            (&mut self.refused, &mut self.reason)
        };
        *count += 1;
        if reason.is_none() {
            *reason = Some(r.reason.clone());
        }
    }
}

impl Unsaved {
    /// Nothing to save: no change the writer has not finished with, none it gave up on, and
    /// no picture of the log waiting for `log.adi`. A quit that finds this closes as it always
    /// did. Never waits.
    pub fn is_empty(&self) -> bool {
        let s = self.standing();
        s.saved() && !self.mirror.as_ref().is_some_and(|m| m.status().pending)
    }

    /// Where the changes stand now, as a quit counts them. Never waits — it reads each ticket,
    /// and does not ask the writer to wait on one, so it is safe to ask under any lock.
    ///
    /// On the 1.13 path a change is saved once it is in `log.adi` — the store is in memory — so
    /// a change the lane has not written yet is still on its way, whatever the store has taken.
    /// Counted from both ends (the writer's tickets, the lane's count), a change owed to either
    /// counts once.
    pub fn standing(&self) -> Standing {
        let mut s = self.stored();
        if let Some((lane, n)) = &self.lane {
            let st = lane.status();
            let owed = n.saturating_sub(st.done) as usize;
            s.pending = s.pending.max(owed);
            if owed > 0 && s.retry_reason.is_none() {
                // What holds the lane now: a file another program wrote, before an older error
                // the lane has not had the chance to try again since.
                s.retry_reason = if st.foreign_write {
                    Some("log.adi was changed by another program or computer".into())
                } else {
                    st.last_error.clone()
                };
            }
        }
        s
    }

    /// Where the changes stand in the STORE — what a read of it can hold, which is what an
    /// export counts ([`crate::logexport`]). On the 1.13 path the store is in memory, so a change
    /// the lane has still to write to `log.adi` is in it already.
    pub fn stored(&self) -> Standing {
        let mut s = self.held.clone();
        for t in &self.changes.tickets {
            match t.refusal() {
                Some(r) => s.count_refusal(&r),
                None if !t.is_resolved() => s.pending += 1,
                None => {}
            }
        }
        s
    }

    /// Wait up to `for_up_to` for the writer to finish with every change — and on the 1.13 path
    /// for its lane to write them to `log.adi` — then say where they stand. ⚠️ The quit's wait:
    /// never call it holding a lock.
    pub fn wait(&self, for_up_to: Duration) -> Standing {
        let start = Instant::now();
        self.wait_for_the_writer(for_up_to);
        if let Some((lane, n)) = &self.lane {
            let _ = lane.wait_written(*n, for_up_to.saturating_sub(start.elapsed()));
        }
        self.standing()
    }

    /// [`Self::wait`] for the store alone, and where the changes stand in it ([`Self::stored`]).
    pub fn wait_stored(&self, for_up_to: Duration) -> Standing {
        self.wait_for_the_writer(for_up_to);
        self.stored()
    }

    fn wait_for_the_writer(&self, for_up_to: Duration) {
        if let Some(writer) = &self.changes.writer {
            let start = Instant::now();
            for t in &self.changes.tickets {
                let left = for_up_to.saturating_sub(start.elapsed());
                if left.is_zero() {
                    break;
                }
                if !t.is_resolved() {
                    let _ = writer.wait_durable(t, left);
                }
            }
        }
    }

    /// Bring `log.adi` up to date now, waiting up to `for_up_to`: `None` when it is written, or
    /// had nothing to write; otherwise what is wrong, for the diagnostic log. The copy is a
    /// convenience beside the logbook, so this is never a question for the operator.
    pub fn write_mirror(&self, for_up_to: Duration) -> Option<String> {
        if let Some((lane, _)) = &self.lane {
            // The 1.13 path: `log.adi` is the log itself, and every change the quit waited for
            // is in it already ([`Self::wait`]) — this is whatever came after.
            let st = lane.flush(for_up_to);
            return st.pending().then(|| {
                format!(
                    "log.adi does not hold {} change(s) yet{}",
                    st.owed(),
                    st.last_error.map(|e| format!(": {e}")).unwrap_or_default()
                )
            });
        }
        let status = self.mirror.as_ref()?.flush(for_up_to);
        if status.pending {
            return Some(format!(
                "log.adi was still being written after {:.1} s",
                for_up_to.as_secs_f32()
            ));
        }
        status
            .last_error
            .map(|e| format!("log.adi could not be brought up to date: {e}"))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::engine::{engine_lock, engine_try_lock, Engine};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::Instant;
    use tempo_core::logbook::{
        adif_header, adif_record_own_log, sqlite::WriteHold, QslVia, QsoEdit,
    };

    /// A data folder of the test's own, gone with the value.
    pub(crate) struct Dir(pub(crate) PathBuf);
    impl Dir {
        pub(crate) fn new(tag: &str) -> Dir {
            static N: AtomicU64 = AtomicU64::new(0);
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos());
            let p = std::env::temp_dir().join(format!(
                "nexus-logstore-{tag}-{}-{}-{nanos}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        pub(crate) fn log(&self) -> PathBuf {
            self.0.join("log.adi")
        }
        pub(crate) fn db(&self) -> PathBuf {
            self.0.join("log.sqlite3")
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// A pre-store `log.adi` of `n` contacts, none carrying an id.
    pub(crate) fn legacy_log(n: usize) -> String {
        let mut s = adif_header();
        for i in 0..n {
            let call = format!("K{i}ABC");
            s.push_str(&format!(
                "<CALL:{}>{call}<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>20260101\
                 <TIME_ON:6>{:02}{:02}{:02}<APP_OTHERLOG_X:3>abc<EOR>\n",
                call.len(),
                i / 3600 % 24,
                i / 60 % 60,
                i % 60,
            ));
        }
        s
    }

    pub(crate) fn no_resolve() -> StoreResolve {
        Arc::new(|_| Resolved::default())
    }

    /// Mirror timings a test can wait out.
    pub(crate) fn fast() -> MirrorOptions {
        MirrorOptions {
            debounce: Duration::from_millis(5),
            max_delay: Duration::from_millis(50),
            accepted: None,
        }
    }

    pub(crate) fn open_fast(d: &Dir) -> Opened {
        open_with(&d.log(), no_resolve(), None, fast()).expect("the store opens")
    }

    pub(crate) fn engine_on_store(d: &Dir) -> Engine {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.attach_log_store(open_fast(d));
        e
    }

    pub(crate) fn qso(call: &str, when: u64) -> QsoRecord {
        let mut r = tempo_core::logbook::Logbook::new();
        r.import_adif(&format!(
            "<CALL:{}>{call}<BAND:3>40m<MODE:2>CW<FREQ:5>7.030<QSO_DATE:8>20260910<TIME_ON:6>120000<EOR>",
            call.len()
        ));
        let mut rec = QsoRecord::clone(&r.records()[0]);
        rec.id = None;
        rec.when_unix = when;
        rec
    }

    /// The rows as the store holds them, read through a connection of the test's own.
    pub(crate) fn stored(d: &Dir) -> Vec<QsoRecord> {
        LogDb::open(&d.db()).unwrap().load_all().unwrap()
    }

    /// Two logs are the same log: the same ids, in the same order, each row the same contact
    /// as the ADIF writer puts it on disk.
    pub(crate) fn same_log<A, B>(a: &[A], b: &[B], what: &str)
    where
        A: std::borrow::Borrow<QsoRecord>,
        B: std::borrow::Borrow<QsoRecord>,
    {
        let ids = |x: &[A]| x.iter().map(|r| r.borrow().id).collect::<Vec<_>>();
        assert_eq!(
            ids(a),
            b.iter().map(|r| r.borrow().id).collect::<Vec<_>>(),
            "{what}: the same rows in the same order"
        );
        for (x, y) in a.iter().zip(b) {
            assert_eq!(
                adif_record_own_log(x.borrow()),
                adif_record_own_log(y.borrow()),
                "{what}: a row differs"
            );
        }
    }

    /// [`same_log`] for the logs of two DIFFERENT engines: each mints its own ids under its own
    /// nonce, so a row the two logged separately is the same contact under a different id. A
    /// minted id is compared by its sequence (the order it was handed out in) and left out of
    /// the text; a provisional id — a function of the file both read — must match exactly.
    pub(crate) fn same_log_across<A, B>(a: &[A], b: &[B], what: &str)
    where
        A: std::borrow::Borrow<QsoRecord>,
        B: std::borrow::Borrow<QsoRecord>,
    {
        use tempo_core::logbook::RecordId;
        let shape = |r: &QsoRecord| match r.id {
            Some(RecordId::Minted { seq, .. }) => format!("minted #{seq}"),
            other => format!("{other:?}"),
        };
        let text = |r: &QsoRecord| {
            let t = adif_record_own_log(r);
            match r.id {
                Some(id @ RecordId::Minted { .. }) => {
                    let id = id.to_string();
                    t.replacen(&format!("<APP_NEXUS_ID:{}>{id}", id.len()), "", 1)
                }
                _ => t,
            }
        };
        assert_eq!(
            a.iter().map(|r| shape(r.borrow())).collect::<Vec<_>>(),
            b.iter().map(|r| shape(r.borrow())).collect::<Vec<_>>(),
            "{what}: the same rows in the same order"
        );
        for (x, y) in a.iter().zip(b) {
            assert_eq!(text(x.borrow()), text(y.borrow()), "{what}: a row differs");
        }
    }

    /// `new`'s rows with each wall-clock stamp set to the matching row of `old`, where both carry
    /// one and they are no more than `spanned` seconds apart — the most wall-clock time the two
    /// engines' calls have taken between them. The stamps the engine takes from the clock itself:
    /// the QSL-sent mark (`date_unix`, `cleared_unix`) and an upload's time (`when_unix`, e.g.
    /// "mark all uploaded"). Everything else — an upload's outcome and detail included — is left
    /// for the comparison to judge, so a stamp missing on one side, or further apart, still fails.
    fn with_wall_clock_of<A, B>(new: &[A], old: &[B], spanned: u64) -> Vec<QsoRecord>
    where
        A: std::borrow::Borrow<QsoRecord>,
        B: std::borrow::Borrow<QsoRecord>,
    {
        let near = |n: Option<u64>, o: Option<u64>| match (n, o) {
            (Some(n), Some(o)) if n.abs_diff(o) <= spanned => Some(o),
            _ => n,
        };
        new.iter()
            .zip(old.iter().map(Some).chain(std::iter::repeat(None)))
            .map(|(n, o)| {
                let mut r = n.borrow().clone();
                if let Some(o) = o.map(|o| o.borrow()) {
                    r.qsl_sent.date_unix = near(r.qsl_sent.date_unix, o.qsl_sent.date_unix);
                    r.qsl_sent.cleared_unix =
                        near(r.qsl_sent.cleared_unix, o.qsl_sent.cleared_unix);
                    let up = &o.upload;
                    for (mine, theirs) in [
                        (&mut r.upload.lotw, &up.lotw),
                        (&mut r.upload.eqsl, &up.eqsl),
                        (&mut r.upload.qrz, &up.qrz),
                        (&mut r.upload.clublog, &up.clublog),
                    ] {
                        if let (Some(m), Some(t)) = (mine.as_mut(), theirs.as_ref()) {
                            if m.when_unix.abs_diff(t.when_unix) <= spanned {
                                m.when_unix = t.when_unix;
                            }
                        }
                    }
                }
                r
            })
            .collect()
    }

    pub(crate) fn flush(e: &Engine) {
        e.flush_log_store(Duration::from_secs(60)).expect("written");
    }

    /// The id of the contact at `at` in `e`'s log: how a test names a row it picked by place.
    pub(crate) fn id_at(e: &Engine, at: usize) -> tempo_core::logbook::RecordId {
        e.log_records()[at]
            .id
            .expect("every row the log holds carries an id")
    }

    // ── the owner ───────────────────────────────────────────────────────────

    /// ★ The switchover. An operator's existing `log.adi` is converted, the log the app holds
    /// is the store's — the same contacts, with the same ids, a load of the file would give —
    /// and from the conversion on `log.adi` is the store's MIRROR: every contact, and then every
    /// change, marked as generated. The file the operator had is kept beside it, byte for byte.
    #[test]
    fn the_store_owns_the_log_and_log_adi_becomes_its_mirror() {
        let d = Dir::new("owner");
        std::fs::write(d.log(), legacy_log(300)).unwrap();
        let original = std::fs::read(d.log()).unwrap();
        let expected = tempo_core::logbook::Logbook::load(&d.log());

        let mut e = engine_on_store(&d);
        assert!(e.log_store_open());
        same_log(e.log_records(), expected.records(), "held after the open");
        same_log(&stored(&d), expected.records(), "stored after the open");
        // The conversion tells the mirror lane, which writes after its debounce, so
        // `log.adi` is the operator's original only until that write lands. Reading it straight
        // after the open raced the lane — green on a quick machine, red on a loaded CI runner.
        // Settle the lane, then hold the file to what the conversion makes it.
        flush(&e);
        assert_eq!(
            mirror::mirror_state(&d.log()),
            MirrorState::Pristine,
            "log.adi is the store's mirror from the conversion on"
        );
        same_log(
            tempo_core::logbook::Logbook::load(&d.log()).records(),
            expected.records(),
            "the mirror carries exactly the converted contacts",
        );
        assert_eq!(
            std::fs::read(d.0.join("log.adi.pre-sqlite")).unwrap(),
            original,
            "and the permanent copy of it is beside it"
        );

        e.log_qso(qso("W9NEW", 1_788_000_000));
        flush(&e);
        let mirror = std::fs::read_to_string(d.log()).unwrap();
        assert!(
            tempo_core::logbook::mirror::is_generated(&d.log()),
            "marked"
        );
        assert_eq!(
            tempo_core::logbook::mirror::mirror_state(&d.log()),
            tempo_core::logbook::mirror::MirrorState::Pristine
        );
        let back = tempo_core::logbook::Logbook::load(&d.log());
        same_log(back.records(), e.log_records(), "the mirror is the log");
        assert!(mirror.contains("W9NEW"));
        same_log(&stored(&d), e.log_records(), "and so is the store");
    }

    // ── launch writes nothing ───────────────────────────────────────────────

    /// Every file a launch could touch, as bytes — and the backup ring's listing.
    fn disk_picture(d: &Dir) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for name in [
            "log.adi",
            "log.sqlite3",
            "log.adi.bak",
            "log.adi.pre-sqlite",
        ] {
            if let Ok(b) = std::fs::read(d.0.join(name)) {
                out.push((name.to_string(), b));
            }
        }
        if let Ok(rd) = std::fs::read_dir(d.0.join("backups")) {
            for e in rd.flatten() {
                out.push((
                    format!("backups/{}", e.file_name().to_string_lossy()),
                    std::fs::read(e.path()).unwrap_or_default(),
                ));
            }
        }
        out.sort();
        out
    }

    // ── the fills (SPEC-2 v3 D2-A) ──────────────────────────────────────────

    /// The resolvers the fill tests hand the engine AND the fill job — one function each, as
    /// the command layer hands both the same functions. A `Q` call has no country; only a `K`
    /// call has a state.
    fn test_country(call: &str) -> Option<String> {
        (!call.starts_with('Q')).then(|| format!("Entity of {call}"))
    }
    fn test_state(call: &str, _grid: Option<&str>) -> Option<String> {
        call.starts_with('K').then(|| "WI".to_string())
    }

    /// A launch with the resolvers set before the log is adopted, as the shell's is.
    fn launch_with_resolvers(d: &Dir) -> Mutex<Engine> {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_dxcc_resolver(test_country);
        e.set_state_resolver(test_state);
        e.attach_log_store(open_fast(d));
        Mutex::new(e)
    }

    /// [`launch_with_resolvers`] onto the 1.13 path: `log.adi` in `d` is the log, its rows in a
    /// store in memory (SPEC-2 v3 C19, D1-A).
    fn launch_on_log_file_with_resolvers(d: &Dir) -> Mutex<Engine> {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_dxcc_resolver(test_country);
        e.set_state_resolver(test_state);
        e.set_log_path(d.log());
        assert!(e.log_on_file(), "premise: the 1.13 path");
        Mutex::new(e)
    }

    /// Each seed of a run on both of the store's homes: the database, and the 1.13 path's store
    /// in memory — `(on the 1.13 path, seed)`.
    fn on_both_homes(seeds: u64) -> impl Iterator<Item = (bool, u64)> {
        [false, true]
            .into_iter()
            .flat_map(move |on_file| (1..=seeds).map(move |seed| (on_file, seed)))
    }

    fn home(on_file: bool) -> &'static str {
        if on_file {
            "the 1.13 path"
        } else {
            "the store"
        }
    }

    /// One run of the fill job, with the same resolvers.
    fn fill(e: &Mutex<Engine>, version: i64) -> crate::logfill::FillOutcome {
        crate::logfill::fill_log_store(e, version, &test_country, &test_state)
            .expect("the fill job runs")
    }

    /// A log.adi of `legacy_log(n)` plus a contact no resolver can place a country for, and one
    /// it can place a country but no state for.
    fn log_to_fill(n: usize) -> String {
        legacy_log(n)
            + "<CALL:5>Q1ZZZ<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>20260102<TIME_ON:6>010101<EOR>\n\
               <CALL:5>DL1XX<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8<QSO_DATE:8>20260102<TIME_ON:6>010202<EOR>\n"
    }

    /// ★ D2-A — THE STORE HOLDS EXACTLY WHAT THE SCREENS SHOW. A log an older build left unfilled
    /// (here: converted with no resolvers) is opened by a launch that has them:
    ///
    /// - the attach fills nothing and writes nothing — memory is the store, row for row;
    /// - the fill job writes every country and state the resolvers can place, into memory and
    ///   the store alike, and records the version it filled for;
    /// - the next run at that version reads nothing and writes nothing — a launch writes
    ///   nothing once its fills are saved;
    /// - a new version (a new cty.dat, a new FCC file, a new build) reads again, and finds only
    ///   what no resolver can place.
    ///
    /// The control is the 1.13 path, which still rewrites `log.adi` when its backfill fills.
    #[test]
    fn the_launch_fills_nothing_and_the_fill_job_saves_the_fills_once() {
        let d = Dir::new("fill-once");
        std::fs::write(d.log(), log_to_fill(40)).unwrap();
        // The conversion, by a launch with no resolvers: every row unfilled in the store.
        flush(&engine_on_store(&d));
        let converted = disk_picture(&d);
        let e = launch_with_resolvers(&d);
        {
            let eng = e.lock().unwrap();
            assert!(
                eng.log_records()
                    .iter()
                    .all(|r| r.country.is_none() && r.state.is_none()),
                "the attach filled nothing"
            );
            same_log(eng.log_records(), &stored(&d), "memory is the store");
            flush(&eng);
        }
        assert_eq!(disk_picture(&d), converted, "and wrote nothing");

        let done = fill(&e, 7);
        assert_eq!(
            done,
            crate::logfill::FillOutcome {
                current: false,
                lacking: 42,
                filled: 41,
            },
            "every contact lacked a field; all but the Q call gained one"
        );
        flush(&e.lock().unwrap());
        {
            let eng = e.lock().unwrap();
            same_log(
                eng.log_records(),
                &stored(&d),
                "the store holds exactly what the screens show",
            );
            for r in eng.log_records() {
                assert_eq!(r.country, test_country(&r.call), "{}", r.call);
                assert_eq!(r.state, test_state(&r.call, None), "{}", r.call);
            }
        }
        let meta = |k| LogDb::open(&d.db()).unwrap().meta(k).unwrap();
        assert_eq!(meta(crate::logfill::FILL_VER), Some(7));

        let settled = disk_picture(&d);
        assert_eq!(
            fill(&e, 7),
            crate::logfill::FillOutcome {
                current: true,
                ..Default::default()
            }
        );
        flush(&e.lock().unwrap());
        assert_eq!(
            disk_picture(&d),
            settled,
            "a launch whose fills are saved writes nothing"
        );

        let again = fill(&e, 8);
        assert_eq!(
            (again.current, again.lacking, again.filled),
            (false, 2, 0),
            "a new version reads again, and finds only what no resolver can place"
        );
        flush(&e.lock().unwrap());
        assert_eq!(meta(crate::logfill::FILL_VER), Some(8));

        // The control: the 1.13 path, the same log, the same backfill, rewrites log.adi.
        let legacy = Dir::new("fill-legacy");
        std::fs::write(legacy.log(), legacy_log(50)).unwrap();
        let _ = tempo_core::logbook::Logbook::load(&legacy.log()); // anchor + sweep marker
        let before = std::fs::read(legacy.log()).unwrap();
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_dxcc_resolver(|call| Some(format!("Entity of {call}")));
        e.set_log_path(legacy.log());
        let e = Mutex::new(e);
        assert_eq!(
            fill(&e, 7),
            crate::logfill::FillOutcome::default(),
            "the fill job does nothing there: its fills are the launch's"
        );
        flush(&e.lock().unwrap());
        assert_ne!(
            std::fs::read(legacy.log()).unwrap(),
            before,
            "control: the 1.13 launch DOES rewrite log.adi when the backfill fills"
        );
    }

    /// ★ D2-A: an imported US contact carries its state from the moment it is imported — in the
    /// store as in memory — where it used to gain it in memory only, at the next launch.
    #[test]
    fn an_imported_us_contact_carries_its_state_from_the_import() {
        let d = Dir::new("import-state");
        let e = launch_with_resolvers(&d);
        e.lock().unwrap().import_adif(
            "<CALL:5>K9ABC<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260910<TIME_ON:6>120000<EOR>\n",
        );
        flush(&e.lock().unwrap());
        let rows = stored(&d);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].state.as_deref(), Some("WI"), "in the store");
        assert_eq!(rows[0].country.as_deref(), Some("Entity of K9ABC"));
        same_log(e.lock().unwrap().log_records(), &rows, "and on the screens");
    }

    /// ★ D2-A: EVERY insert fills country and state before it writes — a logged contact, an
    /// import, a QRZ download's new contacts, a Field Day merge and a `log.adi` taken in at
    /// launch — so no reader of the store ever finds a contact the screens would have filled.
    #[test]
    fn every_insert_path_writes_its_fills_with_the_contact() {
        // A `log.adi` taken in at launch: written over the mirror between two launches.
        let d = Dir::new("insert-fills");
        std::fs::write(d.log(), legacy_log(3)).unwrap();
        flush(&engine_on_store(&d)); // converted (unfilled — the fill job's, not this test's)
        std::fs::write(
            d.log(),
            legacy_log(3)
                + "<CALL:5>K8TAK<BAND:3>40m<MODE:2>CW<QSO_DATE:8>20260911<TIME_ON:6>010101<EOR>\n",
        )
        .unwrap();
        let e = launch_with_resolvers(&d);
        {
            let mut eng = e.lock().unwrap();
            // A logged contact.
            eng.log_qso(qso("K7LOG", 1_788_000_000));
            // An import.
            eng.import_adif(
                "<CALL:5>K6IMP<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260910<TIME_ON:6>130000<EOR>\n",
            );
            // A QRZ download's contact the log lacked.
            let _ = eng.merge_qrz_report(
                "<CALL:5>K5QRZ<BAND:3>15m<MODE:3>SSB<QSO_DATE:8>20260909<TIME_ON:6>140000\
                 <APP_QRZLOG_STATUS:1>C<EOR>\n",
            );
            flush(&eng);
        }
        let rows = stored(&d);
        for call in ["K8TAK", "K7LOG", "K6IMP", "K5QRZ"] {
            let r = rows
                .iter()
                .find(|r| r.call == call)
                .unwrap_or_else(|| panic!("premise: {call} is in the store"));
            assert_eq!(r.country, test_country(call), "{call}'s country, written");
            assert_eq!(r.state.as_deref(), Some("WI"), "{call}'s state, written");
        }
        same_log(
            e.lock().unwrap().log_records(),
            &rows,
            "the store is what the screens show",
        );

        // A Field Day merge into the general log.
        let d = Dir::new("insert-fills-fd");
        let e = launch_with_resolvers(&d);
        {
            let mut eng = e.lock().unwrap();
            let mut s = eng.settings().clone();
            s.fd_active = true;
            s.fd_class = "3A".into();
            s.fd_section = "WI".into();
            s.fd_position_id = "a1b2c3d4".into();
            eng.apply_settings(s);
            eng.set_mode("fieldday-run").unwrap();
            assert!(eng.fd_log_manual("K1ABC", "2A", "EMA", "CW").unwrap());
            assert!(eng.fd_log_manual("W1AW", "1D", "CT", "PH").unwrap());
            assert_eq!(eng.fd_merge_to_general().unwrap().added(), 2);
            flush(&eng);
        }
        let rows = stored(&d);
        let merged = |call: &str| rows.iter().find(|r| r.call == call).cloned().unwrap();
        assert_eq!(merged("K1ABC").state.as_deref(), Some("WI"));
        assert_eq!(merged("K1ABC").country, test_country("K1ABC"));
        assert_eq!(merged("W1AW").country, test_country("W1AW"));
        assert_eq!(
            merged("W1AW").state,
            None,
            "a call no resolver places stays empty"
        );
        same_log(
            e.lock().unwrap().log_records(),
            &rows,
            "the store is what the screens show",
        );
    }

    /// A fill lands only where the field is still empty: a contact given a state since the job
    /// read it keeps that state, and one deleted since is left deleted. (No resolvers here: an
    /// edit fills a missing country itself, which would hide what this is about.)
    #[test]
    fn a_fill_lands_only_where_the_field_is_still_empty() {
        let d = Dir::new("fill-race");
        std::fs::write(d.log(), legacy_log(3)).unwrap();
        flush(&engine_on_store(&d));
        let mut eng = engine_on_store(&d);
        let ids: Vec<_> = eng.log_records().iter().map(|r| r.id.unwrap()).collect();
        // Since the job read them: the operator set row 0's state, and deleted row 2.
        let mut edited = QsoRecord::clone(&eng.log_records()[0]);
        edited.state = Some("MA".into());
        assert!(eng.update_qso(ids[0], edited));
        assert!(eng.delete_qso(ids[2]));
        let fills: Vec<crate::station::LogFill> = ids
            .iter()
            .map(|id| crate::station::LogFill {
                id: *id,
                country: Some("Found".into()),
                state: Some("WI".into()),
            })
            .collect();
        assert_eq!(
            eng.apply_log_fills(&fills, 3),
            2,
            "rows 0 and 1 gained a field"
        );
        flush(&eng);
        let held: Vec<(Option<String>, Option<String>)> = eng
            .log_records()
            .iter()
            .map(|r| (r.country.clone(), r.state.clone()))
            .collect();
        assert_eq!(
            held,
            [
                (Some("Found".into()), Some("MA".into())),
                (Some("Found".into()), Some("WI".into())),
            ],
            "the operator's state kept; the deleted contact not brought back"
        );
        same_log(
            eng.log_records(),
            &stored(&d),
            "and the store holds the same",
        );
    }

    /// ★ C13 × C14: the fill job's write is a change the station follows row by row, as it follows
    /// every other — so the contact and the snapshot after it pay no pass over the log for it. A
    /// fill is an Upgrade, and the hot index rebuilds from the whole log, under the Engine lock,
    /// after any Upgrade it is not walked through.
    #[test]
    #[cfg(debug_assertions)]
    fn the_fill_jobs_write_costs_the_next_contact_no_pass_over_the_log() {
        use tempo_core::logbook::hot::HOT_REBUILDS;
        let d = Dir::new("fill-hot");
        std::fs::write(d.log(), log_to_fill(3)).unwrap();
        flush(&engine_on_store(&d)); // converted with no resolvers: left for the fill job
        let e = launch_with_resolvers(&d);
        let _ = e.lock().unwrap().snapshot();
        HOT_REBUILDS.with(|c| c.set(0));
        let outcome = fill(&e, 3);
        assert!(
            outcome.filled > 0,
            "premise: the job filled contacts ({outcome:?})"
        );
        let mut eng = e.lock().unwrap();
        eng.log_qso(qso("K7NEXT", 1_788_000_000));
        let _ = eng.snapshot();
        assert_eq!(
            HOT_REBUILDS.with(|c| c.get()),
            0,
            "the fills were walked, not rebuilt over"
        );
    }

    /// ★ The fill job never waits and never reads under the Engine lock. With the store's write
    /// lock held elsewhere and a contact submitted, the job's read waits for that contact (P4)
    /// — and the Engine lock is free the whole time it waits. The control shows the wait was
    /// real: the job cannot finish while the write lock is held.
    #[test]
    fn the_fill_job_waits_and_reads_with_the_engine_lock_free() {
        let d = Dir::new("fill-lock");
        std::fs::write(d.log(), legacy_log(4)).unwrap();
        flush(&engine_on_store(&d));
        let e = std::sync::Arc::new(launch_with_resolvers(&d));
        let hold = WriteHold::take(&d.db()).unwrap();
        e.lock().unwrap().log_qso(qso("K4WAIT", 1_788_000_000));
        let job = {
            let e = std::sync::Arc::clone(&e);
            std::thread::spawn(move || fill(&e, 11))
        };
        std::thread::sleep(Duration::from_millis(300));
        assert!(
            !job.is_finished(),
            "control: the job is waiting for the held write"
        );
        for _ in 0..10 {
            assert!(
                e.try_lock().is_ok(),
                "the Engine lock is free while the job waits"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        drop(hold);
        let done = job.join().unwrap();
        assert_eq!(
            done.filled, 4,
            "the converted rows; the logged one arrived filled"
        );
    }

    // ── the log's rows, for a pass over them (SPEC-2 v3 C14) ────────────────

    /// A small deterministic generator: a failing case is reproducible from its seed alone.
    struct Gen(u64);
    impl Gen {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        fn below(&mut self, n: usize) -> usize {
            (self.next() % n.max(1) as u64) as usize
        }
    }

    /// One random change of the kinds the app makes to the log, applied to `e` — a logged
    /// contact, an import (sometimes of a contact the log holds), an edit, a delete, a QSL card,
    /// a QSL-sent mark, a satellite tag, connector stamps, a LoTW report's confirmation, or the
    /// fill job.
    fn random_change(e: &Mutex<Engine>, g: &mut Gen, step: u64) {
        const CALLS: [&str; 6] = ["K1ABC", "W9XYZ", "DL1AB", "Q1ZZZ", "K2DEF/P", "JA1AA"];
        let call = CALLS[g.below(CALLS.len())];
        let when = 1_788_000_000 + step * 60;
        let len = e.lock().unwrap().log_records().len();
        let at = g.below(len);
        match g.below(10) {
            0 => e.lock().unwrap().log_qso(qso(call, when)),
            1 => {
                let _ = e.lock().unwrap().import_adif(&format!(
                    "<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260910<TIME_ON:6>{:06}\
                     <APP_TEMPO_UL_QRZ:19>accepted|1788000500<EOR>\n",
                    call.len(),
                    step % 240_000
                ));
            }
            2 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let mut r = QsoRecord::clone(&eng.log_records()[at]);
                if g.below(2) == 0 {
                    r.band = "40m".into();
                } else {
                    r.call = call.into();
                }
                let id = id_at(&eng, at);
                eng.update_qso(id, r);
            }
            3 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let id = id_at(&eng, at);
                eng.delete_qso(id);
            }
            4 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let id = id_at(&eng, at);
                eng.mark_qsl_card(id, g.below(2) == 0);
            }
            5 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let id = id_at(&eng, at);
                eng.mark_qsl_sent(id, Some(tempo_core::logbook::QslVia::Bureau));
            }
            6 if len > 0 => {
                let sat = (g.below(2) == 0).then_some("RS-44");
                let mut eng = e.lock().unwrap();
                let id = id_at(&eng, at);
                eng.set_sat_tag(id, sat);
            }
            7 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let pushed = QsoRecord::clone(&eng.log_records()[at]);
                let outcome = if g.below(2) == 0 {
                    tempo_core::logbook::UploadOutcome::Accepted
                } else {
                    tempo_core::logbook::UploadOutcome::Rejected
                };
                eng.stamp_qrz_upload(&pushed, outcome, when as i64, None);
                eng.stamp_clublog_upload(&pushed, outcome, when as i64 + 1, None);
            }
            8 if len > 0 => {
                let mut eng = e.lock().unwrap();
                let r = QsoRecord::clone(&eng.log_records()[at]);
                let text = tempo_core::logbook::adif_record(&r)
                    .replace("<EOR>", "<LOTW_QSL_RCVD:1>Y<EOR>");
                let _ = eng.merge_lotw_report(&text);
            }
            _ => {
                let _ = fill(e, (step % 3) as i64);
            }
        }
    }

    /// Every record a pass hands out, whole enough to compare.
    fn pass(
        rows: &LogRows,
        narrow: sqlite::Narrow,
        scope: sqlite::Scope<'_>,
        order: sqlite::Order,
    ) -> Vec<QsoRecord> {
        let mut out = Vec::new();
        rows.each(narrow, scope, order, &mut |r| {
            out.push(r.clone());
            std::ops::ControlFlow::Continue(())
        })
        .expect("the pass reads");
        out
    }

    /// ★ A PASS OVER THE STORE IS A PASS OVER THE LOG IN MEMORY — every row, every field a fold
    /// can ask for (all of them, the stamps included), in log order and newest first, and the
    /// same rows under each scope — after 12 seeded runs of 40 random changes of every kind the
    /// app makes, fills included, on the database and on the 1.13 path's store in memory. The
    /// memory side is the in-memory log's arm, which is the log as every reader saw it before
    /// C14.
    #[test]
    fn a_pass_over_the_store_is_a_pass_over_the_log_in_memory() {
        let every: Vec<&'static str> = [
            "call",
            "band",
            "mode",
            "freq_mhz",
            "freq_rx_mhz",
            "when_unix",
            "time_off_unix",
            "time_known",
            "grid",
            "country",
            "state",
            "name",
            "qth",
            "comment",
            "notes",
            "rst_sent",
            "rst_rcvd",
            "tx_power",
            "dxcc",
            "prop_mode",
            "sat_name",
            "operator",
            "station_callsign",
            "my_grid",
            "my_rig",
            "qsl_card_rcvd_raw",
            "lotw_rcvd_raw",
            "eqsl_rcvd_raw",
            "qrz_status_raw",
            "qsl_sent",
            "qsl_sent_via",
            "qsl_sent_date_unix",
            "qsl_sent_cleared_unix",
            "credit_granted",
            "credit_submitted",
            "ota_my_program",
            "ota_my_ref",
            "ota_their_program",
            "ota_their_ref",
            "ota_iota",
        ]
        .to_vec();
        let whole = sqlite::Narrow {
            columns: Box::leak(every.into_boxed_slice()),
            uploads: true,
        };
        // A whole record as a pass hands it: the passthrough and the contest block are never read.
        let as_passed = |r: &QsoRecord| {
            let mut r = r.clone();
            r.extra.clear();
            r.contest = None;
            r
        };
        let mut changes = 0;
        for (on_file, seed) in on_both_homes(12) {
            let d = Dir::new(&format!("rows-{on_file}-{seed}"));
            std::fs::write(d.log(), log_to_fill(12)).unwrap();
            let e = if on_file {
                launch_on_log_file_with_resolvers(&d)
            } else {
                flush(&engine_on_store(&d));
                launch_with_resolvers(&d)
            };
            let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let seed = format!("{seed}, {}", home(on_file));
            for step in 0..40 {
                random_change(&e, &mut g, step);
                changes += 1;
            }
            let (store, memory) = {
                let eng = e.lock().unwrap();
                (eng.log_rows(), LogRows::Memory(eng.log_records().to_vec()))
            };
            assert!(matches!(store, LogRows::Store(_)), "premise: the store's");
            let held: Vec<QsoRecord> = e
                .lock()
                .unwrap()
                .log_records()
                .iter()
                .map(|r| as_passed(r))
                .collect();
            for order in [sqlite::Order::Log, sqlite::Order::NewestFirst] {
                let from_store = pass(&store, whole, sqlite::Scope::All, order);
                let mut want = held.clone();
                if order == sqlite::Order::NewestFirst {
                    want.reverse();
                }
                assert_eq!(
                    from_store, want,
                    "seed {seed}, {order:?}: every row, every field"
                );
            }
            let ids = |v: Vec<QsoRecord>| v.into_iter().map(|r| r.id).collect::<Vec<_>>();
            let times: Vec<u64> = held.iter().map(|r| r.when_unix).collect();
            let calls: Vec<String> = held.iter().map(|r| sqlite::call_norm_of(&r.call)).collect();
            for t in times.iter().copied().take(6).chain([0, u64::MAX]) {
                let scope = sqlite::Scope::Since(t);
                assert_eq!(
                    ids(pass(&store, whole, scope, sqlite::Order::Log)),
                    ids(pass(&memory, whole, scope, sqlite::Order::Log)),
                    "seed {seed}: since {t}"
                );
            }
            for c in calls.iter().take(6) {
                let scope = sqlite::Scope::CallNorm(c);
                assert_eq!(
                    ids(pass(&store, whole, scope, sqlite::Order::Log)),
                    ids(pass(&memory, whole, scope, sqlite::Order::Log)),
                    "seed {seed}: call {c}"
                );
            }
            assert_eq!(store.count().unwrap().0, held.len() as u64, "seed {seed}");
        }
        assert_eq!(changes, 2 * 12 * 40);
    }

    /// ★ The catch-up sweep picks from the store what it picked from memory: the same contacts,
    /// whole, in log order, each with the legs it is short of — and the room caps it. The oracle
    /// is the pick as `StationCore::requeue_failed_uploads` made it before C14, verbatim.
    #[test]
    fn the_catch_up_sweep_picks_from_the_store_what_it_picked_from_memory() {
        use crate::engine::upload_legs;
        let d = Dir::new("catch-up");
        std::fs::write(d.log(), log_to_fill(20)).unwrap();
        flush(&engine_on_store(&d));
        let e = launch_with_resolvers(&d);
        let mut g = Gen(0xC0FF_EE14);
        for step in 0..60 {
            random_change(&e, &mut g, step);
        }
        let (store, memory, held) = {
            let eng = e.lock().unwrap();
            let held = eng.log_records().to_vec();
            (eng.log_rows(), LogRows::Memory(held.clone()), held)
        };
        let old = |legs: u8, room: usize| -> Vec<(QsoRecord, u8)> {
            held.iter()
                .filter_map(|r| match crate::station::unsent_legs(r, legs) {
                    0 => None,
                    owed => Some((QsoRecord::clone(r), owed)),
                })
                .take(room)
                .collect()
        };
        for legs in [
            upload_legs::QRZ,
            upload_legs::CLUBLOG,
            upload_legs::QRZ | upload_legs::EQSL,
            upload_legs::ALL,
        ] {
            for room in [0, 1, 5, 256] {
                let from_store = crate::station::catch_up_records(&store, legs, room).unwrap();
                let from_memory = crate::station::catch_up_records(&memory, legs, room).unwrap();
                assert_eq!(from_store, old(legs, room), "legs {legs:#b}, room {room}");
                assert_eq!(from_memory, old(legs, room), "legs {legs:#b}, room {room}");
                assert!(from_store.len() <= room);
            }
        }
        assert!(
            !crate::station::catch_up_records(&store, upload_legs::QRZ, 256)
                .unwrap()
                .is_empty(),
            "premise: something is owed"
        );
    }

    // ── the exports, read from the store (SPEC-2 v3 C15) ─────────────────────

    /// One tag, `<NAME:len>value`, with the length in bytes as ADIF counts it.
    fn tag(f: &mut String, name: &str, value: &str) {
        f.push_str(&format!("<{name}:{}>{value}", value.len()));
    }

    /// `n` contacts over three UTC days carrying everything an export writes: Field Day contest
    /// rows with their exchange, foreign tags no build models, private notes, operators and
    /// station calls in other spellings, activations including a two-fer, and text a CSV has to
    /// quote. `day` picks the first of the three days.
    fn export_records(n: usize, day: u32) -> String {
        let mut s = String::new();
        for i in 0..n {
            let mut f = String::new();
            tag(
                &mut f,
                "CALL",
                ["W1AW", "K1ABC/P", "DL1ZZZ", "VP2E/AA9A", "JA1XYZ"][i % 5],
            );
            tag(
                &mut f,
                "QSO_DATE",
                &format!("202609{:02}", day + (i % 3) as u32),
            );
            tag(
                &mut f,
                "TIME_ON",
                ["000000", "235959", "120000", "013000"][i % 4],
            );
            tag(&mut f, "BAND", ["20m", "40m", "2m"][i % 3]);
            tag(&mut f, "MODE", ["FT8", "SSB", "CW"][i % 3]);
            tag(
                &mut f,
                "FREQ",
                ["14.074000", "7.150000", "144.174000"][i % 3],
            );
            tag(&mut f, "NAME", ["Jean-Luc", "Ünïcödé", "Bob, Jr."][i % 3]);
            tag(&mut f, "COMMENT", ["tnx \"73\"", "a,b", "plain"][i % 3]);
            if i % 5 == 0 {
                tag(&mut f, "NOTES", "private, \"quoted\"\nline two");
            }
            if i % 4 != 3 {
                tag(&mut f, "OPERATOR", ["kd9taw", " W1AW ", "K9OP"][i % 4 % 3]);
            }
            if i % 3 == 0 {
                tag(&mut f, "STATION_CALLSIGN", ["KD9TAW", "N0CLUB"][i % 2]);
            }
            if i % 2 == 0 {
                tag(&mut f, "MY_SIG", ["POTA", "pota"][i % 4 / 2]);
                tag(
                    &mut f,
                    "MY_SIG_INFO",
                    ["US-0001", "US-0001,US-0002", "us-0003"][i % 3],
                );
            }
            if i % 4 == 0 {
                tag(&mut f, "CONTEST_ID", "ARRL-FIELD-DAY");
                tag(&mut f, "APP_NEXUS_SESSION", "ARRL-FIELD-DAY:WI");
                tag(
                    &mut f,
                    "APP_NEXUS_QID",
                    &format!("ARRL-FIELD-DAY:WI:a1b2c3d4:{day}{i}"),
                );
                tag(&mut f, "STX", "42");
                tag(&mut f, "SRX", "7");
                tag(&mut f, "STX_STRING", "2A WI");
                tag(&mut f, "SRX_STRING", "1D EMA");
                tag(&mut f, "APP_NEXUS_MYEX", "CLASS::2A;SECTION::WI");
                tag(
                    &mut f,
                    "APP_NEXUS_EX",
                    "CLASS::1D;SECTION::EMA;QTH::Lorain%3B%3A%25 Co",
                );
            }
            if i % 3 == 1 {
                tag(&mut f, "APP_OTHERLOG_F0", "x");
                tag(&mut f, "APP_OTHERLOG_F1", "ünïcödé ✓");
            }
            if i % 6 == 2 {
                tag(&mut f, "APP_TEMPO_UL_CLUBLOG", "duplicate|1700000004|");
                tag(&mut f, "LOTW_QSL_RCVD", "Y");
            }
            s.push_str(&f);
            s.push_str("<EOR>\n");
        }
        s
    }

    /// The operator's `log.adi` for the export tests, as bytes: the committed CP1253 fixture's
    /// three contacts — NOT UTF-8, so they are read lossily, as a real one would be — then 60
    /// contacts of [`export_records`].
    fn export_fixture() -> Vec<u8> {
        let mut bytes =
            include_bytes!("../../tempo-core/tests/fixtures/logbook-cp1253.adi").to_vec();
        bytes.extend_from_slice(export_records(60, 10).as_bytes());
        bytes
    }

    /// Every export and split the Logbook offers of `rows`, labelled by what was asked: ADIF and
    /// CSV whole and bounded at contact times and a second either side, every operator in
    /// `log` in other spellings and one nobody is, every activation it lists and some it does
    /// not.
    fn every_export(
        rows: &crate::logexport::Source,
        log: &tempo_core::logbook::Logbook,
    ) -> Vec<(String, String)> {
        use crate::logexport as x;
        let mut out = Vec::new();
        let mut bounds: Vec<Option<u64>> = vec![None];
        let recs = log.records();
        for r in [recs.first(), recs.get(recs.len() / 2), recs.last()]
            .into_iter()
            .flatten()
        {
            bounds.extend([r.when_unix - 1, r.when_unix, r.when_unix + 1].map(Some));
        }
        for format in ["adif", "csv", "CSV", "Adif"] {
            for &from in &bounds {
                for &to in &bounds {
                    let text = x::export_logbook(rows, format, from, to).expect("exports");
                    out.push((format!("{format} {from:?}..{to:?}"), text.text));
                }
            }
        }
        let mut ops = log.operators();
        ops.extend(["kd9taw", " w1aw", "NOBODY", ""].map(String::from));
        out.push((
            "operators".into(),
            format!("{:?}", x::operators(rows).unwrap()),
        ));
        for op in &ops {
            let text = x::export_for_operator(rows, op).expect("exports");
            out.push((format!("operator {op:?}"), text.text));
        }
        out.push((
            "activations".into(),
            format!("{:?}", x::activations(rows).unwrap()),
        ));
        let mut asks: Vec<(String, u64, Option<String>)> = log
            .activations()
            .into_iter()
            .map(|a| (a.reference, a.day_start_unix, a.callsign))
            .collect();
        let day = recs
            .first()
            .map_or(0, |r| r.when_unix - r.when_unix % 86_400);
        asks.extend([
            ("us-0001".into(), day + 3_600, Some(" kd9taw ".into())),
            ("US-0002".into(), day, None),
            ("".into(), day, Some("KD9TAW".into())),
            ("US-9999".into(), day, Some("KD9TAW".into())),
        ]);
        for (reference, day, call) in &asks {
            let text =
                x::export_for_activation(rows, reference, *day, call.as_deref()).expect("exports");
            out.push((
                format!("activation {reference:?} {day} {call:?}"),
                text.text,
            ));
        }
        out
    }

    /// The same asks of the log in memory through the Logbook's own exports — the Stage 1
    /// answer, whose rules are the ones every export ran before C15 (tempo-core's
    /// `export_rule_tests` holds them to the old code byte for byte).
    fn every_export_in_memory(log: &tempo_core::logbook::Logbook) -> Vec<(String, String)> {
        every_export(
            &crate::logexport::Source::of_rows(LogRows::Memory(log.records().to_vec())),
            log,
        )
    }

    /// ★ EVERY EXPORT FROM THE STORE IS THE EXPORT OF THE LOG IN MEMORY, BYTE FOR BYTE — every
    /// format, range and split of a log converted from an operator's `log.adi` that is not all
    /// UTF-8, carrying contest rows with their exchange, foreign tags, private notes, operators,
    /// station calls and activations, then an import through the engine of more of the same with
    /// the replacement characters a lossy read leaves, and then 8 seeded runs of 24 random
    /// changes of every kind the app makes (logged contacts, imports, edits, deletes, QSL cards
    /// and marks, satellite tags, connector stamps, LoTW confirmations, the fill job), compared
    /// after every fourth — on the database and on the 1.13 path's store in memory. The memory
    /// side is the Logbook's own export of the log the engine holds — what the Export button
    /// wrote before C15.
    #[test]
    fn every_export_from_the_store_is_the_export_of_the_log_in_memory() {
        let mut compared = 0usize;
        let mut nonempty = 0usize;
        for (on_file, seed) in on_both_homes(8) {
            let d = Dir::new(&format!("export-{on_file}-{seed}"));
            std::fs::write(d.log(), export_fixture()).unwrap();
            let e = if on_file {
                launch_on_log_file_with_resolvers(&d)
            } else {
                launch_with_resolvers(&d)
            };
            let _ = e.lock().unwrap().import_adif(
                &export_records(12, 20)
                    .replace("Jean-Luc", "Jos\u{FFFD}")
                    .replace("plain", "a \u{FFFD}\u{FFFD} b"),
            );
            let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let seed = format!("{seed}, {}", home(on_file));
            for step in 0..24 {
                random_change(&e, &mut g, step);
                if step % 4 != 3 {
                    continue;
                }
                let (store, log) = {
                    let eng = e.lock().unwrap();
                    assert!(
                        matches!(eng.log_rows(), LogRows::Store(_)),
                        "premise: the store's rows"
                    );
                    let held: Vec<QsoRecord> = eng
                        .log_records()
                        .iter()
                        .map(|r| QsoRecord::clone(r))
                        .collect();
                    (
                        crate::logexport::Source::of(&eng),
                        tempo_core::logbook::Logbook::from_store(held),
                    )
                };
                let want = every_export_in_memory(&log);
                let got = every_export(&store, &log);
                assert_eq!(got.len(), want.len());
                for ((label, a), (_, b)) in got.iter().zip(&want) {
                    assert!(
                        a == b,
                        "seed {seed}, step {step}: {label} from the store differs from memory\n\
                         store:  {a:.400}\nmemory: {b:.400}"
                    );
                    nonempty += usize::from(a.matches("<EOR>").count() > 0);
                    compared += 1;
                }
                // Premises: the log carries what the ruling is about.
                let recs = log.records();
                assert!(recs.iter().any(|r| r.contest.is_some()), "contest rows");
                assert!(recs.iter().any(|r| !r.extra.is_empty()), "foreign tags");
                assert!(recs.iter().any(|r| r.notes.is_some()), "private notes");
                assert!(
                    recs.iter()
                        .any(|r| r.name.as_deref().is_some_and(|n| n.contains('\u{FFFD}'))),
                    "the lossy read's replacement characters"
                );
            }
        }
        assert!(compared > 5_000, "{compared}");
        assert!(nonempty > 2_000, "the exports held contacts: {nonempty}");
    }

    /// ★ AN EXPORT THE STORE HAS NOT CAUGHT UP WITH IS WRITTEN, AND SAYS HOW MANY CHANGES IT
    /// LACKS (the operator's pick). With the store's write lock held elsewhere (a write taking its
    /// time), a contact logged just before the export is not in the store yet: the export is the
    /// store as it stands — without the contact — and counts the one change still on its way, so
    /// the screen can say so. The control is the same export once the store has it, which holds
    /// it and counts nothing.
    #[test]
    fn an_export_the_store_has_not_caught_up_with_is_written_and_counts_what_it_lacks() {
        let d = Dir::new("export-behind");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let e = launch_with_resolvers(&d);
        flush(&e.lock().unwrap());
        let hold = WriteHold::take(&d.db()).unwrap();
        let source = {
            let mut eng = e.lock().unwrap();
            eng.log_qso(qso("W9LATE", 1_788_100_000));
            crate::logexport::Source::of(&eng)
        };
        let kind = || tempo_core::logbook::ExportKind::Adif {
            from: None,
            to: None,
        };
        let behind = crate::logexport::export_waiting(&source, kind(), Duration::from_millis(300))
            .expect("an export is written while a change is on its way");
        assert_eq!(
            (behind.saving, behind.held),
            (1, 0),
            "and it counts the change it lacks, as still being saved"
        );
        assert!(!behind.text.contains("W9LATE"), "the store as it stands");
        assert_eq!(
            behind.text.matches("<EOR>").count(),
            5,
            "every contact the store holds"
        );
        drop(hold);
        let caught_up = crate::logexport::export_waiting(&source, kind(), Duration::from_secs(60))
            .expect("control: once the store has it");
        assert!(caught_up.text.contains("W9LATE"), "the export holds it");
        assert_eq!(
            (caught_up.saving, caught_up.held),
            (0, 0),
            "and lacks nothing"
        );
    }

    /// ★ AN EXPORT WAITS ABOUT TEN SECONDS FOR A STUCK CHANGE, THEN WRITES THE RESCUE FILE (the
    /// operator's ruling). With the store's write lock held elsewhere, a contact logged just
    /// before the export cannot land — the writer keeps trying for about 21 s before it gives up —
    /// so the export waits out its own bound, measured here, then writes the store as it stands
    /// and counts the change it lacks. The bound that shipped before was a minute: the same export
    /// sat until the writer gave up, which the upper end of the measurement tells apart.
    #[test]
    fn an_export_waits_about_ten_seconds_for_a_stuck_change_then_writes_the_rescue_file() {
        let d = Dir::new("export-wait");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let e = launch_with_resolvers(&d);
        flush(&e.lock().unwrap());
        let hold = WriteHold::take(&d.db()).unwrap();
        let source = {
            let mut eng = e.lock().unwrap();
            eng.log_qso(qso("W9STUCK", 1_788_100_000));
            crate::logexport::Source::of(&eng)
        };
        let asked = Instant::now();
        let written = crate::logexport::export_logbook(&source, "adif", None, None)
            .expect("the rescue file is written");
        let waited = asked.elapsed();
        assert!(
            waited >= Duration::from_millis(9_900) && waited < Duration::from_secs(15),
            "the export waited about ten seconds for the stuck change: {waited:?}"
        );
        assert_eq!(
            (written.saving, written.held),
            (1, 0),
            "and counts it, as still being saved"
        );
        assert!(!written.text.contains("W9STUCK"), "the store as it stands");
        assert_eq!(
            written.text.matches("<EOR>").count(),
            5,
            "every contact the store holds"
        );
        drop(hold);
        let saved = crate::logexport::export_logbook(&source, "adif", None, None)
            .expect("control: once the store has it");
        assert!(saved.text.contains("W9STUCK"), "the export holds it");
        assert_eq!((saved.saving, saved.held), (0, 0), "and lacks nothing");
    }

    // ── the mirror, read from the store (SPEC-2 v3 C15) ──────────────────────

    /// ★ THE MIRROR IS STAGE 1'S, BYTE FOR BYTE — after the conversion of an operator's
    /// `log.adi` that is not all UTF-8 and carries contest rows, foreign tags and private notes,
    /// an import through the engine, and 6 seeded runs of 30 random changes of every kind the app
    /// makes (fills included), `log.adi` is at every fifth change exactly the file Stage 1's
    /// mirror made of the log in memory: `mirror_adif` of it. Stage 1 wrote from memory; the lane
    /// now reads the store.
    #[test]
    fn the_mirror_is_stage_1s_after_every_kind_of_change() {
        let mut compared = 0usize;
        for seed in 1..=6u64 {
            let d = Dir::new(&format!("mirror-{seed}"));
            std::fs::write(d.log(), export_fixture()).unwrap();
            let e = launch_with_resolvers(&d);
            let _ = e
                .lock()
                .unwrap()
                .import_adif(&export_records(10, 20).replace("plain", "a \u{FFFD} b"));
            let mut g = Gen(seed.wrapping_mul(0xA24B_AED4_963E_E407) | 1);
            for step in 0..30 {
                random_change(&e, &mut g, step);
                if step % 5 != 4 {
                    continue;
                }
                let want = {
                    let eng = e.lock().unwrap();
                    flush(&eng);
                    mirror::mirror_adif(eng.log_records())
                };
                let got = std::fs::read(d.log()).unwrap();
                assert!(
                    got == want.as_bytes(),
                    "seed {seed}, step {step}: log.adi is not Stage 1's mirror of the log"
                );
                compared += 1;
            }
        }
        assert_eq!(compared, 36);
    }

    /// ★ The mirror pictures a change only once the store holds it. With the store's write lock
    /// held elsewhere — for longer than the mirror waits for the store in one turn, so that turn
    /// ends with the store still behind — a contact is logged: the mirror, told of it, writes
    /// nothing — a picture without the contact is not a picture of the log — and says a change
    /// is still owed. Once the store has it, the mirror writes it by itself.
    #[test]
    fn the_mirror_waits_for_the_store_to_hold_the_change() {
        let d = Dir::new("mirror-waits");
        std::fs::write(d.log(), legacy_log(3)).unwrap();
        let e = launch_with_resolvers(&d);
        flush(&e.lock().unwrap());
        let status = || {
            e.lock()
                .unwrap()
                .log_mirror_status()
                .expect("the store's mirror")
        };
        let writes = || status().writes;
        let before = writes();
        let hold = WriteHold::take(&d.db()).unwrap();
        e.lock().unwrap().log_qso(qso("W9LATE", 1_788_100_000));
        std::thread::sleep(mirror::READY_WAIT + Duration::from_millis(600));
        let held = status();
        assert_eq!(
            held.writes, before,
            "no picture without the contact: {held:?}"
        );
        assert!(held.pending, "and the change is still owed");
        assert!(!std::fs::read_to_string(d.log()).unwrap().contains("W9LATE"));
        drop(hold);
        let deadline = Instant::now() + Duration::from_secs(20);
        while writes() == before && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            writes() > before,
            "the mirror wrote it once the store had it"
        );
        assert!(std::fs::read_to_string(d.log()).unwrap().contains("W9LATE"));
    }

    // ── the LoTW batch, read from the store (SPEC-2 v3 C15) ──────────────────

    /// The batch file as the upload built it before C15 — `Engine::lotw_upload_adif`'s body,
    /// verbatim but for `self`: the contacts the batch names in the log in memory, in that order.
    fn lotw_adif_before(recs: &[QsoRecord], adif_loc: bool, call: &str, grid: &str) -> String {
        let mut out = tempo_core::logbook::adif_header();
        for r in recs {
            if adif_loc {
                out.push_str(&tempo_core::logbook::adif_record_with_station(
                    r, call, grid,
                ));
            } else {
                out.push_str(&tempo_core::logbook::adif_record(r));
            }
        }
        out
    }

    /// ★ THE LoTW BATCH FROM THE STORE IS THE BATCH THE LOG IN MEMORY MADE — after the export
    /// fixture, contacts with no known time, and 6 seeded runs of 30 random changes with LoTW
    /// stamps of every outcome mixed in (and the operator's "already uploaded" declaration now
    /// and then), the default batch read from the store is exactly the contacts
    /// `lotw_unsent_ids` names in memory: the same records, the same file handed to TQSL in
    /// both location modes (against the pre-C15 builder), the same ids and fingerprints for the
    /// stamp, and the same answer to "is anything owed?".
    #[test]
    fn the_lotw_batch_from_the_store_is_the_batch_the_log_in_memory_made() {
        use tempo_core::logbook::{UploadDetail, UploadOutcome};
        let mut checked = 0usize;
        let mut owed_seen = 0usize;
        for seed in 1..=6u64 {
            let d = Dir::new(&format!("lotw-{seed}"));
            std::fs::write(d.log(), export_fixture()).unwrap();
            let e = launch_with_resolvers(&d);
            // Contacts with no known time of day: never owed.
            let _ = e.lock().unwrap().import_adif(
                "<CALL:5>N0TIM<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260915<EOR>\n\
                 <CALL:5>N1TIM<BAND:3>40m<MODE:2>CW<QSO_DATE:8>20260916<EOR>\n",
            );
            let mut g = Gen(seed.wrapping_mul(0xD1B5_4A32_D192_ED03) | 1);
            for step in 0..30 {
                random_change(&e, &mut g, step);
                {
                    let mut eng = e.lock().unwrap();
                    let len = eng.log_records().len();
                    match g.below(6) {
                        0 | 1 if len > 0 => {
                            let outcome = [
                                UploadOutcome::Pending,
                                UploadOutcome::Accepted,
                                UploadOutcome::Duplicate,
                                UploadOutcome::Rejected,
                                UploadOutcome::AuthFail,
                            ][g.below(5)];
                            let at = id_at(&eng, g.below(len));
                            eng.stamp_lotw_upload(
                                &[at],
                                outcome,
                                1_788_000_000 + step as i64,
                                Some(UploadDetail::RecordRefused).filter(|_| g.below(2) == 0),
                            );
                        }
                        2 if step % 11 == 10 => {
                            eng.mark_lotw_uploaded_all();
                        }
                        _ => {}
                    }
                }
                if step % 3 != 2 {
                    continue;
                }
                let (rows, ids, memory, signed_before) = {
                    let eng = e.lock().unwrap();
                    let ids = eng.lotw_unsent_ids();
                    let memory: Vec<QsoRecord> = eng
                        .log_records()
                        .iter()
                        .filter(|r| r.id.is_some_and(|id| ids.contains(&id)))
                        .map(|r| QsoRecord::clone(r))
                        .collect();
                    (eng.log_rows(), ids.clone(), memory, eng.lotw_signed(&ids))
                };
                let from_store = crate::station::lotw_unsent(&rows).expect("the store reads");
                assert!(
                    from_store == memory,
                    "seed {seed}, step {step}: the store's batch differs from memory's"
                );
                for adif_loc in [false, true] {
                    assert_eq!(
                        crate::station::lotw_batch_adif(&from_store, adif_loc, "KD9TAW", "EN52"),
                        lotw_adif_before(&memory, adif_loc, "KD9TAW", "EN52"),
                        "seed {seed}, step {step}: the file TQSL signs (location in ADIF: {adif_loc})"
                    );
                }
                let signed: Vec<crate::station::LotwSigned> = from_store
                    .iter()
                    .filter_map(crate::station::LotwSigned::of)
                    .collect();
                assert_eq!(
                    signed, signed_before,
                    "seed {seed}, step {step}: the stamp's rows"
                );
                assert_eq!(
                    crate::station::lotw_owed(&rows).unwrap(),
                    !ids.is_empty(),
                    "seed {seed}, step {step}: is anything owed"
                );
                owed_seen += usize::from(!ids.is_empty());
                checked += 1;
            }
        }
        assert_eq!(checked, 60);
        assert!(
            owed_seen > 20,
            "premise: batches with contacts in them ({owed_seen})"
        );
    }

    /// The contacts a list of ids names, read from the store, come back in the order the ids name
    /// them — not log order — with a contact the log does not hold left out and one named twice
    /// there twice: exactly what the log in memory answers for the same ids, on the store and on
    /// the 1.13 path alike. What a LoTW batch chosen by id signs (`station::rows_named`).
    #[test]
    fn the_rows_a_list_of_ids_names_come_back_in_the_order_it_names_them() {
        let d = Dir::new("rows-named");
        std::fs::write(d.log(), export_fixture()).unwrap();
        let e = launch_with_resolvers(&d);
        let (rows, held) = {
            let eng = e.lock().unwrap();
            (eng.log_rows(), eng.log_records().to_vec())
        };
        assert!(matches!(rows, LogRows::Store(_)), "premise: the store's");
        let ids: Vec<tempo_core::logbook::RecordId> = held.iter().filter_map(|r| r.id).collect();
        assert_eq!(ids.len(), held.len(), "premise: every contact has an id");
        let never = tempo_core::logbook::RecordId::Minted {
            posid: 0xdead,
            nonce: 1,
            seq: 999_999,
        };
        assert!(!ids.contains(&never), "premise: an id the log never held");
        // Newest first, one named twice, one the log never held.
        let asked = [ids[40], ids[7], never, ids[52], ids[7], ids[0]];
        let memory: Vec<QsoRecord> = asked
            .iter()
            .filter_map(|id| held.iter().find(|r| r.id == Some(*id)))
            .map(|r| QsoRecord::clone(r))
            .collect();
        assert_eq!(memory.len(), 5, "premise: one left out, one twice");
        let from_store = crate::station::rows_named(&rows, &asked).expect("the store reads");
        assert!(from_store == memory, "the store answers as memory does");
        let from_memory = crate::station::rows_named(&LogRows::Memory(held.clone()), &asked)
            .expect("the log in memory reads");
        assert!(from_memory == memory, "and so does the 1.13 path");
    }

    /// ⛔ PROPERTY 7, the launch after the conversion. Left as the file it was converted from,
    /// `log.adi` is not the store's own picture, so every launch until the first change would
    /// read it and take it in — and taking in is an import. A log holding one contact twice,
    /// one copy confirmed, has the confirmation merged onto the first copy: that launch writes,
    /// the store and `log.adi` both, with the operator having done nothing.
    ///
    /// So the conversion leaves `log.adi` as the store's mirror, and the next launch finds
    /// nothing to read.
    #[test]
    fn the_launch_after_the_conversion_writes_nothing() {
        let d = Dir::new("after-convert");
        let row = "<CALL:5>K1DUP<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260101<TIME_ON:6>120000";
        std::fs::write(
            d.log(),
            format!(
                "{}{row}<EOR>\n{row}<LOTW_QSL_RCVD:1>Y<EOR>\n",
                adif_header()
            ),
        )
        .unwrap();
        flush(&engine_on_store(&d)); // the conversion
        let before = disk_picture(&d);
        flush(&engine_on_store(&d));
        assert!(
            disk_picture(&d) == before,
            "the launch after the conversion wrote to the disk"
        );
        assert_eq!(
            mirror::mirror_state(&d.log()),
            MirrorState::Pristine,
            "log.adi is the store's mirror from the conversion on"
        );
        assert!(
            std::fs::read_to_string(d.0.join("log.adi.pre-sqlite"))
                .unwrap()
                .contains("<LOTW_QSL_RCVD:1>Y"),
            "the file as it was converted is kept beside it"
        );
    }

    /// A `log.adi` taken in that held nothing new — a restored copy of contacts the store
    /// already has — is replaced by the store's mirror, so the next launch does not read and
    /// take it in again.
    #[test]
    fn a_log_adi_taken_in_is_replaced_by_the_mirror_even_when_it_held_nothing_new() {
        let d = Dir::new("take-in-nothing");
        std::fs::write(d.log(), legacy_log(8)).unwrap();
        flush(&engine_on_store(&d)); // the conversion
        std::fs::write(d.log(), legacy_log(5)).unwrap(); // a restored, older copy
        assert_eq!(mirror::mirror_state(&d.log()), MirrorState::Foreign);
        let e = engine_on_store(&d);
        flush(&e);
        assert_eq!(e.log_records().len(), 8, "nothing new, nothing lost");
        assert_eq!(
            mirror::mirror_state(&d.log()),
            MirrorState::Pristine,
            "the mirror replaced the file once its contacts were in"
        );
    }

    /// ★ RESTORING FROM A BACKUP, exactly as `docs/install.md` tells an operator to: quit, move
    /// the database files aside, copy the dated copy over `log.adi`, start. The log is then the
    /// copy's — contacts logged since it was taken are gone from it, which is what a restore is
    /// — and the database moved aside still holds them, so the restore can be undone.
    ///
    /// The control is the same copy put back WITHOUT moving the database: it is taken in as an
    /// import, which adds and never removes, so nothing is restored. That is why the steps say
    /// to move the database first.
    #[test]
    fn restoring_a_backup_copy_by_the_documented_steps_restores_it() {
        let d = Dir::new("restore");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        let e = engine_on_store(&d); // the conversion
        flush(&e);
        // The mirror's first write replaced the converted file; the ring kept that file.
        let ring: Vec<PathBuf> = std::fs::read_dir(d.0.join("backups"))
            .expect("the ring exists")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "adi"))
            .collect();
        assert_eq!(ring.len(), 1, "premise: one dated copy: {ring:?}");
        let copy = ring[0].clone();

        let mut e = e;
        e.log_qso(qso("W1NEW", 1_790_000_000));
        e.log_qso(qso("W2NEW", 1_790_000_600));
        flush(&e);
        drop(e); // quit
        assert_eq!(
            stored(&d).len(),
            12,
            "premise: the store has the new contacts"
        );

        // The control: the copy over `log.adi`, the database left where it is.
        std::fs::copy(&copy, d.log()).unwrap();
        let kept = engine_on_store(&d);
        flush(&kept);
        assert_eq!(
            kept.log_records().len(),
            12,
            "control: with the database in place the copy is only taken in, and nothing goes"
        );
        drop(kept);

        // The documented steps.
        let aside = d.0.join("before-restore");
        std::fs::create_dir_all(&aside).unwrap();
        for name in ["log.sqlite3", "log.sqlite3-wal", "log.sqlite3-shm"] {
            if d.0.join(name).exists() {
                std::fs::rename(d.0.join(name), aside.join(name)).unwrap();
            }
        }
        std::fs::copy(&copy, d.log()).unwrap();
        let restored = engine_on_store(&d);
        flush(&restored);
        let calls: Vec<String> = restored
            .log_records()
            .iter()
            .map(|r| r.call.clone())
            .collect();
        assert_eq!(calls.len(), 10, "the log is the copy's: {calls:?}");
        assert!(
            !calls.iter().any(|c| c.ends_with("NEW")),
            "and only the copy's: {calls:?}"
        );
        assert_eq!(stored(&d).len(), 10, "the new database holds the copy");
        assert_eq!(
            LogDb::open(&aside.join("log.sqlite3"))
                .unwrap()
                .load_all()
                .unwrap()
                .len(),
            12,
            "the database moved aside still has everything, so the restore can be undone"
        );
    }

    // ── parity with the 1.13 path ───────────────────────────────────────────

    /// A report restating the contacts `legacy_log` wrote at `rows` (K<i>ABC, 00:00:i), exactly
    /// as it wrote them, with `tag` added — and `new` calls it never logged.
    fn report(rows: &[usize], new: &[&str], tag: &str) -> String {
        let mut t = adif_header();
        for i in rows {
            let c = format!("K{i}ABC");
            t.push_str(&format!(
                "<CALL:{}>{c}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260101<TIME_ON:6>0000{i:02}{tag}<EOR>\n",
                c.len(),
            ));
        }
        for c in new {
            t.push_str(&format!(
                "<CALL:{}>{c}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260102<TIME_ON:6>120000{tag}<EOR>\n",
                c.len(),
            ));
        }
        t
    }

    /// The contacts a reader of `e`'s log reads, in log order: its store's, once the store holds
    /// every change made before the read — the database on the store path, the store in memory on
    /// the 1.13 path. Never the log in memory, which the cut removes.
    fn store_rows(e: &Engine) -> Vec<QsoRecord> {
        let mut rows = Vec::new();
        let fresh = e
            .log_rows()
            .each_record(DURABLE_WAIT, &mut |r| {
                rows.push(r.clone());
                std::ops::ControlFlow::Continue(())
            })
            .expect("the store reads");
        assert_eq!(
            fresh,
            Freshness::Current,
            "the store holds every change made before the read"
        );
        rows
    }

    /// ★ PROPERTY 3 — THE 1.13 PATH AND THE DATABASE ANSWER ALIKE. The same operations, in the same
    /// order, through a session on the database (the ordinary launch) and one on the 1.13 path —
    /// the database refused: `log.adi` the log's home, its rows in a store in memory. Since SPEC-2
    /// v3 C19 Part C both run the same code, over two stores, and what this holds them to is:
    ///
    /// - after every step, the contacts a reader reads — each session's STORE, read through the
    ///   log's rows — are the same contacts in the same order (each session mints its own ids,
    ///   and a mark stamps its own wall clock, within the seconds the step spanned);
    /// - at the end, each path's durable home holds that log: the database file on the store
    ///   path, and on the 1.13 path `log.adi` — row for row its store, ids and all; and the store
    ///   path's mirror of `log.adi` is the same contacts as the 1.13 path's `log.adi`;
    /// - the purge empties every one of them.
    ///
    /// The operations are every one the app makes to the log through the engine: logging, the
    /// edit, both QSL marks, the satellite tag, a delete, an import, the three report merges,
    /// the park import, the LoTW echo, all four connector stamps, and the purge. Neither
    /// session's log in memory is read: the cut removes it. The 1.13 path's `log.adi` against
    /// 1.13's own code, byte for byte, is `the_1_13_path_writes_log_adi_as_1_13_wrote_it`.
    #[test]
    fn the_database_and_the_1_13_path_answer_alike() {
        let (a, b) = (Dir::new("parity-adif"), Dir::new("parity-store"));
        let start = legacy_log(40);
        std::fs::write(a.log(), &start).unwrap();
        std::fs::write(b.log(), &start).unwrap();
        let mut old = Engine::new("K2DEF", "FN31", 0);
        old.set_log_path(a.log());
        assert!(old.log_on_file(), "premise: the 1.13 path");
        let mut new = engine_on_store(&b);
        assert!(!new.log_on_file(), "premise: the database");
        same_log(&store_rows(&new), &store_rows(&old), "at the start");
        // A contact by its place in the log, as the log's reader reads it.
        fn row_at(e: &Engine, at: usize) -> QsoRecord {
            store_rows(e).swap_remove(at)
        }
        fn id_of(e: &Engine, at: usize) -> tempo_core::logbook::RecordId {
            row_at(e, at).id.expect("an id")
        }

        type Step = (&'static str, Box<dyn Fn(&mut Engine)>);
        let steps: Vec<Step> = vec![
            ("log", Box::new(|e| e.log_qso(qso("W1NEW", 1_788_000_000)))),
            (
                "log 2",
                Box::new(|e| e.log_qso(qso("W2NEW", 1_788_000_600))),
            ),
            (
                "edit",
                Box::new(move |e| {
                    let mut r = row_at(e, 3);
                    r.name = Some("Edited".into());
                    assert!(e.update_qso(r.id.unwrap(), r));
                }),
            ),
            (
                "call fix",
                Box::new(move |e| {
                    let mut r = row_at(e, 4);
                    r.call = "K4FIX".into();
                    assert!(e.update_qso(r.id.unwrap(), r));
                }),
            ),
            (
                "qsl sent",
                Box::new(move |e| assert!(e.mark_qsl_sent(id_of(e, 5), Some(QslVia::Bureau)))),
            ),
            (
                "qsl withdrawn",
                Box::new(move |e| assert!(e.mark_qsl_sent(id_of(e, 5), None))),
            ),
            (
                "card",
                Box::new(move |e| assert!(e.mark_qsl_card(id_of(e, 6), true))),
            ),
            (
                "sat",
                Box::new(move |e| assert!(e.set_sat_tag(id_of(e, 7), Some("AO-91")))),
            ),
            (
                "delete",
                Box::new(move |e| assert!(e.delete_qso(id_of(e, 8)))),
            ),
            (
                "form edit",
                Box::new(move |e| {
                    let id = id_of(e, 20);
                    let stored = e.logged_row(id).expect("held");
                    let key = QsoEdit::project(&stored).key();
                    let mut edit = QsoEdit::project(&stored);
                    edit.comment = Some("the form's edit".into());
                    edit.qsl_sent_via = Some("D".into());
                    edit.qsl_card = true;
                    assert!(matches!(e.edit_qso(id, &key, &edit), Ok(Ok(_))));
                }),
            ),
            (
                "lotw batch",
                Box::new(move |e| {
                    let signed = e.lotw_signed(&[id_of(e, 21), id_of(e, 22)]);
                    e.stamp_lotw_batch(
                        &signed,
                        tempo_core::logbook::UploadOutcome::Pending,
                        1_788_000_111,
                        None,
                    );
                }),
            ),
            (
                "import",
                Box::new(|e| {
                    e.import_adif(&report(&[11], &["K9ZZZ"], "<QSL_RCVD:1>Y"));
                }),
            ),
            (
                "lotw report",
                Box::new(|e| {
                    e.merge_lotw_report(&report(&[12, 13], &[], "<LOTW_QSL_RCVD:1>Y"));
                }),
            ),
            (
                "eqsl report",
                Box::new(|e| {
                    e.merge_eqsl_report(&report(&[15], &[], "<EQSL_QSL_RCVD:1>Y"));
                }),
            ),
            (
                "qrz report",
                Box::new(|e| {
                    e.merge_qrz_report(&report(&[16], &["N0NEW"], "<APP_QRZLOG_STATUS:1>C"));
                }),
            ),
            (
                "parks",
                Box::new(|e| {
                    e.import_pota_log(&report(&[17], &[], "<SIG:4>POTA<SIG_INFO:7>US-0001"));
                }),
            ),
            (
                "own echo",
                Box::new(|e| {
                    e.merge_lotw_own_echo(&report(&[18], &[], ""), 1_788_000_000);
                }),
            ),
            (
                "qrz stamp",
                Box::new(move |e| {
                    let r = row_at(e, 0);
                    e.stamp_qrz_upload(
                        &r,
                        tempo_core::logbook::UploadOutcome::Accepted,
                        1_788_000_000,
                        None,
                    );
                }),
            ),
            (
                "clublog stamp",
                Box::new(move |e| {
                    let r = row_at(e, 1);
                    e.stamp_clublog_upload(
                        &r,
                        tempo_core::logbook::UploadOutcome::Duplicate,
                        1_788_000_000,
                        None,
                    );
                }),
            ),
            (
                "eqsl stamp",
                Box::new(move |e| {
                    let r = row_at(e, 2);
                    e.stamp_eqsl_upload(
                        &r,
                        tempo_core::logbook::UploadOutcome::Rejected,
                        1_788_000_000,
                        None,
                    );
                }),
            ),
            (
                "lotw stamp",
                Box::new(move |e| {
                    e.stamp_lotw_upload(
                        &[id_of(e, 9), id_of(e, 10)],
                        tempo_core::logbook::UploadOutcome::Pending,
                        1_788_000_000,
                        None,
                    );
                }),
            ),
            (
                "mark all uploaded",
                Box::new(|e| {
                    e.mark_lotw_uploaded_all();
                }),
            ),
        ];
        // The QSL-sent mark stamps the wall clock inside the engine (`StationCore::mark_qsl_sent`),
        // once per engine, so a second boundary between the two calls moves that stamp and
        // nothing else — and the stamp stays on the row for every step after. The database's row
        // takes the 1.13 path's stamp when the two lie within the most seconds any pair so far
        // spanned; every other byte, and any stamp further apart or missing on one side, still
        // has to match exactly.
        let mut spanned = 0;
        for (what, step) in &steps {
            let before = crate::engine::now_unix_secs();
            step(&mut old);
            step(&mut new);
            spanned = spanned.max(crate::engine::now_unix_secs() - before);
            let on_file = store_rows(&old);
            let aligned = with_wall_clock_of(&store_rows(&new), &on_file, spanned);
            same_log_across(&aligned, &on_file, what);
        }
        // Every step DID something — the census that keeps the comparisons above from passing
        // over a log nothing happened to.
        let held = store_rows(&new);
        let any = |f: &dyn Fn(&QsoRecord) -> bool| held.iter().any(f);
        for (what, hit) in [
            ("logged", any(&|r| r.call == "W2NEW")),
            ("edited", any(&|r| r.name.as_deref() == Some("Edited"))),
            ("call fixed", any(&|r| r.call == "K4FIX")),
            ("qsl withdrawn", any(&|r| r.qsl_sent.cleared_unix.is_some())),
            ("card", any(&|r| r.qsl_rcvd.card)),
            ("sat", any(&|r| r.sat_name.as_deref() == Some("AO-91"))),
            (
                "form edit",
                any(&|r| {
                    r.comment.as_deref() == Some("the form's edit")
                        && r.qsl_sent.via == Some(QslVia::Direct)
                        && r.qsl_rcvd.card
                }),
            ),
            (
                "lotw batch",
                any(&|r| {
                    r.upload
                        .lotw
                        .as_ref()
                        .is_some_and(|u| u.when_unix == 1_788_000_111)
                }),
            ),
            ("imported", any(&|r| r.call == "K9ZZZ")),
            (
                "import upgraded",
                any(&|r| r.call == "K11ABC" && r.qsl_rcvd.card),
            ),
            ("lotw", any(&|r| r.qsl_rcvd.lotw)),
            ("eqsl", any(&|r| r.qsl_rcvd.eqsl)),
            ("qrz", any(&|r| r.qsl_rcvd.qrz)),
            ("qrz added", any(&|r| r.call == "N0NEW")),
            (
                "park",
                any(&|r| r.ota.their_ref.as_deref() == Some("US-0001")),
            ),
            ("qrz stamp", any(&|r| r.upload.qrz.is_some())),
            ("clublog stamp", any(&|r| r.upload.clublog.is_some())),
            ("eqsl stamp", any(&|r| r.upload.eqsl.is_some())),
            ("lotw stamp", any(&|r| r.upload.lotw.is_some())),
        ] {
            assert!(
                hit,
                "the step '{what}' left no trace — the comparison above proved nothing"
            );
        }
        assert!(
            any(&|r| r.call == "K18ABC"
                && r.upload.lotw.as_ref().is_some_and(|u| {
                    u.outcome == tempo_core::logbook::UploadOutcome::Accepted
                        && u.when_unix == 1_788_000_000
                        && u.detail.is_none()
                })),
            "the own echo promoted its row (and the later mark-all left it alone)"
        );
        flush(&new);
        flush(&old);
        let on_file = store_rows(&old);
        same_log_across(
            &with_wall_clock_of(&stored(&b), &on_file, spanned),
            &on_file,
            "the database file",
        );
        let (mirror, file) = (
            tempo_core::logbook::Logbook::load(&b.log()),
            tempo_core::logbook::Logbook::load(&a.log()),
        );
        same_log(
            file.records(),
            &on_file,
            "the 1.13 path's log.adi is its store, ids and all",
        );
        same_log_across(
            &with_wall_clock_of(mirror.records(), file.records(), spanned),
            file.records(),
            "the mirror against the 1.13 path's log.adi",
        );

        // And the purge.
        old.clear_logbook();
        new.clear_logbook();
        flush(&new);
        flush(&old);
        assert!(store_rows(&new).is_empty() && stored(&b).is_empty());
        assert!(tempo_core::logbook::Logbook::load(&b.log()).is_empty());
        assert!(
            store_rows(&old).is_empty(),
            "the 1.13 path's store is empty"
        );
        assert!(
            tempo_core::logbook::Logbook::load(&a.log()).is_empty(),
            "and so is its log.adi"
        );
    }

    // ── durability ──────────────────────────────────────────────────────────

    /// A command waits for exactly its own change: the tickets it collected, and not the whole
    /// queue. After the wait a connection of the test's own sees the change.
    #[test]
    fn a_command_waits_for_its_own_change_and_then_it_is_on_disk() {
        let d = Dir::new("durable");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        let mut e = engine_on_store(&d);
        let (ok, durability) = e.with_log_tickets(|e| e.mark_qsl_card(id_at(e, 3), true));
        assert!(ok);
        assert_eq!(durability.len(), 1, "one change, one ticket");
        durability.wait(DURABLE_WAIT).expect("durable");
        let id = e.log_records()[3].id;
        let row = stored(&d).into_iter().find(|r| r.id == id).expect("stored");
        assert!(
            row.qsl_rcvd.card,
            "the mark is on disk when the wait returns"
        );

        // Nothing changed, nothing to wait for.
        let nowhere = tempo_core::logbook::RecordId::Provisional {
            hash: 999,
            ordinal: 0,
        };
        let (_, none) = e.with_log_tickets(|e| e.mark_qsl_card(nowhere, true));
        assert!(none.is_empty());
    }

    /// ★ SPEC-2 C16: THE LOGBOOK FORM'S WHOLE EDIT IS ONE COMMIT. The fields, the QSL-sent mark
    /// and the paper-card mark the form changes are one change — one ticket, one write — where
    /// the form used to send three commands, each its own write, with a window between each for
    /// another writer's change to land in. On disk, all three are there when the wait returns.
    ///
    /// The control is the same edit sent as those three commands, on the row beside it: three
    /// tickets. So the one ticket above is the edit being one change, not a count that cannot
    /// tell one change from three.
    #[test]
    fn the_form_s_whole_edit_is_one_commit() {
        let d = Dir::new("one-commit");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        let mut e = engine_on_store(&d);
        flush(&e);
        let form = |stored: &QsoRecord| {
            let mut edit = QsoEdit::project(stored);
            edit.name = Some("Edited".into());
            edit.qsl_sent_via = Some("B".into());
            edit.qsl_card = true;
            edit
        };

        let id = id_at(&e, 3);
        let stored = e.logged_row(id).expect("held");
        let (key, edit) = (QsoEdit::project(&stored).key(), form(&stored));
        let (done, durability) = e.with_log_tickets(|e| e.edit_qso(id, &key, &edit));
        assert!(matches!(done, Ok(Ok(_))), "{done:?}");
        assert_eq!(durability.len(), 1, "the whole edit is one change");
        durability.wait(DURABLE_WAIT).expect("durable");
        let row = stored_row(&d, id);
        assert_eq!(
            (row.name.as_deref(), row.qsl_sent.via, row.qsl_rcvd.card),
            (Some("Edited"), Some(QslVia::Bureau), true),
            "all three are on disk when the one wait returns"
        );

        let id = id_at(&e, 4);
        let mut rec = QsoRecord::clone(&e.logged_row(id).expect("held"));
        rec.name = Some("Edited".into());
        let (_, three) = e.with_log_tickets(|e| {
            e.update_qso(id, rec)
                && e.mark_qsl_sent(id, Some(QslVia::Bureau))
                && e.mark_qsl_card(id, true)
        });
        assert_eq!(
            three.len(),
            3,
            "control: the form's three commands are three changes"
        );
    }

    /// The row `id` as the store holds it, read through a connection of the test's own.
    fn stored_row(d: &Dir, id: tempo_core::logbook::RecordId) -> QsoRecord {
        stored(d)
            .into_iter()
            .find(|r| r.id == Some(id))
            .expect("stored")
    }

    // ── reading the store (SPEC-2's read path) ──────────────────────────────

    /// ★ A READ OF THE STORE SEES EVERY CHANGE MADE BEFORE IT WAS ASKED FOR — the way a read of
    /// the log in memory always has — and it is the log, row for row.
    ///
    /// The control is the write lock taken elsewhere: while the contact cannot be committed,
    /// the read says it is Stale and the contact is not in what it saw — so the Current read
    /// afterwards is the wait's doing, not a store that happened to be caught up.
    #[test]
    fn a_read_of_the_store_sees_every_change_made_before_it_was_asked_for() {
        let d = Dir::new("reads");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        let mut e = engine_on_store(&d);
        flush(&e);

        let hold = WriteHold::take(&d.db()).expect("hold the write lock");
        e.log_qso(qso("W1READ", 1_788_000_000));
        let reads = e.log_store_reads().expect("the store owns the log");
        let (rows, fresh) = reads.rows(Duration::from_millis(300)).expect("read");
        assert!(matches!(fresh, Freshness::Stale(_)), "{fresh:?}");
        assert_eq!(rows.len(), 10, "control: the store as it stood");
        assert!(
            rows.iter().all(|r| r.call != "W1READ"),
            "control: the contact is not in the store yet"
        );

        drop(hold);
        let (rows, fresh) = reads.rows(DURABLE_WAIT).expect("read");
        assert_eq!(fresh, Freshness::Current);
        same_log(&rows, e.log_records(), "a read of the store is the log");
    }

    /// ★ A NEW STATION KEEPS ITS LOG IN A STORE (SPEC-2 v3 C19, D1-A): an empty one in this
    /// process's memory, with no file behind it and no `log.adi` beside it, until the launch
    /// gives it the operator's. A contact it logs is in that store, where every pass over the
    /// log reads it. The control: the 1.13 path, which leaves the store for `log.adi`.
    #[test]
    fn a_new_station_keeps_its_log_in_an_empty_store_in_memory() {
        let sc = crate::station::StationCore::new();
        let store = sc.store.as_ref().expect("a new station holds a store");
        let name = store.db_path().to_string_lossy().into_owned();
        assert!(
            name.starts_with("file:/nexus-log-") && name.ends_with("?vfs=memdb"),
            "in memory: {name}"
        );
        assert!(!store.db_path().exists(), "no file behind it");
        assert!(store.log_path().is_none(), "and no log.adi beside it");
        assert!(store.mirror_status().is_none(), "so nothing to mirror");

        let mut e = Engine::new("K2DEF", "FN31", 0);
        assert!(e.log_store_open(), "an engine's station too");
        e.log_qso(qso("W1NEW", 1_788_200_000));
        let mut calls = Vec::new();
        let fresh = e
            .log_rows()
            .each_record(DURABLE_WAIT, &mut |r| {
                calls.push(r.call.clone());
                std::ops::ControlFlow::Continue(())
            })
            .expect("the store reads");
        assert_eq!(fresh, Freshness::Current);
        assert_eq!(calls, ["W1NEW"], "the contact is in the store");

        assert!(!e.log_on_file(), "and no log.adi to keep");
        let d = Dir::new("new-station");
        e.set_log_path(d.log());
        assert!(
            e.log_on_file() && e.log_store_open(),
            "control: the 1.13 path keeps log.adi, its rows in a store in memory"
        );
    }

    /// ⛔ A CONTACT WRITTEN BEFORE THE LAUNCH ATTACHES THE OPERATOR'S LOG IS NOT CARRIED INTO IT
    /// — exactly as before a new station held a store. The attach, and the fallback to
    /// `log.adi`, REPLACE the log a station was built with: before SPEC-2 v3 C19 an empty log in
    /// memory with nowhere to write, now an empty store in memory, and either is replaced the
    /// same way, whatever was written into it. So the launch must write nothing before the
    /// attach, and nothing can: src-tauri's
    /// `nothing_can_write_the_log_before_the_launch_attaches_it` pins why. The two stations
    /// here differ in that alone: one built as a station is now, one with its store taken away,
    /// as a station was built before.
    #[test]
    fn a_contact_written_before_the_attach_is_not_carried_into_the_operator_s_log_as_before() {
        for built_before_c19 in [false, true] {
            for fallback in [false, true] {
                let d = Dir::new(&format!("pre-attach-{built_before_c19}-{fallback}"));
                std::fs::write(d.log(), legacy_log(3)).unwrap();
                let mut sc = crate::station::StationCore::new();
                if built_before_c19 {
                    sc.store = None;
                }
                // The write the FT auto-log makes: the station's append.
                let _ = sc.append(vec![qso("W9EARLY", 1_788_300_000)], false);
                if fallback {
                    sc.set_log_path(d.log());
                } else {
                    sc.attach_store(open_fast(&d));
                }
                let what = format!("built before C19: {built_before_c19}, fallback: {fallback}");
                let calls: Vec<&str> = sc.logbook.records().iter().map(|r| &*r.call).collect();
                assert_eq!(
                    calls.len(),
                    3,
                    "the operator's log, whole ({what}): {calls:?}"
                );
                assert!(!calls.contains(&"W9EARLY"), "and nothing else ({what})");
                if !fallback {
                    let store = sc.store.as_ref().expect("the operator's store");
                    store.flush(DURABLE_WAIT).expect("written");
                    let rows = stored(&d);
                    assert_eq!(rows.len(), 3, "the store holds its own rows ({what})");
                    assert!(rows.iter().all(|r| r.call != "W9EARLY"), "only ({what})");
                }
                let file = String::from_utf8_lossy(&std::fs::read(d.log()).unwrap()).into_owned();
                assert!(!file.contains("W9EARLY"), "nor is it in log.adi ({what})");
            }
        }
    }

    /// ★ POSITIVE CONTROL for the launch's guard: a change sent to the launch's engine before the
    /// attach — which would be lost with the placeholder the attach replaces — is a panic in a
    /// debug build, naming it.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(
        expected = "a change reached the launch's placeholder log before the launch attached"
    )]
    fn a_change_before_the_launch_attaches_the_log_is_refused_in_a_debug_build() {
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.refuse_log_changes_until_attached();
        e.log_qso(qso("W9EARLY", 1_788_300_000));
    }

    /// The attach, and the 1.13 path, replace the launch's placeholder: its engine then logs as
    /// any does, into the operator's log. Reading the placeholder before (the launch seeds the
    /// decoder's hash table from it) is no change, and is allowed.
    #[test]
    fn the_attach_and_the_1_13_path_replace_the_launchs_placeholder() {
        for on_file in [false, true] {
            let d = Dir::new(&format!("placeholder-{on_file}"));
            std::fs::write(d.log(), legacy_log(3)).unwrap();
            let mut e = Engine::new("K2DEF", "FN31", 0);
            e.refuse_log_changes_until_attached();
            assert_eq!(e.log_rows().count().expect("reads").0, 0, "a read");
            if on_file {
                e.set_log_path(d.log());
            } else {
                e.attach_log_store(open_fast(&d));
            }
            e.log_qso(qso("W9LATE", 1_788_300_000));
            flush(&e);
            assert_eq!(e.log_records().len(), 4, "on_file {on_file}");
        }
    }

    /// ★ POSITIVE CONTROL: a read of the store under the Engine lock is a panic in a debug
    /// build — the wait for the writer is the first thing it does, and it is fenced. Every
    /// other read here runs with the lock released, and stays quiet.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(
        expected = "io_fence: a wait for the logbook store to hold every change ran while this thread holds"
    )]
    fn a_read_of_the_store_under_the_engine_lock_is_refused() {
        let d = Dir::new("reads-fence");
        std::fs::write(d.log(), legacy_log(3)).unwrap();
        let engine = Mutex::new(engine_on_store(&d));
        let e = engine_lock(&engine);
        let reads = e.log_store_reads().expect("the store owns the log");
        let _ = reads.rows(Duration::ZERO);
    }

    // ── property 5: no I/O under the engine lock ────────────────────────────

    /// A change to the contact `id` the way a command makes it ([`crate::logwrite::change_row`]):
    /// planned with the Engine lock released, made under it — its durability.
    fn by_command(
        engine: &Mutex<Engine>,
        id: tempo_core::logbook::RecordId,
        op: tempo_core::logbook::LogOp,
    ) -> Durability {
        let (made, durability) = crate::logwrite::change_row(
            engine,
            id,
            "test",
            |_, row| Ok(crate::station::ops_on(row, std::slice::from_ref(&op))),
            |_, made| made,
        );
        assert!(matches!(made, Ok(Ok(Some(_)))), "the change is made");
        durability
    }

    /// ★ PROPERTY 5. With the store's write lock held elsewhere — a write that is taking its
    /// time — every log command still returns at once, another thread can `try_lock` the engine
    /// the whole time, and only the off-lock wait feels the stall. The FT auto-log appends under
    /// the lock with no read at all; a change to a held row plans with the lock released, on the
    /// store as it stands with this process's own changes still on their way laid over it, so
    /// its plan does not wait for the stalled write either (SPEC-2 v3 C19).
    ///
    /// The positive control is the timed-out wait: it proves the writer really was stalled, so
    /// the prompt returns above are not a writer that simply finished first.
    #[test]
    fn nothing_under_the_engine_lock_waits_for_a_stalled_write() {
        use tempo_core::logbook::LogOp;
        let d = Dir::new("stall");
        std::fs::write(d.log(), legacy_log(20)).unwrap();
        let engine = Arc::new(Mutex::new(engine_on_store(&d)));
        let (second, fifth) = {
            let e = engine_lock(&engine);
            (id_at(&e, 2), id_at(&e, 5))
        };

        let hold = WriteHold::take(&d.db()).expect("hold the write lock");
        let started = Instant::now();
        let a = {
            let mut e = engine_lock(&engine);
            e.with_log_tickets(|e| e.log_qso(qso("W1STALL", 1_788_000_000)))
                .1
        };
        let b = by_command(
            &engine,
            second,
            LogOp::MarkQslCard {
                id: second,
                received: true,
            },
        );
        let c = by_command(&engine, fifth, LogOp::Delete(fifth));
        assert_eq!((a.len(), b.len(), c.len()), (1, 1, 1));
        let durability = vec![a, b, c];
        let under_lock = started.elapsed();
        assert!(
            under_lock < Duration::from_millis(500),
            "three changes took {under_lock:?} with the writer stalled"
        );

        // Another thread takes the engine lock while the write is still stalled.
        let other = Arc::clone(&engine);
        let took = std::thread::spawn(move || engine_try_lock(&other).is_ok())
            .join()
            .unwrap();
        assert!(took, "try_lock succeeds during the stalled write");

        // The control: the write really is stalled.
        assert!(
            durability[0].wait(Duration::from_millis(300)).is_err(),
            "control: the change cannot be on disk while the lock is held elsewhere"
        );

        drop(hold);
        for w in &durability {
            w.wait(DURABLE_WAIT)
                .expect("durable once the lock is released");
        }
        let e = engine_lock(&engine);
        same_log(&stored(&d), e.log_records(), "after the stall");
    }

    // ── C11: the fence ──────────────────────────────────────────────────────

    /// A store-backed engine behind the Engine mutex, as the app holds it.
    fn shared_on_store(d: &Dir) -> Arc<Mutex<Engine>> {
        Arc::new(Mutex::new(engine_on_store(d)))
    }

    /// ★ C11 POSITIVE CONTROL. A command that waited for its change while still holding the
    /// Engine lock would hold the radio loop for as long as the disk takes; in a debug build the
    /// fence stops it at the wait.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(
        expected = "io_fence: a wait for a logbook change to reach the disk ran while this thread \
                    holds the Engine lock"
    )]
    fn a_durable_wait_under_the_engine_lock_trips_the_fence() {
        let d = Dir::new("fence-wait");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let engine = shared_on_store(&d);
        let id = id_at(&engine_lock(&engine), 1);
        let durability = by_command(
            &engine,
            id,
            tempo_core::logbook::LogOp::MarkQslCard { id, received: true },
        );
        let _e = engine_lock(&engine);
        let _ = durability.wait(DURABLE_WAIT); // `_e` is held
    }

    /// The other direction: the same wait with the guard dropped first passes, and the change
    /// is on disk.
    #[cfg(debug_assertions)]
    #[test]
    fn a_durable_wait_after_the_engine_lock_is_released_passes_the_fence() {
        let d = Dir::new("fence-wait-ok");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let engine = shared_on_store(&d);
        let id = id_at(&engine_lock(&engine), 1);
        let durability = by_command(
            &engine,
            id,
            tempo_core::logbook::LogOp::MarkQslCard { id, received: true },
        );
        durability
            .wait(DURABLE_WAIT)
            .expect("durable, with no lock held");
        let id = engine_lock(&engine).log_records()[1].id;
        assert!(
            stored(&d).iter().any(|r| r.id == id && r.qsl_rcvd.card),
            "the mark is on disk"
        );
    }

    /// ★ C11 POSITIVE CONTROL. Remote redeems an append's receipt after it has released the
    /// engine; redeemed under the lock, the fence stops it.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(
        expected = "io_fence: a wait for an append to reach the disk ran while this thread holds \
                    the Engine lock"
    )]
    fn an_append_receipt_synced_under_the_engine_lock_trips_the_fence() {
        let d = Dir::new("fence-receipt");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let engine = shared_on_store(&d);
        let mut e = engine_lock(&engine);
        let crate::engine::LogWriteOutcome::PendingSync(receipts) =
            e.log_qso_for_sync(qso("W1FNC", 1_788_000_000))
        else {
            panic!("premise: the contact is appended and owes a receipt");
        };
        for r in receipts {
            let _ = r.sync(); // `e` is still held
        }
    }

    /// ★ C11 POSITIVE CONTROL. Opening the store — at launch, a conversion of a lifetime log —
    /// belongs before the engine is locked.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(
        expected = "io_fence: opening the logbook store ran while this thread holds the Engine lock"
    )]
    fn opening_the_store_under_the_engine_lock_trips_the_fence() {
        let d = Dir::new("fence-open");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let engine = Arc::new(Mutex::new(Engine::new("K2DEF", "FN31", 0)));
        let _e = engine_lock(&engine);
        let _ = open_fast(&d);
    }

    /// Why `tests/engine_guard.rs` scans for raw locks: a guard taken with `Mutex::lock()` is the
    /// same lock, and the fence cannot see it — the very wait the first control above stops goes
    /// through here unremarked.
    #[cfg(debug_assertions)]
    #[test]
    fn a_raw_engine_lock_is_invisible_to_the_fence() {
        let d = Dir::new("fence-raw");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let engine = shared_on_store(&d);
        let mut raw = engine.lock().unwrap();
        let (_, durability) = raw.with_log_tickets(|e| e.mark_qsl_card(id_at(e, 1), true));
        assert_eq!(
            tempo_core::logbook::io_fence::engine_guards_held(),
            0,
            "a raw guard is not counted"
        );
        durability
            .wait(DURABLE_WAIT)
            .expect("the fence does not fire: it cannot see this lock");
    }

    /// The datagram WSJT-X sends when it logs a contact (UDP type 12, LoggedAdif), as it
    /// sends it: its own header, lower-case tags, no COUNTRY.
    const WSJTX_LOGGED_ADIF: &str = "\n<adif_ver:5>3.1.0\n<programid:6>WSJT-X\n<EOH>\n\
        <call:5>W1ABC <gridsquare:4>FN42 <mode:3>FT8 <rst_sent:3>-10 <rst_rcvd:3>-12 \
        <qso_date:8>20260923 <time_on:6>120015 <qso_date_off:8>20260923 <time_off:6>120115 \
        <band:3>20m <freq:9>14.075512 <station_callsign:5>K2DEF <my_gridsquare:4>FN31 <EOR>";

    /// ★ PROPERTY 5, the companion import (SPEC-2 v3 C19 Part B). WSJT-X's LoggedAdif is imported
    /// beside the radio loop, never in its tick ([`crate::logwrite::import_logged_contact`]). With
    /// the store's write lock held elsewhere the import is still made at once — planned on the
    /// store as it stands, with this process's changes laid over it, and made under the Engine
    /// lock — and the contact reaches the disk when the write clears.
    ///
    /// The positive control is the timed-out wait: the writer really was stalled, so the
    /// prompt return is not a write that simply finished first.
    #[test]
    fn the_companion_import_is_made_while_the_writer_is_stalled() {
        let d = Dir::new("companion");
        std::fs::write(d.log(), legacy_log(20)).unwrap();
        let engine = std::sync::Mutex::new(engine_on_store(&d));

        let hold = WriteHold::take(&d.db()).expect("hold the write lock");
        let started = Instant::now();
        let (added, durable) = crate::logwrite::import_logged_contact(&engine, WSJTX_LOGGED_ADIF);
        let took = started.elapsed();
        assert_eq!(added, Ok(1), "the contact WSJT-X logged is in the log");
        assert!(
            took < Duration::from_millis(500),
            "the import took {took:?} with the writer stalled"
        );
        assert!(
            durable.wait(Duration::from_millis(300)).is_err(),
            "control: the contact cannot be on disk while the write lock is held elsewhere"
        );

        drop(hold);
        durable
            .wait(DURABLE_WAIT)
            .expect("durable once the lock is released");
        let e = crate::engine::engine_lock(&engine);
        same_log(&stored(&d), e.log_records(), "after the stall");
        assert!(
            stored(&d).iter().any(|r| r.call == "W1ABC"),
            "the store has the contact"
        );
    }

    // ── the quit (C10) ──────────────────────────────────────────────────────

    /// A quit waits for what is on its way, and only that. With nothing submitted since the
    /// last write there is nothing to wait for, which is what lets the window close as it
    /// always did; a change held up by a stalled disk is seen, counted, waited for no longer
    /// than the quit allows, and seen to land once the disk clears.
    ///
    /// The control is the stalled change itself: the check that answers "nothing" after the
    /// write answers "one" before it, so its empty answer is not a check that cannot see.
    #[test]
    fn a_quit_waits_for_exactly_the_changes_on_their_way() {
        let d = Dir::new("quit-wait");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        let mut e = engine_on_store(&d);
        flush(&e);
        assert!(
            e.log_unsaved().is_empty(),
            "nothing on its way: nothing to wait for"
        );

        let hold = WriteHold::take(&d.db()).expect("stall the store");
        assert!(e.mark_qsl_card(id_at(&e, 3), true));
        let unsaved = e.log_unsaved();
        assert!(!unsaved.is_empty(), "control: a change on its way is seen");
        assert_eq!(
            unsaved.standing(),
            Standing {
                pending: 1,
                retryable: 0,
                refused: 0,
                reason: None,
                retry_reason: None,
            },
            "one change, not yet on disk, not refused"
        );
        let started = Instant::now();
        assert_eq!(
            unsaved.wait(Duration::from_millis(200)).pending,
            1,
            "still held up by the stalled disk"
        );
        assert!(
            started.elapsed() >= Duration::from_millis(200),
            "and it waited the time it was given, not less"
        );

        drop(hold);
        let s = unsaved.wait(DURABLE_WAIT);
        assert!(s.saved(), "it lands once the disk clears: {s:?}");
        let id = e.log_records()[3].id;
        assert!(
            stored(&d)
                .into_iter()
                .find(|r| r.id == id)
                .expect("stored")
                .qsl_rcvd
                .card,
            "and it is on disk when the wait says so"
        );
        assert_eq!(unsaved.write_mirror(DURABLE_WAIT), None, "log.adi too");
        assert!(e.log_unsaved().is_empty(), "then nothing is left");
    }

    /// `log.adi` is part of what a quit waits for. A change the database already holds is
    /// still on its way to the ADIF copy beside it for as long as the mirror waits for the log
    /// to go quiet (a second, on the shipped timings), and a quit that closed then would leave
    /// the file behind the logbook for the next program that reads it.
    #[test]
    fn a_quit_waits_for_the_log_adi_copy_too() {
        let d = Dir::new("quit-mirror");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        // A mirror that waits far longer than this test takes to look (the shipped wait is a
        // second): the gap between the database and the copy is then certain, not a race a slow
        // disk could lose.
        let slow = MirrorOptions {
            debounce: Duration::from_secs(30),
            max_delay: Duration::from_secs(60),
            accepted: None,
        };
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.attach_log_store(open_with(&d.log(), no_resolve(), None, slow).expect("the store opens"));
        flush(&e);
        assert!(e.log_unsaved().is_empty(), "premise: all written");

        assert!(e.mark_qsl_card(id_at(&e, 4), true));
        let unsaved = e.log_unsaved();
        assert!(
            unsaved.wait(DURABLE_WAIT).saved(),
            "the database has the change"
        );
        assert!(
            !unsaved.is_empty(),
            "but log.adi does not yet, and a quit must wait for it"
        );
        let id = e.log_records()[4].id;
        let in_log_adi = || {
            tempo_core::logbook::Logbook::load(&d.log())
                .records()
                .iter()
                .find(|r| r.id == id)
                .is_some_and(|r| r.qsl_rcvd.card)
        };
        assert!(!in_log_adi(), "control: the copy really is behind");
        assert_eq!(
            unsaved.write_mirror(DURABLE_WAIT),
            None,
            "written, no error"
        );
        assert!(
            in_log_adi(),
            "log.adi has the change once the quit has written it"
        );
        assert!(e.log_unsaved().is_empty(), "and nothing is left");
    }

    /// A change the store REFUSES is not a change still saving: waiting will never put it on
    /// disk, and the quit has to be able to say so — how many, and why — instead of waiting out
    /// its minute and then offering to keep waiting.
    ///
    /// Queued behind a change held up by a stalled disk, so the quit sees both on their way
    /// before either resolves: the refusal happens before any write (a row with no id could
    /// never be addressed again), and on its own it would resolve before anything could look.
    #[test]
    fn a_quit_is_told_which_changes_the_store_refused_and_why() {
        let d = Dir::new("quit-refused");
        let mut opened = open_fast(&d);
        let mut log = tempo_core::logbook::Logbook::new();
        log.add(qso("W1GOOD", 1_788_000_000));

        let hold = WriteHold::take(&d.db()).expect("stall the store");
        let good = Change::appended(&log.records()[log.len() - 1..], log.marks(), |_| {
            (None, None)
        });
        opened.store.submit(good).expect("a ticket");
        let mut orphan = qso("K5ORPHAN", 1_788_000_100);
        orphan.id = None;
        let refused = Change {
            rev: log.revision() + 1,
            upsert: vec![sqlite::RowWrite::new(Arc::new(orphan))],
            ..Change::default()
        };
        opened.store.submit(refused).expect("a ticket");
        let unsaved = opened.store.unsaved();
        assert_eq!(
            unsaved.standing().pending,
            2,
            "both on their way while the disk is held"
        );

        drop(hold);
        let s = unsaved.wait(DURABLE_WAIT);
        assert_eq!(
            (s.pending, s.refused),
            (0, 1),
            "one landed, one refused: {s:?}"
        );
        assert!(
            s.reason.as_deref().is_some_and(|r| r.contains("K5ORPHAN")),
            "and the reason, in the writer's words: {s:?}"
        );
        assert!(!s.saved(), "a refusal is not saved");
        assert!(
            stored(&d).iter().any(|r| r.call == "W1GOOD"),
            "the good change is on disk"
        );
    }

    // ── a change the database refused, sent again from memory (C10b) ────────

    /// The log as the store holds it, as a `Logbook` of its own — what a test submits changes
    /// against the way the station does.
    fn log_of(opened: &mut Opened) -> tempo_core::logbook::Logbook {
        let mut log = tempo_core::logbook::Logbook::new();
        for r in std::mem::take(&mut opened.records) {
            log.add(r);
        }
        log
    }

    /// Make `op` in memory and hand what it did to the store, as the station does.
    fn change(
        store: &mut LogStore,
        log: &mut tempo_core::logbook::Logbook,
        op: tempo_core::logbook::LogOp,
    ) -> Option<Ticket> {
        let effects = log.apply(op.clone());
        let change = Change::of(&op, &effects, log, |_| (None, None));
        store.submit(change)
    }

    /// Wait (up to `secs`) until the writer has finished with `t`.
    fn resolved(t: &Ticket, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if t.is_resolved() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        t.is_resolved()
    }

    /// A change the store refuses for what it IS is held in memory and never sent again by
    /// itself — however long it waits, and even when every change is being sent again. The
    /// screen and the quit hear about it: how many, and why.
    ///
    /// The control is the same store answering "nothing held" before the refusal.
    #[test]
    fn a_change_refused_for_what_it_is_is_held_and_never_sent_again() {
        let d = Dir::new("resend-lasting");
        let mut opened = open_fast(&d);
        let log = log_of(&mut opened);
        let store = &mut opened.store;
        assert_eq!(store.save_trouble(), None, "control: nothing held yet");

        let mut orphan = qso("K5ORPHAN", 1_788_000_100);
        orphan.id = None;
        let refused = Change {
            rev: log.revision() + 1,
            upsert: vec![sqlite::RowWrite::new(Arc::new(orphan))],
            ..Change::default()
        };
        let t = store.submit(refused).expect("a ticket");
        assert!(resolved(&t, 30), "the writer gives up on it");
        let r = t.refusal().expect("refused");
        assert!(!r.retryable, "a row with no id is refused every time");

        let now = Instant::now();
        store.collect(now);
        let later = now + Duration::from_secs(24 * 3600);
        assert_eq!(
            store.resend(log.marks(), false, later),
            0,
            "never sent again by itself"
        );
        assert_eq!(
            store.resend(log.marks(), true, later),
            0,
            "not even when all are sent"
        );
        let trouble = store.save_trouble().expect("on the screen");
        assert_eq!((trouble.retrying, trouble.held), (0, 1));
        assert!(trouble.reason.contains("K5ORPHAN"), "{trouble:?}");
        let s = store.unsaved().standing();
        assert_eq!(
            (s.pending, s.retryable, s.refused),
            (0, 0, 1),
            "the quit asks about it: {s:?}"
        );
        assert!(!store.unsaved().is_empty(), "so a quit is held for it");
    }

    /// An export counts a change the store refused for what it is APART from the changes still
    /// being saved: those will land, this one never will, and the screen says which. The file is
    /// the store as it stands, without it. The control is the same export before the refusal,
    /// which lacks nothing.
    #[test]
    fn an_export_counts_a_change_refused_for_good_apart_from_the_changes_still_saving() {
        let d = Dir::new("export-held");
        let mut opened = open_fast(&d);
        let log = log_of(&mut opened);
        let store = &mut opened.store;
        let adif = || tempo_core::logbook::ExportKind::Adif {
            from: None,
            to: None,
        };
        let export = |store: &LogStore| {
            crate::logexport::export_waiting(
                &crate::logexport::Source::of_store(store),
                adif(),
                Duration::from_millis(200),
            )
            .expect("an export is written")
        };
        let before = export(store);
        assert_eq!(
            (before.saving, before.held),
            (0, 0),
            "control: nothing lacking"
        );

        let mut orphan = qso("K5ORPHAN", 1_788_000_100);
        orphan.id = None;
        let refused = Change {
            rev: log.revision() + 1,
            upsert: vec![sqlite::RowWrite::new(Arc::new(orphan))],
            ..Change::default()
        };
        let t = store.submit(refused).expect("a ticket");
        assert!(resolved(&t, 30), "the writer gives up on it");
        assert!(!t.refusal().expect("refused").retryable, "for good");

        let after = export(store);
        assert_eq!(
            (after.saving, after.held),
            (0, 1),
            "counted as held, not as still being saved"
        );
        assert!(!after.text.contains("K5ORPHAN"), "and not in the file");
        assert_eq!(after.text, before.text, "the store as it stands");
    }

    /// ★ A change the database dropped for a reason that can pass is sent again FROM MEMORY once
    /// its wait is up — and lands carrying the rows as memory holds them THEN, a later change to
    /// the same row included. Never out of order, never twice.
    ///
    /// The drop is written straight into the store's record of it: the real refusal that puts
    /// one there — another program holding the database past the writer's retry ladder, about
    /// 21 s — is `the_database_s_busy_refusal_is_sent_again_and_the_snapshot_says_so` below.
    #[test]
    fn a_dropped_change_is_sent_again_from_memory_when_its_wait_is_up() {
        use tempo_core::logbook::LogOp;
        let d = Dir::new("resend-due");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        let mut opened = open_fast(&d);
        let mut log = log_of(&mut opened);
        let store = &mut opened.store;
        let id = log.records()[3].id.expect("an id");
        let rows_before = stored(&d).len();

        // The change the database dropped: in memory, not in the store.
        let dropped = LogOp::MarkQslCard { id, received: true };
        let effects = log.apply(dropped.clone());
        let lost = Change::of(&dropped, &effects, &log, |_| (None, None));
        let t0 = Instant::now();
        store.dropped.push(Dropped {
            held: Arc::new(Held::of(&lost)),
            rev: lost.rev,
            refusal: Refusal {
                reason: "logbook database: database is locked".into(),
                retryable: true,
            },
            resends: 0,
            due: t0 + Duration::from_secs(5),
        });
        // A later change to the same row, made and saved while the first waits.
        let mut edited = QsoRecord::clone(&log.records()[3]);
        edited.comment = Some("a later edit".into());
        let t = change(
            store,
            &mut log,
            LogOp::Edit {
                id,
                rec: Box::new(edited),
            },
        )
        .expect("sent");
        store
            .writer
            .wait_durable(&t, DURABLE_WAIT)
            .expect("the later change lands");
        let trouble = store.save_trouble().expect("on the screen while it waits");
        assert_eq!((trouble.retrying, trouble.held), (1, 0));

        assert_eq!(
            store.resend(log.marks(), false, t0),
            0,
            "not before its wait is up"
        );
        // Sent while the disk is held up again: the screen keeps saying so until it lands.
        let hold = WriteHold::take(&d.db()).expect("stall the store");
        assert_eq!(
            store.resend(log.marks(), false, t0 + Duration::from_secs(5)),
            1,
            "then sent"
        );
        let trouble = store
            .save_trouble()
            .expect("still on the screen while it is on its way");
        assert_eq!((trouble.retrying, trouble.held), (1, 0));
        drop(hold);
        let s = store.unsaved().wait(DURABLE_WAIT);
        assert!(s.saved(), "it lands: {s:?}");

        let row = stored(&d)
            .into_iter()
            .find(|r| r.id == Some(id))
            .expect("stored");
        assert!(row.qsl_rcvd.card, "the dropped change is in the store");
        assert_eq!(
            row.comment.as_deref(),
            Some("a later edit"),
            "and the later change to the same row survives it: never out of order"
        );
        assert_eq!(stored(&d).len(), rows_before, "no row twice");
        assert_eq!(store.save_trouble(), None, "the screen clears");
        assert_eq!(
            store.resend(log.marks(), true, t0 + Duration::from_secs(3600)),
            0,
            "and nothing is sent again after it landed"
        );
    }

    /// ★ The real refusal, end to end: another program holds the database past the writer's
    /// retry ladder (about 21 s — this test waits it out), the writer drops the change, and the
    /// engine keeps it: the snapshot says so, the quit counts it as a change that sending again
    /// can save, and sending it again lands it once the database is free.
    ///
    /// And a read of the store follows it too: stale while the change is held — it truly is not
    /// saved — and current again once the re-send has landed it, so the folds keep their answers
    /// again rather than reading the whole log at every ask for the rest of the session. An export
    /// (the operator's pick) is written all the while — the rescue a failing disk needs — and
    /// counts the change it lacks until the re-send lands it.
    #[test]
    fn the_database_s_busy_refusal_is_sent_again_and_the_snapshot_says_so() {
        let d = Dir::new("resend-busy");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        let mut e = engine_on_store(&d);
        flush(&e);
        assert_eq!(e.snapshot().log_save_trouble, None, "control: no trouble");

        let hold = WriteHold::take(&d.db()).expect("another program holds the database");
        assert!(e.mark_qsl_card(id_at(&e, 3), true));
        let dropped = eventually_for(Duration::from_secs(60), || {
            e.log_unsaved().standing().retryable == 1
        });
        assert!(dropped, "the writer gives up after its retry ladder");
        let trouble = e.snapshot().log_save_trouble.expect("the snapshot says so");
        assert_eq!((trouble.retrying, trouble.held), (1, 0), "{trouble:?}");
        assert!(trouble.reason.contains("locked"), "{trouble:?}");
        assert_eq!(e.log_resend_due(), 0, "not sent again the moment it drops");
        let adif = || tempo_core::logbook::ExportKind::Adif {
            from: None,
            to: None,
        };
        let held = crate::logexport::export_waiting(
            &crate::logexport::Source::of(&e),
            adif(),
            Duration::from_millis(200),
        )
        .expect("an export while the change is held is written — the rescue a failing disk needs");
        assert_eq!(
            (held.saving, held.held),
            (1, 0),
            "and counts the change it lacks, as still being saved: it is sent again"
        );
        assert!(
            !held.text.contains("<QSL_RCVD:1>Y"),
            "the store as it stands, without the change"
        );
        let read = |e: &Engine| {
            e.log_rows()
                .each_record(Duration::from_millis(100), &mut |_| {
                    std::ops::ControlFlow::Continue(())
                })
                .expect("the store reads")
        };
        assert!(
            matches!(read(&e), Freshness::Stale(_)),
            "a read while the change is held is stale: it truly is not saved"
        );
        assert!(
            !e.log_unsaved().is_empty(),
            "a quit waits for it (what the close's logbook_waiting asks)"
        );

        drop(hold);
        assert_eq!(e.log_resend_all(), 1, "sent again, from memory");
        assert!(e.log_unsaved().wait(DURABLE_WAIT).saved(), "and it lands");
        flush(&e);
        assert!(
            e.log_unsaved().is_empty(),
            "and a quit has nothing left to wait for, log.adi included"
        );
        let saved = crate::logexport::export_waiting(
            &crate::logexport::Source::of(&e),
            adif(),
            DURABLE_WAIT,
        )
        .expect("an export once the re-send has landed the change");
        assert!(saved.text.contains("<QSL_RCVD:1>Y"), "carries the change");
        assert_eq!((saved.saving, saved.held), (0, 0), "and lacks nothing");
        assert_eq!(
            read(&e),
            Freshness::Current,
            "a read once the re-send has landed the change is current again"
        );
        let id = e.log_records()[3].id;
        assert!(
            stored(&d)
                .into_iter()
                .find(|r| r.id == id)
                .is_some_and(|r| r.qsl_rcvd.card),
            "the change is in the store"
        );
        assert_eq!(e.snapshot().log_save_trouble, None, "and the screen clears");
    }

    /// ⛔ A DROPPED CHANGE SURVIVES ANOTHER WINDOW'S COMMIT TO THE SAME ROW. When a second window
    /// on the same data folder commits, this window re-reads the store and keeps its own version
    /// of every row it has a change of its own for — the change on its way. A change the writer
    /// dropped is no longer on its way, and it is still this window's: if the re-read took the
    /// store's copy, memory would lose it, and the re-send (built from memory) would carry the
    /// loss to disk. So the re-read keeps those rows too, and the re-send lands them.
    #[test]
    fn another_windows_commit_does_not_erase_a_change_waiting_to_be_sent_again() {
        use tempo_core::logbook::LogOp;
        let d = Dir::new("resend-reload");
        std::fs::write(d.log(), legacy_log(6)).unwrap();
        let mut a = open_fast(&d);
        let mut b = open_fast(&d);
        let mut log_a = log_of(&mut a);
        let mut log_b = log_of(&mut b);
        let id = log_a.records()[2].id.expect("an id");

        // Window A's change to the row, dropped by its writer: in A's memory only.
        let dropped = LogOp::MarkQslCard { id, received: true };
        let effects = log_a.apply(dropped.clone());
        let lost = Change::of(&dropped, &effects, &log_a, |_| (None, None));
        a.store.dropped.push(Dropped {
            held: Arc::new(Held::of(&lost)),
            rev: lost.rev,
            refusal: Refusal {
                reason: "logbook database: database is locked".into(),
                retryable: true,
            },
            resends: 0,
            due: Instant::now() + Duration::from_secs(3600),
        });
        // Window B commits its own change to the same row.
        let mut theirs = QsoRecord::clone(&log_b.records()[2]);
        theirs.comment = Some("from window B".into());
        let t = change(
            &mut b.store,
            &mut log_b,
            LogOp::Edit {
                id,
                rec: Box::new(theirs),
            },
        )
        .expect("sent");
        b.store
            .writer
            .wait_durable(&t, DURABLE_WAIT)
            .expect("B's change lands");

        let (merged, _) = a
            .store
            .reload(log_a.records(), false)
            .expect("A re-reads the store");
        let row = merged.iter().find(|r| r.id == Some(id)).expect("the row");
        assert!(
            row.qsl_rcvd.card,
            "A keeps its own version of the row it has a dropped change for"
        );

        // And the change A kept lands when it is sent again.
        let mut log_after = tempo_core::logbook::Logbook::new();
        for r in merged {
            log_after.add(QsoRecord::clone(&r));
        }
        assert_eq!(a.store.resend(log_after.marks(), true, Instant::now()), 1);
        assert!(a.store.unsaved().wait(DURABLE_WAIT).saved());
        assert!(
            stored(&d)
                .into_iter()
                .find(|r| r.id == Some(id))
                .is_some_and(|r| r.qsl_rcvd.card),
            "A's dropped change reaches the store"
        );
    }

    /// Poll `f` until it says yes or `limit` passes.
    fn eventually_for(limit: Duration, mut f: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        f()
    }

    // ── two windows, one store ──────────────────────────────────────────────

    /// Poll `f` until it says yes or ten seconds pass. The other window's commit is seen by
    /// this one's writer within its poll interval, not instantly.
    pub(crate) fn eventually(mut f: impl FnMut() -> bool) -> bool {
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        f()
    }

    fn find(e: &Engine, call: &str) -> Option<QsoRecord> {
        e.log_records()
            .iter()
            .find(|r| r.call == call)
            .map(|r| QsoRecord::clone(r))
    }

    /// ★ Two radio windows on ONE data folder — the shipped two-instance mode, now on one store.
    ///
    /// - A contact one window logs reaches the other on its freshness poll, with no restart.
    /// - A stamp made in one window survives the other window's change to the same row: the
    ///   second window re-reads before it changes anything, as the 1.13 path re-read `log.adi`.
    /// - An edit made in one window arrives in the other AS AN EDIT, and a delete as a delete —
    ///   the two gaps a shared `log.adi` had to leave open (a duplicate, a resurrection),
    ///   closed, because rows are matched by id.
    #[test]
    fn two_windows_share_one_store() {
        let d = Dir::new("two");
        std::fs::write(d.log(), legacy_log(12)).unwrap();
        let mut a = engine_on_store(&d);
        let mut b = engine_on_store(&d);
        same_log(
            a.log_records(),
            b.log_records(),
            "both open on the same log",
        );

        // A logs; B sees it.
        a.log_qso(qso("W1AAA", 1_788_000_000));
        flush(&a);
        assert!(
            eventually(|| {
                b.sync_shared_log_if_changed();
                find(&b, "W1AAA").is_some()
            }),
            "B picks up A's contact on its freshness poll"
        );

        // A stamps a row; B then marks a card on THE SAME row. B must not write its stale copy
        // of the row over A's stamp.
        let target = QsoRecord::clone(&a.log_records()[2]);
        assert!(a.stamp_qrz_upload(
            &target,
            tempo_core::logbook::UploadOutcome::Accepted,
            1_788_000_100,
            None
        ));
        flush(&a);
        // Wait until B's writer has SEEN A's stamp commit — without B polling — so the change
        // below is the case under test: a foreign commit B has not yet folded in.
        assert!(eventually(|| b.log_store_foreign_pending()));
        assert!(b.mark_qsl_card(target.id.unwrap(), true));
        flush(&b);
        let row = stored(&d)
            .into_iter()
            .find(|r| r.id == target.id)
            .expect("stored");
        assert!(row.qsl_rcvd.card, "B's mark is stored");
        assert!(
            row.upload.qrz.is_some(),
            "and A's stamp survived B's change to the same row"
        );

        // A edits a row's CALL; B sees an edit, not a new contact beside the old one.
        let mut edited = find(&a, "K5ABC").unwrap();
        edited.call = "K5ABD".into();
        assert!(a.update_qso(edited.id.unwrap(), edited));
        // A deletes a row; B must not bring it back.
        let gone = find(&a, "K7ABC").and_then(|r| r.id).unwrap();
        assert!(a.delete_qso(gone));
        flush(&a);
        assert!(eventually(|| {
            b.sync_shared_log_if_changed();
            find(&b, "K5ABD").is_some()
        }));
        assert!(find(&b, "K5ABC").is_none(), "an edit, not a duplicate");
        assert!(find(&b, "K7ABC").is_none(), "a delete, not a resurrection");

        // And B's own next change does not resurrect or duplicate anything either.
        b.log_qso(qso("W2BBB", 1_788_000_900));
        flush(&b);
        assert!(eventually(|| {
            a.sync_shared_log_if_changed();
            find(&a, "W2BBB").is_some()
        }));
        same_log(a.log_records(), b.log_records(), "the two windows agree");
        same_log(&stored(&d), a.log_records(), "and so does the store");
    }

    /// ⛔ A POSITION HELD ACROSS ANOTHER WINDOW'S DELETE STILL NAMES ITS CONTACT. Until SPEC-2's
    /// C16 the Logbook and the Remote found a row, then changed it by position, in one hold of
    /// the engine lock — and the change first folds in whatever another window committed. A
    /// fold-in that removed the other window's deleted row would shift every row after it, and
    /// the change would land on a different contact. So the fold-in a change makes moves
    /// nothing; the deleted row lingers until the next freshness poll, which may move rows
    /// because nobody holds a position across it. Every change is addressed by id since C16, so
    /// nothing holds a position any more; this pins the in-place fold-in until it goes (§4.7).
    #[test]
    fn a_position_held_across_another_windows_delete_still_names_its_contact() {
        let d = Dir::new("positions");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        let mut a = engine_on_store(&d);
        let mut b = engine_on_store(&d);
        let target = QsoRecord::clone(&b.log_records()[5]);

        assert!(
            a.delete_qso(id_at(&a, 1)),
            "A deletes a row ABOVE B's target"
        );
        flush(&a);
        assert!(eventually(|| b.log_store_foreign_pending()));

        // B changes the row at position 5, by its id — without polling first.
        assert!(b.mark_qsl_card(target.id.unwrap(), true));
        assert_eq!(
            b.log_records()[5].id,
            target.id,
            "position 5 is still B's contact"
        );
        flush(&b);
        let marked: Vec<_> = stored(&d)
            .into_iter()
            .filter(|r| r.qsl_rcvd.card)
            .map(|r| r.id)
            .collect();
        assert_eq!(
            marked,
            vec![target.id],
            "the card went on the contact B meant, and only it"
        );

        // The poll, which holds no position, finishes the fold-in: A's delete arrives.
        assert!(b.sync_shared_log_if_changed());
        assert_eq!(b.log_records().len(), 9, "the deleted row is gone from B");
        assert!(
            b.log_records().iter().all(|r| r.call != "K1ABC"),
            "and it is the row A deleted"
        );
        // And A, polling in turn, has B's card: the two windows agree.
        assert!(eventually(|| {
            a.sync_shared_log_if_changed();
            a.log_records().iter().any(|r| r.qsl_rcvd.card)
        }));
        same_log(b.log_records(), a.log_records(), "the two windows agree");
    }

    /// ⛔ ANOTHER WINDOW'S COMMIT DOES NOT UNDO THIS ONE'S FILLS — and a window opened before
    /// them takes them in. The fills are in the store (D2-A), so a re-read of it — the freshness
    /// poll's, which may move rows, and the in-place one a change makes first — brings them
    /// with every row it takes; the other window's own contacts arrive filled, because it fills
    /// before it writes.
    #[test]
    fn another_windows_commit_keeps_this_ones_fills() {
        let d = Dir::new("refill");
        std::fs::write(d.log(), legacy_log(6)).unwrap();
        flush(&engine_on_store(&d)); // the conversion: every row unfilled
        let a = launch_with_resolvers(&d);
        // A window opened before the fills were saved.
        let early = launch_with_resolvers(&d);
        assert_eq!(
            fill(&a, 1).filled,
            6,
            "premise: the fill job filled every row"
        );
        flush(&a.lock().unwrap());
        let unfilled = |e: &Mutex<Engine>| {
            e.lock()
                .unwrap()
                .log_records()
                .iter()
                .filter(|r| r.country.is_none())
                .map(|r| r.call.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(unfilled(&a), Vec::<String>::new(), "premise: A filled them");
        assert_eq!(
            unfilled(&early).len(),
            6,
            "premise: the early window has not"
        );
        assert!(eventually(|| {
            early.lock().unwrap().sync_shared_log_if_changed();
            unfilled(&early).is_empty()
        }));

        let b = launch_with_resolvers(&d);
        b.lock().unwrap().log_qso(qso("W1AAA", 1_788_000_000));
        flush(&b.lock().unwrap());
        assert!(eventually(|| {
            let mut a = a.lock().unwrap();
            a.sync_shared_log_if_changed();
            find(&a, "W1AAA").is_some()
        }));
        assert_eq!(
            unfilled(&a),
            Vec::<String>::new(),
            "after the freshness poll's re-read, every row carries its country — the new one too"
        );

        b.lock().unwrap().log_qso(qso("W2BBB", 1_788_000_100));
        flush(&b.lock().unwrap());
        assert!(eventually(|| a.lock().unwrap().log_store_foreign_pending()));
        let first = id_at(&a.lock().unwrap(), 0);
        assert!(
            a.lock().unwrap().mark_qsl_card(first, true),
            "a change, which re-reads in place"
        );
        assert!(
            find(&a.lock().unwrap(), "W2BBB").is_some(),
            "premise: the change re-read"
        );
        assert_eq!(
            unfilled(&a),
            Vec::<String>::new(),
            "after a change's in-place re-read, likewise"
        );
        flush(&a.lock().unwrap());
        same_log(
            a.lock().unwrap().log_records(),
            &stored(&d),
            "and A shows what the store holds",
        );
    }

    /// ⛔ A LoTW STAMP DOES NOT BRING BACK A CONTACT ANOTHER WINDOW DELETED WHILE TQSL RAN.
    /// The stamp finds its contacts by id, so it holds no position and its re-read may move
    /// rows: the contact the other window deleted is gone before the stamp looks for it. The
    /// in-place re-read a change by position makes would keep it, stamp it, and so write it
    /// back into the store.
    #[test]
    fn a_lotw_stamp_does_not_bring_back_a_contact_another_window_deleted() {
        use tempo_core::logbook::UploadOutcome;
        let d = Dir::new("lotw-two");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let mut a = engine_on_store(&d);
        let mut b = engine_on_store(&d);
        let ids: Vec<_> = (0..5).map(|at| id_at(&a, at)).collect();
        let signed = a.lotw_signed(&ids);
        assert_eq!(signed.len(), 5, "premise: A hands TQSL five contacts");

        let deleted = b.log_records()[2].id;
        assert!(
            b.delete_qso(deleted.unwrap()),
            "B deletes one while A's TQSL runs"
        );
        flush(&b);
        assert!(eventually(|| a.log_store_foreign_pending()));
        let done = a.stamp_lotw_batch(&signed, UploadOutcome::Pending, 1_788_000_000, None);
        flush(&a);
        let rows = stored(&d);
        assert!(
            rows.iter().all(|r| r.id != deleted),
            "the deleted contact stays deleted"
        );
        assert!(
            rows.iter().all(|r| r.upload.lotw.is_some()),
            "and every other contact carries the stamp"
        );
        same_log(a.log_records(), &rows, "A holds what the store holds");
        assert_eq!(
            (done.stamped, done.changed, done.gone),
            (4, 0, 1),
            "four stamped; the deleted contact is counted gone"
        );
    }

    /// ★ THE DIAGNOSIS NAMES THE CONTACTS IT READ — so what is done by its report acts on the contacts
    /// it was about, even once another window has deleted a row before them (C17a; what the Awards
    /// view uploads by). The confirmation diagnostics read the store, and a report's positions are
    /// the store's AS IT READ: another window's delete of an earlier row, committed after, moves
    /// every later contact up one place. The report's ids are those of the rows it read, and name
    /// the same contacts after the delete — each still in the store with its call, unless it was
    /// the one deleted; its positions, looked up in the store after the delete, name other
    /// contacts: the control that the gap is real. (Before the cut the gap was a window's log in
    /// memory showing the deleted row; the log in memory is not read here.)
    #[test]
    fn the_diagnosis_names_the_contacts_it_read_across_another_windows_delete() {
        let d = Dir::new("diag-gap");
        std::fs::write(d.log(), legacy_log(8)).unwrap();
        let a = engine_on_store(&d);
        let mut b = engine_on_store(&d);
        let read = stored(&d);
        let report = a
            .confirmation_diagnostics_inputs()
            .diagnose_named(1_800_000_000, |_| None)
            .expect("the store reads");
        let id_of =
            |rows: &[QsoRecord], i: usize| rows.get(i).and_then(|r| r.id).map(|id| id.to_string());
        let bucket = report
            .buckets
            .iter()
            .find(|b| b.qso_indices.len() > 2)
            .expect("premise: a bucket of three contacts or more");
        let ids = bucket
            .qso_ids
            .clone()
            .expect("the desktop's report names its contacts");
        assert_eq!(
            ids,
            bucket
                .qso_indices
                .iter()
                .map(|&i| id_of(&read, i))
                .collect::<Vec<_>>(),
            "each id is the contact the diagnosis read at that position"
        );
        assert!(!report.diagnoses.is_empty(), "premise: contacts diagnosed");
        for diag in &report.diagnoses {
            let r = &read[diag.index];
            assert_eq!(diag.id, r.id.map(|id| id.to_string()), "row {}", diag.index);
            assert_eq!(diag.call.as_deref(), Some(r.call.as_str()));
        }

        // Another window deletes a contact before the last of them, and commits.
        let first = *bucket.qso_indices.iter().min().expect("positions");
        let last = *bucket.qso_indices.iter().max().expect("positions");
        assert!(last > first, "premise: the bucket spans places");
        let gone = read[first].id.expect("an id");
        assert!(b.delete_qso(gone));
        flush(&b);
        let after = stored(&d);
        assert_eq!(
            after.len(),
            read.len() - 1,
            "premise: the delete is in the store"
        );

        // The report's ids still name the contacts it read.
        for (id, &i) in ids.iter().zip(&bucket.qso_indices) {
            let named = id.as_deref().expect("every contact named");
            if named == gone.to_string() {
                assert!(
                    after.iter().all(|r| r.id != Some(gone)),
                    "the one deleted is gone, and an upload by id finds nothing to sign"
                );
                continue;
            }
            let now = after
                .iter()
                .find(|r| r.id.map(|x| x.to_string()).as_deref() == Some(named))
                .expect("a contact the report named is still in the store");
            assert_eq!(now.call, read[i].call, "the contact it read at {i}");
        }
        // The control: the same positions, looked up in the store after the delete, name other
        // contacts — which an upload by position would sign.
        let by_position: Vec<Option<String>> = bucket
            .qso_indices
            .iter()
            .map(|&i| id_of(&after, i))
            .collect();
        assert_ne!(by_position, ids, "the gap is real");
    }

    // ── a log.adi the store does not account for ────────────────────────────

    /// ★ The transition hazard, at the door. The store was converted; then a 1.13 build (the
    /// operator went back for a day) appended a contact to `log.adi`. The next open finds the
    /// file is not the store's own picture, takes the contact in, and only then lets the mirror
    /// replace the file — so the contact is in the store AND in the new `log.adi`.
    ///
    /// Control: the same open on a PRISTINE mirror takes nothing in.
    #[test]
    fn a_contact_another_build_appended_to_log_adi_is_taken_in_at_the_next_start() {
        let d = Dir::new("foreign-open");
        std::fs::write(d.log(), legacy_log(8)).unwrap();
        {
            let mut e = engine_on_store(&d);
            e.log_qso(qso("W1OLD", 1_788_000_000)); // so the mirror writes a pristine picture
            flush(&e);
        }
        assert_eq!(
            tempo_core::logbook::mirror::mirror_state(&d.log()),
            tempo_core::logbook::mirror::MirrorState::Pristine
        );
        // Control: a pristine file takes nothing in.
        {
            let opened = open_fast(&d);
            assert!(opened.foreign.is_none(), "control: nothing to take in");
        }

        // A 1.13 build appends its own contact to the file.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(d.log())
            .unwrap();
        use std::io::Write as _;
        f.write_all(
            b"<CALL:6>W9FROM<BAND:3>40m<MODE:2>CW<QSO_DATE:8>20260915<TIME_ON:6>010203<EOR>\n",
        )
        .unwrap();
        drop(f);

        let mut e = engine_on_store(&d);
        assert!(
            find(&e, "W9FROM").is_some(),
            "the appended contact is in the log"
        );
        e.log_qso(qso("W2NOW", 1_788_001_000));
        flush(&e);
        assert!(
            stored(&d).iter().any(|r| r.call == "W9FROM"),
            "and in the store"
        );
        let mirror = std::fs::read_to_string(d.log()).unwrap();
        assert!(
            mirror.contains("W9FROM") && mirror.contains("W2NOW"),
            "and the mirror that replaced the file still carries it"
        );
    }

    /// The same hazard mid-session: something appends to `log.adi` while the store owns it.
    /// The mirror refuses to replace the file (the contact would go), the freshness poll takes
    /// the file in, and the mirror resumes with the contact in it.
    #[test]
    fn a_log_adi_written_mid_session_is_taken_in_and_the_mirror_resumes() {
        let d = Dir::new("foreign-live");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let mut e = engine_on_store(&d);
        e.log_qso(qso("W1ONE", 1_788_000_000));
        flush(&e);

        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(d.log())
            .unwrap();
        use std::io::Write as _;
        f.write_all(
            b"<CALL:6>W9LIVE<BAND:3>40m<MODE:2>CW<QSO_DATE:8>20260915<TIME_ON:6>010203<EOR>\n",
        )
        .unwrap();
        drop(f);

        e.log_qso(qso("W1TWO", 1_788_000_700));
        flush(&e);
        let status = e.log_mirror_status().unwrap();
        assert!(status.foreign_write, "the mirror refused the file");
        assert!(
            std::fs::read_to_string(d.log()).unwrap().contains("W9LIVE"),
            "and left the appended contact where it was"
        );

        assert!(e.sync_shared_log_if_changed(), "the poll takes the file in");
        assert!(find(&e, "W9LIVE").is_some());
        e.log_qso(qso("W1THREE", 1_788_001_400));
        flush(&e);
        let mirror = std::fs::read_to_string(d.log()).unwrap();
        assert!(
            ["W9LIVE", "W1TWO", "W1THREE"]
                .iter()
                .all(|c| mirror.contains(c)),
            "the mirror resumed, with everything in it"
        );
        assert!(!e.log_mirror_status().unwrap().foreign_write);
        assert!(stored(&d).iter().any(|r| r.call == "W9LIVE"));
    }

    // ── the failure matrix (property 2) ─────────────────────────────────────

    /// Every path below must leave the operator's `log.adi` exactly as it was.
    fn untouched(d: &Dir, before: &[u8], why: &str) {
        assert_eq!(
            std::fs::read(d.log()).expect("log.adi still exists"),
            before,
            "{why}: log.adi must be neither deleted nor changed"
        );
    }

    /// A data folder on network storage: the store is refused for the session, nothing is
    /// converted or created, and the 1.13 path opens the log unharmed — every contact of
    /// `log.adi` in its store in memory, the store every reader reads (compared row for row, ids
    /// and all, with the file it loaded), and `log.adi` byte for byte as it was.
    #[test]
    fn a_network_folder_keeps_the_log_in_log_adi() {
        let d = Dir::new("network");
        std::fs::write(d.log(), legacy_log(6)).unwrap();
        let before = std::fs::read(d.log()).unwrap();
        let err = open_with(
            &d.log(),
            no_resolve(),
            Some("a network drive (nfs4)".into()),
            fast(),
        )
        .err()
        .expect("refused");
        assert!(matches!(err, OpenError::NetworkFolder(_)), "{err}");
        assert!(err.to_string().contains("nfs4"), "{err}");
        assert!(!d.db().exists(), "no database was created on the share");
        assert!(
            !d.0.join("log.adi.pre-sqlite").exists(),
            "nothing was converted"
        );
        untouched(&d, &before, "network");
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_log_path(d.log());
        assert!(e.log_on_file(), "the 1.13 path: log.adi is the log");
        let held = store_rows(&e);
        assert_eq!(held.len(), 6, "the fallback opens the whole log");
        same_log(
            &held,
            tempo_core::logbook::Logbook::load(&d.log()).records(),
            "its store is log.adi's contacts",
        );
        untouched(&d, &before, "the 1.13 path's open");
    }

    /// A read-only data folder: the conversion is refused before a database exists (the safety
    /// copy could not be written), and the operator's file is untouched.
    #[cfg(unix)]
    #[test]
    fn a_read_only_folder_refuses_the_conversion_and_touches_nothing() {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("readonly");
        std::fs::write(d.log(), legacy_log(6)).unwrap();
        let before = std::fs::read(d.log()).unwrap();
        let mut perms = std::fs::metadata(&d.0).unwrap().permissions();
        perms.set_mode(0o555);
        std::fs::set_permissions(&d.0, perms).unwrap();
        let result = open_with(&d.log(), no_resolve(), None, fast());
        let mut perms = std::fs::metadata(&d.0).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&d.0, perms).unwrap();
        let err = result.err().expect("refused");
        assert!(
            matches!(err, OpenError::Conversion(migrate::Error::Copy { .. })),
            "refused at the safety copy: {err}"
        );
        assert!(!d.db().exists(), "no half-made database");
        untouched(&d, &before, "read-only");
    }

    /// A database from a NEWER build at the destination — the operator went back a version. It
    /// is refused and left exactly as it is; `log.adi` is untouched.
    #[test]
    fn a_database_from_a_newer_build_is_refused_and_left_alone() {
        let d = Dir::new("newer");
        std::fs::write(d.log(), legacy_log(6)).unwrap();
        let before = std::fs::read(d.log()).unwrap();
        LogDb::open(&d.db())
            .unwrap()
            .set_meta("schema_version", 99)
            .unwrap();
        let db_before = std::fs::read(d.db()).unwrap();
        let err = open_with(&d.log(), no_resolve(), None, fast())
            .err()
            .expect("refused");
        assert!(
            matches!(
                err,
                OpenError::Conversion(migrate::Error::Db(sqlite::Error::SchemaVersion { .. }))
            ),
            "{err}"
        );
        assert_eq!(
            std::fs::read(d.db()).unwrap(),
            db_before,
            "the newer store is untouched"
        );
        untouched(&d, &before, "newer store");
    }

    /// ★ THE SCREEN SAYS SO. A refused store reaches the snapshot the UI renders from, as the
    /// diagnostic log words it and with whether the data folder is the cause — the one refusal
    /// the operator fixes in Settings. A store in use reaches it as nothing, so there is nothing
    /// to show.
    #[test]
    fn a_refused_store_reaches_the_snapshot_with_its_reason() {
        let seen = |err: &OpenError| {
            let mut e = Engine::new("K2DEF", "FN31", 0);
            e.note_log_store_problem(err);
            e.snapshot().log_store_problem
        };

        let share = Dir::new("snap-network");
        std::fs::write(share.log(), legacy_log(3)).unwrap();
        let err = open_with(
            &share.log(),
            no_resolve(),
            Some("a network drive (nfs4)".into()),
            fast(),
        )
        .err()
        .expect("refused");
        assert_eq!(
            seen(&err),
            Some(crate::dto::LogStoreProblem {
                network_folder: true,
                reason: err.to_string(),
            }),
            "a network folder, and the reason the diagnostic log gives"
        );

        let newer = Dir::new("snap-newer");
        std::fs::write(newer.log(), legacy_log(3)).unwrap();
        LogDb::open(&newer.db())
            .unwrap()
            .set_meta("schema_version", 99)
            .unwrap();
        let err = open_with(&newer.log(), no_resolve(), None, fast())
            .err()
            .expect("refused");
        let problem = seen(&err).expect("the reason reaches the snapshot");
        assert!(
            !problem.network_folder,
            "a newer build's database is not the folder's fault"
        );
        assert!(problem.reason.contains("schema version 99"), "{problem:?}");

        let fine = Dir::new("snap-fine");
        std::fs::write(fine.log(), legacy_log(3)).unwrap();
        let e = engine_on_store(&fine);
        assert_eq!(
            e.snapshot().log_store_problem,
            None,
            "a store in use says nothing"
        );
        flush(&e);
    }

    /// A database already at the destination that holds a DIFFERENT log (a folder copied from
    /// another machine): it is the store, and the `log.adi` beside it — which it does not
    /// account for — is taken into it, so neither log loses a contact.
    #[test]
    fn a_store_already_at_the_destination_takes_the_log_beside_it_in() {
        let other = Dir::new("other-machine");
        std::fs::write(other.log(), legacy_log(3)).unwrap();
        drop(open_fast(&other)); // converts the OTHER log into its store
        let d = Dir::new("pre-existing");
        std::fs::copy(other.db(), d.db()).unwrap();
        let mut ours = adif_header();
        ours.push_str(
            "<CALL:5>G4OUR<BAND:3>20m<MODE:3>SSB<QSO_DATE:8>20260101<TIME_ON:6>101010<EOR>\n",
        );
        std::fs::write(d.log(), &ours).unwrap();

        let e = engine_on_store(&d);
        let calls: Vec<&str> = e.log_records().iter().map(|r| r.call.as_str()).collect();
        assert!(
            calls.contains(&"G4OUR"),
            "this folder's contact is in: {calls:?}"
        );
        assert!(
            calls.contains(&"K0ABC"),
            "and so are the store's: {calls:?}"
        );
        // `log.adi` stays as it was only until the mirror lane's first write, which the take-in
        // queues — asserting it unchanged before that write raced the lane. Settle it: the file
        // is then the store's mirror, and it carries this folder's contact and the store's.
        flush(&e);
        assert!(stored(&d).iter().any(|r| r.call == "G4OUR"));
        assert_eq!(
            mirror::mirror_state(&d.log()),
            MirrorState::Pristine,
            "pre-existing store: log.adi is the store's mirror once the take-in lands"
        );
        let mirrored = tempo_core::logbook::Logbook::load(&d.log());
        let mirrored: Vec<&str> = mirrored.records().iter().map(|r| r.call.as_str()).collect();
        assert!(
            mirrored.contains(&"G4OUR") && mirrored.contains(&"K0ABC"),
            "pre-existing store: the mirror carries both logs: {mirrored:?}"
        );
    }

    /// A conversion a crash cut short resumes at the next open and completes — nothing
    /// duplicated, nothing dropped — and the operator's file is untouched throughout.
    #[test]
    fn a_conversion_cut_short_completes_at_the_next_open() {
        let d = Dir::new("resume");
        std::fs::write(d.log(), legacy_log(700)).unwrap();
        let before = std::fs::read(d.log()).unwrap();
        let source = tempo_core::logbook::Logbook::load(&d.log());
        {
            // What a crash leaves: the copy taken, the source length recorded, two chunks in,
            // and no "done".
            migrate::take_pre_sqlite_copy(&d.log(), &before).unwrap();
            let mut db = LogDb::open(&d.db()).unwrap();
            db.set_meta("migration_source_len", before.len() as i64)
                .unwrap();
            db.insert_all(
                source.records()[..512]
                    .iter()
                    .map(|r| (&**r, Resolved::default())),
            )
            .unwrap();
        }
        let opened = open_fast(&d);
        assert_eq!(
            opened.outcome,
            migrate::Outcome::Converted {
                written: 188,
                resumed_at: 512,
                total: 700
            }
        );
        // The conversion itself — resumed or not — never writes the operator's file.
        untouched(&d, &before, "resume");
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.attach_log_store(opened);
        same_log(e.log_records(), source.records(), "every contact, once");
        // Attached, the mirror lane is told, and writes after its debounce: asserting
        // `log.adi` unchanged HERE raced the lane. Settle it, then hold the file to what the
        // completed conversion makes it, with the operator's own copy kept beside it.
        flush(&e);
        assert_eq!(
            mirror::mirror_state(&d.log()),
            MirrorState::Pristine,
            "resume: log.adi is the store's mirror once the conversion completes"
        );
        assert_eq!(
            std::fs::read(d.0.join("log.adi.pre-sqlite")).unwrap(),
            before,
            "resume: the operator's file is kept beside it, byte for byte"
        );
    }

    /// The start-up screen hears a first launch's conversion through the store's open: not
    /// counted yet, then counted up to every contact. A launch whose store is already converted
    /// hears nothing — the screen has nothing to show it.
    #[test]
    fn a_launch_passes_the_conversions_progress_through() {
        let d = Dir::new("progress");
        std::fs::write(d.log(), legacy_log(40)).unwrap();
        let mut seen = Vec::new();
        let opened = open_reporting_with(&d.log(), no_resolve(), None, fast(), None, &mut |p| {
            seen.push(p)
        })
        .expect("the store opens");
        assert_eq!(
            opened.outcome,
            migrate::Outcome::Converted {
                written: 40,
                resumed_at: 0,
                total: 40
            }
        );
        assert_eq!(
            seen.first(),
            Some(&migrate::Progress { done: 0, total: 0 }),
            "{seen:?}"
        );
        assert_eq!(
            seen.last(),
            Some(&migrate::Progress {
                done: 40,
                total: 40
            }),
            "{seen:?}"
        );
        drop(opened);

        seen.clear();
        let again = open_reporting_with(&d.log(), no_resolve(), None, fast(), None, &mut |p| {
            seen.push(p)
        })
        .expect("the store opens again");
        assert_eq!(again.outcome, migrate::Outcome::AlreadyDone);
        assert!(seen.is_empty(), "{seen:?}");
    }

    // ── the FT duplicate guard (hard gate) ──────────────────────────────────

    /// ⛔ THE GUARD ANSWERS FROM THE HOT INDEX AND NEVER FROM THE STORE. With the store's writer
    /// held back (its write lock taken elsewhere), a contact is logged: the station's hot index
    /// has it at once, the store does not — shown by reading the store through a connection of
    /// the test's own (the control). Logged again, it is refused as a duplicate at once. A guard
    /// that consulted the store would have found nothing there and logged it twice; one that
    /// waited for the store would not have answered while the lock was held. Once the store
    /// catches up it holds the contact once: the refusal wrote nothing.
    #[test]
    fn the_duplicate_guard_answers_from_the_hot_index_while_the_store_lags() {
        let d = Dir::new("dedup-guard");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let mut e = engine_on_store(&d);
        let hold = WriteHold::take(&d.db()).expect("hold the write lock");
        let rec = qso("W1DUP", 1_788_000_000);
        assert!(!matches!(
            e.log_qso_for_sync(rec.clone()),
            crate::engine::LogWriteOutcome::Duplicate
        ));
        assert!(
            e.station().hot().worked_call("W1DUP"),
            "the hot index has the contact at once"
        );
        assert!(
            stored(&d).iter().all(|r| r.call != "W1DUP"),
            "control: the store does NOT hold the contact yet"
        );
        let started = Instant::now();
        let again = e.log_qso_for_sync(rec);
        assert!(
            matches!(again, crate::engine::LogWriteOutcome::Duplicate),
            "refused from the hot index, though the store has never seen the first"
        );
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "and at once"
        );
        drop(hold);
        flush(&e);
        assert_eq!(
            stored(&d).iter().filter(|r| r.call == "W1DUP").count(),
            1,
            "once the store catches up it holds the contact once"
        );
    }

    // ── D1-A: the 1.13 path, its log in a store in memory ───────────────────

    /// A station on the 1.13 path over a `log.adi` of `n` contacts.
    fn on_log_file(d: &Dir, n: usize) -> crate::station::StationCore {
        std::fs::write(d.log(), legacy_log(n)).unwrap();
        let mut sc = crate::station::StationCore::new();
        sc.set_log_path(d.log());
        sc
    }

    fn lane_of(sc: &crate::station::StationCore) -> Arc<LogFileWriter> {
        Arc::clone(
            sc.store
                .as_ref()
                .and_then(LogStore::lane)
                .expect("the 1.13 path's lane"),
        )
    }

    /// Hold the lane's rewrites for a moment, as a slow network drive does: the folder refuses
    /// the temporary file a rewrite writes first, and nothing else — `log.adi` itself can still
    /// be appended to, as another computer appends to it. `false` lets them through again.
    #[cfg(unix)]
    fn folder_refuses_new_files(d: &Dir, refuses: bool) {
        use std::os::unix::fs::PermissionsExt;
        let mode = if refuses { 0o500 } else { 0o700 };
        std::fs::set_permissions(&d.0, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    /// The calls `log.adi` holds, in file order.
    fn calls_in_file(d: &Dir) -> Vec<String> {
        Logbook::load(&d.log())
            .records()
            .iter()
            .map(|r| r.call.clone())
            .collect()
    }

    /// The calls the station's log holds, in log order.
    fn calls_held(sc: &crate::station::StationCore) -> Vec<String> {
        sc.logbook
            .records()
            .iter()
            .map(|r| r.call.clone())
            .collect()
    }

    /// ★ A CONTACT DELETED HERE STAYS DELETED WHILE THE FILE CATCHES UP. The lane writes
    /// `log.adi` after the change, so for a moment the file still holds a contact the log has
    /// deleted. If another machine appends in that moment, the lane holds its rewrite and the
    /// station takes the file in: the other machine's contact comes in, and the deleted one —
    /// in the file only because the file lags — must not. The contact here is one the launch
    /// loaded, which the lane has never written itself.
    #[cfg(unix)]
    #[test]
    fn a_delete_the_file_has_not_caught_up_with_is_not_taken_back_in() {
        let d = Dir::new("lagging-delete");
        let mut sc = on_log_file(&d, 3);
        let gone = QsoRecord::clone(&sc.logbook.records()[0]);
        let lane = lane_of(&sc);
        // The lane cannot write the delete yet, as on a slow drive.
        folder_refuses_new_files(&d, true);
        assert!(sc.delete_qso(gone.id.unwrap()));
        // Another machine appends meanwhile.
        Logbook::append(&d.log(), &qso("W7OTHER", 1_788_400_000)).unwrap();
        folder_refuses_new_files(&d, false);
        assert!(
            eventually(|| lane.status().foreign_write),
            "premise: the lane holds its rewrite for the other machine's file: {:?}",
            lane.status()
        );
        assert!(sc.take_in_log_file_if_changed(), "the file is taken in");
        let st = lane.flush(DURABLE_WAIT);
        assert!(!st.pending(), "and then written: {st:?}");
        let held = calls_held(&sc);
        assert!(
            !held.contains(&gone.call),
            "the contact deleted here stays deleted: {held:?}"
        );
        assert!(
            held.contains(&"W7OTHER".to_string()),
            "and the other machine's contact is taken in: {held:?}"
        );
        let file = calls_in_file(&d);
        assert!(
            !file.contains(&gone.call) && file.contains(&"W7OTHER".to_string()),
            "the file agrees: {file:?}"
        );
    }

    /// ★ THE LANE NEVER REWRITES A FILE ANOTHER MACHINE HAS CHANGED — the data-loss fix D1-A adds
    /// to 1.13's rules. 1.13 read the shared file, changed the log and rewrote the file in one
    /// hold of the engine lock; the lane rewrites it a moment after the change. A contact another
    /// machine appends in that moment is in no picture this station has: a rewrite then would
    /// delete it. The lane finds the file changed, holds the rewrite, and writes once the station
    /// has taken the file in — the other machine's contact kept, in the log and in the file.
    #[cfg(unix)]
    #[test]
    fn a_contact_another_machine_appends_while_a_change_is_on_its_way_is_kept() {
        let d = Dir::new("foreign-mid-change");
        let mut sc = on_log_file(&d, 3);
        let lane = lane_of(&sc);
        folder_refuses_new_files(&d, true);
        let mut edited = QsoRecord::clone(&sc.logbook.records()[1]);
        edited.name = Some("Edited here".into());
        assert!(sc.update_qso(edited.id.unwrap(), edited));
        Logbook::append(&d.log(), &qso("W7OTHER", 1_788_400_000)).unwrap();
        folder_refuses_new_files(&d, false);
        assert!(
            eventually(|| lane.status().foreign_write),
            "the lane holds its rewrite: {:?}",
            lane.status()
        );
        assert!(
            calls_in_file(&d).contains(&"W7OTHER".to_string()),
            "and the other machine's contact is still in the file"
        );
        assert!(
            sc.sync_shared_log_if_changed(),
            "the freshness poll takes it in"
        );
        let st = lane.flush(DURABLE_WAIT);
        assert!(
            !st.pending() && !st.foreign_write,
            "then the lane writes: {st:?}"
        );
        let held = calls_held(&sc);
        assert!(held.contains(&"W7OTHER".to_string()), "{held:?}");
        let on_disk = Logbook::load(&d.log());
        let calls: Vec<&str> = on_disk.records().iter().map(|r| &*r.call).collect();
        assert!(calls.contains(&"W7OTHER"), "kept in the file: {calls:?}");
        assert_eq!(calls.len(), 4, "once: {calls:?}");
        assert_eq!(
            on_disk.records()[1].name.as_deref(),
            Some("Edited here"),
            "and the change made here is in it"
        );
    }

    /// ★ A QUIT ON THE 1.13 PATH WAITS FOR `log.adi` ITSELF — the log's home there; the store is
    /// memory. A change the lane has not written is still on its way, whatever the store holds,
    /// and the quit says why while the lane is held; once the file holds it, the quit is done. An
    /// export, which reads the store, counts it as there already.
    #[cfg(unix)]
    #[test]
    fn a_quit_on_the_1_13_path_waits_for_log_adi() {
        let d = Dir::new("quit-1-13");
        let mut sc = on_log_file(&d, 3);
        let lane = lane_of(&sc);
        folder_refuses_new_files(&d, true);
        let first = sc.logbook.records()[0].id.unwrap();
        assert!(sc.mark_qsl_card(first, true));
        Logbook::append(&d.log(), &qso("W7OTHER", 1_788_400_000)).unwrap();
        folder_refuses_new_files(&d, false);
        assert!(eventually(|| lane.status().foreign_write), "premise: held");
        let unsaved = sc.store.as_ref().unwrap().unsaved();
        assert!(!unsaved.is_empty(), "the quit has something to save");
        let s = unsaved.wait(Duration::from_millis(300));
        assert_eq!(s.pending, 1, "the change is still on its way: {s:?}");
        assert!(
            s.retry_reason
                .as_deref()
                .is_some_and(|r| r.contains("another program or computer")),
            "and the quit says why: {s:?}"
        );
        assert!(
            unsaved.wait_stored(Duration::from_secs(10)).saved(),
            "an export counts it: the store has it"
        );
        // What the quit runs before it waits: the file taken in, so the lane can write.
        assert!(sc.take_in_log_file_if_changed());
        let s = unsaved.wait(DURABLE_WAIT);
        assert!(s.saved(), "{s:?}");
        let on_disk = Logbook::load(&d.log());
        assert!(
            on_disk.records()[0].qsl_rcvd.card,
            "the change is in log.adi"
        );
        assert_eq!(on_disk.len(), 4, "with the other machine's contact");
    }

    /// An engine on the 1.13 path, its `log.adi` in `d` holding `n` contacts, every one of them
    /// in its store in memory.
    fn engine_on_log_file(d: &Dir, n: usize) -> Engine {
        std::fs::write(d.log(), legacy_log(n)).unwrap();
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_log_path(d.log());
        assert!(e.log_on_file(), "premise: the 1.13 path");
        e.flush_log_store(DURABLE_WAIT).expect("settled");
        e
    }

    /// A read of `e`'s log held open on a thread of its own, as a long pass over it holds its
    /// store, until the sender sends or is dropped. The thread answers the read.
    fn read_held_open(
        e: &Engine,
    ) -> (
        std::sync::mpsc::Sender<()>,
        std::thread::JoinHandle<Result<Freshness, sqlite::Error>>,
    ) {
        let rows = e.log_rows();
        let (open, opened) = std::sync::mpsc::channel();
        let (release, released) = std::sync::mpsc::channel::<()>();
        let reader = std::thread::spawn(move || {
            let mut first = true;
            rows.each_record(Duration::ZERO, &mut |_| {
                if std::mem::take(&mut first) {
                    let _ = open.send(());
                    let _ = released.recv();
                }
                std::ops::ControlFlow::Continue(())
            })
        });
        opened.recv().expect("the read is open");
        (release, reader)
    }

    /// ★ A CONTACT LOGGED WHILE AN EDIT WAITS FOR A READ IS IN `log.adi` AT ONCE (the 1.13 path,
    /// SPEC-2 v3 C19 D1-A). On the store in memory an edit's commit waits for a read open on
    /// it, and the edit's rewrite of `log.adi` waits with it. A contact logged meanwhile — the FT
    /// auto-log's append — goes into the file at once, as 1.13 appended it: a launch from the
    /// file as it then stands, which is what a crash leaves, has it. Once the read ends, the file
    /// holds both, the contact once.
    #[test]
    fn a_contact_logged_while_an_edit_waits_for_a_read_is_in_log_adi_at_once() {
        let d = Dir::new("logged-ahead");
        let mut e = engine_on_log_file(&d, 10);
        let edited = e.log_records()[3].call.clone();
        let (release, reader) = read_held_open(&e);
        assert!(e.mark_qsl_card(id_at(&e, 3), true), "the edit is made");
        e.log_qso(qso("W9LOGGED", 1_788_500_000));
        let ahead = eventually_for(Duration::from_millis(1_500), || {
            calls_in_file(&d).contains(&"W9LOGGED".to_string())
        });
        let crash = Logbook::load(&d.log());
        drop(release);
        reader.join().expect("the read ran").expect("and read");
        assert!(
            ahead,
            "the logged contact is in log.adi while the edit waits: {:?}",
            calls_in_file(&d)
        );
        assert!(
            !crash
                .records()
                .iter()
                .any(|r| r.call == edited && r.qsl_rcvd.card),
            "premise: the edit waited for the read"
        );
        let s = e.log_unsaved().wait(DURABLE_WAIT);
        assert!(s.saved(), "{s:?}");
        let on_disk = Logbook::load(&d.log());
        assert!(
            on_disk
                .records()
                .iter()
                .any(|r| r.call == edited && r.qsl_rcvd.card),
            "then the edit is in it"
        );
        assert_eq!(
            calls_in_file(&d)
                .iter()
                .filter(|c| *c == "W9LOGGED")
                .count(),
            1,
            "and the contact, once"
        );
    }

    /// ★ …AND WHEN THE WRITER GIVES THE EDIT UP (the operator's "never silently lose a contact",
    /// on the 1.13 path). A read held open past the writer's four tries (about 21 s) while an
    /// edit and then a logged contact are made: the contact is in `log.adi` at once, and a launch
    /// from the file while the edit is held has it. The writer gives the edit up and the screen
    /// says so; the edit is kept, sent again once its wait is up (the snapshot poll's re-send)
    /// and saved, and then the file holds both, the contact once.
    #[test]
    fn a_contact_logged_behind_an_edit_the_writer_gives_up_is_in_log_adi_at_once() {
        let d = Dir::new("logged-ahead-giveup");
        let mut e = engine_on_log_file(&d, 10);
        let edited = e.log_records()[3].call.clone();
        let (release, reader) = read_held_open(&e);
        assert!(e.mark_qsl_card(id_at(&e, 3), true), "the edit is made");
        e.log_qso(qso("W9LOGGED", 1_788_500_000));
        assert!(
            eventually_for(Duration::from_millis(1_500), || {
                calls_in_file(&d).contains(&"W9LOGGED".to_string())
            }),
            "the logged contact is in log.adi at once"
        );
        let gave_up = eventually_for(Duration::from_secs(60), || {
            e.snapshot().log_save_trouble.is_some()
        });
        let launch = Logbook::load(&d.log());
        drop(release);
        reader.join().expect("the read ran").expect("and read");
        assert!(
            gave_up,
            "premise: the writer gave the edit up, and the screen says so"
        );
        assert!(
            launch.records().iter().any(|r| r.call == "W9LOGGED"),
            "a launch from log.adi while the edit is held has the logged contact"
        );
        assert!(
            !launch
                .records()
                .iter()
                .any(|r| r.call == edited && r.qsl_rcvd.card),
            "premise: the edit is held"
        );
        assert!(
            eventually_for(Duration::from_secs(30), || e.log_resend_due() > 0),
            "the edit is sent again once its wait is up"
        );
        let s = e.log_unsaved().wait(DURABLE_WAIT);
        assert!(s.saved(), "{s:?}");
        e.flush_log_store(DURABLE_WAIT).expect("written");
        assert_eq!(e.snapshot().log_save_trouble, None, "the screen clears");
        let on_disk = Logbook::load(&d.log());
        assert!(
            on_disk
                .records()
                .iter()
                .any(|r| r.call == edited && r.qsl_rcvd.card),
            "then the file holds the edit"
        );
        assert_eq!(
            calls_in_file(&d)
                .iter()
                .filter(|c| *c == "W9LOGGED")
                .count(),
            1,
            "and the contact, once"
        );
    }

    /// ★ A SECOND EDIT DURING A LONG READ WAITS FOR IT, as a 1.13 edit never failed because a
    /// read was running (the 1.13 path). A read held 6 s on the store in memory — past the 5 s a
    /// connection waited for a lock — and two edits made while it runs: the first is made at
    /// once, and the writer waits for the read to commit it; the second's plan waits behind that
    /// writer, and is made once the writer lets its lock go — at its first retry, about 5 s in,
    /// or when the read ends — never the whole minute such a read may wait for the lock. Both are
    /// saved then, with nothing on the screen.
    #[test]
    fn a_second_edit_during_a_long_read_waits_for_it_on_the_1_13_path() {
        let d = Dir::new("second-edit");
        let mut e = engine_on_log_file(&d, 10);
        let (first, second) = (id_at(&e, 3), id_at(&e, 5));
        let hold = Duration::from_secs(6);
        let (release, reader) = read_held_open(&e);
        let started = Instant::now();
        let timer = std::thread::spawn(move || {
            std::thread::sleep(hold);
            let _ = release.send(());
        });
        assert!(
            e.mark_qsl_card(first, true),
            "the first edit is made at once"
        );
        // Long enough for the writer to have taken its lock for the first, and to wait for the
        // read holding it.
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            e.mark_qsl_card(second, true),
            "the second edit waits for the read, and is made"
        );
        let waited = started.elapsed();
        assert!(
            waited < hold + Duration::from_secs(2),
            "no longer than the read: {waited:?}"
        );
        timer.join().expect("the timer ran");
        reader.join().expect("the read ran").expect("and read");
        let s = e.log_unsaved().wait(DURABLE_WAIT);
        assert!(s.saved(), "{s:?}");
        assert_eq!(e.snapshot().log_save_trouble, None, "nothing on the screen");
        let carded: Vec<_> = Logbook::load(&d.log())
            .records()
            .iter()
            .filter(|r| r.qsl_rcvd.card)
            .map(|r| r.call.clone())
            .collect();
        assert_eq!(carded, ["K3ABC", "K5ABC"], "both are in log.adi");
    }

    /// ★ THE NAS TRADES ARE 1.13'S (the operator's D1 choice, pinned). Two machines on one
    /// `log.adi`: an edit made on the other machine arrives here as a second contact beside the
    /// one it edited, and a contact deleted there comes back with this machine's next rewrite —
    /// on the 1.13 path exactly as with no store at all, 1.13's own code. A file with no
    /// tombstones, which other loggers also read, cannot say "deleted" or "was this row".
    #[test]
    fn the_nas_trades_are_1_13_s_on_the_1_13_path() {
        use tempo_core::logbook::LogOp;
        for last_resort in [false, true] {
            let how = if last_resort {
                "no store"
            } else {
                "the 1.13 path"
            };
            let d = Dir::new(&format!("nas-trades-{last_resort}"));
            let mut sc = on_log_file(&d, 4);
            if last_resort {
                sc.store = None;
            }
            let written = |sc: &crate::station::StationCore| {
                if let Some(store) = &sc.store {
                    store.flush(DURABLE_WAIT).expect("written");
                }
            };
            let theirs = sc.logbook.records().to_vec();

            // The other machine corrects a call, and saves.
            let mut other = Logbook::load(&d.log());
            let mut fixed = QsoRecord::clone(&other.records()[1]);
            fixed.call = "K1FIX".into();
            let _ = other.apply(LogOp::Edit {
                id: fixed.id.unwrap(),
                rec: Box::new(fixed),
            });
            other.save(&d.log()).unwrap();
            assert!(
                sc.sync_shared_log_if_changed(),
                "{how}: the file is taken in"
            );
            let held = calls_held(&sc);
            assert!(
                held.contains(&theirs[1].call) && held.contains(&"K1FIX".to_string()),
                "{how}: the edit arrives beside the contact it edited: {held:?}"
            );
            assert_eq!(held.len(), 5, "{how}: {held:?}");

            // The other machine deletes a contact, and saves.
            let mut other = Logbook::load(&d.log());
            let gone = QsoRecord::clone(&other.records()[2]);
            let _ = other.apply(LogOp::Delete(gone.id.unwrap()));
            other.save(&d.log()).unwrap();
            assert!(
                !calls_in_file(&d).contains(&gone.call),
                "premise: gone from the file"
            );
            assert!(
                sc.sync_shared_log_if_changed(),
                "{how}: the file is taken in"
            );
            assert!(
                calls_held(&sc).contains(&gone.call),
                "{how}: this machine still holds it"
            );
            let first = sc.logbook.records()[0].id.unwrap();
            assert!(sc.mark_qsl_card(first, true));
            written(&sc);
            assert!(
                calls_in_file(&d).contains(&gone.call),
                "{how}: and this machine's next rewrite puts it back"
            );
        }
    }

    /// A `log.adi` with each minted id's nonce blanked (`RecordId::Minted`, `posid:nonce:seq`):
    /// two sessions mint under nonces of their own, the one place two files of the same log may
    /// differ, and not a difference in what they hold. Same length; the bytes are compared.
    fn nonces_aside(file: &[u8]) -> String {
        let text = String::from_utf8_lossy(file);
        let tag = "<APP_NEXUS_ID:";
        let mut out = String::with_capacity(text.len());
        let mut rest = &*text;
        while let Some(at) = rest.find(tag) {
            let (head, tail) = rest.split_at(at);
            out.push_str(head);
            let close = tail.find('>').map_or(tail.len(), |i| i + 1);
            out.push_str(&tail[..close]);
            let value = &tail.as_bytes()[close..];
            if value.len() > 26 && value[8] == b':' && value[25] == b':' {
                out.push_str(&tail[close..close + 9]);
                out.push_str("NONCE-----------");
                rest = &tail[close + 25..];
            } else {
                rest = &tail[close..];
            }
        }
        out.push_str(rest);
        out
    }

    /// ★ THE 1.13 PATH WRITES `log.adi` AS 1.13 WROTE IT (SPEC-2 v3 C19, D1-A). Two sessions on
    /// one starting `log.adi`: one on the 1.13 path, its log in a store in memory and the file
    /// kept by the lane; one with its store taken away, the file written by 1.13's own code, the
    /// last resort that still has it. Each makes the same 40 random changes of every kind the app
    /// makes, 8 seeds. After every change the two files are the same bytes, and each change was
    /// written the same way — appended to the file, or the file replaced (a hard link to the
    /// file before the change tells: an append grows the linked file, a rewrite leaves it
    /// behind). And after every run the store every reader of the log reads holds exactly the log.
    ///
    /// No resolvers: what a change FILLS is the store's rule on both of its homes (SPEC-2 v3
    /// D2-A — an import fills its own rows, and 1.13's code also fills older ones after it),
    /// held to the store path by `the_store_path_answers_exactly_as_the_adif_path`. This test
    /// holds the WRITING to 1.13's.
    #[test]
    fn the_1_13_path_writes_log_adi_as_1_13_wrote_it() {
        fn start(d: &Dir) -> Mutex<Engine> {
            std::fs::write(d.log(), log_to_fill(12)).unwrap();
            let mut e = Engine::new("K2DEF", "FN31", 0);
            e.set_log_path(d.log());
            flush(&e);
            Mutex::new(e)
        }
        // A hard link to the file as it stands: it follows an append, and not a rewrite.
        fn pin(d: &Dir) -> PathBuf {
            let at = d.0.join("before-the-change");
            let _ = std::fs::remove_file(&at);
            std::fs::hard_link(d.log(), &at).unwrap();
            at
        }
        let same_file = |d: &Dir, pinned: &Path| {
            std::fs::read(pinned).unwrap() == std::fs::read(d.log()).unwrap()
        };
        let (mut steps, mut rewrites, mut in_place) = (0, 0, 0);
        for seed in 1..=8u64 {
            let (a, b) = (
                Dir::new(&format!("d1a-lane-{seed}")),
                Dir::new(&format!("d1a-113-{seed}")),
            );
            let lane = start(&a);
            let old = start(&b);
            old.lock().unwrap().without_log_store();
            assert!(lane.lock().unwrap().log_on_file(), "premise: the 1.13 path");
            assert!(!old.lock().unwrap().log_store_open(), "premise: no store");
            let key = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
            let (mut ga, mut gb) = (Gen(key), Gen(key));
            for step in 0..40 {
                let (pa, pb) = (pin(&a), pin(&b));
                random_change(&lane, &mut ga, step);
                random_change(&old, &mut gb, step);
                flush(&lane.lock().unwrap());
                let (fa, fb) = (
                    nonces_aside(&std::fs::read(a.log()).unwrap()),
                    nonces_aside(&std::fs::read(b.log()).unwrap()),
                );
                assert!(
                    fa == fb,
                    "seed {seed}, step {step}: log.adi differs from 1.13's\n--- 1.13 path\n{fa}\n--- 1.13\n{fb}"
                );
                let (kept_a, kept_b) = (same_file(&a, &pa), same_file(&b, &pb));
                assert_eq!(
                    kept_a, kept_b,
                    "seed {seed}, step {step}: written the same way (true: appended or untouched)"
                );
                if kept_b {
                    in_place += 1;
                } else {
                    rewrites += 1;
                }
                steps += 1;
            }
            let eng = lane.lock().unwrap();
            let mut stored = Vec::new();
            let fresh = eng
                .log_rows()
                .each_record(DURABLE_WAIT, &mut |r| {
                    stored.push(r.clone());
                    std::ops::ControlFlow::Continue(())
                })
                .expect("the store reads");
            assert_eq!(fresh, Freshness::Current, "seed {seed}");
            same_log(
                &stored,
                eng.log_records(),
                &format!("seed {seed}: the store is the log"),
            );
        }
        assert_eq!(steps, 8 * 40);
        assert!(
            rewrites > 0 && in_place > 0,
            "control: the runs both rewrote and appended ({rewrites} rewrites, {in_place} in place)"
        );
    }

    /// ★ THE FT AUTO-LOG ON THE 1.13 PATH LOGS WHAT 1.13 LOGGED (the FT gate's parity, SPEC-2 v3
    /// C19 D1-A). The funnel every logged contact passes through — the sequencer's `log_qso` and
    /// the synced `log_qso_for_sync` — driven with repeats inside and outside the duplicate
    /// window, in lockstep on two sessions over the same `log.adi`: one on the 1.13 path (its log
    /// in a store in memory, the file kept by the lane) and one with no store at all, the file
    /// written by 1.13's own code. At every step: the same answer to the caller (refused as a
    /// duplicate, or logged with one receipt); the contact in the log the moment the call
    /// returns, as the same record under the next id the station mints; once the receipt is
    /// redeemed, the contact in `log.adi`; and the two files the same bytes, written the same way.
    #[test]
    fn the_ft_auto_log_on_the_1_13_path_logs_what_1_13_logged() {
        use crate::engine::LogWriteOutcome;
        const CALLS: [&str; 3] = ["W1AW", "JA1AA", "DL1AB"];
        const BANDS: [(&str, f64); 2] = [("20m", 14.074), ("40m", 7.074)];
        const MODES: [&str; 2] = ["FT8", "FT4"];
        let (mut logged, mut refused) = (0, 0);
        for seed in 0..6u64 {
            let (a, b) = (
                Dir::new(&format!("ft113-lane-{seed}")),
                Dir::new(&format!("ft113-old-{seed}")),
            );
            let mut lane = Engine::new("K2DEF", "FN31", 0);
            lane.set_log_path(a.log());
            let mut old = Engine::new("K2DEF", "FN31", 0);
            old.set_log_path(b.log());
            old.without_log_store();
            assert!(lane.log_on_file() && !old.log_store_open(), "premise");
            let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
            let mut when = 1_788_000_000u64;
            let mut last: Option<QsoRecord> = None;
            for step in 0..40 {
                let rec = match &last {
                    // The same contact handed in again moments later: the sequencer's repeat.
                    Some(prev) if g.below(4) == 0 => {
                        let mut again = prev.clone();
                        again.id = None;
                        again.when_unix += 15;
                        again
                    }
                    _ => {
                        when += [30, 90, 200, 400][g.below(4)];
                        let mut r = qso(CALLS[g.below(CALLS.len())], when);
                        let (band, freq) = BANDS[g.below(BANDS.len())];
                        r.band = band.into();
                        r.freq_mhz = freq;
                        r.mode = MODES[g.below(MODES.len())].into();
                        r.station_callsign = Some("K2DEF".into());
                        r
                    }
                };
                last = Some(rec.clone());
                let at = format!(
                    "seed {seed} step {step}: {} {} {}",
                    rec.call, rec.band, rec.mode
                );
                let synced = step % 2 == 1;
                let (held_a, held_b) = (lane.log_records().len(), old.log_records().len());
                let outcomes = [&mut lane, &mut old].map(|e| {
                    if synced {
                        Some(e.log_qso_for_sync(rec.clone()))
                    } else {
                        e.log_qso(rec.clone());
                        None
                    }
                });
                let [oa, ob] = outcomes;
                let refused_here = lane.log_records().len() == held_a;
                assert_eq!(
                    refused_here,
                    old.log_records().len() == held_b,
                    "{at}: refused by both, or logged by both"
                );
                match (oa, ob) {
                    (Some(LogWriteOutcome::Duplicate), Some(LogWriteOutcome::Duplicate)) => {}
                    (
                        Some(LogWriteOutcome::PendingSync(ra)),
                        Some(LogWriteOutcome::PendingSync(rb)),
                    ) => {
                        assert_eq!((ra.len(), rb.len()), (1, 1), "{at}: one receipt each");
                        for r in ra.into_iter().chain(rb) {
                            r.sync().unwrap_or_else(|e| panic!("{at}: redeemed: {e}"));
                        }
                        // Redeemed: the contact is in log.adi, before any flush.
                        for (d, what) in [(&a, "the 1.13 path"), (&b, "1.13")] {
                            let last = Logbook::load(&d.log()).records().last().cloned();
                            assert_eq!(
                                last.map(|r| (r.call.clone(), r.when_unix)),
                                Some((rec.call.clone(), rec.when_unix)),
                                "{at}: {what}: the redeemed contact is in log.adi"
                            );
                        }
                    }
                    (None, None) => {}
                    (oa, ob) => panic!(
                        "{at}: the caller heard different answers: {:?} / {:?}",
                        oa.map(|o| matches!(o, LogWriteOutcome::Duplicate)),
                        ob.map(|o| matches!(o, LogWriteOutcome::Duplicate))
                    ),
                }
                if refused_here {
                    refused += 1;
                    continue;
                }
                logged += 1;
                same_log_across(lane.log_records(), old.log_records(), &at);
                let row = lane.log_records().last().cloned().expect("logged");
                let mut expected = rec;
                expected.id = row.id;
                assert_eq!(
                    *row, expected,
                    "{at}: the contact as handed in, with its id"
                );
                flush(&lane);
                assert_eq!(
                    nonces_aside(&std::fs::read(a.log()).unwrap_or_default()),
                    nonces_aside(&std::fs::read(b.log()).unwrap_or_default()),
                    "{at}: log.adi is the file 1.13 wrote"
                );
            }
        }
        assert!(
            logged >= 60 && refused >= 20,
            "logged {logged}, refused {refused}"
        );
    }

    /// ★ ON THE 1.13 PATH A LOGGED CONTACT'S RECEIPT IS REDEEMED ONLY ONCE `log.adi` HOLDS IT —
    /// what a Remote log answers `fileSynced` on. While the file refuses the contact, the
    /// receipt says so, at once, as 1.13's failed append did; once the file takes it, the lane
    /// appends it, and it is there once.
    #[cfg(unix)]
    #[test]
    fn a_receipt_on_the_1_13_path_is_redeemed_only_once_log_adi_holds_the_contact() {
        use crate::engine::LogWriteOutcome;
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("receipt-1-13");
        std::fs::write(d.log(), legacy_log(2)).unwrap();
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_log_path(d.log());
        let mode = |m| std::fs::set_permissions(d.log(), std::fs::Permissions::from_mode(m));
        mode(0o444).unwrap();
        let LogWriteOutcome::PendingSync(receipts) =
            e.log_qso_for_sync(qso("W1RCPT", 1_788_500_000))
        else {
            panic!("logged, with a receipt");
        };
        assert_eq!(receipts.len(), 1);
        let asked = Instant::now();
        let redeemed: Result<Vec<()>, _> = receipts.into_iter().map(|r| r.sync()).collect();
        mode(0o644).unwrap();
        assert!(
            redeemed.is_err(),
            "not redeemed while log.adi refuses the contact"
        );
        assert!(
            asked.elapsed() < Duration::from_secs(10),
            "and said at once: {:?}",
            asked.elapsed()
        );
        flush(&e);
        let file = calls_in_file(&d);
        assert_eq!(
            file.iter().filter(|c| *c == "W1RCPT").count(),
            1,
            "then the lane appends it, once: {file:?}"
        );
    }

    /// The launch adopts the 1.13 path holding the engine lock, and loads `log.adi` into its
    /// store in memory under it, as 1.13 loaded the file: nothing on that path asserts the lock
    /// is free.
    #[test]
    fn the_1_13_path_loads_under_the_engine_lock_as_the_launch_holds_it() {
        let d = Dir::new("fallback-under-lock");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let shared = Mutex::new(Engine::new("K2DEF", "FN31", 0));
        let mut eng = engine_lock(&shared);
        eng.set_log_path(d.log());
        assert!(eng.log_on_file());
        assert_eq!(eng.log_records().len(), 5);
    }

    // ── the launch's hot index (SPEC-2 v3 C19) ──────────────────────────────

    /// A launch after an earlier session deleted two contacts, so the rows the store hands back
    /// carry rowids with gaps in them: the launch's index is keyed by those rowids.
    fn a_store_with_gaps(tag: &str) -> Dir {
        let d = Dir::new(tag);
        std::fs::write(d.log(), legacy_log(40)).unwrap();
        let mut e = engine_on_store(&d);
        for call in ["K3ABC", "K17ABC"] {
            let id = e
                .log_records()
                .iter()
                .find(|r| r.call == call)
                .and_then(|r| r.id)
                .expect("a contact to delete");
            assert!(e.delete_qso(id));
        }
        flush(&e);
        d
    }

    /// ★ The launch's hot index is built by the store while it opens — before the engine is
    /// locked — and the attach INSTALLS it, building nothing under the lock, when it was keyed by
    /// the resolver the station holds: the one `Arc` the shell hands both. Installed, it answers
    /// as the index the station would have built from its own rows, and follows the next contact
    /// as that one would.
    #[test]
    #[cfg(debug_assertions)] // reads the debug build's rebuild counter
    fn the_attach_installs_the_index_the_store_built_with_the_stations_resolver() {
        use tempo_core::logbook::hot::{HotIndex, HOT_REBUILDS};
        let d = a_store_with_gaps("hot-launch");
        let resolver: Arc<crate::station::DxccResolve> = Arc::new(test_country);
        let build = HotBuild::keyed_by(Some(Arc::clone(&resolver)));
        let opened = open_reporting_with(
            &d.log(),
            no_resolve(),
            None,
            fast(),
            Some(build),
            &mut |_| {},
        )
        .expect("the store opens");
        assert!(
            opened.hot.is_some(),
            "the store built the index as it opened"
        );
        let mut e = Engine::new("K2DEF", "FN31", 0);
        e.set_dxcc_resolver_shared(Arc::clone(&resolver));
        HOT_REBUILDS.with(|c| c.set(0));
        e.attach_log_store(opened);
        assert_eq!(
            HOT_REBUILDS.with(|c| c.get()),
            0,
            "installed as the store built it, not built again under the lock"
        );
        let keys = crate::station::StationKeys(Some(&*resolver));
        let own = |e: &Engine| HotIndex::build(&e.station().logbook, &keys);
        assert!(
            e.station().hot().answers_as(&own(&e)),
            "the station's own index, exactly"
        );
        assert!(
            e.station().hot().entity_worked_on("Entity of K5ABC", "20m"),
            "keyed by the station's resolver"
        );

        HOT_REBUILDS.with(|c| c.set(0));
        e.log_qso(qso("W9NEW", 1_788_000_000));
        assert_eq!(
            HOT_REBUILDS.with(|c| c.get()),
            0,
            "the next contact is followed, not rebuilt"
        );
        assert!(
            e.station().hot().answers_as(&own(&e)),
            "and followed as the station's own index follows it"
        );
        assert!(e.station().hot().worked_call("W9NEW"));
    }

    /// The CONTROL: an index the store keyed by any other resolver — even the same function in
    /// another `Arc`, since two closures cannot be compared — is not installed. The attach builds
    /// the station's own from its rows, as it always has, and the answers are the same.
    #[test]
    #[cfg(debug_assertions)] // reads the debug build's rebuild counter
    fn an_index_keyed_by_another_resolver_is_built_again_not_installed() {
        use tempo_core::logbook::hot::{HotIndex, HOT_REBUILDS};
        let d = a_store_with_gaps("hot-other");
        let build = HotBuild::keyed_by(Some(Arc::new(test_country)));
        let opened = open_reporting_with(
            &d.log(),
            no_resolve(),
            None,
            fast(),
            Some(build),
            &mut |_| {},
        )
        .expect("the store opens");
        let mut e = Engine::new("K2DEF", "FN31", 0);
        let resolver: Arc<crate::station::DxccResolve> = Arc::new(test_country);
        e.set_dxcc_resolver_shared(Arc::clone(&resolver));
        HOT_REBUILDS.with(|c| c.set(0));
        e.attach_log_store(opened);
        assert_eq!(
            HOT_REBUILDS.with(|c| c.get()),
            1,
            "built from the log, as before"
        );
        let keys = crate::station::StationKeys(Some(&*resolver));
        assert!(e
            .station()
            .hot()
            .answers_as(&HotIndex::build(&e.station().logbook, &keys)));
    }

    /// The launch's build, timed (SPEC-2 v3 C19; §3.4 measured 220 ms at 150k and 1.1 s at
    /// 500k), and held to the build from the same rows loaded whole. A release bench:
    ///
    /// `HOT_BENCH_ROWS=500000 cargo test --release -p tempo-app --lib -- --ignored
    /// the_launch_index_bench --nocapture`
    #[test]
    #[ignore = "a release bench: the launch's hot index build at 150k (or HOT_BENCH_ROWS) rows"]
    fn the_launch_index_bench() {
        use tempo_core::logbook::hot::HotIndex;
        let n: usize = std::env::var("HOT_BENCH_ROWS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(150_000);
        const BANDS: [&str; 10] = [
            "160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m",
        ];
        const MODES: [&str; 4] = ["FT8", "CW", "SSB", "FT4"];
        const PREFIXES: [&str; 8] = ["W", "K", "N", "DL", "JA", "G", "VE", "PY"];
        let d = Dir::new("hot-bench");
        let mut log = tempo_core::logbook::Logbook::new();
        for i in 0..n {
            let station = i % 50_000;
            let call = format!(
                "{}{}{}{}",
                PREFIXES[station % PREFIXES.len()],
                station % 10,
                ["AB", "XYZ", "CD", "EFG"][station / 10 % 4],
                station / 40
            );
            let mut r = qso(&call, 1_600_000_000 + i as u64 * 97);
            r.band = BANDS[i % BANDS.len()].into();
            r.mode = MODES[i / 3 % MODES.len()].into();
            r.grid = (i % 5 != 0).then(|| {
                let c = |k: usize| (b'A' + (k % 18) as u8) as char;
                format!("{}{}{}{}", c(i), c(i / 18), i % 10, i / 10 % 10)
            });
            r.qsl_rcvd.lotw = i % 3 == 0;
            log.add(r);
        }
        {
            let mut db = LogDb::open(&d.db()).expect("a store");
            db.insert_all(log.records().iter().map(|r| (&**r, Resolved::default())))
                .expect("written");
        }
        let resolver: Arc<crate::station::DxccResolve> = Arc::new(|call: &str| {
            let base = tempo_core::message::base_call(call);
            (base.len() >= 3).then(|| base[..2].to_string())
        });
        let keys = crate::station::StationKeys(Some(&*resolver));
        let db = LogDb::open(&d.db()).expect("the store");
        let t = Instant::now();
        let (records, from_store) = db
            .in_one_snapshot(|db| Ok((db.load_all()?, HotIndex::from_store(db, &keys)?)))
            .expect("loaded");
        let both = t.elapsed();
        let t = Instant::now();
        let alone = db
            .in_one_snapshot(|db| HotIndex::from_store(db, &keys))
            .expect("built");
        let build = t.elapsed();
        let copy = tempo_core::logbook::Logbook::from_store(records);
        let t = Instant::now();
        let from_rows = HotIndex::build(&copy, &keys);
        let from_memory = t.elapsed();
        assert!(
            from_store.answers_as(&from_rows),
            "the store's build is the log's"
        );
        assert!(alone.answers_as(&from_rows));

        // The read before a contest session opens: the last four days and an hour, whole — here
        // 2,000 contacts, a contest weekend's worth, on top of the lifetime log.
        let now = crate::engine::now_unix_secs();
        {
            let mut db = LogDb::open(&d.db()).expect("the store");
            let mut weekend = tempo_core::logbook::Logbook::new();
            for k in 0..2_000u64 {
                weekend.add(qso(&format!("K{k}REC"), now - k * 120));
            }
            db.insert_all(
                weekend
                    .records()
                    .iter()
                    .map(|r| (&**r, Resolved::default())),
            )
            .expect("written");
        }
        let bound = now.saturating_sub(crate::engine::SESSION_READ_WINDOW);
        let t = Instant::now();
        let session = db
            .in_one_snapshot(|db| rows_since(db, bound))
            .expect("read");
        let session_read = t.elapsed();
        assert_eq!(session.len(), 2_000, "the weekend, and nothing older");
        println!(
            "HOT-LAUNCH n={n} build_from_store_ms={:.1} load_all_plus_build_ms={:.1} \
             build_from_memory_ms={:.1} session_read_2000_ms={:.1}",
            build.as_secs_f64() * 1e3,
            both.as_secs_f64() * 1e3,
            from_memory.as_secs_f64() * 1e3,
            session_read.as_secs_f64() * 1e3,
        );
    }
}
