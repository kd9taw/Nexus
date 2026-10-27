//! ★ A purge with history holds ONE CHUNK of the log while it records what it took out — not the
//! log. Each contact a purge takes out is kept whole in the history so it can be put back, and
//! that is read and written inside the purge's one transaction; read all at once, it held every
//! contact twice over (the record and its ADIF) for as long as the transaction ran.
//!
//! Measured inside the transaction, with a counting global allocator: every allocation raises a
//! high-water mark, so the peak is the peak DURING the purge, however briefly it was held. Both
//! purges are measured — one naming every row (the operator's Purge), and one naming none (the
//! store cleared whole) — at two sizes four times apart: the peak must not grow with the log.
//!
//! The positive control: the same instrument, around a read of the whole log, must see the log.
//!
//! Its own test binary: the counting allocator counts the whole process. SQLite allocates through
//! its own allocator, so its page cache is outside the count.

use std::alloc::{GlobalAlloc, Layout, System};
use std::collections::HashMap;
use std::sync::atomic::{AtomicIsize, Ordering};

use tempo_core::logbook::history::{Kind, Note, Source};
use tempo_core::logbook::sqlite::{Batch, LogDb, Resolved};
use tempo_core::logbook::{parse_adif, QsoRecord, RecordId};

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicIsize = AtomicIsize::new(0);

fn moved(by: isize) {
    let now = LIVE.fetch_add(by, Ordering::SeqCst) + by;
    PEAK.fetch_max(now, Ordering::SeqCst);
}

// SAFETY: every call is forwarded to `System` unchanged; the counters are plain atomics and never
// allocate.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            moved(layout.size() as isize);
        }
        p
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            moved(layout.size() as isize);
        }
        p
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
        LIVE.fetch_sub(layout.size() as isize, Ordering::SeqCst);
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            moved(new_size as isize - layout.size() as isize);
        }
        p
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// The peak live heap `f` raised the process to, above where it stood when `f` began.
fn peak_during<T>(f: impl FnOnce() -> T) -> (T, isize) {
    let base = LIVE.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    let out = f();
    (out, PEAK.load(Ordering::SeqCst) - base)
}

/// Contact `i`, the size a real one runs to, with an id of its own.
fn contact(i: usize) -> QsoRecord {
    let tag = |name: &str, v: &str| format!("<{name}:{}>{v}", v.len());
    let mut s = String::new();
    s.push_str(&tag("CALL", &format!("K{i}AB")));
    s.push_str(&tag(
        "QSO_DATE",
        &format!("2026{:02}{:02}", 1 + i % 12, 1 + i % 28),
    ));
    s.push_str(&tag(
        "TIME_ON",
        &format!("{:02}{:02}{:02}", i % 24, i % 60, (i / 7) % 60),
    ));
    s.push_str(&tag("BAND", ["20m", "40m", "15m"][i % 3]));
    s.push_str(&tag("MODE", ["FT8", "SSB", "CW"][i % 3]));
    s.push_str(&tag("FREQ", "14.074000"));
    s.push_str(&tag("GRIDSQUARE", "FN31pr"));
    s.push_str(&tag("COUNTRY", "United States"));
    s.push_str(&tag("RST_SENT", "-10"));
    s.push_str(&tag("RST_RCVD", "-12"));
    s.push_str(&tag("COMMENT", "tnx for the contact, 73 and good DX"));
    s.push_str(&tag("NOTES", "a private note the operator keeps"));
    s.push_str(&tag("APP_OTHERLOG_ID", &format!("{i:08}")));
    s.push_str("<EOR>\n");
    let mut r = parse_adif(&s).remove(0);
    r.id = Some(id(i));
    r
}

fn id(i: usize) -> RecordId {
    RecordId::Provisional {
        hash: i as u64 + 1,
        ordinal: 0,
    }
}

/// A store holding `n` contacts.
fn store(dir: &std::path::Path, n: usize) -> LogDb {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    let mut db = LogDb::open(&dir.join("log.sqlite3")).unwrap();
    let rows: Vec<QsoRecord> = (0..n).map(contact).collect();
    db.insert_all(rows.iter().map(|r| (r, Resolved::default())))
        .unwrap();
    db
}

fn note(run: &str) -> Note {
    Note {
        run: run.into(),
        at_ms: 1_700_000_000_000,
        source: Source::Operator,
        edit: Kind::Edit,
        undoes: HashMap::new(),
    }
}

/// The peak of a purge of `n` contacts with history — naming every row, or `clear`ing the store —
/// and the peak of a read of the whole log, the control.
fn purge_peak(n: usize, clear: bool) -> (isize, isize) {
    let dir = std::env::temp_dir().join(format!(
        "nexus-history-peak-{n}-{clear}-{}",
        std::process::id()
    ));
    let mut db = store(&dir, n);
    let (log, control) = peak_during(|| db.load_all().unwrap());
    assert_eq!(log.len(), n, "premise: the store holds the log");
    drop(log);
    let ids: Vec<RecordId> = if clear {
        Vec::new()
    } else {
        (0..n).map(id).collect()
    };
    let purge = note("purge");
    let (made, peak) = peak_during(|| {
        db.apply(Batch {
            clear,
            remove: &ids,
            history: Some(&purge),
            ..Batch::default()
        })
    });
    made.unwrap();
    assert_eq!(
        db.row_count().unwrap(),
        0,
        "the purge took every contact out"
    );
    assert_eq!(
        db.history_run("purge").unwrap().len(),
        n,
        "and recorded each, whole"
    );
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
    (peak, control)
}

#[test]
fn a_purge_with_history_holds_one_chunk_of_the_log_not_the_log() {
    const SMALL: usize = 6_000;
    const BIG: usize = 24_000;
    for clear in [false, true] {
        let (small, _) = purge_peak(SMALL, clear);
        let (big, control) = purge_peak(BIG, clear);
        let mib = |b: isize| b as f64 / 1_048_576.0;
        println!(
            "purge ({}): {SMALL} contacts {:.1} MiB, {BIG} contacts {:.1} MiB; the whole log \
             read {:.1} MiB",
            if clear {
                "the store cleared"
            } else {
                "every row named"
            },
            mib(small),
            mib(big),
            mib(control)
        );
        assert!(
            control > 20 * 1_048_576,
            "the control: the instrument sees a whole log ({:.1} MiB)",
            mib(control)
        );
        assert!(
            big < control / 3,
            "a purge of {BIG} holds {:.1} MiB, a third or more of the whole log's {:.1} MiB",
            mib(big),
            mib(control)
        );
        assert!(
            big < small * 2,
            "a purge's peak grows with the log: {:.1} MiB at {SMALL}, {:.1} MiB at {BIG}",
            mib(small),
            mib(big)
        );
    }
}
