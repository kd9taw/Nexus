//! ★ THE PURGE'S WORK UNDER THE ENGINE LOCK DOES NOT GROW WITH THE LOG (SPEC-2 v3 §4.11: no log
//! path holds the Engine lock past 5 ms). The §4.11 bench times the purge at 150,000 and 500,000
//! contacts; this counts it, deterministically, at a few thousand: what the thread holding the
//! lock allocates and frees while the purge runs under it. The purge drops every row and names
//! nothing it must look up, so what it does there is one small change whatever the log's size —
//! not the log's index freed, nor a list of every contact's id built and hashed. Nor is that
//! index freed under the lock of the change that next finds the purge landed (the radio loop's
//! logged contact, as likely as any): it is freed on the store's writer.
//!
//! Its own binary: the counting allocator counts the whole process, so nothing else runs beside
//! it.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tempo_app::engine::{engine_lock, Engine};
use tempo_app::logstore;
use tempo_core::logbook::adif_header;
use tempo_core::logbook::sqlite::Resolved;

// ── the counting allocator ──────────────────────────────────────────────────────────────────

struct Counting;

/// Bytes allocated, and bytes freed, on a thread that asked to be counted ([`on_this_thread`])
/// since the counters were last taken.
static HERE_TOOK: AtomicUsize = AtomicUsize::new(0);
static HERE_FREED: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Whether this thread's allocations are the ones being measured. Const-initialised and with
    /// no destructor, so reading it from inside the allocator never allocates.
    static HERE: Cell<bool> = const { Cell::new(false) };
}

fn here() -> bool {
    HERE.try_with(Cell::get).unwrap_or(false)
}

// SAFETY: every call is forwarded to `System` unchanged; the counters are plain atomics and a
// const thread-local, and never allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() && here() {
            HERE_TOOK.fetch_add(layout.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() && here() {
            HERE_TOOK.fetch_add(layout.size(), Ordering::Relaxed);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        if here() {
            HERE_FREED.fetch_add(layout.size(), Ordering::Relaxed);
        }
        unsafe { System.dealloc(ptr, layout) };
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() && here() {
            HERE_FREED.fetch_add(layout.size(), Ordering::Relaxed);
            HERE_TOOK.fetch_add(new_size, Ordering::Relaxed);
        }
        p
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// What a stretch of work cost the thread it ran on.
#[derive(Debug, Clone, Copy)]
struct Cost {
    took: usize,
    freed: usize,
}

/// `f`, with the bytes it allocated and freed on this thread.
fn on_this_thread<T>(f: impl FnOnce() -> T) -> (T, Cost) {
    HERE_TOOK.store(0, Ordering::Relaxed);
    HERE_FREED.store(0, Ordering::Relaxed);
    HERE.with(|h| h.set(true));
    let out = f();
    HERE.with(|h| h.set(false));
    (
        out,
        Cost {
            took: HERE_TOOK.load(Ordering::Relaxed),
            freed: HERE_FREED.load(Ordering::Relaxed),
        },
    )
}

// ── the log ─────────────────────────────────────────────────────────────────────────────────

/// A `log.adi` of `n` contacts, each its own call, grid and minute.
fn log_text(n: usize) -> String {
    let mut s = adif_header();
    for i in 0..n {
        let call = format!(
            "K{}{}{:04}",
            i % 10,
            (b'A' + (i / 10 % 26) as u8) as char,
            i
        );
        let grid = format!("FN{:02}", i % 100);
        s.push_str(&format!(
            "<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<FREQ:6>14.074<QSO_DATE:8>2026{:02}{:02}\
             <TIME_ON:6>{:02}{:02}00<GRIDSQUARE:4>{grid}<EOR>\n",
            call.len(),
            1 + i / 28 % 12,
            1 + i % 28,
            i / 60 % 24,
            i % 60,
        ));
    }
    s
}

/// What the purge of an `n`-contact log costs the thread holding the Engine lock: the purge made
/// as the purge command makes it — under the lock, its change's tickets collected — and waited
/// for once the lock is released; and then the next contact logged, which finds the purge landed
/// and lets it go. Before it, the log is attached, written, and has taken one more contact, so
/// nothing the attach itself left behind is freed under the purge's lock.
fn purge_cost(n: usize) -> (Cost, Cost) {
    let dir = std::env::temp_dir().join(format!("nexus-purge-hold-{n}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    let log_path = dir.join("log.adi");
    std::fs::write(&log_path, log_text(n)).expect("the log written");
    // Held with no Engine guard until the purge: the engine a test owns outright, as the launch
    // holds it before the app shares it.
    let mut engine = Engine::new("K2DEF", "FN31", 0);
    let opened = logstore::open(&log_path, Arc::new(|_| Resolved::default()), None)
        .expect("the store opens");
    engine.attach_log_store(opened);
    let settle = |e: &Engine| {
        e.flush_log_store(Duration::from_secs(120))
            .expect("the store's writer catches up")
    };
    settle(&engine);
    let one_more = log_text(n + 1)
        .split("<EOR>")
        .nth(n)
        .map(|r| format!("{}{r}<EOR>", adif_header()))
        .expect("the next record");
    let _ = engine.import_adif(&one_more);
    settle(&engine);
    let engine = Mutex::new(engine);

    let mut guard = engine_lock(&engine);
    let ((removed, durability), cost) =
        on_this_thread(|| guard.with_log_tickets(|e| e.clear_logbook()));
    drop(guard);
    assert_eq!(removed, n + 1, "premise: the purge takes out every contact");
    durability
        .wait(Duration::from_secs(120))
        .expect("the purge is on disk");
    let next = log_text(n + 2)
        .split("<EOR>")
        .nth(n + 1)
        .map(|r| format!("{}{r}<EOR>", adif_header()))
        .expect("a record");
    let contact = tempo_core::logbook::parse_adif(&next)
        .pop()
        .expect("the contact");
    let mut guard = engine_lock(&engine);
    let ((), after) = on_this_thread(|| guard.log_qso(contact));
    drop(guard);
    drop(engine);
    let _ = std::fs::remove_dir_all(&dir);
    (cost, after)
}

#[test]
fn the_purge_builds_and_frees_nothing_the_size_of_the_log_under_the_engine_lock() {
    let (small, small_after) = purge_cost(2_000);
    let (big, big_after) = purge_cost(8_000);
    println!("the purge under the lock: {small:?} at 2,000 contacts, {big:?} at 8,000");
    println!(
        "the next contact, finding it landed: {small_after:?} at 2,000, {big_after:?} at 8,000"
    );
    // The log's index freed, or a list of its 8,000 ids built and hashed, is hundreds of KB; one
    // small change is a few.
    const SMALL_CHANGE: usize = 64 * 1024;
    assert!(
        big.took < SMALL_CHANGE && big.freed < SMALL_CHANGE,
        "the purge of 8,000 contacts allocated {} B and freed {} B under the lock",
        big.took,
        big.freed
    );
    assert!(
        big.took <= small.took + 4096 && big.freed <= small.freed + 4096,
        "and what it does there does not grow with the log: {small:?} at 2,000, {big:?} at 8,000"
    );
    assert!(
        big_after.freed < SMALL_CHANGE && big_after.freed <= small_after.freed + 4096,
        "the rows the purge took out are freed off the lock, not by the next change: \
         {small_after:?} at 2,000, {big_after:?} at 8,000"
    );
}
