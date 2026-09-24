//! The small journals' own writer thread: the Field Day contest journal and the
//! store-and-forward message queue.
//!
//! # Why a thread
//!
//! Each of these journals is rewritten whole on every change — a temporary file, written and
//! fsynced, renamed over the journal — and the changes are made under the Engine lock, often by
//! the radio loop itself: the FT Field Day sequencer journals every contact from inside the slot
//! it logs it in, and the message queue is journaled from TX planning and decode handling. The
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

use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};

/// One journal write.
enum Op {
    /// Replace `path` with `bytes`: write `tmp`, fsync it, rename it over `path`.
    Replace {
        path: PathBuf,
        tmp: PathBuf,
        bytes: Vec<u8>,
        /// The words a failure is reported with, before the error itself.
        failure: &'static str,
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

#[derive(Default)]
struct Shared {
    counts: Mutex<Counts>,
    done: Condvar,
}

impl Shared {
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
                            apply(op);
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
            apply(op);
            return;
        };
        let mut c = lock(&lane.shared.counts);
        c.submitted += 1;
        if let Err(returned) = tx.send(op) {
            // The thread is gone: write here rather than lose it.
            c.submitted -= 1;
            drop(c);
            apply(returned.0);
        }
    }

    /// Wait until every write handed over so far is done — for a caller that reads a journal
    /// back. Returns at once when there is nothing on its way.
    pub fn settle(&self) {
        self.mark().wait_for_it();
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

/// One write, exactly as the journal's caller made it inline before this thread existed.
fn apply(op: Op) {
    match op {
        Op::Replace {
            path,
            tmp,
            bytes,
            failure,
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
            if let Err(e) = res {
                eprintln!("{failure}: {e}");
            }
        }
        Op::Remove { path } => {
            let _ = std::fs::remove_file(&path);
        }
    }
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
}
