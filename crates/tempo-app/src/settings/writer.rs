//! `settings.json`'s own writer: a thread that writes the newest snapshot handed to it, so a change
//! to a setting no longer holds the radio loop while the file is written.
//!
//! # Why a thread
//!
//! About sixty commands change a setting and then rewrite the whole file ([`Settings::save`]: a
//! temporary file written and fsynced, renamed over `settings.json`, the folder fsynced), and they
//! did it holding the Engine lock, which the radio loop takes on every 20 ms tick. So the loop
//! waited on the disk each time a setting changed: a few milliseconds on an SSD, far longer on a
//! slow disk or behind an antivirus scan. Now the command hands the writer a snapshot (a clone, no
//! I/O) and lets go of the lock; the writer's thread writes it.
//!
//! # What does not change
//!
//! - **The change itself.** The caller makes it under the lock, exactly where it always did, so
//!   everything that reads the setting (the transmit gates included) sees it at the same moment.
//! - **What is written, and how.** The thread calls [`Settings::save`]: the same bytes, the same
//!   temporary file, the same fsyncs and rename.
//! - **The order.** A snapshot is numbered as it is handed over, and it is handed over under the
//!   Engine lock, so the numbers follow the order the Engine made its changes in.
//!
//! # The newest wins
//!
//! Each snapshot is the whole configuration, so a newer one supersedes an older one still waiting:
//! the thread writes only the newest it holds, and an older snapshot never lands over a newer
//! file. A slider that sends forty changes while one write is on the disk costs one more write,
//! not forty.
//!
//! # One writer per file, one write at a time
//!
//! [`SettingsWriter::of`] hands every caller in the process the same writer for the same file:
//! the commands' snapshots, Remote's saves and the base profile's mirrors meet there. Every write
//! of the file, by the thread or by a caller, holds the writer's file lock, because
//! [`Settings::tmp_path`] is per PROCESS: two writes of one file at once in this process would
//! share one temporary file and could publish a mixture of both, invalid JSON that the next launch
//! reads as defaults (`license_class` back to `Open`, the Part 97 lockout gone).
//!
//! # Who still waits
//!
//! - **A save whose result decides the change**, Remote's save-then-publish: it stays synchronous
//!   ([`SettingsWriter::save_now`]), under the Engine lock as before. Being newer than anything
//!   waiting, a successful one supersedes it.
//! - **A read-modify-write of the file**, the base profile's mirrors: [`SettingsWriter::rewrite`]
//!   writes whatever is waiting first, then reads.
//! - **A command whose answer is the save's result**: [`Ticket::wait`], once the lock is released.
//! - **A quit, a restart, a relaunch and the Windows installer**: [`SettingsWriter::flush`], so
//!   the last change reaches the disk before the process goes.
//!
//! A snapshot still on its way when the process dies without any of those (a crash, a power cut)
//! is lost, and the file holds the one before it: [`Settings::save`]'s rename keeps it whole.

use super::Settings;
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// How a snapshot reaches the disk: [`Settings::save`] in the app, a stand-in in the tests.
type Write = Arc<dyn Fn(&Settings, &Path) -> io::Result<()> + Send + Sync>;

/// What a caller says when the write carrying its change fails.
type OnFailure = Box<dyn FnOnce(&io::Error) + Send>;

/// The newest snapshot handed over and not yet written.
struct Pending {
    seq: u64,
    snapshot: Settings,
    /// Every caller whose change this snapshot carries, the superseded ones' included.
    on_failure: Vec<OnFailure>,
}

#[derive(Default)]
struct State {
    /// The last number handed out, to a queued snapshot or a synchronous save.
    issued: u64,
    /// The last number handed to a queued snapshot: what a flush waits for.
    queued: u64,
    pending: Option<Pending>,
    /// Every snapshot numbered up to here has been written, or failed with its callers told, or
    /// been superseded by a save that succeeded.
    done: u64,
    /// …and every one numbered up to here is on the disk.
    saved: u64,
    /// How the newest failed write failed, and its number.
    error: Option<(u64, io::ErrorKind, String)>,
    /// The writer is going: the thread writes what is waiting, then ends.
    closing: bool,
}

