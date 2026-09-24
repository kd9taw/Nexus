//! The debug fence that proves the logbook's disk work never runs under the Engine lock —
//! SPEC-1 v3's **C11**.
//!
//! # The rule
//!
//! The radio loop takes the Engine lock on every tick of its 20 ms heartbeat. Anything that
//! waits on the disk while holding that lock makes the loop wait on the disk: the stall behind a
//! whole-file `log.adi` rewrite that the logbook database exists to remove, and the runtime
//! starvation behind #335's frozen waterfall. The store keeps two promises about it:
//!
//! 1. **Its writes run on the log lanes** — the database writer's thread (`nexus-logdb`) and the
//!    `log.adi` mirror's (`nexus-log-mirror`). Neither can take the Engine lock: this crate cannot
//!    name the Engine. [`on_log_lane`] asserts it.
//! 2. **Everything else it does on the disk, and every wait for the lanes, runs with no Engine
//!    lock held on the thread doing it** — opening and converting the store (before the engine
//!    is locked at launch), and a command waiting for its own change to commit (after it has
//!    released the lock). [`off_engine_lock`] asserts it.
//!
//! # How it knows the Engine is locked
//!
//! `tempo_app::engine::EngineGuard` is the one way to hold the Engine lock, and each guard
//! carries an [`EngineHeld`], which counts the guards alive on its thread. This crate cannot
//! depend on tempo-app, so that count is the only way "the Engine is locked here" can reach the
//! code that touches the disk. A guard taken with a raw `Mutex::lock()` is invisible to it —
//! which is why tempo-app's scan forbids one anywhere outside `EngineGuard`.
//!
//! # Debug builds only
//!
//! Every check here, and the count itself, is compiled out of a release build: there the
//! functions are empty and [`EngineHeld`] is a zero-sized marker with nothing to drop. What a
//! failed check reports is a design error, and a debug build (and every test run) is where it
//! is found. It changes nothing about when anything locks, waits or transmits.
//!
//! # Where the fence stands, and where it does not
//!
//! **Fenced:** the writer's database transactions and its database copy, and the mirror's
//! rewrite of `log.adi` — which takes the dated ring snapshot, writes and syncs the temporary
//! file, renames it over the log and syncs the folder ([`on_log_lane`]); a wait for a change
//! to commit (`LogWriter::wait_durable`, which `LogAppendReceipt::sync` and the app's
//! `Durability::wait` go through), an append receipt's sync, a wait for a database copy, the
//! conversion of `log.adi`, and the store's open ([`off_engine_lock`]). And SPEC-2's read path
//! (C12): every read of the store through its reader (`LogReader::read`) and the wait for the
//! store to hold every change made before a read (`LogWriter::wait_committed`), which the app's
//! `StoreReads` runs in that order.
//!
//! **Not fenced, each for a reason recorded where it happens** — every one of these runs under
//! the Engine lock today, so a fence there would stop a debug build rather than prove anything:
//!
//! - the re-read of the store after ANOTHER process committed (`LogStore::reload`) and the
//!   read of a `log.adi` the mirror refused (`take_in_refused_log_file`): only when a second
//!   Nexus shares the data folder, at the points the 1.13 path re-read `log.adi` under the
//!   same lock;
//! - the exit's flush of the writer and the mirror (`flush_logbook`), under the lock
//!   deliberately so no change lands after it — once the radio loop has stopped;
//! - the Field Day contest journal (`persist_fd_log`), still rewritten and synced under the lock
//!   on every contact — the FT Field Day sequencer's own log included — and the other small
//!   journals written the same way (the held-QSO and store-and-forward journals,
//!   `settings.json`, the pending-QSO publish). None of them is the logbook store;
//! - everything on the 1.13 path, which runs the session on `log.adi` itself when the store
//!   could not be opened: its loads, appends and whole-file saves are under the lock by design.
//!
//! The primitives underneath (`LogDb`, `Logbook::save`, the ring's own functions) are shared
//! with those paths and with the tests that drive them directly, so the fence stands at the
//! store's operations, not inside the primitives.

