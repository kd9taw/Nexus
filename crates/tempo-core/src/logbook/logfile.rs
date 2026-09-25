//! The `log.adi` lane of a session on the 1.13 path — SPEC-2 v3 **C19, D1-A**.
//!
//! When the logbook database cannot be used — a data folder on a network drive, a database a
//! newer build wrote — the session runs on `log.adi`, as 1.13 did. Since D1-A its log is held in
//! a store all the same: one in memory, loaded from `log.adi` at launch, so every reader of the
//! log has one implementation whichever path the session is on (the operator's choice: "Same
//! code, in-memory database"). What keeps `log.adi` on that path is this lane, and it keeps it
//! with 1.13's rules:
//!
//! - **A pure append is appended**, record by record, by 1.13's own append
//!   ([`Logbook::append_for_sync`]: the file made with a header if it is not there, each record
//!   written as the operator's own log writes it), and synced.
//! - **Anything else rewrites the whole file**, from the store, by 1.13's own save
//!   ([`Logbook::save_text`]: the dated snapshot, a temporary file of this process's synced before
//!   the rename, the folder synced after). The file is the file 1.13 wrote: [`Export`] of the
//!   store's rows is [`Logbook::adif`] of the same rows.
//! - **A change is saved when it is in the file.** [`LogFileWriter::wait_saved`] is what an
//!   operator's command waits for, as a 1.13 command returned once its write had happened — or
//!   had failed; the lane then tries again, where 1.13 waited for the next save.
//! - **The file another program wrote is never overwritten** — below.
//!
//! # A file another machine changed
//!
//! A data folder can be shared — `NEXUS_DATA_DIR` on a NAS is the documented two-computer setup
//! — and then another Nexus appends to this `log.adi`. 1.13 took such appends in before every
//! rewrite (the station's recovery, under the engine lock, straight before the change and its
//! save), so its window for losing one was the width of that lock hold. The station still takes
//! the file in before a change; but the rewrite now happens later, on this lane. So right before
//! it replaces the file — before it reads the store, once the new text is made, and again with
//! the new file written and synced, the moment before the rename — the lane checks that the file
//! is still the one it last wrote, or the one the station last took in
//! ([`LogFileWriter::accept`]). If it is not, the lane does not replace it: it reports the file
//! ([`Status::foreign_write`]) and holds the rewrite until the station has taken the file in.
//! That narrows 1.13's window to the instant between the last check and the rename. It cannot
//! close it: only a lock shared across machines could.
//!
//! An append is 1.13's too: it is made whatever the file holds — appending loses nothing — and it
//! is the file's accounted state afterwards only when the file was accounted before it and grew by
//! exactly what was written.
//!
//! One thing 1.13 never had to handle: while the lane still owes the file changes, the file lags
//! the log, and a row it holds may be one the log has since edited or deleted. The lane knows
//! which rows the file held when this log last accounted for it — its own last rewrite and
//! appends, or the file the station last took in ([`LogFileWriter::holds`]) — and when the
//! station takes in a file while the lane still owes it changes, it takes in only the others:
//! what another machine added.
//!
//! The rest of 1.13's trades with a shared file stay what they were, because the rules that make
//! them are unchanged: an edit made on the other machine arrives as the import rules treat a
//! restated contact, and a contact the other machine deleted comes back with this machine's next
//! rewrite. They are the price of a file with no ids and no tombstones that other loggers read.
//!
//! # Order
//!
//! Every change the station makes is handed to the lane in the order it was made: an append with
//! its rows ([`LogFileWriter::append`]), a rewrite ([`LogFileWriter::rewrite`]), or — for a change
//! whose rows are already in the file, like the station taking the file in — a note that nothing
//! needs writing ([`LogFileWriter::noted`]). Each is numbered as it is handed over, and the lane
//! works through them in that order: the `n`th is in the file once `n` are done
//! ([`LogFileWriter::wait_written`]). The numbering is the lane's own, not the store's revision,
//! because a change the store refused and is sent again keeps its revision, and is still one more
//! thing to write. A rewrite reads the store once the store holds every change up to its own, so
//! a rewrite can include a later append; the lane knows every row a rewrite wrote, and never
//! appends one of them again.

use super::mirror::{file_stamp, FileStamp, MirrorSource, Readiness, READY_WAIT};
use super::{adif_record_own_log, Export, ExportKind, Logbook, QsoRecord, RecordId, Saved};
use std::collections::{HashSet, VecDeque};
use std::ops::ControlFlow;
use std::path::PathBuf;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// How long the lane waits before it tries again a write it could not make — the store behind
/// the change, a disk that refused, a file another machine changed.
const RETRY: Duration = Duration::from_millis(1_000);

/// What the lane has done, for the station, a waiting command and the quit.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// How many changes have been handed to the lane — the number the newest one was given.
    pub sent: u64,
    /// How many of them are in the file, in the order they were handed over: every change
    /// numbered up to this is written.
    pub done: u64,
    /// The file changed under the lane — another machine or program wrote it since the lane last
    /// wrote it or the station last took it in — and the lane will not rewrite it until the station
    /// has taken it in ([`LogFileWriter::accept`]).
    pub foreign_write: bool,
    /// That file's stamp.
    pub foreign_stamp: Option<FileStamp>,
    /// The file as the lane last wrote it or the station last took it in: the one the lane may
    /// replace. `None` until one of them has happened, and after a write that could not account
    /// for every byte in the file.
    pub known: Option<FileStamp>,
    /// Why the last write failed, while none has succeeded since.
    pub last_error: Option<String>,
}