struct Shared {
    path: PathBuf,
    write: Write,
    state: Mutex<State>,
    /// The thread waits here for a snapshot.
    work: Condvar,
    /// A flush and a ticket wait here for the writes.
    written: Condvar,
    /// Held across every write of the file; see the module header.
    file: Mutex<()>,
}

/// What a flush found when it returned.
#[derive(Debug, PartialEq, Eq)]
pub enum Flushed {
    /// Every snapshot handed over before the flush is on the disk.
    Saved,
    /// The last write before the flush failed: its callers have been told, and the file keeps
    /// the snapshot before it. The failure, in words.
    Failed(String),
    /// Time ran out with a write still on its way.
    OutOfTime,
}

/// A write of the waiting snapshot, made and not yet recorded ([`Shared::finish`]).
struct Written {
    seq: u64,
    result: io::Result<()>,
    on_failure: Vec<OnFailure>,
}

impl Shared {
    /// Write the waiting snapshot, if there is one. The caller holds the file lock, and hands the
    /// write to [`Shared::finish`] once it has let go of it.
    fn write_pending(&self) -> Option<Written> {
        let pending = lock(&self.state).pending.take()?;
        // A panic in the write is a failed write, never a thread that dies with waiters waiting.
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            (self.write)(&pending.snapshot, &self.path)
        }))
        .unwrap_or_else(|_| Err(io::Error::other("the settings write panicked")));
        Some(Written {
            seq: pending.seq,
            result,
            on_failure: pending.on_failure,
        })
    }

    /// Record a write as done, with no lock held. A failed one tells every caller it carried
    /// FIRST, so a flush or a ticket that has returned has seen the failure reported, and a quit's
    /// last report goes out before the process does.
    fn finish(&self, written: Option<Written>) {
        let Some(Written {
            seq,
            result,
            on_failure,
        }) = written
        else {
            return;
        };
        if let Err(e) = &result {
            for tell in on_failure {
                // A report that panics must not take the writes behind it, or a waiter, with it.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tell(e)));
            }
        }
        let mut st = lock(&self.state);
        st.done = st.done.max(seq);
        match result {
            Ok(()) => st.saved = st.saved.max(seq),
            // An older failure recorded late never replaces a newer one's words.
            Err(e) if st.error.as_ref().is_none_or(|(at, ..)| *at <= seq) => {
                st.error = Some((seq, e.kind(), e.to_string()));
            }
            Err(_) => {}
        }
        drop(st);
        self.written.notify_all();
    }
}

/// The thread: write the newest snapshot waiting, until the writer goes and nothing is left.
fn run(shared: Arc<Shared>) {
    loop {
        {
            let mut st = lock(&shared.state);
            while st.pending.is_none() && !st.closing {
                st = shared.work.wait(st).unwrap_or_else(PoisonError::into_inner);
            }
            if st.pending.is_none() {
                return;
            }
        }
        let file = lock(&shared.file);
        let written = shared.write_pending();
        drop(file);
        shared.finish(written);
    }
}

/// The writer of one settings file. See the module header.
pub struct SettingsWriter {
    shared: Arc<Shared>,
    /// Started by the first queued snapshot; `None` inside when it could not be.
    thread: OnceLock<Option<JoinHandle<()>>>,
}

impl SettingsWriter {
    /// The one writer of the settings file at `path` in this process, made on first use. No thread
    /// is started until a snapshot is queued.
    pub fn of(path: &Path) -> Arc<SettingsWriter> {
        static WRITERS: LazyLock<Mutex<HashMap<PathBuf, Arc<SettingsWriter>>>> =
            LazyLock::new(Default::default);
        Arc::clone(lock(&WRITERS).entry(path.to_path_buf()).or_insert_with(|| {
            Arc::new(SettingsWriter::new(
                path.to_path_buf(),
                Arc::new(Settings::save),
            ))
        }))
    }