#[cfg(debug_assertions)]
use std::cell::Cell;
use std::marker::PhantomData;

#[cfg(debug_assertions)]
thread_local! {
    // How many Engine guards this thread holds. Per thread, because the lock is per thread:
    // only a guard held HERE makes this thread's disk work a wait the radio loop feels.
    static ENGINE_HELD: Cell<u32> = const { Cell::new(0) };
    // Whether this thread is one of the log lanes.
    static LOG_LANE: Cell<bool> = const { Cell::new(false) };
}

/// One Engine guard, counted on the thread that holds it. It rides inside
/// `tempo_app::engine::EngineGuard` and is made by nothing else.
///
/// Zero-sized in every build. It is `!Send` like the `MutexGuard` beside it — the count it
/// keeps belongs to one thread — and `Sync` like it, so the guard it rides in keeps exactly the
/// auto traits a bare `MutexGuard` has.
#[must_use = "the count lasts exactly as long as this token"]
pub struct EngineHeld {
    _thread_bound: PhantomData<std::sync::MutexGuard<'static, ()>>,
}

impl EngineHeld {
    /// Count one more Engine guard on this thread, until the token drops.
    #[inline]
    pub fn acquired() -> EngineHeld {
        #[cfg(debug_assertions)]
        let _ = ENGINE_HELD.try_with(|n| n.set(n.get() + 1));
        EngineHeld {
            _thread_bound: PhantomData,
        }
    }
}

#[cfg(debug_assertions)]
impl Drop for EngineHeld {
    fn drop(&mut self) {
        let _ = ENGINE_HELD.try_with(|n| n.set(n.get().saturating_sub(1)));
    }
}

/// How many Engine guards this thread holds — always 0 in a release build, which counts
/// nothing.
pub fn engine_guards_held() -> u32 {
    #[cfg(debug_assertions)]
    {
        ENGINE_HELD.try_with(Cell::get).unwrap_or(0)
    }
    #[cfg(not(debug_assertions))]
    {
        0
    }
}

/// Mark this thread as a log lane: the database writer and the mirror call it once, first
/// thing, on the threads they own.
#[inline]
pub fn enter_log_lane() {
    #[cfg(debug_assertions)]
    let _ = LOG_LANE.try_with(|l| l.set(true));
}

/// Assert that `op` — disk work, or a wait for it — runs with no Engine lock held on this
/// thread. Debug builds only.
#[inline]
#[track_caller]
pub fn off_engine_lock(op: &str) {
    #[cfg(debug_assertions)]
    {
        let held = engine_guards_held();
        assert!(
            held == 0,
            "io_fence: {op} ran while this thread holds the Engine lock ({held} guard(s)). The \
             radio loop needs that lock every 20 ms, so this would make it wait on the disk: \
             release the lock first, or hand the work to the log's own threads."
        );
    }
    #[cfg(not(debug_assertions))]
    let _ = op;
}

/// Assert that `work` — a pass over the whole log, in memory — runs with no Engine lock held on
/// this thread. Debug builds only.
///
/// Not disk work, and the same rule for the same reason: the radio loop needs that lock every
/// 20 ms, and a pass over a lifetime log takes tens to hundreds of milliseconds (a needs fold
/// over 500,000 contacts, 225 ms). Take the log's rows under the lock — a copy of pointers,
/// `Engine::log_snapshot` — release it, then do the work. SPEC-2 v3's C12 put every such pass a
/// poll or a timer runs behind this assertion.
#[inline]
#[track_caller]
pub fn whole_log_off_engine_lock(work: &str) {
    #[cfg(debug_assertions)]
    {
        let held = engine_guards_held();
        assert!(
            held == 0,
            "io_fence: {work} is a pass over the whole log and ran while this thread holds the \
             Engine lock ({held} guard(s)). The radio loop needs that lock every 20 ms: take the \
             log's rows under the lock (a copy of pointers), release it, then do the work."
        );
    }
    #[cfg(not(debug_assertions))]
    let _ = work;
}

