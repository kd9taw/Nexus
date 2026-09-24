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

use tempo_core::logbook::mirror::{
    self, FileStamp, MirrorOptions, MirrorState, MirrorWriter, StoreSource,
};
use tempo_core::logbook::reader::LogReader;
use tempo_core::logbook::sqlite::{self, LogDb, Resolved};
use tempo_core::logbook::writer::{self, Change, LogWriter, Refusal, Ticket, Touched};
use tempo_core::logbook::{migrate, Logbook, QsoRecord};

/// cty.dat's answer for a record — the entity NAME and CQ zone the store writes beside it.
/// Injected by the shell, which owns the table; tempo-app does not depend on `propagation`.
pub type StoreResolve = Arc<dyn Fn(&QsoRecord) -> Resolved<'static> + Send + Sync>;

/// How long an operator command waits for its change to reach the disk before it says so.
pub const DURABLE_WAIT: Duration = Duration::from_secs(60);

/// The store, open and owning the log.
pub struct LogStore {
    writer: Arc<LogWriter>,
    /// Read connections to the store, opened on first use — see [`StoreReads`].
    reader: Arc<LogReader>,
    /// Shared with a quit, which writes it with the engine lock released (see [`Unsaved`]).
    mirror: Arc<MirrorWriter>,
    log_path: PathBuf,
    db_path: PathBuf,
    resolve: StoreResolve,
    /// This process's changes the writer has not finished with, and the rows each touched.
    inflight: Vec<InFlight>,
    /// This process's changes the writer GAVE UP ON: not in the store, still in memory — sent
    /// again from memory when the refusal can pass, held for the quit when it cannot. See
    /// [`LogStore::resend`].
    dropped: Vec<Dropped>,
    /// The latest refusal, in the store's words — what the screen says while any is held.
    last_refusal: Option<String>,
    /// The writer's count of ANOTHER process's commits when memory last matched the store.
    synced_foreign: u64,
    /// Tickets being collected for a command that will wait on them (see
    /// [`crate::engine::Engine::with_log_tickets`]).
    collector: Option<Vec<Ticket>>,
}

impl std::fmt::Debug for LogStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogStore")
            .field("db_path", &self.db_path)
            .field("log_path", &self.log_path)
            .field("inflight", &self.inflight.len())
            .field("dropped", &self.dropped.len())
            .finish()
    }
}

/// What [`open`] hands the station: the store, the records to hold, and — when `log.adi`
/// held something the store did not account for — that file, to be taken in.
pub struct Opened {
    pub(crate) store: LogStore,
    pub(crate) records: Vec<QsoRecord>,
    pub(crate) foreign: Option<ForeignLog>,
    /// What the conversion did.
    pub outcome: migrate::Outcome,
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
/// the start-up screen shows while it works (see [`migrate::migrate_log_reporting`]).
pub fn open_reporting(
    log_path: &Path,
    resolve: StoreResolve,
    network: Option<String>,
    progress: &mut dyn FnMut(migrate::Progress),
) -> Result<Opened, OpenError> {
    open_reporting_with(
        log_path,
        resolve,
        network,
        MirrorOptions::default(),
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
    open_reporting_with(log_path, resolve, network, mirror_options, &mut |_| {})
}

/// [`open_with`] and [`open_reporting`] in one: the mirror's timings, and the conversion's
/// progress.
fn open_reporting_with(
    log_path: &Path,
    resolve: StoreResolve,
    network: Option<String>,
    mut mirror_options: MirrorOptions,
    progress: &mut dyn FnMut(migrate::Progress),
) -> Result<Opened, OpenError> {
    // All of it — the conversion, the load, the read of a foreign `log.adi`, the sweep — before
    // the engine is locked.
    tempo_core::logbook::io_fence::off_engine_lock("opening the logbook store");
    if let Some(why) = network {
        return Err(OpenError::NetworkFolder(why));
    }
    let db_path = migrate::database_path(log_path);
    let outcome = migrate::migrate_log_reporting(log_path, &db_path, |r| resolve(r), progress)
        .map_err(OpenError::Conversion)?;
    let db = LogDb::open(&db_path).map_err(OpenError::Store)?;
    let records = db.load_all().map_err(OpenError::Store)?;

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
            mirror,
            log_path: log_path.to_path_buf(),
            db_path,
            resolve,
            inflight: Vec::new(),
            dropped: Vec::new(),
            last_refusal: None,
            synced_foreign,
            collector: None,
        },
        records,
        foreign,
        outcome,
    })
}

