//! The small journals' own writer thread: the Field Day contest journal, the
//! store-and-forward message queue and the JS8 station's inbox.
//!
//! # Why a thread
//!
//! Each of these journals is rewritten whole on every change — a temporary file, written and
//! fsynced, renamed over the journal, and the folder fsynced after the rename where the
//! platform has a folder handle to sync (unix; Windows has none) — and the changes are made under the Engine lock, often by
//! the radio loop itself: the FT Field Day sequencer journals every contact from inside the slot
//! it logs it in, the message queue is journaled from TX planning and decode handling, and the
//! JS8 inbox from decode handling and the once-a-second JS8 tick. The
//! fsync finishes when the disk says so, and while it ran under the lock the radio loop waited
//! on the disk. Now the caller hands the bytes to this thread — a channel send, no I/O — and the
//! thread writes them.
//!
//! # What does not change
//!
//! - **What is written:** the same bytes, made by the caller under the same lock at the same
//!   moment; the same temporary file; the same write, fsync and rename; the same message when
//!   one fails.
//! - **The order:** one thread takes the writes in the order they were handed over, and they
//!   are all handed over under the Engine lock, so it is the order the Engine made its changes
//!   in. A journal removed and then written again is removed, then written.
//! - **Nothing is dropped.** Dropping the writer waits for everything it was given. A thread that
//!   cannot be started means each write is made on the caller's thread, as it always was. The
//!   exit path waits for the journals ([`JournalWriter::mark`]) before the process goes.
//!
//! # What does change, and who waits
//!
//! A journal reaches the disk a moment after the call rather than before it returns. Callers
//! that need it there before they go on wait for it:
//! - the Field Day rebuild, which re-reads the journal it has just written
//!   ([`JournalWriter::settle`] — under the Engine lock, on a change of mode, never in a radio
//!   tick; it waits for exactly the write the lock used to wait for);
//! - an operator's contest-logging command, which returns only once the contact is on disk, as
//!   it always has ([`JournalMark::wait`] — after the Engine lock is released).
//!
//! The radio loop never waits: an FT Field Day contact's journal lands a moment later. Until then
//! the contact is in the contest log in memory, and every later write of the journal carries it.
//!
//! # A write that fails
//!
//! It is reported on the console, as it always was, and KEPT per journal
//! ([`JournalWriter::trouble`]): how many writes have failed since the last good one, and when
//! that one landed. A later good write clears it — each write carries the whole journal, so it
//! heals every failure before it. The caller reads it to tell the operator; nothing is retried.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};

/// One journal write.
enum Op {
    /// Replace `path` with `bytes`: write `tmp`, fsync it, rename it over `path`, fsync the
    /// folder.
    Replace {
        path: PathBuf,
        tmp: PathBuf,
        bytes: Vec<u8>,
        /// The words a failure is reported with, before the error itself.
        failure: &'static str,
        /// Where to keep rolling copies once the journal is saved ([`keep_copies`]), if anywhere.
        copies: Option<PathBuf>,
    },
    /// Remove `path` — an empty journal.
    Remove { path: PathBuf },
}

/// How many writes were handed over, and how many are done.
#[derive(Default)]
struct Counts {
    submitted: u64,
    completed: u64,
}

/// The writes to one journal that failed since its last good one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JournalTrouble {
    /// Writes that failed in a row, the newest included.
    pub failed: u32,
    /// When the last good write landed (Unix UTC seconds), or `None` when none has this run.
    pub last_good_unix: Option<u64>,
}

#[derive(Default)]
struct Shared {
    counts: Mutex<Counts>,
    done: Condvar,
    /// Each journal's last outcome: `failed` 0 after a good write.
    outcomes: Mutex<HashMap<PathBuf, JournalTrouble>>,
}

impl Shared {
    /// Run `op` and keep how it went.
    fn run(&self, op: Op) {
        let (path, ok) = apply(op);
        let mut all = lock(&self.outcomes);
        let o = all.entry(path).or_insert(JournalTrouble {
            failed: 0,
            last_good_unix: None,
        });
        if ok {
            o.failed = 0;
            o.last_good_unix = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .ok()
                .map(|d| d.as_secs());
        } else {
            o.failed = o.failed.saturating_add(1);
        }
    }