/// Assert that `op` runs on a log lane — the database writer's thread or the mirror's — and so
/// with no Engine lock held either. Debug builds only.
#[inline]
#[track_caller]
pub fn on_log_lane(op: &str) {
    #[cfg(debug_assertions)]
    {
        let lane = LOG_LANE.try_with(Cell::get).unwrap_or(false);
        assert!(
            lane,
            "io_fence: {op} ran on thread {:?}, not on the log's own threads: it belongs to the \
             database writer or the log.adi mirror, where nothing can hold the Engine lock.",
            std::thread::current().name().unwrap_or("<unnamed>")
        );
    }
    off_engine_lock(op);
}

#[cfg(all(test, debug_assertions))]
mod tests {
    use super::*;

    #[test]
    fn the_count_follows_the_tokens_on_this_thread_only() {
        assert_eq!(engine_guards_held(), 0);
        let a = EngineHeld::acquired();
        let b = EngineHeld::acquired();
        assert_eq!(engine_guards_held(), 2);
        // Another thread's count is its own: a guard held there is not one held here.
        let there = std::thread::spawn(engine_guards_held).join().unwrap();
        assert_eq!(there, 0, "another thread holds nothing");
        drop(a);
        assert_eq!(engine_guards_held(), 1);
        drop(b);
        assert_eq!(engine_guards_held(), 0);
    }

    /// The two directions of the no-lock check: quiet with no guard, a panic with one.
    #[test]
    fn off_engine_lock_is_quiet_with_no_guard_held() {
        off_engine_lock("a test op");
        let held = EngineHeld::acquired();
        drop(held);
        off_engine_lock("a test op after the guard dropped");
    }

    /// ★ POSITIVE CONTROL: the fence fires. Disk work under an Engine guard is a panic.
    #[test]
    #[should_panic(expected = "io_fence: a test op ran while this thread holds the Engine lock")]
    fn off_engine_lock_fires_under_an_engine_guard() {
        let _held = EngineHeld::acquired();
        off_engine_lock("a test op");
    }

    /// The whole-log assertion's two directions: quiet with no guard, or after the guard drops…
    #[test]
    fn whole_log_off_engine_lock_is_quiet_with_no_guard_held() {
        whole_log_off_engine_lock("a test pass");
        let held = EngineHeld::acquired();
        drop(held);
        whole_log_off_engine_lock("a test pass after the guard dropped");
    }

    /// ★ POSITIVE CONTROL: …and a panic under a guard.
    #[test]
    #[should_panic(
        expected = "io_fence: a test pass is a pass over the whole log and ran while this thread holds the Engine lock"
    )]
    fn whole_log_off_engine_lock_fires_under_an_engine_guard() {
        let _held = EngineHeld::acquired();
        whole_log_off_engine_lock("a test pass");
    }

    /// ★ POSITIVE CONTROL: work that belongs to a log lane, done anywhere else, is a panic —
    /// even with no Engine lock held.
    #[test]
    #[should_panic(expected = "not on the log's own threads")]
    fn on_log_lane_fires_off_a_lane() {
        on_log_lane("a test write");
    }

    /// Run `f` on a thread of its own that has made itself a lane, re-raising its panic here —
    /// so the lane mark never outlives the test, and `should_panic` still reads the fence's own
    /// message.
    fn on_a_lane(f: impl FnOnce() + Send + 'static) {
        let lane = std::thread::spawn(move || {
            enter_log_lane();
            f();
        });
        if let Err(panic) = lane.join() {
            std::panic::resume_unwind(panic);
        }
    }

    /// ★ POSITIVE CONTROL: a lane that somehow held the Engine lock is still caught.
    #[test]
    #[should_panic(expected = "io_fence: a test write ran while this thread holds the Engine lock")]
    fn on_log_lane_fires_on_a_lane_holding_an_engine_guard() {
        on_a_lane(|| {
            let _held = EngineHeld::acquired();
            on_log_lane("a test write");
        });
    }

    #[test]
    fn on_log_lane_is_quiet_on_a_lane() {
        on_a_lane(|| on_log_lane("a test write"));
    }
}