    fn new(path: PathBuf, write: Write) -> SettingsWriter {
        SettingsWriter {
            shared: Arc::new(Shared {
                path,
                write,
                state: Mutex::new(State::default()),
                work: Condvar::new(),
                written: Condvar::new(),
                file: Mutex::new(()),
            }),
            thread: OnceLock::new(),
        }
    }

    /// Hand over `snapshot`, the Engine's settings as the caller's change left them, and return at
    /// once; the thread writes it. `on_failure` is told if the write that carries this change
    /// fails.
    ///
    /// ⛔ **Call it holding the Engine lock, with a snapshot taken under that same hold.** That is
    /// what numbers snapshots in the order the Engine made its changes; a snapshot handed over
    /// after the lock was let go can be numbered after a newer one, and then it wins. A debug
    /// build checks the lock is held (the `EngineGuard` count the logbook fence keeps).
    pub fn queue(
        &self,
        snapshot: Settings,
        on_failure: impl FnOnce(&io::Error) + Send + 'static,
    ) -> Ticket {
        debug_assert!(
            tempo_core::logbook::io_fence::engine_guards_held() > 0,
            "settings writer: queue without the Engine lock. A snapshot is numbered as it is \
             handed over; one handed over after the lock was let go can be numbered after a \
             newer one and win. Take the snapshot and queue it in one hold of the lock."
        );
        let started = self
            .thread
            .get_or_init(|| {
                let shared = Arc::clone(&self.shared);
                std::thread::Builder::new()
                    .name("nexus-settings".into())
                    .spawn(move || run(shared))
                    .ok()
            })
            .is_some();
        let mut st = lock(&self.shared.state);
        st.issued += 1;
        let seq = st.issued;
        st.queued = seq;
        let mut callers = st
            .pending
            .take()
            .map(|superseded| superseded.on_failure)
            .unwrap_or_default();
        callers.push(Box::new(on_failure));
        st.pending = Some(Pending {
            seq,
            snapshot,
            on_failure: callers,
        });
        drop(st);
        if started {
            self.shared.work.notify_one();
        } else {
            // No thread: write it here, as every save was made before the thread existed.
            let file = lock(&self.shared.file);
            let written = self.shared.write_pending();
            drop(file);
            self.shared.finish(written);
        }
        Ticket {
            seq,
            shared: Arc::clone(&self.shared),
        }
    }

    /// Write `snapshot` now, on this thread, and answer with the result: for a save whose result
    /// decides whether the change is made at all (Remote's save-then-publish).
    ///
    /// ⛔ **The snapshot must be at least as new as everything handed over before it** (taken under
    /// the Engine lock, from the Engine's settings), because a successful save supersedes the
    /// snapshot waiting: it is never written after this one. A debug build checks the lock is held.
    pub fn save_now(&self, snapshot: &Settings) -> io::Result<()> {
        debug_assert!(
            tempo_core::logbook::io_fence::engine_guards_held() > 0,
            "settings writer: save_now without the Engine lock. It supersedes the waiting \
             snapshot on the strength of its number, so it must be taken and saved in one hold \
             of the lock, after everything handed over before it."
        );
        let file = lock(&self.shared.file);
        let seq = {
            let mut st = lock(&self.shared.state);
            st.issued += 1;
            st.issued
        };
        let result = (self.shared.write)(snapshot, &self.shared.path);
        if result.is_ok() {
            let superseded = {
                let mut st = lock(&self.shared.state);
                st.done = st.done.max(seq);
                st.saved = st.saved.max(seq);
                if st.pending.as_ref().is_some_and(|p| p.seq < seq) {
                    st.pending.take()
                } else {
                    None
                }
            };
            self.shared.written.notify_all();
            // Its changes are on the disk inside this snapshot, so nobody is told anything.
            drop(superseded);
        }
        drop(file);
        result
    }