impl Status {
    /// Changes handed to the lane that are not in the file yet.
    pub fn pending(&self) -> bool {
        self.done < self.sent
    }

    /// How many.
    pub fn owed(&self) -> u64 {
        self.sent.saturating_sub(self.done)
    }
}

/// A change handed to the lane.
enum Msg {
    Append {
        rev: u64,
        rows: Vec<Arc<QsoRecord>>,
    },
    Rewrite {
        rev: u64,
    },
    Noted {
        rev: u64,
    },
    /// Look again: the station has taken a file in, and a rewrite held for it may go now.
    Wake,
    Flush(mpsc::SyncSender<Status>),
}

/// What the lane owes the file, in order.
enum Job {
    Append { rev: u64, rows: Vec<Arc<QsoRecord>> },
    Rewrite { rev: u64 },
    Noted { rev: u64 },
}

impl Job {
    fn rev(&self) -> u64 {
        match self {
            Job::Append { rev, .. } | Job::Rewrite { rev } | Job::Noted { rev } => *rev,
        }
    }
}

struct Shared {
    status: Mutex<Status>,
    changed: Condvar,
    /// The rows the file holds as this log last accounted for it: those of the lane's last
    /// rewrite and its appends since, or of the file the station last took in. What keeps an
    /// append from writing a row the file already holds, and what the station asks when it takes
    /// the file in ([`LogFileWriter::holds`]).
    file_ids: Mutex<HashSet<RecordId>>,
}

fn lock(shared: &Shared) -> MutexGuard<'_, Status> {
    shared.status.lock().unwrap_or_else(PoisonError::into_inner)
}

fn file_ids(shared: &Shared) -> MutexGuard<'_, HashSet<RecordId>> {
    shared
        .file_ids
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// The lane's thread and the handle onto it.
pub struct LogFileWriter {
    path: PathBuf,
    tx: Option<mpsc::Sender<Msg>>,
    shared: Arc<Shared>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl std::fmt::Debug for LogFileWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LogFileWriter")
            .field("status", &self.status())
            .finish()
    }
}

impl LogFileWriter {
    /// Start the lane keeping `path` from `source`. Nothing in the file is accounted until the
    /// station says what it holds ([`Self::accept`]): the launch, for the file it loaded, or
    /// else the station's first look at it, as 1.13's first recovery read it.
    pub fn start(path: PathBuf, source: Arc<dyn MirrorSource>) -> LogFileWriter {
        LogFileWriter::with_retry(path, source, RETRY)
    }

    /// The lane with its retry interval named — what a test shortens.
    pub fn with_retry(
        path: PathBuf,
        source: Arc<dyn MirrorSource>,
        retry: Duration,
    ) -> LogFileWriter {
        let (tx, rx) = mpsc::channel::<Msg>();
        let shared = Arc::new(Shared {
            status: Mutex::new(Status::default()),
            changed: Condvar::new(),
            file_ids: Mutex::new(HashSet::new()),
        });
        let lane = Arc::clone(&shared);
        let file = path.clone();
        let handle = std::thread::Builder::new()
            .name("nexus-log-file".into())
            .spawn(move || run(file, source, rx, lane, retry))
            .ok();
        LogFileWriter {
            path,
            tx: Some(tx),
            shared,
            handle,
        }
    }

    /// Hand a change over, numbered: the number it is written under. The count moves under the
    /// same lock as the send, so the numbers are the order the lane receives the changes in.
    fn hand_over(&self, msg: Msg) -> u64 {
        let mut st = lock(&self.shared);
        st.sent += 1;
        let n = st.sent;
        if let Some(tx) = &self.tx {
            let _ = tx.send(msg);
        }
        n
    }

    /// The change at `rev` appended `rows` at the end of the log: append them to the file. Never
    /// touches the disk here; returns at once, with the change's number
    /// ([`Self::wait_written`]).
    pub fn append(&self, rev: u64, rows: Vec<Arc<QsoRecord>>) -> u64 {
        self.hand_over(Msg::Append { rev, rows })
    }

    /// The change at `rev` did anything but append: rewrite the file from the store, once the
    /// store holds it. Returns at once, with the change's number.
    pub fn rewrite(&self, rev: u64) -> u64 {
        self.hand_over(Msg::Rewrite { rev })
    }

    /// The change at `rev` writes nothing to the file — its rows came from the file — and is in
    /// the file once every change before it is. Returns at once, with the change's number.
    pub fn noted(&self, rev: u64) -> u64 {
        self.hand_over(Msg::Noted { rev })
    }