impl LogStore {
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

    /// The database's path.
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// The `log.adi` this store mirrors to.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// The mirror's state, for a caller that surfaces a refusal.
    pub fn mirror_status(&self) -> mirror::Status {
        self.mirror.status()
    }

    /// cty.dat's answer as the store writes it.
    pub(crate) fn resolved(&self) -> impl Fn(&QsoRecord) -> (Option<String>, Option<u8>) + '_ {
        |r| {
            let x = (self.resolve)(r);
            (x.entity.map(str::to_string), x.cq_zone)
        }
    }

    /// Hand one change to the writer, and its revision to the mirror. Never touches the disk. A
    /// change that writes nothing is not sent, and has no ticket.
    pub(crate) fn submit(&mut self, change: Change) -> Option<Ticket> {
        if change.is_empty() {
            return None;
        }
        self.collect(Instant::now());
        let ticket = self.send(change, 0);
        if let Some(c) = &mut self.collector {
            c.push(ticket.clone());
        }
        Some(ticket)
    }

    /// Hand a change to the writer and its revision to the mirror, and keep the change in flight
    /// — `resends` times sent again already. No I/O, and no copy of the log: the mirror reads the
    /// store once it holds the change.
    fn send(&mut self, change: Change, resends: u32) -> Ticket {
        let touched = Touched::of(&change);
        let ticket = self.writer.submit(change);
        self.inflight.push(InFlight {
            ticket: ticket.clone(),
            touched,
            resends,
        });
        self.mirror.dirty(ticket.revision());
        ticket
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
                touched: f.touched,
                rev: f.ticket.revision(),
                refusal,
                resends: f.resends,
                due: now + wait,
            });
        }
        self.inflight = kept;
    }

    /// Send again, FROM MEMORY, the changes the writer gave up on for a reason that can pass —
    /// every one when `all` (a quit, and its Keep trying), otherwise only those whose wait is up.
    /// How many went out. A change refused for what it is is never sent again: it would be
    /// refused every time, and a loop is not a save. No I/O.
    ///
    /// ⚠️ **Why this cannot write a change twice, or out of order.** Each re-send is built here,
    /// from `log` as it stands, and submitted here, under the owner's lock — the lock every
    /// change is made and submitted under ([`Change::resend`]). So it carries the rows as memory
    /// holds them NOW, including whatever an earlier change did to them; a later change to the
    /// same rows is submitted after it, and the writer applies changes that share a row in the
    /// order they were submitted. And it carries state, not a delta: applied twice, or after
    /// the first attempt landed despite its error, it writes what is already there.
    pub(crate) fn resend(&mut self, log: &Logbook, all: bool, now: Instant) -> usize {
        self.collect(now);
        let (go, stay): (Vec<Dropped>, Vec<Dropped>) = std::mem::take(&mut self.dropped)
            .into_iter()
            .partition(|d| d.refusal.retryable && (all || d.due <= now));
        self.dropped = stay;
        if go.is_empty() {
            return 0;
        }
        let batch: Vec<(&Touched, u64)> = go.iter().map(|d| (&d.touched, d.rev)).collect();
        let changes = Change::resend_each(&batch, log, self.resolved());
        let mut sent = 0;
        for (d, change) in go.into_iter().zip(changes) {
            // A change always writes something, and so does a re-send of it: its rows are
            // either in memory (written) or not (removed).
            if change.is_empty() {
                continue;
            }
            self.send(change, d.resends + 1);
            sent += 1;
        }
        tempo_core::applog::info(
            "logbook",
            &format!("sending {sent} logbook change(s) the database refused again, from memory"),
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
            .map(|f| f.touched.clone())
            .chain(self.dropped.iter().map(|d| d.touched.clone()))
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
        self.mirror.accept(stamp);
    }

    /// Have the mirror picture the store as it stands, changing nothing in it — for a `log.adi`
    /// that is not the store's own picture yet (the file a conversion read, one just taken in),
    /// so the next launch finds a mirror and has nothing to read or take in.
    pub(crate) fn refresh_mirror(&self) {
        self.mirror.dirty(self.writer.submitted_rev());
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
            changes: Durability::new(Some(self.writer()), tickets),
            held: self.held(),
            mirror: Some(Arc::clone(&self.mirror)),
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
        let mirrored = self.mirror.flush(deadline);
        logged?;
        match mirrored.last_error {
            Some(e) if mirrored.pending => Err(format!("log.adi was not brought up to date: {e}")),
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
}

impl Durability {
    pub(crate) fn new(writer: Option<Arc<LogWriter>>, tickets: Vec<Ticket>) -> Durability {
        Durability { writer, tickets }
    }

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
        Ok(())
    }

    /// Each change as a receipt the Remote service redeems the same way it redeems an append
    /// to `log.adi` ([`tempo_core::logbook::LogAppendReceipt::sync`]).
    pub fn into_receipts(self) -> Vec<tempo_core::logbook::LogAppendReceipt> {
        let Some(writer) = self.writer else {
            return Vec::new();
        };
        self.tickets
            .into_iter()
            .map(|t| {
                tempo_core::logbook::LogAppendReceipt::durable(Arc::clone(&writer), t, DURABLE_WAIT)
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
    /// The rows it writes or removes.
    touched: Touched,
    /// How many times these rows have been sent again after the writer gave up on them — 0 for a
    /// change on its first way to disk.
    resends: u32,
}

/// A change of this process's the writer gave up on ([`Refusal`]): not in the store, still in
/// memory.
struct Dropped {
    /// The rows it wrote or removed — what a re-send carries, as memory holds them then.
    touched: Touched,
    /// The revision it first went out with, which a re-send keeps.
    rev: u64,
    refusal: Refusal,
    resends: u32,
    /// When it is next sent again by itself (a refusal that can pass).
    due: Instant,
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

    /// Where the changes stand now. Never waits — it reads each ticket, and does not ask the
    /// writer to wait on one, so it is safe to ask under any lock.
    pub fn standing(&self) -> Standing {
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

    /// Wait up to `for_up_to` for the writer to finish with every change, then say where they
    /// stand. ⚠️ The quit's wait: never call it holding a lock.
    pub fn wait(&self, for_up_to: Duration) -> Standing {
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
        self.standing()
    }

    /// Bring `log.adi` up to date now, waiting up to `for_up_to`: `None` when it is written, or
    /// had nothing to write; otherwise what is wrong, for the diagnostic log. The copy is a
    /// convenience beside the logbook, so this is never a question for the operator.
    pub fn write_mirror(&self, for_up_to: Duration) -> Option<String> {
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
mod tests {
    use super::*;
    use crate::engine::{engine_lock, engine_try_lock, Engine};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Mutex;
    use std::time::Instant;
    use tempo_core::logbook::{adif_header, adif_record_own_log, sqlite::WriteHold, QslVia};

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
        assert!(eng.update_qso(0, edited));
        assert!(eng.delete_qso(2));
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
                eng.update_qso(at, r);
            }
            3 if len > 0 => {
                e.lock().unwrap().delete_qso(at);
            }
            4 if len > 0 => {
                e.lock().unwrap().mark_qsl_card(at, g.below(2) == 0);
            }
            5 if len > 0 => {
                e.lock()
                    .unwrap()
                    .mark_qsl_sent(at, Some(tempo_core::logbook::QslVia::Bureau));
            }
            6 if len > 0 => {
                let sat = (g.below(2) == 0).then_some("RS-44");
                e.lock().unwrap().set_sat_tag(at, sat);
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
    /// app makes, fills included. The memory side is the 1.13 path's arm, which is the log as
    /// every reader saw it before C14.
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
        for seed in 1..=12u64 {
            let d = Dir::new(&format!("rows-{seed}"));
            std::fs::write(d.log(), log_to_fill(12)).unwrap();
            flush(&engine_on_store(&d));
            let e = launch_with_resolvers(&d);
            let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
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
        assert_eq!(changes, 12 * 40);
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
                    out.push((format!("{format} {from:?}..{to:?}"), text));
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
            out.push((format!("operator {op:?}"), text));
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
            out.push((format!("activation {reference:?} {day} {call:?}"), text));
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
    /// after every fourth. The memory side is the Logbook's own export of the log the engine
    /// holds — what the Export button wrote before C15.
    #[test]
    fn every_export_from_the_store_is_the_export_of_the_log_in_memory() {
        let mut compared = 0usize;
        let mut nonempty = 0usize;
        for seed in 1..=8u64 {
            let d = Dir::new(&format!("export-{seed}"));
            std::fs::write(d.log(), export_fixture()).unwrap();
            let e = launch_with_resolvers(&d);
            let _ = e.lock().unwrap().import_adif(
                &export_records(12, 20)
                    .replace("Jean-Luc", "Jos\u{FFFD}")
                    .replace("plain", "a \u{FFFD}\u{FFFD} b"),
            );
            let mut g = Gen(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
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

    /// ★ AN EXPORT NEVER LEAVES OUT A CHANGE IT WAS ASKED AFTER. With the store's write lock held
    /// elsewhere (a write taking its time), a contact logged just before the export is not in
    /// the store yet: the export says so and writes no file, rather than a file without the
    /// contact. The control is the same export once the store has it, which holds it.
    #[test]
    fn an_export_the_store_has_not_caught_up_with_is_refused_not_short() {
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
        let refused = crate::logexport::export_waiting(&source, kind(), Duration::from_millis(300));
        let why = refused.expect_err("an export missing the contact is refused");
        assert!(why.contains("not saved to its database yet"), "{why}");
        drop(hold);
        let text = crate::logexport::export_waiting(&source, kind(), Duration::from_secs(60))
            .expect("control: once the store has it");
        assert!(text.contains("W9LATE"), "and the export holds it");
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
    /// held elsewhere, a contact is logged: the mirror, told of it, writes nothing — a picture
    /// without the contact is not a picture of the log — and says a change is still owed. Once
    /// the store has it, the mirror writes it by itself.
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
        std::thread::sleep(Duration::from_millis(600));
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
    /// verbatim but for `self`: the records at `indices` in the log in memory, in that order.
    fn lotw_adif_before(
        recs: &[Arc<QsoRecord>],
        indices: &[usize],
        adif_loc: bool,
        call: &str,
        grid: &str,
    ) -> String {
        let mut out = tempo_core::logbook::adif_header();
        for &i in indices {
            if let Some(r) = recs.get(i) {
                if adif_loc {
                    out.push_str(&tempo_core::logbook::adif_record_with_station(
                        r, call, grid,
                    ));
                } else {
                    out.push_str(&tempo_core::logbook::adif_record(r));
                }
            }
        }
        out
    }

    /// ★ THE LoTW BATCH FROM THE STORE IS THE BATCH THE LOG IN MEMORY MADE — after the export
    /// fixture, contacts with no known time, and 6 seeded runs of 30 random changes with LoTW
    /// stamps of every outcome mixed in (and the operator's "already uploaded" declaration now
    /// and then), the default batch read from the store is exactly the contacts
    /// `lotw_unsent_indices` names in memory: the same records, the same file handed to TQSL in
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
                            let at = g.below(len);
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
                let (rows, indices, memory, signed_before, held) = {
                    let eng = e.lock().unwrap();
                    let indices = eng.lotw_unsent_indices();
                    (
                        eng.log_rows(),
                        indices.clone(),
                        eng.lotw_rows_at(&indices),
                        eng.lotw_signed(&indices),
                        eng.log_records().to_vec(),
                    )
                };
                let from_store = crate::station::lotw_unsent(&rows).expect("the store reads");
                assert!(
                    from_store == memory,
                    "seed {seed}, step {step}: the store's batch differs from memory's"
                );
                for adif_loc in [false, true] {
                    assert_eq!(
                        crate::station::lotw_batch_adif(&from_store, adif_loc, "KD9TAW", "EN52"),
                        lotw_adif_before(&held, &indices, adif_loc, "KD9TAW", "EN52"),
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
                    !indices.is_empty(),
                    "seed {seed}, step {step}: is anything owed"
                );
                owed_seen += usize::from(!indices.is_empty());
                checked += 1;
            }
        }
        assert_eq!(checked, 60);
        assert!(
            owed_seen > 20,
            "premise: batches with contacts in them ({owed_seen})"
        );
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

    // ── parity with the ADIF path ───────────────────────────────────────────

    /// ★ PROPERTY 3 — THE OLD PATH IS THE ORACLE. The same operations, in the same order,
    /// through an engine whose log is `log.adi` (the 1.13 path) and one whose log is the store.
    /// After every step the two logs are the same log; at the end the store on disk is too,
    /// and so is the mirror against the 1.13 file.
    ///
    /// The operations are every one the app makes to the log through the engine: logging,
    /// the edit, both QSL marks, the satellite tag, a delete, an import, the three report
    /// merges, the park import, the LoTW echo, all four connector stamps, and the purge.
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

    #[test]
    fn the_store_path_answers_exactly_as_the_adif_path() {
        let (a, b) = (Dir::new("parity-adif"), Dir::new("parity-store"));
        let start = legacy_log(40);
        std::fs::write(a.log(), &start).unwrap();
        std::fs::write(b.log(), &start).unwrap();
        let mut old = Engine::new("K2DEF", "FN31", 0);
        old.set_log_path(a.log());
        let mut new = engine_on_store(&b);
        same_log(new.log_records(), old.log_records(), "at the start");

        type Step = (&'static str, Box<dyn Fn(&mut Engine)>);
        let steps: Vec<Step> = vec![
            ("log", Box::new(|e| e.log_qso(qso("W1NEW", 1_788_000_000)))),
            (
                "log 2",
                Box::new(|e| e.log_qso(qso("W2NEW", 1_788_000_600))),
            ),
            (
                "edit",
                Box::new(|e| {
                    let mut r = QsoRecord::clone(&e.log_records()[3]);
                    r.name = Some("Edited".into());
                    assert!(e.update_qso(3, r));
                }),
            ),
            (
                "call fix",
                Box::new(|e| {
                    let mut r = QsoRecord::clone(&e.log_records()[4]);
                    r.call = "K4FIX".into();
                    assert!(e.update_qso(4, r));
                }),
            ),
            (
                "qsl sent",
                Box::new(|e| assert!(e.mark_qsl_sent(5, Some(QslVia::Bureau)))),
            ),
            (
                "qsl withdrawn",
                Box::new(|e| assert!(e.mark_qsl_sent(5, None))),
            ),
            ("card", Box::new(|e| assert!(e.mark_qsl_card(6, true)))),
            (
                "sat",
                Box::new(|e| assert!(e.set_sat_tag(7, Some("AO-91")))),
            ),
            ("delete", Box::new(|e| assert!(e.delete_qso(8)))),
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
                Box::new(|e| {
                    let r = QsoRecord::clone(&e.log_records()[0]);
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
                Box::new(|e| {
                    let r = QsoRecord::clone(&e.log_records()[1]);
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
                Box::new(|e| {
                    let r = QsoRecord::clone(&e.log_records()[2]);
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
                Box::new(|e| {
                    e.stamp_lotw_upload(
                        &[9, 10],
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
        // nothing else — and the stamp stays on the row for every step after. The store's row
        // takes the ADIF path's stamp when the two lie within the most seconds any pair so far
        // spanned; every other byte, and any stamp further apart or missing on one side, still
        // has to match exactly.
        let mut spanned = 0;
        for (what, step) in &steps {
            let before = crate::engine::now_unix_secs();
            step(&mut old);
            step(&mut new);
            spanned = spanned.max(crate::engine::now_unix_secs() - before);
            let aligned = with_wall_clock_of(new.log_records(), old.log_records(), spanned);
            same_log_across(&aligned, old.log_records(), what);
        }
        // Every step DID something — the census that keeps the comparisons above from passing
        // over a log nothing happened to.
        let held = new.log_records();
        let any = |f: &dyn Fn(&QsoRecord) -> bool| held.iter().any(|r| f(r));
        for (what, hit) in [
            ("logged", any(&|r| r.call == "W2NEW")),
            ("edited", any(&|r| r.name.as_deref() == Some("Edited"))),
            ("call fixed", any(&|r| r.call == "K4FIX")),
            ("qsl withdrawn", any(&|r| r.qsl_sent.cleared_unix.is_some())),
            ("card", any(&|r| r.qsl_rcvd.card)),
            ("sat", any(&|r| r.sat_name.as_deref() == Some("AO-91"))),
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
        same_log(
            &stored(&b),
            new.log_records(),
            "the store on disk is the store's memory",
        );
        same_log_across(
            &with_wall_clock_of(&stored(&b), old.log_records(), spanned),
            old.log_records(),
            "the store on disk",
        );
        let (mirror, file) = (
            tempo_core::logbook::Logbook::load(&b.log()),
            tempo_core::logbook::Logbook::load(&a.log()),
        );
        same_log_across(
            &with_wall_clock_of(mirror.records(), file.records(), spanned),
            file.records(),
            "the mirror against the 1.13 file",
        );

        // And the purge.
        old.clear_logbook();
        new.clear_logbook();
        flush(&new);
        assert!(new.log_records().is_empty() && stored(&b).is_empty());
        assert!(tempo_core::logbook::Logbook::load(&b.log()).is_empty());
    }

    // ── durability ──────────────────────────────────────────────────────────

    /// A command waits for exactly its own change: the tickets it collected, and not the whole
    /// queue. After the wait a connection of the test's own sees the change.
    #[test]
    fn a_command_waits_for_its_own_change_and_then_it_is_on_disk() {
        let d = Dir::new("durable");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        let mut e = engine_on_store(&d);
        let (ok, durability) = e.with_log_tickets(|e| e.mark_qsl_card(3, true));
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
        let (_, none) = e.with_log_tickets(|e| e.mark_qsl_card(999, true));
        assert!(none.is_empty());
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

    /// ★ PROPERTY 5. With the store's write lock held elsewhere — a write that is taking its
    /// time — every log command still returns at once under the engine lock, another thread
    /// can `try_lock` the engine the whole time, and only the off-lock wait feels the stall.
    ///
    /// The positive control is the timed-out wait: it proves the writer really was stalled, so
    /// the prompt returns above are not a writer that simply finished first.
    #[test]
    fn nothing_under_the_engine_lock_waits_for_a_stalled_write() {
        let d = Dir::new("stall");
        std::fs::write(d.log(), legacy_log(20)).unwrap();
        let engine = Arc::new(Mutex::new(engine_on_store(&d)));

        let hold = WriteHold::take(&d.db()).expect("hold the write lock");
        let started = Instant::now();
        let durability = {
            let mut e = engine_lock(&engine);
            let (_, a) = e.with_log_tickets(|e| e.log_qso(qso("W1STALL", 1_788_000_000)));
            let (_, b) = e.with_log_tickets(|e| e.mark_qsl_card(2, true));
            let (_, c) = e.with_log_tickets(|e| e.delete_qso(5));
            assert_eq!((a.len(), b.len(), c.len()), (1, 1, 1));
            vec![a, b, c]
        };
        let under_lock = started.elapsed();
        assert!(
            under_lock < Duration::from_millis(500),
            "three changes under the engine lock took {under_lock:?} with the writer stalled"
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
        let mut e = engine_lock(&engine);
        let (_, durability) = e.with_log_tickets(|e| e.mark_qsl_card(1, true));
        let _ = durability.wait(DURABLE_WAIT); // `e` is still held
    }

    /// The other direction: the same wait with the guard dropped first passes, and the change
    /// is on disk.
    #[cfg(debug_assertions)]
    #[test]
    fn a_durable_wait_after_the_engine_lock_is_released_passes_the_fence() {
        let d = Dir::new("fence-wait-ok");
        std::fs::write(d.log(), legacy_log(5)).unwrap();
        let engine = shared_on_store(&d);
        let durability = {
            let mut e = engine_lock(&engine);
            e.with_log_tickets(|e| e.mark_qsl_card(1, true)).1
        };
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
        let (_, durability) = raw.with_log_tickets(|e| e.mark_qsl_card(1, true));
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

    /// ★ PROPERTY 5, the radio loop's own write. In companion mode the radio loop imports
    /// WSJT-X's LoggedAdif datagram inside a tick, under the engine lock. With the store's
    /// write lock held elsewhere the import still returns at once — the contact is in the log
    /// and the tick moves on — and the contact reaches the disk when the write clears. The loop
    /// never waits for it (it collects no ticket; the test collects one only to know when).
    ///
    /// The positive control is the timed-out wait: the writer really was stalled, so the
    /// prompt return is not a write that simply finished first.
    #[test]
    fn the_radio_loops_companion_import_touches_no_disk_under_the_lock() {
        let d = Dir::new("companion");
        std::fs::write(d.log(), legacy_log(20)).unwrap();
        let mut e = engine_on_store(&d);

        let hold = WriteHold::take(&d.db()).expect("hold the write lock");
        let started = Instant::now();
        let ((added, ..), durable) = e.with_log_tickets(|e| e.import_adif(WSJTX_LOGGED_ADIF));
        let under_lock = started.elapsed();
        assert_eq!(added, 1, "the contact WSJT-X logged is in the log");
        assert!(
            under_lock < Duration::from_millis(500),
            "the import under the engine lock took {under_lock:?} with the writer stalled"
        );
        assert!(
            durable.wait(Duration::from_millis(300)).is_err(),
            "control: the contact cannot be on disk while the write lock is held elsewhere"
        );

        drop(hold);
        durable
            .wait(DURABLE_WAIT)
            .expect("durable once the lock is released");
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
        assert!(e.mark_qsl_card(3, true));
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

        assert!(e.mark_qsl_card(4, true));
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
        let good = Change::appended(&log, 1, |_| (None, None));
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
            store.resend(&log, false, later),
            0,
            "never sent again by itself"
        );
        assert_eq!(
            store.resend(&log, true, later),
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
            touched: Touched::of(&lost),
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
            store.resend(&log, false, t0),
            0,
            "not before its wait is up"
        );
        // Sent while the disk is held up again: the screen keeps saying so until it lands.
        let hold = WriteHold::take(&d.db()).expect("stall the store");
        assert_eq!(
            store.resend(&log, false, t0 + Duration::from_secs(5)),
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
            store.resend(&log, true, t0 + Duration::from_secs(3600)),
            0,
            "and nothing is sent again after it landed"
        );
    }

    /// ★ The real refusal, end to end: another program holds the database past the writer's
    /// retry ladder (about 21 s — this test waits it out), the writer drops the change, and the
    /// engine keeps it: the snapshot says so, the quit counts it as a change that sending again
    /// can save, and sending it again lands it once the database is free.
    ///
    /// And an export (SPEC-2 v3 C15) follows the change, not the writer's watermark: refused
    /// while the change is held, made once the re-send has landed it — though the read's own
    /// freshness, capped for good by the drop, calls every read after it stale.
    #[test]
    fn the_database_s_busy_refusal_is_sent_again_and_the_snapshot_says_so() {
        let d = Dir::new("resend-busy");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        let mut e = engine_on_store(&d);
        flush(&e);
        assert_eq!(e.snapshot().log_save_trouble, None, "control: no trouble");

        let hold = WriteHold::take(&d.db()).expect("another program holds the database");
        assert!(e.mark_qsl_card(3, true));
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
        .expect_err("no export while the change is held");
        assert!(held.contains("locked"), "and it says why: {held}");

        drop(hold);
        assert_eq!(e.log_resend_all(), 1, "sent again, from memory");
        assert!(e.log_unsaved().wait(DURABLE_WAIT).saved(), "and it lands");
        let text = crate::logexport::export_waiting(
            &crate::logexport::Source::of(&e),
            adif(),
            DURABLE_WAIT,
        )
        .expect("an export once the re-send has landed the change");
        assert!(text.contains("<QSL_RCVD:1>Y"), "and it carries the change");
        let fresh = e
            .log_rows()
            .each_record(Duration::from_millis(100), &mut |_| {
                std::ops::ControlFlow::Continue(())
            })
            .expect("the store reads");
        assert!(
            matches!(fresh, Freshness::Stale(_)),
            "control: the read's own freshness is stale for good after the drop: {fresh:?}"
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
            touched: Touched::of(&lost),
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
        assert_eq!(a.store.resend(&log_after, true, Instant::now()), 1);
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
    fn eventually(mut f: impl FnMut() -> bool) -> bool {
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
        let at = b
            .log_records()
            .iter()
            .position(|r| r.id == target.id)
            .unwrap();
        // Wait until B's writer has SEEN A's stamp commit — without B polling — so the change
        // below is the case under test: a foreign commit B has not yet folded in.
        assert!(eventually(|| b.log_store_foreign_pending()));
        assert!(b.mark_qsl_card(at, true));
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
        let at = a
            .log_records()
            .iter()
            .position(|r| r.call == "K5ABC")
            .unwrap();
        let mut edited = QsoRecord::clone(&a.log_records()[at]);
        edited.call = "K5ABD".into();
        assert!(a.update_qso(at, edited));
        // A deletes a row; B must not bring it back.
        let gone = a
            .log_records()
            .iter()
            .position(|r| r.call == "K7ABC")
            .unwrap();
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

    /// ⛔ A POSITION HELD ACROSS ANOTHER WINDOW'S DELETE STILL NAMES ITS CONTACT. The Logbook
    /// and the Remote find a row, then change it by position, in one hold of the engine lock —
    /// and the change first folds in whatever another window committed. A fold-in that removed
    /// the other window's deleted row would shift every row after it, and the change would land
    /// on a different contact. So the fold-in a change makes moves nothing; the deleted row
    /// lingers until the next freshness poll, which may move rows because nobody holds a
    /// position across it.
    #[test]
    fn a_position_held_across_another_windows_delete_still_names_its_contact() {
        let d = Dir::new("positions");
        std::fs::write(d.log(), legacy_log(10)).unwrap();
        let mut a = engine_on_store(&d);
        let mut b = engine_on_store(&d);
        let target = QsoRecord::clone(&b.log_records()[5]);

        assert!(a.delete_qso(1), "A deletes a row ABOVE B's target");
        flush(&a);
        assert!(eventually(|| b.log_store_foreign_pending()));

        // B changes the row it holds position 5 for — without polling first.
        assert!(b.mark_qsl_card(5, true));
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
        assert!(
            a.lock().unwrap().mark_qsl_card(0, true),
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
        let signed = a.lotw_signed(&[0, 1, 2, 3, 4]);
        assert_eq!(signed.len(), 5, "premise: A hands TQSL five contacts");

        let deleted = b.log_records()[2].id;
        assert!(b.delete_qso(2), "B deletes one while A's TQSL runs");
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
    /// converted or created, and the 1.13 path opens the log unharmed.
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
        assert_eq!(e.log_records().len(), 6, "the fallback opens the whole log");
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
        let opened =
            open_reporting_with(&d.log(), no_resolve(), None, fast(), &mut |p| seen.push(p))
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
        let again =
            open_reporting_with(&d.log(), no_resolve(), None, fast(), &mut |p| seen.push(p))
                .expect("the store opens again");
        assert_eq!(again.outcome, migrate::Outcome::AlreadyDone);
        assert!(seen.is_empty(), "{seen:?}");
    }

    // ── the FT duplicate guard (hard gate) ──────────────────────────────────

    /// ⛔ THE GUARD ANSWERS FROM MEMORY AND NEVER FROM THE STORE. With the store's writer held
    /// back (its write lock taken elsewhere), a contact is logged: memory has it, the store does
    /// not — shown by reading the store through a connection of the test's own (the control).
    /// Logged again, it is refused as a duplicate at once. A guard that consulted the store
    /// would have found nothing there and logged it twice; one that waited for the store would
    /// not have answered while the lock was held.
    #[test]
    fn the_duplicate_guard_answers_from_memory_while_the_store_lags() {
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
            stored(&d).iter().all(|r| r.call != "W1DUP"),
            "control: the store does NOT hold the contact yet"
        );
        let started = Instant::now();
        let again = e.log_qso_for_sync(rec);
        assert!(
            matches!(again, crate::engine::LogWriteOutcome::Duplicate),
            "refused from memory, though the store has never seen the first"
        );
        assert!(
            started.elapsed() < Duration::from_millis(200),
            "and at once"
        );
        assert_eq!(
            e.log_records().iter().filter(|r| r.call == "W1DUP").count(),
            1
        );
        drop(hold);
        flush(&e);
        assert_eq!(
            stored(&d).iter().filter(|r| r.call == "W1DUP").count(),
            1,
            "once the store catches up it holds the contact once"
        );
    }
}
