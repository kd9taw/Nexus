//! ★ SPEC-2 v3 §4.11: THE LOG PATH'S RELEASE BENCH (C19 Part D). Run by hand:
//! `cargo test -p tempo-app --release --test log_bench -- --ignored --nocapture`
//! (`LOG_BENCH_ROWS` sets the sizes, comma-separated; 150,000 and 500,000 by default).
//!
//! A real launch at each size — a `log.adi` of that many contacts converted into the store and
//! attached, as an operator's first launch after the upgrade does — and then every write path the
//! app makes, each through the entry point a command or the radio loop uses, on the engine a
//! command shares. What it measures is what §4.11 bounds:
//!
//! 1. **The engine's steady-state log memory**: the live Rust heap the attached log holds above
//!    the engine before it — at most 60 MiB at 150k contacts and 200 MiB at 500k. SQLite's page
//!    cache is its own allocator's and outside the count, as in tempo-core's `mirror_memory`.
//!    Reported now; asserted once the cut removes the log in memory ([`AFTER_THE_CUT`]).
//! 2. **No whole-log allocation per logged contact**: the largest single allocation one contact's
//!    log call makes on the calling thread, against a whole-log one (a pointer per contact), and
//!    the bytes it allocates, at each size.
//! 3. **No Engine-lock hold over 5 ms in any log path**: timed by a watcher thread that spins
//!    `try_lock` on the shared engine and times each run of "held". It can only overestimate a
//!    hold: two holds with a gap shorter than one of its spins read as one.
//!
//! Its own binary: the counting allocator counts the whole process, so nothing else runs beside
//! it. A bench, never a gate: it is `#[ignore]`d, and it prints everything it saw before it
//! asserts, so one bound failing does not hide the others.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, TryLockError};
use std::time::{Duration, Instant};

use tempo_app::engine::{engine_lock, Engine};
use tempo_app::logstore::{self, DURABLE_WAIT, READ_WAIT};
use tempo_app::logwrite;
use tempo_app::station::{LogFill, LotwSigned};
use tempo_core::logbook::sqlite::Resolved;
use tempo_core::logbook::{
    adif_header, adif_record, LogOp, QsoEdit, QsoRecord, UploadOutcome, UploadService, UploadStatus,
};

/// Whether the cut has removed the log in memory: the memory bounds are asserted from then on.
/// Until then the attached log still holds every contact in memory beside the store, and the
/// number is reported, not held to the bound.
const AFTER_THE_CUT: bool = false;

/// §4.11's bound on an Engine-lock hold in any log path.
const HOLD_BOUND: Duration = Duration::from_millis(5);

// ── the counting allocator ──────────────────────────────────────────────────────────────────

struct Counting;

/// Live heap bytes, process-wide.
static LIVE: AtomicIsize = AtomicIsize::new(0);
/// Bytes allocated, and the largest single allocation, on a thread that asked to be counted
/// ([`on_this_thread`]) since the counters were last taken.
static HERE_BYTES: AtomicUsize = AtomicUsize::new(0);
static HERE_LARGEST: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    /// Whether this thread's allocations are the ones being measured. Const-initialised and with
    /// no destructor, so reading it from inside the allocator never allocates.
    static HERE: Cell<bool> = const { Cell::new(false) };
}

fn took(size: usize) {
    LIVE.fetch_add(size as isize, Ordering::Relaxed);
    if HERE.try_with(Cell::get).unwrap_or(false) {
        HERE_BYTES.fetch_add(size, Ordering::Relaxed);
        HERE_LARGEST.fetch_max(size, Ordering::Relaxed);
    }
}

// SAFETY: every call is forwarded to `System` unchanged; the counters are plain atomics and a
// const thread-local, and never allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            took(layout.size());
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            took(layout.size());
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            // A growth is an allocation of the new size: a Vec over the whole log that grows is
            // exactly a whole-log allocation.
            LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
            took(new_size);
        }
        p
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// `f`, with the bytes it allocated on this thread and the largest single allocation among them.
fn on_this_thread<T>(f: impl FnOnce() -> T) -> (T, usize, usize) {
    HERE_BYTES.store(0, Ordering::Relaxed);
    HERE_LARGEST.store(0, Ordering::Relaxed);
    HERE.with(|h| h.set(true));
    let out = f();
    HERE.with(|h| h.set(false));
    (
        out,
        HERE_BYTES.load(Ordering::Relaxed),
        HERE_LARGEST.load(Ordering::Relaxed),
    )
}

// ── the lock watcher ────────────────────────────────────────────────────────────────────────

