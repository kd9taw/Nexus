//! The instruments the log benches share (SPEC-2 v3 §4.11): a watcher timing the Engine-lock
//! holds of whatever runs beside it, SQLite's own heap, and how busy the machine is. One copy,
//! included by source (`#[path]`) into each bench that uses it — `crates/tempo-app/tests/
//! log_bench.rs` and `src-tauri/src/log_queries/tests.rs` (`log_query_bench`) — so the two
//! measure the same way; moving this file breaks both paths at build time. It uses std alone:
//! SQLite's C API is declared here and linked in each bench through tempo-core, so neither crate
//! needs a dependency for it. Test code only: nothing here is in a shipped build.
//!
//! Each includer uses what it needs; the rest would read as dead code there.
#![allow(dead_code)]

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

// ── the lock watcher ────────────────────────────────────────────────────────────────────────

/// A thread beside the bench that spins `try_lock` on the shared engine and keeps, since it was
/// last asked: the longest run of "held" it saw, how many runs, how many times it looked, and
/// the longest it went WITHOUT looking. That last is its blind spot: descheduled, it sees a hold
/// that begins or ends meanwhile only when it runs again, so a hold can read short or long by up
/// to that much, and two holds with a gap it missed read as one.
pub struct Watcher {
    stop: Arc<AtomicBool>,
    seen: Arc<Seen>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[derive(Default)]
struct Seen {
    longest_ns: AtomicU64,
    holds: AtomicU64,
    looks: AtomicU64,
    blind_ns: AtomicU64,
    since: Mutex<Option<Instant>>,
}

/// What the watcher saw over one path.
pub struct Holds {
    pub longest: Duration,
    pub holds: u64,
    /// The mean time between two looks.
    pub period: Duration,
    /// The longest time between two looks.
    pub blind: Duration,
}

impl Watcher {
    pub fn start<T: Send + 'static>(engine: &Arc<Mutex<T>>) -> Watcher {
        let (stop, seen) = (Arc::new(AtomicBool::new(false)), Arc::new(Seen::default()));
        *seen.since.lock().expect("not poisoned") = Some(Instant::now());
        let thread = {
            let (engine, stop, seen) = (Arc::clone(engine), Arc::clone(&stop), Arc::clone(&seen));
            std::thread::spawn(move || {
                let (mut held_since, mut last): (Option<Instant>, Instant) = (None, Instant::now());
                while !stop.load(Ordering::Relaxed) {
                    let held = match engine.try_lock() {
                        Ok(guard) => {
                            drop(guard);
                            false
                        }
                        Err(TryLockError::WouldBlock) => true,
                        Err(TryLockError::Poisoned(_)) => false,
                    };
                    let now = Instant::now();
                    seen.looks.fetch_add(1, Ordering::Relaxed);
                    seen.blind_ns
                        .fetch_max((now - last).as_nanos() as u64, Ordering::Relaxed);
                    last = now;
                    match (held, held_since) {
                        (true, None) => held_since = Some(now),
                        (false, Some(t)) => {
                            seen.longest_ns
                                .fetch_max((now - t).as_nanos() as u64, Ordering::Relaxed);
                            seen.holds.fetch_add(1, Ordering::Relaxed);
                            held_since = None;
                        }
                        _ => {}
                    }
                    std::hint::spin_loop();
                }
            })
        };
        Watcher {
            stop,
            seen,
            thread: Some(thread),
        }
    }

    /// What the watcher saw since this was last asked, once it has seen the last release.
    pub fn holds(&self) -> Holds {
        std::thread::sleep(Duration::from_millis(3));
        let seen = &self.seen;
        let span = seen
            .since
            .lock()
            .expect("not poisoned")
            .replace(Instant::now())
            .map_or(Duration::ZERO, |t| t.elapsed());
        let looks = seen.looks.swap(0, Ordering::Relaxed).max(1);
        Holds {
            longest: Duration::from_nanos(seen.longest_ns.swap(0, Ordering::Relaxed)),
            holds: seen.holds.swap(0, Ordering::Relaxed),
            period: span / u32::try_from(looks).unwrap_or(u32::MAX),
            blind: Duration::from_nanos(seen.blind_ns.swap(0, Ordering::Relaxed)),
        }
    }

    /// The longest hold since this was last asked.
    pub fn longest(&self) -> Duration {
        self.holds().longest
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

// ── SQLite's heap, and the machine ──────────────────────────────────────────────────────────

// SQLite's own memory statistics, read as tempo-core's `sqlite_memory_statistics` test reads them.
// The logbook turns them off before its first connection (`quiet_memory_statistics`); a process
// that initialises SQLite first keeps them on, as that test's control shows. The bundled SQLite
// is linked into the bench's binary through tempo-core, so its C API is there to call without
// naming the crate that wraps it.
extern "C" {
    fn sqlite3_initialize() -> std::os::raw::c_int;
    fn sqlite3_memory_used() -> i64;
    fn sqlite3_memory_highwater(reset: std::os::raw::c_int) -> i64;
}

/// Initialise SQLite with its memory statistics on, before anything in this process opens the
/// logbook — so its heap can be reported beside the Rust heap. It costs a mutex per SQLite
/// allocation, which is why the app keeps them off.
pub fn keep_sqlite_statistics() {
    // SAFETY: SQLite's documented initialisation, called before any other use of SQLite in this
    // process (the bench's first act); it is idempotent after the first call.
    let _ = unsafe { sqlite3_initialize() };
}

/// SQLite's heap in bytes, or `None` while its statistics are off (they read 0 then).
pub fn sqlite_heap() -> Option<i64> {
    // SAFETY: a plain read of SQLite's statistics counter.
    let used = unsafe { sqlite3_memory_used() };
    (used > 0).then_some(used)
}

/// The most SQLite's heap has held since the last call, which starts the next span from its heap
/// as it is now. `None` while its statistics are off.
pub fn sqlite_heap_peak() -> Option<i64> {
    // SAFETY: a plain read of SQLite's statistics counter, reset to its current value.
    let peak = unsafe { sqlite3_memory_highwater(1) };
    (peak > 0).then_some(peak)
}

/// The 1-, 5- and 15-minute load averages, and how many CPUs they are shared among.
pub fn load_average() -> (Vec<f64>, usize) {
    let load = std::fs::read_to_string("/proc/loadavg")
        .map(|s| {
            s.split_whitespace()
                .take(3)
                .filter_map(|v| v.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let cpus = std::thread::available_parallelism().map_or(1, |n| n.get());
    (load, cpus)
}

/// The line a bench opens with: the load, and whether the machine is too busy for its hold times
/// to be published (a watcher that is descheduled reads holds short).
pub fn load_line(bench: &str) -> String {
    let (load, cpus) = load_average();
    let busy = load.first().is_some_and(|l| *l > cpus as f64 / 2.0);
    format!(
        "{bench} load average {load:?} on {cpus} CPUs: {}",
        if busy {
            "BUSY — the watcher may be descheduled, so the hold times are not to be published"
        } else {
            "quiet enough for the hold times"
        }
    )
}