    /// Read the file, let `edit` change it, and write it back when `edit` says it did: the base
    /// profile's mirrors. Whatever was handed over and not yet written is written first, so the
    /// read sees this process's last change, and nothing else writes the file in between.
    ///
    /// **Exempt from the Engine-lock precondition**, deliberately: it numbers nothing and
    /// supersedes nothing, so no hand-over can be reordered by it. The base window's "use one
    /// radio" (`use_single_radio`) calls it with no Engine lock; a per-radio window's mirrors call
    /// it under one.
    pub fn rewrite(&self, edit: impl FnOnce(&mut Settings) -> bool) -> io::Result<()> {
        let file = lock(&self.shared.file);
        let written = self.shared.write_pending();
        let mut s = Settings::load(&self.shared.path);
        let result = if edit(&mut s) {
            (self.shared.write)(&s, &self.shared.path)
        } else {
            Ok(())
        };
        drop(file);
        self.shared.finish(written);
        result
    }

    /// Wait until every snapshot handed over so far has been written, for at most `within`, and
    /// say what was found. A quit, a restart, a relaunch and the Windows installer flush, so the
    /// last change reaches the disk before the process goes.
    ///
    /// A write that FAILS also ends the wait (it is done; its callers have been told), so the
    /// answer says which it was: [`Flushed::Saved`] when the newest change is on the disk,
    /// [`Flushed::Failed`] when the last write before the flush failed and the file keeps the
    /// snapshot before it, [`Flushed::OutOfTime`] when `within` ran out first.
    ///
    /// ⚠️ **Never holding the Engine lock**: the radio loop would wait on the disk. The fence
    /// stops it in a debug build.
    pub fn flush(&self, within: Duration) -> Flushed {
        tempo_core::logbook::io_fence::off_engine_lock("a wait for the settings writer");
        let deadline = Instant::now() + within;
        let mut st = lock(&self.shared.state);
        let upto = st.queued;
        while st.done < upto {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Flushed::OutOfTime;
            }
            st = self
                .shared
                .written
                .wait_timeout(st, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        // Done includes a write that failed (its callers told), so ask whether it landed.
        if st.saved >= upto {
            return Flushed::Saved;
        }
        Flushed::Failed(match &st.error {
            Some((_, _, message)) => message.clone(),
            None => "the settings were not saved".to_string(),
        })
    }
}

impl Drop for SettingsWriter {
    /// Write what is waiting, then end the thread. The process's writers live as long as it does;
    /// the tests' are dropped.
    fn drop(&mut self) {
        lock(&self.shared.state).closing = true;
        self.shared.work.notify_all();
        if let Some(Some(thread)) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// One queued snapshot, for a caller that answers with the save's result.
pub struct Ticket {
    seq: u64,
    shared: Arc<Shared>,
}

impl Ticket {
    /// Wait for the write that carried this snapshot's change (this snapshot's own, or a newer one
    /// that superseded it) and answer with its result.
    ///
    /// ⚠️ **Never holding the Engine lock**: release it first. The fence stops it in a debug build.
    pub fn wait(self) -> io::Result<()> {
        tempo_core::logbook::io_fence::off_engine_lock("a wait for a settings write");
        let mut st = lock(&self.shared.state);
        while st.done < self.seq {
            st = self
                .shared
                .written
                .wait(st)
                .unwrap_or_else(PoisonError::into_inner);
        }
        if st.saved >= self.seq {
            return Ok(());
        }
        Err(match &st.error {
            Some((_, kind, message)) => io::Error::new(*kind, message.clone()),
            None => io::Error::other("the settings were not saved"),
        })
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests;