/// A thread beside the bench that spins `try_lock` on the shared engine and keeps the longest
/// run of "held" it has seen since it was last asked.
struct Watcher {
    stop: Arc<AtomicBool>,
    longest_ns: Arc<AtomicU64>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Watcher {
    fn start(engine: &Arc<Mutex<Engine>>) -> Watcher {
        let (stop, longest_ns) = (
            Arc::new(AtomicBool::new(false)),
            Arc::new(AtomicU64::new(0)),
        );
        let thread = {
            let (engine, stop, longest_ns) = (
                Arc::clone(engine),
                Arc::clone(&stop),
                Arc::clone(&longest_ns),
            );
            std::thread::spawn(move || {
                let mut since: Option<Instant> = None;
                while !stop.load(Ordering::Relaxed) {
                    let held = match engine.try_lock() {
                        Ok(guard) => {
                            drop(guard);
                            false
                        }
                        Err(TryLockError::WouldBlock) => true,
                        Err(TryLockError::Poisoned(_)) => false,
                    };
                    match (held, since) {
                        (true, None) => since = Some(Instant::now()),
                        (false, Some(t)) => {
                            longest_ns.fetch_max(t.elapsed().as_nanos() as u64, Ordering::Relaxed);
                            since = None;
                        }
                        _ => {}
                    }
                    std::hint::spin_loop();
                }
            })
        };
        Watcher {
            stop,
            longest_ns,
            thread: Some(thread),
        }
    }

    /// The longest hold since this was last asked, once the watcher has seen the last release.
    fn longest(&self) -> Duration {
        std::thread::sleep(Duration::from_millis(3));
        Duration::from_nanos(self.longest_ns.swap(0, Ordering::Relaxed))
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

// ── the log ─────────────────────────────────────────────────────────────────────────────────

const BANDS: [&str; 10] = [
    "160m", "80m", "40m", "30m", "20m", "17m", "15m", "12m", "10m", "6m",
];
const MODES: [&str; 4] = ["FT8", "CW", "SSB", "FT4"];
const PREFIXES: [&str; 8] = ["W", "K", "N", "DL", "JA", "G", "VE", "PY"];

/// The call of station `station`: 50,000 of them, so a lifetime log works each many times.
fn call_of(station: usize) -> String {
    format!(
        "{}{}{}{}",
        PREFIXES[station % PREFIXES.len()],
        station % 10,
        ["AB", "XYZ", "CD", "EFG"][station / 10 % 4],
        station / 40
    )
}

/// A lifetime log of `n` contacts as an operator's `log.adi` holds them — the shape C13's hot
/// index bench built, with a note and a foreign tag most real rows carry, and a third of them
/// confirmed at LoTW.
fn log_text(n: usize) -> String {
    let tag = |name: &str, v: &str| format!("<{name}:{}>{v}", v.len());
    let mut s = adif_header();
    for i in 0..n {
        let when = 1_600_000_000u64 + i as u64 * 97;
        let (days, secs) = (when / 86_400, when % 86_400);
        let (y, m, d) = civil(days as i64);
        s.push_str(&tag("CALL", &call_of(i % 50_000)));
        s.push_str(&tag("QSO_DATE", &format!("{y:04}{m:02}{d:02}")));
        s.push_str(&tag(
            "TIME_ON",
            &format!("{:02}{:02}{:02}", secs / 3600, secs / 60 % 60, secs % 60),
        ));
        s.push_str(&tag("BAND", BANDS[i % BANDS.len()]));
        s.push_str(&tag("MODE", MODES[i / 3 % MODES.len()]));
        s.push_str(&tag("FREQ", "14.074000"));
        if i % 5 != 0 {
            let c = |k: usize| (b'A' + (k % 18) as u8) as char;
            s.push_str(&tag(
                "GRIDSQUARE",
                &format!("{}{}{}{}", c(i), c(i / 18), i % 10, i / 10 % 10),
            ));
        }
        s.push_str(&tag("RST_SENT", "-10"));
        s.push_str(&tag("RST_RCVD", "-12"));
        s.push_str(&tag("COMMENT", "tnx for the contact, 73"));
        if i % 3 == 0 {
            s.push_str(&tag("LOTW_QSL_RCVD", "Y"));
        }
        s.push_str(&tag("APP_OTHERLOG_ID", &format!("{i:08}")));
        s.push_str("<EOR>\n");
    }
    s
}

/// The proleptic Gregorian date of `days` since 1970-01-01 (Howard Hinnant's `civil_from_days`).
fn civil(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

/// A contact the log does not hold yet, the `k`th.
fn fresh(k: usize) -> QsoRecord {
    let call = format!("ZZ{k}NEW");
    let mut lb = tempo_core::logbook::Logbook::new();
    lb.import_adif(&format!(
        "{}<CALL:{}>{call}<BAND:3>20m<MODE:3>FT8<FREQ:9>14.074000<QSO_DATE:8>20300101\
         <TIME_ON:6>{:02}{:02}00<GRIDSQUARE:4>FN31<EOR>",
        adif_header(),
        call.len(),
        k / 60 % 24,
        k % 60
    ));
    QsoRecord::clone(&lb.records()[0])
}

/// Every `stride`th row of the log, read with the lock released as every reader reads it.
fn rows_every(engine: &Mutex<Engine>, stride: usize) -> Vec<QsoRecord> {
    let rows = engine_lock(engine).log_rows();
    let (mut out, mut i) = (Vec::new(), 0usize);
    rows.each_record(READ_WAIT, &mut |r| {
        if i % stride == 0 {
            out.push(r.clone());
        }
        i += 1;
        ControlFlow::Continue(())
    })
    .expect("the log reads");
    out
}

/// Wait, with the lock released, until the store holds every change made so far.
fn settle(engine: &Mutex<Engine>) {
    let rows = engine_lock(engine).log_rows();
    rows.count().expect("the log reads");
}

/// A report restating `rows` with `extra` tags on each.
fn restating(rows: &[QsoRecord], extra: &str) -> String {
    let mut s = adif_header();
    for r in rows {
        s.push_str(&adif_record(r).replace("<EOR>", &format!("{extra}<EOR>")));
    }
    s
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

fn mib(bytes: isize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

// ── the bench ───────────────────────────────────────────────────────────────────────────────

/// What one size measured: its memory, its per-contact allocations, and each path's longest
/// Engine-lock hold — and what broke a bound, in words.
fn bench(n: usize) -> Vec<String> {
    let mut broken = Vec::new();
    let dir = std::env::temp_dir().join(format!("nexus-log-bench-{n}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("a scratch folder");
    let log_path = dir.join("log.adi");
    std::fs::write(&log_path, log_text(n)).expect("the log written");

    // The launch: the engine first, so its own allocations are the baseline, then the store.
    let mut engine = Engine::new("KD9TAW", "EN52", 0);
    engine.set_dxcc_resolver(|call| {
        let base = tempo_core::message::base_call(call);
        (base.len() >= 3).then(|| base[..2].to_string())
    });
    let base = LIVE.load(Ordering::SeqCst);
    let started = Instant::now();
    let opened = logstore::open(&log_path, Arc::new(|_| Resolved::default()), None)
        .expect("the store opens");
    engine.attach_log_store(opened);
    let launch = started.elapsed();
    let shared = Arc::new(Mutex::new(engine));
    settle(&shared);
    let _ = engine_lock(&shared).snapshot();
    let memory = LIVE.load(Ordering::SeqCst) - base;
    let bound = if n <= 150_000 { 60.0 } else { 200.0 };
    println!(
        "LOG_BENCH rows={n} launch={:.1}s steady_state_log_memory={:.1}MiB (bound {bound} MiB at \
         this size, {})",
        launch.as_secs_f64(),
        mib(memory),
        if AFTER_THE_CUT {
            "asserted"
        } else {
            "asserted after the cut"
        }
    );
    if AFTER_THE_CUT && mib(memory) > bound {
        broken.push(format!("{n} rows: the log holds {:.1} MiB", mib(memory)));
    }

    let watcher = Watcher::start(&shared);
    let mut holds: Vec<(&str, Duration)> = Vec::new();
    let mut hold = |path: &'static str, watcher: &Watcher| {
        let longest = watcher.longest();
        holds.push((path, longest));
    };

    // Logged contacts: the funnel every log path passes through, under the lock as the radio loop
    // and the commands hold it.
    let whole_log = 8 * n;
    let (mut largest, mut bytes) = (0usize, Vec::new());
    let _ = watcher.longest();
    for k in 0..200 {
        let rec = fresh(k);
        let ((), b, l) = on_this_thread(|| engine_lock(&shared).log_qso(rec));
        largest = largest.max(l);
        bytes.push(b);
    }
    hold("log a contact (the FT and manual funnel)", &watcher);
    bytes.sort_unstable();
    println!(
        "  per logged contact: median {} B, largest single allocation {} B (a whole-log one is {} \
         B)",
        bytes[bytes.len() / 2],
        largest,
        whole_log
    );
    if largest >= whole_log {
        broken.push(format!(
            "{n} rows: a logged contact made a {largest} B allocation, a whole log's worth"
        ));
    }
    settle(&shared);

    for _ in 0..5 {
        let _ = engine_lock(&shared).snapshot();
    }
    hold("a snapshot", &watcher);
    // C13's bench's last case: a snapshot while a QSO is under way with a partner who sent no
    // grid, which asks the log for the grid it last logged for them.
    engine_lock(&shared).call_station("QQ9NOGRID");
    for _ in 0..5 {
        let _ = engine_lock(&shared).snapshot();
    }
    hold("a snapshot during a QSO (a partner with no grid)", &watcher);

    // By id: the commands' planned changes, one row each.
    let picked = rows_every(&shared, (n / 200).max(1));
    let id = |i: usize| {
        picked[i % picked.len()]
            .id
            .expect("every row carries an id")
    };
    let wait = |d: logstore::Durability| d.wait(DURABLE_WAIT).expect("on disk");
    for i in 0..5 {
        let (made, d) = logwrite::change_ops(
            &shared,
            id(i),
            None,
            &[LogOp::MarkQslCard {
                id: id(i),
                received: true,
            }],
            "card",
        );
        made.expect("reads").expect("made");
        wait(d);
    }
    hold("a QSL card mark by id", &watcher);
    for i in 5..10 {
        let (made, d) = logwrite::change_ops(
            &shared,
            id(i),
            None,
            &[logwrite::qsl_sent(
                id(i),
                Some(tempo_core::logbook::QslVia::Bureau),
            )],
            "sent",
        );
        made.expect("reads").expect("made");
        wait(d);
    }
    hold("a QSL-sent mark by id", &watcher);
    for i in 10..15 {
        let op = LogOp::SetSatTag {
            id: id(i),
            sat_name: Some("RS-44".into()),
        };
        let (made, d) = logwrite::change_ops(&shared, id(i), None, &[op], "sat");
        made.expect("reads").expect("made");
        wait(d);
    }
    hold("a satellite tag by id", &watcher);
    for r in picked.iter().skip(15).take(5) {
        let id = r.id.expect("an id");
        let (key, mut edit) = (QsoEdit::project(r).key(), QsoEdit::project(r));
        edit.comment = Some("edited".into());
        let (made, d) = logwrite::edit_row(&shared, id, &key, &edit);
        made.expect("reads").expect("made");
        wait(d);
    }
    hold("the Logbook form's edit by id", &watcher);
    for r in picked.iter().skip(20).take(5) {
        let id = r.id.expect("an id");
        let key = QsoEdit::project(r).key();
        let (made, d) = logwrite::update_row(
            &shared,
            id,
            &key,
            |row| QsoRecord {
                comment: Some("corrected".into()),
                ..row.clone()
            },
            |_, _| (),
        );
        made.expect("reads").expect("made").expect("changed");
        wait(d);
    }
    hold("a correction of a found row", &watcher);
    for i in 25..30 {
        let (made, d) =
            logwrite::change_ops(&shared, id(i), None, &[LogOp::Delete(id(i))], "delete");
        made.expect("reads").expect("made");
        wait(d);
    }
    hold("a delete by id", &watcher);

    // The stamps and the fills.
    for r in picked.iter().skip(30).take(5) {
        let status = UploadStatus {
            outcome: UploadOutcome::Accepted,
            when_unix: 1_900_000_000,
            detail: None,
        };
        let (_, d) = logwrite::stamp_push(&shared, r, UploadService::Qrz, status);
        wait(d);
    }
    hold("a connector's upload stamp", &watcher);
    let batch: Vec<QsoRecord> = picked.iter().skip(35).take(100).cloned().collect();
    let signed: Vec<LotwSigned> = batch.iter().filter_map(LotwSigned::of).collect();
    let status = UploadStatus {
        outcome: UploadOutcome::Pending,
        when_unix: 1_900_000_100,
        detail: None,
    };
    let (_, d) = logwrite::stamp_lotw_batch(&shared, &signed, &status);
    wait(d);
    hold("a LoTW batch's stamps (100 contacts)", &watcher);
    let fills: Vec<LogFill> = picked
        .iter()
        .skip(40)
        .take(logwrite::FILL_CHUNK)
        .filter_map(|r| {
            Some(LogFill {
                id: r.id?,
                country: Some("Filled".into()),
                state: None,
            })
        })
        .collect();
    logwrite::fill(&shared, &fills, 1).expect("the fill job writes");
    settle(&shared);
    hold("the fill job's chunk", &watcher);

    // The bulk changes, on the rows of the calls they bring.
    let some = rows_every(&shared, (n / 100).max(1));
    let text = restating(&some, "<QSL_RCVD:1>Y") + &adif_record(&fresh(9_000));
    let (made, d) = logwrite::import_adif(&shared, &text);
    made.expect("the import is made");
    wait(d);
    hold("an import (100 restated, one new)", &watcher);
    let (made, d) = logwrite::merge_lotw_report(&shared, &restating(&some, "<LOTW_QSL_RCVD:1>Y"));
    made.expect("the report merges");
    wait(d);
    hold("a LoTW confirmation report (100 contacts)", &watcher);
    let (made, d) =
        logwrite::merge_qrz_report(&shared, &restating(&some, "<APP_QRZLOG_STATUS:1>C"));
    made.expect("the download merges");
    wait(d);
    hold("QRZ's download (100 contacts)", &watcher);
    let (made, d) =
        logwrite::import_pota_log(&shared, &restating(&some, "<SIG:4>POTA<SIG_INFO:6>K-0001"));
    made.expect("the stamps are made");
    wait(d);
    hold("a pota.app export's park stamps (100 contacts)", &watcher);
    // The LoTW batch's rows: the report merge above confirmed the others, and an own-QSO report
    // promotes only a contact not confirmed yet.
    let (made, d) = logwrite::merge_lotw_own_echo(&shared, &restating(&batch, ""), 1_900_000_200);
    made.expect("the report merges");
    wait(d);
    hold("LoTW's own-QSO report (100 contacts)", &watcher);
    let (made, d) = logwrite::mark_lotw_uploaded_all(&shared, 1_900_000_300);
    made.expect("the declaration is made");
    wait(d);
    hold(
        "\"already uploaded\" (every contact owed to LoTW)",
        &watcher,
    );

    // Last, since it ends the log: the purge.
    let removed = engine_lock(&shared).clear_logbook();
    settle(&shared);
    hold("the purge", &watcher);
    drop(watcher);
    println!("  purged {removed} contacts");

    println!(
        "  longest Engine-lock hold, by path (the bound is {:.0} ms):",
        ms(HOLD_BOUND)
    );
    for (path, longest) in &holds {
        let over = *longest > HOLD_BOUND;
        println!(
            "    {:>9.3} ms  {path}{}",
            ms(*longest),
            if over { "  ← over" } else { "" }
        );
        if over {
            broken.push(format!(
                "{n} rows: {path} held the Engine lock {:.3} ms",
                ms(*longest)
            ));
        }
    }
    drop(shared);
    let _ = std::fs::remove_dir_all(&dir);
    broken
}

#[test]
#[ignore = "a release-build bench, run by hand"]
fn the_log_path_bench() {
    let sizes: Vec<usize> = std::env::var("LOG_BENCH_ROWS")
        .unwrap_or_else(|_| "150000,500000".into())
        .split(',')
        .map(|s| s.trim().parse().expect("a row count"))
        .collect();
    let broken: Vec<String> = sizes.into_iter().flat_map(bench).collect();
    assert!(
        broken.is_empty(),
        "§4.11's bounds, broken:\n  {}",
        broken.join("\n  ")
    );
}

/// The instruments, held to what they measure. The counting allocator: a Vec over the whole log,
/// made on this thread, is the largest allocation and counts every byte; one made on another
/// thread is not this thread's.
#[test]
fn the_instruments_see_what_they_measure() {
    let n = 100_000usize;
    let (v, bytes, largest) = on_this_thread(|| vec![0u64; n]);
    assert_eq!(largest, 8 * n, "the whole-log allocation is the largest");
    assert!(bytes >= 8 * n);
    drop(v);
    let (_, bytes, largest) = on_this_thread(|| {
        std::thread::spawn(move || vec![0u64; n].len())
            .join()
            .expect("the thread ran")
    });
    assert!(
        largest < 8 * n && bytes < 8 * n,
        "another thread's allocation is not this thread's: {largest} B, {bytes} B"
    );

    // The watcher: a hold it cannot miss is seen, and at least as long as it was.
    let engine = Arc::new(Mutex::new(Engine::new("KD9TAW", "EN52", 0)));
    let watcher = Watcher::start(&engine);
    let _ = watcher.longest();
    {
        let _held = engine_lock(&engine);
        std::thread::sleep(Duration::from_millis(20));
    }
    let seen = watcher.longest();
    assert!(
        seen >= Duration::from_millis(20) && seen < Duration::from_secs(2),
        "a 20 ms hold, seen as {seen:?}"
    );
    assert!(
        watcher.longest() < Duration::from_millis(1),
        "and nothing held since"
    );
}