    /// Wait until the write numbered `upto` — and so every write before it — is done.
    fn wait_for(&self, upto: u64) {
        let mut c = lock(&self.counts);
        while c.completed < upto {
            c = self.done.wait(c).unwrap_or_else(PoisonError::into_inner);
        }
    }
}

/// The thread and the way to it.
struct Lane {
    /// `None` when the thread could not be started: writes are then made where they are asked.
    tx: Option<Sender<Op>>,
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Lane {
    fn start() -> Lane {
        let (tx, rx) = channel::<Op>();
        let shared = Arc::new(Shared::default());
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("nexus-journal".into())
                .spawn(move || {
                    crate::logbook::io_fence::enter_log_lane();
                    // Ends when the writer is dropped — after every write it was given.
                    for op in rx {
                        crate::logbook::io_fence::on_log_lane("a journal write");
                        // A write that panicked must not take the writes behind it with it, nor
                        // leave a waiter waiting for a count that will never move.
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            shared.run(op);
                        }));
                        lock(&shared.counts).completed += 1;
                        shared.done.notify_all();
                    }
                })
                .ok()
        };
        Lane {
            tx: thread.is_some().then_some(tx),
            shared,
            thread,
        }
    }
}

impl Drop for Lane {
    fn drop(&mut self) {
        drop(self.tx.take());
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

/// The journals' writer. No thread exists until the first write.
#[derive(Default)]
pub struct JournalWriter {
    lane: OnceLock<Lane>,
}

impl JournalWriter {
    /// Replace the journal at `path` with `bytes` — `tmp` written and fsynced, then renamed over
    /// it; `failure` and the error reported if any of that fails. Returns at once.
    pub fn replace(&self, path: &Path, tmp: PathBuf, bytes: Vec<u8>, failure: &'static str) {
        self.submit(Op::Replace {
            path: path.to_path_buf(),
            tmp,
            bytes,
            failure,
            copies: None,
        });
    }

    /// [`Self::replace`], and once the journal is saved, a copy of it kept in the folder `copies`
    /// ([`keep_copies`]). A copy that fails is reported in the diagnostic log and changes
    /// nothing else: the journal's own outcome is its save's alone.
    pub fn replace_keeping_copies(
        &self,
        path: &Path,
        tmp: PathBuf,
        bytes: Vec<u8>,
        failure: &'static str,
        copies: PathBuf,
    ) {
        self.submit(Op::Replace {
            path: path.to_path_buf(),
            tmp,
            bytes,
            failure,
            copies: Some(copies),
        });
    }

    /// Remove the journal at `path`. Returns at once.
    pub fn remove(&self, path: &Path) {
        self.submit(Op::Remove {
            path: path.to_path_buf(),
        });
    }

    fn submit(&self, op: Op) {
        let lane = self.lane.get_or_init(Lane::start);
        let Some(tx) = &lane.tx else {
            lane.shared.run(op);
            return;
        };
        let mut c = lock(&lane.shared.counts);
        c.submitted += 1;
        if let Err(returned) = tx.send(op) {
            // The thread is gone: write here rather than lose it.
            c.submitted -= 1;
            drop(c);
            lane.shared.run(returned.0);
        }
    }

    /// Wait until every write handed over so far is done — for a caller that reads a journal
    /// back. Returns at once when there is nothing on its way.
    pub fn settle(&self) {
        self.mark().wait_for_it();
    }

    /// The writes to the journal at `path` that failed since its last good one — `None` when its
    /// last write was good, or none has been made. Writes still on their way are not counted.
    pub fn trouble(&self, path: &Path) -> Option<JournalTrouble> {
        let lane = self.lane.get()?;
        let all = lock(&lane.shared.outcomes);
        all.get(path).copied().filter(|o| o.failed > 0)
    }

    /// The writes handed over so far: [`JournalMark::wait`] returns once they are all done.
    pub fn mark(&self) -> JournalMark {
        match self.lane.get() {
            Some(lane) => JournalMark {
                upto: lock(&lane.shared.counts).submitted,
                shared: Some(Arc::clone(&lane.shared)),
            },
            None => JournalMark {
                upto: 0,
                shared: None,
            },
        }
    }
}

/// A point in the journals' writes, to wait for once every lock is released.
#[must_use = "a mark does nothing until it is waited for"]
pub struct JournalMark {
    shared: Option<Arc<Shared>>,
    upto: u64,
}

impl JournalMark {
    /// Wait until every write before the mark is done.
    ///
    /// ⚠️ **Never holding the Engine lock** — the fence stops it in a debug build. The one
    /// caller that must wait under the lock, the Field Day rebuild, uses
    /// [`JournalWriter::settle`].
    pub fn wait(&self) {
        crate::logbook::io_fence::off_engine_lock("a wait for a journal write");
        self.wait_for_it();
    }

    fn wait_for_it(&self) {
        if let Some(shared) = &self.shared {
            shared.wait_for(self.upto);
        }
    }
}

/// One write, exactly as the journal's caller made it inline before this thread existed. Returns
/// the journal and whether the write was made.
fn apply(op: Op) -> (PathBuf, bool) {
    match op {
        Op::Replace {
            path,
            tmp,
            bytes,
            failure,
            copies,
        } => {
            if let Some(dir) = path.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            let res = std::fs::File::create(&tmp)
                .and_then(|mut f| {
                    std::io::Write::write_all(&mut f, &bytes)?;
                    f.sync_all() // on disk BEFORE the rename publishes it
                })
                .and_then(|()| std::fs::rename(&tmp, &path));
            match &res {
                // …and the folder, so the rename itself survives a power cut: without it the
                // machine can come back with the journal from before this write.
                Ok(()) => crate::logbook::sync_parent_dir(&path),
                Err(e) => eprintln!("{failure}: {e}"),
            }
            // Only a saved journal is copied, and the copy's outcome is not the journal's.
            if let (Ok(()), Some(dir)) = (&res, &copies) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map_or(0, |d| d.as_secs());
                if let Err(e) = keep_copies(dir, &path, &bytes, now) {
                    eprintln!("tempo: journal copy failed: {e}");
                    crate::applog::warn(
                        "journal_copy_failed",
                        &format!("{} -> {}: {e}", path.display(), dir.display()),
                    );
                }
            }
            (path, res.is_ok())
        }
        Op::Remove { path } => {
            let res = std::fs::remove_file(&path);
            let gone = match &res {
                Ok(()) => true,
                Err(e) => e.kind() == std::io::ErrorKind::NotFound,
            };
            (path, gone)
        }
    }
}

/// How many of a journal's latest saves [`keep_copies`] keeps.
pub const SAVES_KEPT: usize = 10;
/// How many of a journal's hourly copies [`keep_copies`] keeps: at most one per clock hour, so
/// at least the last 48 hours the journal was saved in.
pub const HOURS_KEPT: usize = 48;

/// Keep copies of the journal `journal`, just saved as `bytes`, in the folder `dir`, each
/// written to a temporary file, fsynced and renamed into place:
/// - `<stem>.save-NNNNNN.<ext>`, numbered on from the highest there, the newest
///   [`SAVES_KEPT`] kept;
/// - `<stem>.hour-YYYYMMDD-HH.<ext>` (UTC hour of `now_unix`), made by the hour's first save and
///   replaced by its later ones, the newest [`HOURS_KEPT`] kept.
///
/// Only this journal's own copies are counted or removed; the journal itself is never touched,
/// and nothing reads a copy back. Restoring from one is the operator's act.
pub fn keep_copies(dir: &Path, journal: &Path, bytes: &[u8], now_unix: u64) -> std::io::Result<()> {
    let stem = journal
        .file_stem()
        .ok_or_else(|| std::io::Error::other("a journal with no file name"))?
        .to_string_lossy();
    let ext = journal
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    std::fs::create_dir_all(dir)?;
    let save = format!("{stem}.save-");
    let hour = format!("{stem}.hour-");
    let mut saves: Vec<(u64, PathBuf)> = named(dir, &save, &ext)?
        .into_iter()
        .filter_map(|(mid, p)| Some((mid.parse().ok()?, p)))
        .collect();
    let next = saves.iter().map(|(n, _)| n + 1).max().unwrap_or(1);
    let (y, mo, d, h, _, _) = crate::logbook::datetime_utc(now_unix);
    let tmp = dir.join(format!("{stem}.copy.tmp"));
    let newest = dir.join(format!("{save}{next:06}{ext}"));
    publish(&tmp, &newest, bytes)?;
    saves.push((next, newest));
    publish(
        &tmp,
        &dir.join(format!("{hour}{y:04}{mo:02}{d:02}-{h:02}{ext}")),
        bytes,
    )?;
    saves.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    for (_, old) in saves.iter().skip(SAVES_KEPT) {
        std::fs::remove_file(old)?;
    }
    let mut hours = named(dir, &hour, &ext)?;
    hours.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, old) in hours.iter().skip(HOURS_KEPT) {
        std::fs::remove_file(old)?;
    }
    Ok(())
}