    /// The station has taken in the file with `stamp`, whose rows carry `ids`: the lane may
    /// replace it, for as long as the file keeps that stamp, and those are the rows it holds.
    /// Takes effect at once — the station's next look at the file finds it accounted — and wakes
    /// a rewrite held for it.
    ///
    /// Only the file the station read: one that has changed again since is still unaccounted.
    /// The station takes a file in only while the lane is not writing (idle, or holding a rewrite
    /// for exactly this), so nothing of the lane's own can land between its read and this.
    pub fn accept(&self, stamp: FileStamp, ids: HashSet<RecordId>) {
        if file_stamp(&self.path) == Some(stamp) {
            let mut st = lock(&self.shared);
            st.known = Some(stamp);
            if st.foreign_stamp == Some(stamp) {
                st.foreign_write = false;
                st.foreign_stamp = None;
            }
            *file_ids(&self.shared) = ids;
        }
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Wake);
        }
    }

    /// How many changes have been handed over so far: once that many are done, the file holds
    /// every one of them.
    pub fn sent(&self) -> u64 {
        lock(&self.shared).sent
    }

    /// Wait up to `deadline` for the file to hold every change numbered up to `n`, through the
    /// lane's retries — the quit's wait — and say why not when it does not. ⚠️ It waits: never
    /// call it holding a lock.
    pub fn wait_written(&self, n: u64, deadline: Duration) -> Result<(), String> {
        self.wait(n, deadline, false)
    }

    /// [`Self::wait_written`] for a command: it answers as soon as a write the change needs has
    /// failed, as 1.13's append and save reported a failure at once. The lane keeps trying all
    /// the same, and the change is in the file once the disk takes it. ⚠️ It waits: never call
    /// it holding a lock.
    pub fn wait_saved(&self, n: u64, deadline: Duration) -> Result<(), String> {
        self.wait(n, deadline, true)
    }

    fn wait(&self, n: u64, deadline: Duration, fail_fast: bool) -> Result<(), String> {
        super::io_fence::off_engine_lock("a wait for log.adi to hold a change");
        let until = Instant::now() + deadline;
        let mut st = lock(&self.shared);
        loop {
            if st.done >= n {
                return Ok(());
            }
            // The lane works in order, so the write it last failed is one this change waits on.
            if let Some(why) = st.last_error.as_ref().filter(|_| fail_fast) {
                return Err(why.clone());
            }
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(why_not(&st, deadline));
            }
            st = self
                .shared
                .changed
                .wait_timeout(st, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// Write anything owed now, and wait up to `deadline` for it to be in the file — the exit
    /// path, and a quit's wait. The status as it then stands.
    pub fn flush(&self, deadline: Duration) -> Status {
        let (reply, answer) = mpsc::sync_channel(1);
        if let Some(tx) = &self.tx {
            if tx.send(Msg::Flush(reply)).is_ok() {
                if let Ok(s) = answer.recv_timeout(deadline) {
                    return s;
                }
            }
        }
        self.status()
    }

    /// What the lane has done so far. Never waits on the disk.
    pub fn status(&self) -> Status {
        lock(&self.shared).clone()
    }

    /// Whether the file held the row `id` when this log last accounted for it — with the lane's
    /// last rewrite or an append since, or in the file the station last took in. What the
    /// station asks when it takes the file in while the lane still owes it changes: such a row is
    /// this log's own as it stood before those changes, since edited or deleted here, and not
    /// another machine's news.
    pub fn holds(&self, id: RecordId) -> bool {
        file_ids(&self.shared).contains(&id)
    }
}

impl Drop for LogFileWriter {
    /// Dropping the handle closes the channel: the lane makes what it still owes one last try,
    /// and ends. The thread is joined, so a write already in flight finishes rather than being cut
    /// off by the process exiting.
    fn drop(&mut self) {
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// Why the file does not hold a change yet, in words for the operator's diagnostic log.
fn why_not(st: &Status, waited: Duration) -> String {
    if st.foreign_write {
        "log.adi was changed by another program or computer; Nexus writes it again once it has \
         taken that in"
            .to_string()
    } else if let Some(e) = &st.last_error {
        format!("log.adi could not be written: {e}")
    } else {
        format!(
            "log.adi was still being written after {:.1} s",
            waited.as_secs_f32()
        )
    }
}

/// What the lane knows between writes.
struct Lane {
    path: PathBuf,
    shared: Arc<Shared>,
    /// The foreign stamp last reported, so a refusal is logged once per file.
    reported: Option<FileStamp>,
}

impl Lane {
    fn status(&self) -> MutexGuard<'_, Status> {
        lock(&self.shared)
    }

    fn file_ids(&self) -> MutexGuard<'_, HashSet<RecordId>> {
        file_ids(&self.shared)
    }

    fn known(&self) -> Option<FileStamp> {
        self.status().known
    }

    fn set_known(&self, stamp: Option<FileStamp>) {
        self.status().known = stamp;
    }

    /// `k` more changes, the next in order, are in the file.
    fn advance(&self, k: u64) {
        let mut st = self.status();
        st.done += k;
        st.last_error = None;
        drop(st);
        self.shared.changed.notify_all();
    }

    fn fail(&self, why: String) {
        if self.status().last_error.as_deref() != Some(why.as_str()) {
            crate::applog::error("logbook", &why);
        }
        self.status().last_error = Some(why);
        self.shared.changed.notify_all();
    }

    /// The file is not the one the lane may replace: say so, once per file, and write nothing.
    /// Not always another machine: after a write that could not account for every byte, the
    /// lane's own file is unaccounted too until the station has read it.
    fn refuse_foreign(&mut self, now: Option<FileStamp>) {
        {
            let mut st = self.status();
            st.foreign_write = true;
            st.foreign_stamp = now;
        }
        if self.reported != now {
            self.reported = now;
            crate::applog::warn(
                "logbook",
                "log.adi is not the file Nexus last wrote or read (another program or computer \
                 may have written it); Nexus reads it in before it writes the file again",
            );
        }
        self.shared.changed.notify_all();
    }

    /// 1.13's append: every row not already in the file, each written and synced; the file then
    /// accounted only if it was accounted before — or was not there, and this append made it —
    /// and grew by exactly what was written.
    fn append(&mut self, rows: &[Arc<QsoRecord>]) -> bool {
        let fresh: Vec<&Arc<QsoRecord>> = {
            let held = self.file_ids();
            rows.iter()
                .filter(|r| r.id.is_none_or(|id| !held.contains(&id)))
                .collect()
        };
        if fresh.is_empty() {
            self.advance(1);
            return true;
        }
        super::io_fence::on_log_lane("an append to log.adi");
        let before = file_stamp(&self.path);
        // No file: 1.13's append makes it, the header first, and it holds what is written here.
        // 1.13 left such a file unaccounted, and its next recovery read it back; the lane would
        // hold its next rewrite for a look the station makes only between writes.
        let made = before.is_none();
        if made {
            self.file_ids().clear();
        }
        let accountable = made || before == self.known();
        let was = before.map_or(super::adif_header().len() as u64, |(len, _)| len);
        let mut written = 0u64;
        for r in fresh {
            match Logbook::append_for_sync(&self.path, r).and_then(|receipt| receipt.sync()) {
                Ok(()) => {
                    written += adif_record_own_log(r).len() as u64;
                    if let Some(id) = r.id {
                        self.file_ids().insert(id);
                    }
                }
                Err(e) => {
                    // Rows already appended are in `file_ids`, so a retry appends the rest.
                    self.set_known(None);
                    self.fail(format!("log.adi could not be appended to: {e}"));
                    return false;
                }
            }
        }
        let after = file_stamp(&self.path);
        self.set_known(match (accountable, after) {
            (true, Some((now, t))) if now == was + written => Some((now, t)),
            _ => None,
        });
        self.advance(1);
        true
    }

    /// Whether the file is still the one the lane may replace — or there is none to lose.
    fn unchanged(&self) -> bool {
        let now = file_stamp(&self.path);
        now.is_none() || now == self.known()
    }

    /// 1.13's save of the store's rows, once the store holds every change up to `upto` and only
    /// over the file the lane may replace.
    fn rewrite(&mut self, upto: u64, source: &dyn MirrorSource) -> bool {
        if source.ready(upto, READY_WAIT) == Readiness::Behind {
            return false;
        }
        if !self.unchanged() {
            self.refuse_foreign(file_stamp(&self.path));
            return false;
        }
        super::io_fence::on_log_lane("a rewrite of log.adi");
        let mut text = Export::new(ExportKind::Adif {
            from: None,
            to: None,
        });
        let mut ids = HashSet::new();
        if let Err(why) = source.each(&mut |r| {
            text.add(r);
            if let Some(id) = r.id {
                ids.insert(id);
            }
            ControlFlow::Continue(())
        }) {
            self.fail(format!(
                "the logbook could not be read to write log.adi: {why}"
            ));
            return false;
        }
        let body = text.finish();
        // Again, now the text is made: reading a big log takes long enough for another machine
        // to have written meanwhile. And once more the moment before the rename, with the new
        // file written and synced — a big file on a network drive takes a while to write.
        if !self.unchanged() {
            self.refuse_foreign(file_stamp(&self.path));
            return false;
        }
        let still = || {
            #[cfg(test)]
            tests::before_rename(&self.path);
            self.unchanged()
        };
        match Logbook::save_text(&self.path, &body, &still) {
            Ok(Saved::CalledOff) => {
                self.refuse_foreign(file_stamp(&self.path));
                false
            }
            Ok(Saved::Replaced(stamp)) => {
                self.set_known(stamp.map(|(t, len)| (len, t)));
                *self.file_ids() = ids;
                self.reported = None;
                let mut st = self.status();
                st.foreign_write = false;
                st.foreign_stamp = None;
                true
            }
            Err(e) => {
                // Nothing was replaced: a save fails before its rename, and the rename is all or
                // nothing. So the file is still the one the lane may replace, and the lane simply
                // tries again. (1.13 dropped its fingerprint here and read the file back at its
                // next recovery; should the rename have happened after all, the lane's check
                // before its next rewrite finds a file it does not know, and the station reads
                // it.) The rows of both pictures count as held.
                self.file_ids().extend(ids);
                self.fail(format!("log.adi could not be written: {e}"));
                false
            }
        }
    }

    /// Work through what is owed, in order, until it is all done or a write cannot be made now.
    fn work(&mut self, jobs: &mut VecDeque<Job>, source: &dyn MirrorSource) {
        while let Some(job) = jobs.front() {
            match job {
                Job::Noted { .. } => {
                    self.advance(1);
                    jobs.pop_front();
                }
                Job::Append { rows, .. } => {
                    let rows = rows.clone();
                    if !self.append(&rows) {
                        return;
                    }
                    jobs.pop_front();
                }
                Job::Rewrite { .. } => {
                    // One rewrite for every rewrite and note in a row: a picture of the log is
                    // the newest state, not a journal of how it got there.
                    let run = jobs
                        .iter()
                        .take_while(|j| matches!(j, Job::Rewrite { .. } | Job::Noted { .. }))
                        .count();
                    let upto = jobs.iter().take(run).map(Job::rev).max().unwrap_or(0);
                    if !self.rewrite(upto, source) {
                        return;
                    }
                    jobs.drain(..run);
                    self.advance(run as u64);
                }
            }
        }
    }
}

/// One message into the lane's queue. A wake-up queues nothing: taking any message makes the
/// lane look at its work again.
fn take(msg: Msg, jobs: &mut VecDeque<Job>, flushes: &mut Vec<mpsc::SyncSender<Status>>) {
    match msg {
        Msg::Append { rev, rows } => jobs.push_back(Job::Append { rev, rows }),
        Msg::Rewrite { rev } => jobs.push_back(Job::Rewrite { rev }),
        Msg::Noted { rev } => jobs.push_back(Job::Noted { rev }),
        Msg::Wake => {}
        Msg::Flush(reply) => flushes.push(reply),
    }
}

fn run(
    path: PathBuf,
    source: Arc<dyn MirrorSource>,
    rx: mpsc::Receiver<Msg>,
    shared: Arc<Shared>,
    retry: Duration,
) {
    super::io_fence::enter_log_lane();
    let mut lane = Lane {
        path,
        shared,
        reported: None,
    };
    let mut jobs: VecDeque<Job> = VecDeque::new();
    let mut flushes: Vec<mpsc::SyncSender<Status>> = Vec::new();
    loop {
        let first = if jobs.is_empty() {
            match rx.recv() {
                Ok(m) => Some(m),
                Err(_) => return, // the handle went away, and nothing is owed
            }
        } else {
            match rx.recv_timeout(retry) {
                Ok(m) => Some(m),
                Err(RecvTimeoutError::Timeout) => None,
                Err(RecvTimeoutError::Disconnected) => {
                    // The handle went away: one last try at what is owed, then stop.
                    lane.work(&mut jobs, &*source);
                    return;
                }
            }
        };
        if let Some(m) = first {
            take(m, &mut jobs, &mut flushes);
        }
        while let Ok(m) = rx.try_recv() {
            take(m, &mut jobs, &mut flushes);
        }
        lane.work(&mut jobs, &*source);
        let now = lane.status().clone();
        for reply in flushes.drain(..) {
            let _ = reply.send(now.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::mirror::MirrorSource;

    type Hook = Box<dyn Fn() + Send>;

    /// What a test does the moment before a rewrite of `path` renames its new file over the old
    /// — where another machine's write can still land. One per path, so tests running beside
    /// each other never see each other's.
    static BEFORE_RENAME: Mutex<Vec<(PathBuf, Hook)>> = Mutex::new(Vec::new());

    pub(super) fn before_rename(path: &std::path::Path) {
        let hooks = BEFORE_RENAME.lock().unwrap_or_else(PoisonError::into_inner);
        for (p, hook) in hooks.iter() {
            if p == path {
                hook();
            }
        }
    }

    /// A folder of the test's own, gone with the value.
    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            use std::sync::atomic::{AtomicU64, Ordering};
            static N: AtomicU64 = AtomicU64::new(0);
            let p = std::env::temp_dir().join(format!(
                "nexus-logfile-{tag}-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        fn log(&self) -> PathBuf {
            self.0.join("log.adi")
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// `n` contacts as a load settles them, ids and all, from a log of `n` in `d`.
    fn rows(d: &Dir, n: usize) -> Vec<Arc<QsoRecord>> {
        let mut text = super::super::adif_header();
        for i in 0..n {
            text.push_str(&format!(
                "<CALL:{}>{}<BAND:3>20m<MODE:3>FT8<QSO_DATE:8>20260101\
                 <TIME_ON:6>{:02}{:02}{:02}<NOTES:6>note {}<EOR>\n",
                format!("K{i}ABC").len(),
                format_args!("K{i}ABC"),
                i / 3600 % 24,
                i / 60 % 60,
                i % 60,
                i % 10,
            ));
        }
        let path = d.0.join("seed.adi");
        std::fs::write(&path, text).unwrap();
        Logbook::load(&path).records().to_vec()
    }

    /// The log as a test holds it: its rows, and whether the store is behind.
    #[derive(Default)]
    struct Held {
        rows: Mutex<Vec<Arc<QsoRecord>>>,
        behind: std::sync::atomic::AtomicBool,
    }

    impl Held {
        fn set(&self, rows: &[Arc<QsoRecord>]) {
            *self.rows.lock().unwrap() = rows.to_vec();
        }
        fn log(&self) -> Logbook {
            let mut log = Logbook::new();
            for r in self.rows.lock().unwrap().iter() {
                log.add(QsoRecord::clone(r));
            }
            log
        }
    }

    impl MirrorSource for Held {
        fn ready(&self, _: u64, _: Duration) -> Readiness {
            if self.behind.load(std::sync::atomic::Ordering::SeqCst) {
                Readiness::Behind
            } else {
                Readiness::Ready
            }
        }
        fn each(&self, each: &mut dyn FnMut(&QsoRecord) -> ControlFlow<()>) -> Result<(), String> {
            for r in self.rows.lock().unwrap().iter() {
                if each(r).is_break() {
                    break;
                }
            }
            Ok(())
        }
    }

    fn lane(d: &Dir, held: &Arc<Held>) -> LogFileWriter {
        LogFileWriter::with_retry(
            d.log(),
            Arc::clone(held) as Arc<dyn MirrorSource>,
            Duration::from_millis(20),
        )
    }

    /// Wait until the file holds every change up to the `n`th handed over.
    fn written(w: &LogFileWriter, n: u64) {
        w.wait_written(n, Duration::from_secs(30))
            .unwrap_or_else(|e| panic!("change {n}: {e}"));
    }

    fn file(d: &Dir) -> Vec<u8> {
        std::fs::read(d.log()).unwrap()
    }

    /// What 1.13's own save and appends leave in a folder of their own, for the same log.
    fn saved_by_1_13(log: &Logbook) -> Vec<u8> {
        let d = Dir::new("oracle");
        log.save(&d.log()).unwrap();
        file(&d)
    }

    /// ★ THE FILE IS THE FILE 1.13 WROTE. A rewrite is 1.13's save of the same log, byte for
    /// byte, private notes and all; an append after it is 1.13's append; and a rewrite that
    /// already carried an appended contact never writes it twice.
    #[test]
    fn a_rewrite_and_an_append_leave_the_file_1_13_left() {
        let d = Dir::new("bytes");
        let held = Arc::new(Held::default());
        let all = rows(&d, 6);
        held.set(&all[..4]);
        let w = lane(&d, &held);
        w.rewrite(1);
        written(&w, 1);
        assert_eq!(
            file(&d),
            saved_by_1_13(&held.log()),
            "a rewrite is 1.13's save"
        );

        // Two contacts logged: memory first, then carried — each appended.
        held.set(&all[..6]);
        w.append(2, all[4..5].to_vec());
        w.append(3, all[5..6].to_vec());
        written(&w, 3);
        let oracle = {
            let o = Dir::new("oracle-append");
            Logbook::from_store(all[..4].iter().map(|r| QsoRecord::clone(r)).collect())
                .save(&o.log())
                .unwrap();
            for r in &all[4..6] {
                Logbook::append(&o.log(), r).unwrap();
            }
            file(&o)
        };
        assert_eq!(file(&d), oracle, "and an append 1.13's append");

        // A rewrite that reads the store once it already holds a later append: that append is
        // in the file, once.
        let more = rows(&Dir::new("more"), 8);
        let extra = Arc::new(QsoRecord {
            id: more[7].id,
            ..QsoRecord::clone(&more[7])
        });
        let mut now: Vec<Arc<QsoRecord>> = all.clone();
        now.push(Arc::clone(&extra));
        held.set(&now);
        w.rewrite(4);
        w.append(5, vec![extra]);
        written(&w, 5);
        assert_eq!(
            file(&d),
            saved_by_1_13(&held.log()),
            "the append the rewrite already carried is not written again"
        );
    }

    /// ★ A FILE SOMETHING ELSE CHANGED IS NEVER REWRITTEN — the data-loss fix D1-A adds to 1.13's
    /// rules. Another machine appends a contact after the lane's last write; the rewrite that
    /// follows would replace the file with a picture that lacks it. The lane refuses, says so,
    /// and holds the rewrite until the station has taken the file in; then it writes, and the
    /// other machine's contact is in the file.
    #[test]
    fn a_rewrite_is_held_while_the_file_holds_what_another_machine_wrote() {
        let d = Dir::new("foreign");
        let held = Arc::new(Held::default());
        let all = rows(&d, 4);
        held.set(&all[..2]);
        let w = lane(&d, &held);
        w.rewrite(1);
        written(&w, 1);
        let ours = file(&d);

        // The other machine appends, as 1.13's append does.
        Logbook::append(&d.log(), &all[2]).unwrap();
        let theirs = file(&d);
        assert_ne!(theirs, ours, "premise: the file grew");

        // A change here that rewrites: the file is not the lane's, so it is not replaced.
        w.rewrite(2);
        assert!(
            w.wait_written(2, Duration::from_millis(300)).is_err(),
            "the rewrite waits"
        );
        let st = w.status();
        assert!(st.foreign_write, "and says why: {st:?}");
        assert_eq!(
            file(&d),
            theirs,
            "the other machine's contact is still there"
        );

        // The station takes the file in: its rows now hold the other machine's contact.
        held.set(&all[..3]);
        w.accept(
            crate::logbook::mirror::file_stamp(&d.log()).unwrap(),
            HashSet::new(),
        );
        written(&w, 2);
        assert!(!w.status().foreign_write);
        let calls: Vec<String> = Logbook::load(&d.log())
            .records()
            .iter()
            .map(|r| r.call.clone())
            .collect();
        assert_eq!(
            calls,
            ["K0ABC", "K1ABC", "K2ABC"],
            "and the rewrite keeps it"
        );
    }

    /// ★ …NOR IN THE MOMENT BEFORE THE RENAME. Writing a big file to a network drive takes a
    /// while, and another machine can append while it goes: the lane looks once more with its
    /// new file written, the moment before the rename, and calls the rewrite off — the other
    /// machine's contact stays, and nothing of the lane's is left behind.
    #[test]
    fn a_rewrite_is_called_off_when_the_file_changes_while_it_is_written() {
        use std::sync::atomic::{AtomicBool, Ordering::SeqCst};
        let d = Dir::new("foreign-last-moment");
        let held = Arc::new(Held::default());
        let all = rows(&d, 4);
        held.set(&all[..2]);
        let w = lane(&d, &held);
        w.rewrite(1);
        written(&w, 1);

        // The other machine appends while the next rewrite is being written, once.
        let fired = Arc::new(AtomicBool::new(false));
        {
            let (fired, path, theirs) = (Arc::clone(&fired), d.log(), Arc::clone(&all[2]));
            BEFORE_RENAME.lock().unwrap().push((
                d.log(),
                Box::new(move || {
                    if !fired.swap(true, SeqCst) {
                        Logbook::append(&path, &theirs).unwrap();
                    }
                }),
            ));
        }
        w.rewrite(2);
        let called_off = w.wait_written(2, Duration::from_millis(400)).is_err();
        BEFORE_RENAME.lock().unwrap().retain(|(p, _)| *p != d.log());
        assert!(
            fired.load(SeqCst),
            "premise: the other machine wrote in that moment"
        );
        assert!(called_off, "the rewrite is called off, and waits");
        assert!(w.status().foreign_write, "and says why: {:?}", w.status());
        let calls = |d: &Dir| -> Vec<String> {
            Logbook::load(&d.log())
                .records()
                .iter()
                .map(|r| r.call.clone())
                .collect()
        };
        assert_eq!(
            calls(&d),
            ["K0ABC", "K1ABC", "K2ABC"],
            "their contact is still there"
        );
        assert!(
            !d.log()
                .with_extension(format!("adi.{}.tmp", std::process::id()))
                .exists(),
            "and the lane's own new file is gone"
        );

        // The station takes the file in; then the rewrite goes, and keeps it.
        held.set(&all[..3]);
        w.accept(
            crate::logbook::mirror::file_stamp(&d.log()).unwrap(),
            HashSet::new(),
        );
        written(&w, 2);
        assert_eq!(calls(&d), ["K0ABC", "K1ABC", "K2ABC"]);
    }

    /// An accept names a file: once the file has changed again, it accounts for nothing.
    #[test]
    fn an_accept_for_a_file_that_has_changed_since_accounts_for_nothing() {
        let d = Dir::new("stale-accept");
        let held = Arc::new(Held::default());
        let all = rows(&d, 3);
        held.set(&all[..1]);
        let w = lane(&d, &held);
        w.rewrite(1);
        written(&w, 1);
        Logbook::append(&d.log(), &all[1]).unwrap();
        let read = crate::logbook::mirror::file_stamp(&d.log()).unwrap();
        std::thread::sleep(Duration::from_millis(20));
        Logbook::append(&d.log(), &all[2]).unwrap();
        w.accept(read, HashSet::new());
        w.rewrite(2);
        assert!(w.wait_written(2, Duration::from_millis(300)).is_err());
        assert!(
            w.status().foreign_write,
            "the second append is still unaccounted"
        );
    }

    /// An append is made whatever the file holds — appending loses nothing — and leaves the file
    /// accounted only when it grew by exactly what was written. After another machine's append
    /// it is not, and the next rewrite waits for the station.
    #[test]
    fn an_append_over_a_file_another_machine_wrote_is_made_and_leaves_it_unaccounted() {
        let d = Dir::new("append-foreign");
        let held = Arc::new(Held::default());
        let all = rows(&d, 3);
        held.set(&all[..1]);
        let w = lane(&d, &held);
        w.rewrite(1);
        written(&w, 1);
        assert!(
            w.status().known.is_some(),
            "premise: the lane's own write is accounted"
        );
        Logbook::append(&d.log(), &all[1]).unwrap();
        w.append(2, vec![Arc::clone(&all[2])]);
        written(&w, 2);
        let calls: Vec<String> = Logbook::load(&d.log())
            .records()
            .iter()
            .map(|r| r.call.clone())
            .collect();
        assert_eq!(calls, ["K0ABC", "K1ABC", "K2ABC"], "appended after theirs");
        assert_eq!(w.status().known, None, "and the file is not accounted");
    }

    /// A file the lane's own append makes — none was there — holds exactly what it wrote: it is
    /// the lane's to replace, and the rewrite that follows is not held for the station.
    #[test]
    fn a_file_the_lanes_own_append_makes_is_its_own_to_replace() {
        let d = Dir::new("made");
        let held = Arc::new(Held::default());
        let all = rows(&d, 2);
        held.set(&all[..1]);
        let w = lane(&d, &held);
        w.append(1, vec![Arc::clone(&all[0])]);
        written(&w, 1);
        assert!(w.status().known.is_some(), "accounted: {:?}", w.status());
        held.set(&all[..2]);
        w.rewrite(2);
        written(&w, 2);
        assert!(!w.status().foreign_write, "{:?}", w.status());
        assert_eq!(file(&d), saved_by_1_13(&held.log()));
    }

    /// Order: a change is written only once every change before it is; a note is in the file once
    /// what came before it is; and a store behind the change holds the rewrite (and everything
    /// after it) until it has caught up.
    #[test]
    fn changes_are_written_in_the_order_they_were_made() {
        let d = Dir::new("order");
        let held = Arc::new(Held::default());
        let all = rows(&d, 3);
        held.set(&all[..1]);
        held.behind.store(true, std::sync::atomic::Ordering::SeqCst);
        let w = lane(&d, &held);
        w.rewrite(1);
        w.noted(2);
        w.append(3, vec![Arc::clone(&all[1])]);
        assert!(w.wait_written(1, Duration::from_millis(200)).is_err());
        let st = w.status();
        assert_eq!((st.done, st.sent), (0, 3), "nothing yet: {st:?}");
        assert!(st.pending());
        assert!(!d.log().exists(), "not even the append after it");
        held.behind
            .store(false, std::sync::atomic::Ordering::SeqCst);
        written(&w, 3);
        assert!(!w.status().pending());
    }

    /// A change the store refused and is sent again keeps its revision — it is the same change —
    /// and is still one more thing to write: a wait for it is a wait for its own write, not for
    /// whatever already passed that revision.
    #[test]
    fn a_change_sent_again_under_an_old_revision_is_waited_for_as_its_own() {
        let d = Dir::new("resend");
        let held = Arc::new(Held::default());
        held.set(&rows(&d, 2));
        let w = lane(&d, &held);
        let first = w.rewrite(7);
        written(&w, first);
        held.behind.store(true, std::sync::atomic::Ordering::SeqCst);
        let again = w.rewrite(3);
        assert_eq!(again, first + 1, "numbered in the order it was handed over");
        assert!(
            w.wait_written(again, Duration::from_millis(200)).is_err(),
            "not written while the store is behind it"
        );
        assert!(w.status().pending());
        held.behind
            .store(false, std::sync::atomic::Ordering::SeqCst);
        written(&w, again);
    }

    /// ★ A WRITE THE DISK REFUSES IS SAID, WAITED ON AND MADE AGAIN. A waiting command hears why;
    /// the lane tries again, and once the disk takes the file the change is in it.
    #[cfg(unix)]
    #[test]
    fn a_write_the_disk_refuses_is_said_and_made_again_once_it_can_be() {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("refused");
        let held = Arc::new(Held::default());
        let all = rows(&d, 2);
        held.set(&all[..2]);
        let locked = std::fs::Permissions::from_mode(0o500);
        std::fs::set_permissions(&d.0, locked).unwrap();
        let w = lane(&d, &held);
        w.rewrite(1);
        let why = w
            .wait_written(1, Duration::from_millis(300))
            .expect_err("the folder refuses the file");
        assert!(why.contains("could not be written"), "{why}");
        // A command hears it at once, as 1.13's failed save said so at once — not at its deadline.
        let asked = Instant::now();
        let why = w
            .wait_saved(1, Duration::from_secs(30))
            .expect_err("a command is told");
        assert!(
            asked.elapsed() < Duration::from_secs(5),
            "at once: {:?}",
            asked.elapsed()
        );
        assert!(why.contains("could not be written"), "{why}");
        std::fs::set_permissions(&d.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        written(&w, 1);
        assert_eq!(w.status().last_error, None, "and the error is gone");
        assert_eq!(file(&d), saved_by_1_13(&held.log()));
    }

    /// A rewrite the disk refuses over the file the lane knows leaves that file as it was, still
    /// the lane's to replace: the lane makes it again once the disk takes it, with no look from
    /// the station — nothing else wrote the file.
    #[cfg(unix)]
    #[test]
    fn a_rewrite_the_disk_refused_is_made_again_without_the_station() {
        use std::os::unix::fs::PermissionsExt;
        let d = Dir::new("refused-rewrite");
        let held = Arc::new(Held::default());
        let all = rows(&d, 3);
        held.set(&all[..2]);
        let w = lane(&d, &held);
        w.rewrite(1);
        written(&w, 1);
        std::fs::set_permissions(&d.0, std::fs::Permissions::from_mode(0o500)).unwrap();
        held.set(&all[..3]);
        w.rewrite(2);
        let refused = w.wait_saved(2, Duration::from_secs(30));
        std::fs::set_permissions(&d.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        assert!(refused.is_err(), "premise: the folder refused the rewrite");
        written(&w, 2);
        let st = w.status();
        assert!(!st.foreign_write && st.last_error.is_none(), "{st:?}");
        assert_eq!(file(&d), saved_by_1_13(&held.log()));
    }

    /// The lane's writes are its own: the exit waits for them (`flush`), and dropping the handle
    /// makes what is owed one last try.
    #[test]
    fn a_flush_waits_for_what_is_owed_and_the_drop_writes_it() {
        let d = Dir::new("flush");
        let held = Arc::new(Held::default());
        let all = rows(&d, 2);
        held.set(&all[..1]);
        let w = lane(&d, &held);
        w.rewrite(1);
        let st = w.flush(Duration::from_secs(30));
        assert!(!st.pending(), "{st:?}");
        held.set(&all[..2]);
        w.append(2, vec![Arc::clone(&all[1])]);
        drop(w);
        assert_eq!(
            Logbook::load(&d.log()).len(),
            2,
            "the append owed at the drop is in the file"
        );
    }
}