/// The files in `dir` named `<prefix><middle><suffix>`, with their middles.
fn named(dir: &Path, prefix: &str, suffix: &str) -> std::io::Result<Vec<(String, PathBuf)>> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if let Some(mid) = name
            .strip_prefix(prefix)
            .and_then(|rest| rest.strip_suffix(suffix))
        {
            found.push((mid.to_string(), entry.path()));
        }
    }
    Ok(found)
}

/// Write `bytes` to `tmp`, fsync it, and rename it over `to`.
fn publish(tmp: &Path, to: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let mut f = std::fs::File::create(tmp)?;
    std::io::Write::write_all(&mut f, bytes)?;
    f.sync_all()?;
    drop(f);
    std::fs::rename(tmp, to)
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            let p = std::env::temp_dir().join(format!(
                "nexus-journal-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn write(w: &JournalWriter, path: &Path, text: &str) {
        w.replace(
            path,
            path.with_extension("json.tmp"),
            text.as_bytes().to_vec(),
            "test journal",
        );
    }

    /// A writer that is never asked starts no thread — an Engine that journals nothing costs
    /// nothing — and has nothing to wait for.
    #[test]
    fn a_writer_never_asked_starts_nothing_and_waits_for_nothing() {
        let w = JournalWriter::default();
        w.settle();
        w.mark().wait();
        assert!(w.lane.get().is_none());
    }

    /// ★ THE ORDER IS THE ORDER THEY WERE HANDED OVER, a removal included — with the thread
    /// held up behind a first write so every later one is queued at once. Each journal ends as
    /// its LAST write left it: a write reordered past a later one would leave its own text, and
    /// the removal reordered after the last write would leave no file.
    #[cfg(unix)]
    #[test]
    fn writes_land_in_the_order_they_were_handed_over() {
        let d = Dir::new("order");
        // The first write's temporary file is a FIFO: opening it for writing blocks until a
        // reader opens it, so the thread is held on that open with the rest queued behind it.
        let stall = d.0.join("stall.json");
        let fifo = stall.with_extension("json.tmp");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success(), "premise: a FIFO to hold the thread on");
        let w = JournalWriter::default();
        write(&w, &stall, "held");
        let (a, b) = (d.0.join("a.json"), d.0.join("b.json"));
        write(&w, &a, "a1");
        write(&w, &b, "b1");
        write(&w, &a, "a2");
        w.remove(&b);
        write(&w, &a, "a3");
        write(&w, &b, "b2");
        w.remove(&a);
        write(&w, &a, "a4");
        let queued = w.mark();
        std::thread::sleep(Duration::from_millis(100));
        assert!(
            !a.exists() && !b.exists(),
            "control: the thread really was held — nothing behind the stall is written yet"
        );
        // Let the first write through: read the FIFO to its end (the write then fails at its
        // fsync — a FIFO cannot be synced — and is reported, as any failed write is).
        let mut reader = std::fs::File::open(&fifo).expect("open the FIFO for reading");
        let mut sink = Vec::new();
        std::io::Read::read_to_end(&mut reader, &mut sink).expect("drain");
        queued.wait();
        assert_eq!(sink, b"held", "the stalled write carried its own bytes");
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "a4");
        assert_eq!(std::fs::read_to_string(&b).unwrap(), "b2");
    }

    /// ★ NOTHING IS DROPPED: the writer's drop waits for every write it was given, however
    /// many are queued.
    #[test]
    fn dropping_the_writer_writes_everything_it_was_given() {
        let d = Dir::new("drop");
        let path = d.0.join("j.json");
        let w = JournalWriter::default();
        for i in 0..200 {
            write(&w, &path, &format!("state {i}"));
        }
        drop(w);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "state 199");
    }

    /// A wait for the journals under an Engine guard is the wait the thread exists to remove:
    /// the fence stops it in a debug build.
    #[cfg(debug_assertions)]
    #[test]
    #[should_panic(expected = "io_fence: a wait for a journal write ran while this thread holds")]
    fn a_journal_wait_under_an_engine_guard_trips_the_fence() {
        let d = Dir::new("fence");
        let w = JournalWriter::default();
        write(&w, &d.0.join("j.json"), "x");
        let _held = crate::logbook::io_fence::EngineHeld::acquired();
        w.mark().wait();
    }

    /// ★ A FAILED WRITE IS KEPT UNTIL A GOOD ONE: how many failed in a row and when the last good
    /// one landed, per journal. The write is made to fail by a folder where its temporary file goes.
    #[test]
    fn a_failed_write_is_kept_until_a_good_one_clears_it() {
        let d = Dir::new("trouble");
        let path = d.0.join("fieldday.adi");
        let tmp = path.with_extension("adi.tmp");
        let w = JournalWriter::default();
        let put = |text: &str| {
            w.replace(&path, tmp.clone(), text.as_bytes().to_vec(), "test journal");
            w.settle();
        };
        let now = || {
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs()
        };

        std::fs::create_dir(&tmp).unwrap();
        put("one");
        assert!(!path.exists(), "control: the first write really failed");
        assert_eq!(
            w.trouble(&path),
            Some(JournalTrouble {
                failed: 1,
                last_good_unix: None
            }),
            "a failed write with no good one before it this run"
        );
        std::fs::remove_dir(&tmp).unwrap();
        let before = now();
        put("two");
        let after = now();
        assert_eq!(w.trouble(&path), None, "a good write clears it");

        std::fs::create_dir(&tmp).unwrap();
        put("three");
        put("four");
        let t = w.trouble(&path).expect("the failures are kept");
        assert_eq!(t.failed, 2, "two in a row");
        let good = t
            .last_good_unix
            .expect("the good write before them is kept");
        assert!(
            (before..=after).contains(&good),
            "when the good write landed: {good} in {before}..={after}"
        );
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "two",
            "control: the journal is the last good write's"
        );
        assert_eq!(w.trouble(&d.0.join("other.json")), None, "kept per journal");
        std::fs::remove_dir(&tmp).unwrap();
        put("five");
        assert_eq!(w.trouble(&path), None, "and the next good write clears it");
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "five");
    }

    /// ⭐ **The folder is synced after the rename, once per journal written**, so a power cut
    /// cannot bring back the journal from before a write the operator saw land. The tmp's own
    /// fsync makes its CONTENT durable; the rename that publishes it is the folder's. Seen
    /// through [`crate::logbook::dir_sync_seam`], which records the request on every platform
    /// (only a unix build has a folder handle to sync). A removal syncs nothing here.
    #[test]
    fn each_write_syncs_the_journals_folder_after_the_rename() {
        use crate::logbook::dir_sync_seam::count;
        let d = Dir::new("dirsync");
        let a = d.0.join("fieldday.adi");
        let b = d.0.join("queue.json");
        let w = JournalWriter::default();
        write(&w, &a, "one");
        write(&w, &a, "two");
        write(&w, &b, "q");
        w.settle();
        assert_eq!(count(&a), 2, "one folder sync per write of the journal");
        assert_eq!(count(&b), 1, "and one for the other journal's write");
        w.remove(&b);
        w.settle();
        assert_eq!(count(&b), 1, "CONTROL: a removal asks for none");
    }

    /// The write itself is the one the callers used to make: the same temporary file, left
    /// behind by nothing, and a failure reported rather than raised.
    #[test]
    fn a_replace_leaves_only_the_journal_and_a_failure_is_reported() {
        let d = Dir::new("replace");
        let path = d.0.join("sub").join("fieldday.adi");
        let w = JournalWriter::default();
        w.replace(
            &path,
            path.with_extension("adi.tmp"),
            b"<EOH>".to_vec(),
            "test journal",
        );
        w.settle();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"<EOH>",
            "the folder is made"
        );
        assert!(
            !path.with_extension("adi.tmp").exists(),
            "the tmp is renamed"
        );

        // A journal whose folder is a FILE cannot be written: reported, and the lane goes on.
        let blocked = d.0.join("blocked");
        std::fs::write(&blocked, "a file where a folder should be").unwrap();
        let bad = blocked.join("x.adi");
        w.replace(&bad, bad.with_extension("adi.tmp"), b"x".to_vec(), "test");
        write(&w, &d.0.join("after.json"), "after");
        w.settle();
        assert!(!bad.exists());
        assert_eq!(
            std::fs::read_to_string(d.0.join("after.json")).unwrap(),
            "after",
            "a failed write does not stop the ones behind it"
        );
    }

    /// The copies of `journal` in `dir`: (file name, contents), sorted by name.
    fn copies(dir: &Path) -> Vec<(String, String)> {
        let mut all: Vec<(String, String)> = std::fs::read_dir(dir)
            .unwrap()
            .map(|e| {
                let p = e.unwrap().path();
                (
                    p.file_name().unwrap().to_string_lossy().into_owned(),
                    std::fs::read_to_string(&p).unwrap(),
                )
            })
            .collect();
        all.sort();
        all
    }

    /// 2026-10-17 21:30:00 UTC.
    const SAT_2130Z: u64 = 1_792_272_600;

    /// A saved journal leaves its copies beside it: the newest save and the hour's copy hold
    /// exactly what the journal holds.
    #[test]
    fn a_saved_journal_is_copied_to_its_history_folder() {
        let d = Dir::new("copies-saved");
        let path = d.0.join("fieldday_backup_7.adi");
        let history = d.0.join("fieldday_history");
        let w = JournalWriter::default();
        for text in ["<CALL:4>K1AB<EOR>", "<CALL:4>K1AB<EOR><CALL:4>W9XY<EOR>"] {
            w.replace_keeping_copies(
                &path,
                path.with_extension("adi.tmp"),
                text.as_bytes().to_vec(),
                "test journal",
                history.clone(),
            );
        }
        w.settle();
        let journal = std::fs::read_to_string(&path).unwrap();
        assert_eq!(journal, "<CALL:4>K1AB<EOR><CALL:4>W9XY<EOR>");
        let names: Vec<String> = copies(&history).into_iter().map(|(n, _)| n).collect();
        let hour = names
            .iter()
            .find(|n| n.starts_with("fieldday_backup_7.hour-"))
            .expect("the hour's copy");
        assert_eq!(
            std::fs::read_to_string(history.join(hour)).unwrap(),
            journal
        );
        assert_eq!(
            std::fs::read_to_string(history.join("fieldday_backup_7.save-000002.adi")).unwrap(),
            journal,
            "the newest save is the journal"
        );
        assert_eq!(
            std::fs::read_to_string(history.join("fieldday_backup_7.save-000001.adi")).unwrap(),
            "<CALL:4>K1AB<EOR>",
            "the one before is the save before"
        );
        assert_eq!(names.len(), 3, "two saves and one hour: {names:?}");
    }

    /// The rotation keeps exactly the newest [`SAVES_KEPT`] saves, and never touches another
    /// journal's copies.
    #[test]
    fn the_rotation_keeps_exactly_the_newest_ten_saves() {
        let d = Dir::new("copies-rotation");
        let journal = d.0.join("fieldday_backup_7.adi");
        let history = d.0.join("fieldday_history");
        std::fs::create_dir_all(&history).unwrap();
        let other = history.join("fieldday_backup_71.save-000001.adi");
        std::fs::write(&other, "another position").unwrap();
        for n in 1..=13 {
            keep_copies(
                &history,
                &journal,
                format!("save {n}").as_bytes(),
                SAT_2130Z,
            )
            .unwrap();
        }
        let saves: Vec<(String, String)> = copies(&history)
            .into_iter()
            .filter(|(n, _)| n.starts_with("fieldday_backup_7.save-"))
            .collect();
        let want: Vec<(String, String)> = (4..=13)
            .map(|n| {
                (
                    format!("fieldday_backup_7.save-{n:06}.adi"),
                    format!("save {n}"),
                )
            })
            .collect();
        assert_eq!(saves, want);
        assert_eq!(saves.len(), SAVES_KEPT);
        assert_eq!(
            std::fs::read_to_string(&other).unwrap(),
            "another position",
            "another journal's copy is untouched"
        );
    }

    /// The hour's first save makes its copy and later saves in that hour replace it; a new hour
    /// makes a new one; the newest [`HOURS_KEPT`] are kept.
    #[test]
    fn one_copy_per_hour_kept_for_the_last_48() {
        let d = Dir::new("copies-hourly");
        let journal = d.0.join("fieldday_backup_7.adi");
        let history = d.0.join("fieldday_history");
        let hours = |h: &Path| -> Vec<(String, String)> {
            copies(h)
                .into_iter()
                .filter(|(n, _)| n.starts_with("fieldday_backup_7.hour-"))
                .collect()
        };
        keep_copies(&history, &journal, b"first", SAT_2130Z).unwrap();
        keep_copies(&history, &journal, b"second", SAT_2130Z + 1700).unwrap();
        assert_eq!(
            hours(&history),
            [(
                "fieldday_backup_7.hour-20261017-21.adi".to_string(),
                "second".to_string()
            )],
            "one copy for the hour, holding its latest save"
        );
        keep_copies(&history, &journal, b"third", SAT_2130Z + 1800).unwrap();
        assert_eq!(
            hours(&history),
            [
                (
                    "fieldday_backup_7.hour-20261017-21.adi".to_string(),
                    "second".to_string()
                ),
                (
                    "fieldday_backup_7.hour-20261017-22.adi".to_string(),
                    "third".to_string()
                ),
            ]
        );
        for h in 2..=50u64 {
            keep_copies(&history, &journal, b"later", SAT_2130Z + h * 3600).unwrap();
        }
        let kept = hours(&history);
        assert_eq!(kept.len(), HOURS_KEPT);
        assert_eq!(
            kept.first().unwrap().0,
            "fieldday_backup_7.hour-20261018-00.adi",
            "the oldest kept is 47 hours before the newest"
        );
        assert_eq!(
            kept.last().unwrap().0,
            "fieldday_backup_7.hour-20261019-23.adi"
        );
    }

    /// A history folder that cannot be made fails the copy and nothing else: the journal is
    /// saved, and its outcome is a good write.
    #[test]
    fn a_copy_that_fails_leaves_the_journal_saved() {
        let d = Dir::new("copies-fail");
        let path = d.0.join("fieldday_backup_7.adi");
        let history = d.0.join("fieldday_history");
        std::fs::write(&history, "a file where the folder goes").unwrap();
        let w = JournalWriter::default();
        w.replace_keeping_copies(
            &path,
            path.with_extension("adi.tmp"),
            b"<CALL:4>K1AB<EOR>".to_vec(),
            "test journal",
            history.clone(),
        );
        w.settle();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "<CALL:4>K1AB<EOR>");
        assert_eq!(w.trouble(&path), None, "the journal's save is good");
        assert!(
            keep_copies(&history, &path, b"x", SAT_2130Z).is_err(),
            "premise: the copy itself fails"
        );
    }

    /// A journal whose save fails is not copied: a copy is only ever of a saved journal.
    #[test]
    fn a_journal_that_was_not_saved_is_not_copied() {
        let d = Dir::new("copies-unsaved");
        let path = d.0.join("fieldday_backup_7.adi");
        std::fs::create_dir_all(&path).unwrap(); // a folder where the journal goes
        let history = d.0.join("fieldday_history");
        let w = JournalWriter::default();
        w.replace_keeping_copies(
            &path,
            path.with_extension("adi.tmp"),
            b"<CALL:4>K1AB<EOR>".to_vec(),
            "test journal",
            history.clone(),
        );
        w.settle();
        assert!(w.trouble(&path).is_some(), "premise: the save failed");
        assert!(!history.exists(), "nothing was copied");
    }
}
