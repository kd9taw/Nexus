//! ★ SPEC-2 v3 C15: what the `log.adi` mirror holds while it writes — ONE chunk of the log,
//! whatever the log's size — measured inside its write loop, with a positive control.
//!
//! Its own test binary, because the measurement is a counting global allocator: every
//! allocation and free in the process moves a live-bytes counter, and every allocation raises a
//! high-water mark to it. The mark is the peak DURING the write, however briefly it was held —
//! not a sample taken after a chunk had already been dropped, which is how an earlier instrument
//! read "0 MiB" (SPEC-2 v3 §3.4). Alone in its binary, nothing else allocates beside it.
//!
//! The positive control is the thing the bound exists to forbid: the whole log as one `String`
//! (`mirror_adif`, what the lane built before C15) must trip the same bound.
//!
//! SQLite allocates through its own allocator, not Rust's, so its page cache is outside this
//! count: what is measured is the lane's own holding — the records it decodes, the text it
//! serialises, its buffers.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tempo_core::logbook::mirror::{mirror_adif, MirrorWriter, StoreSource};
use tempo_core::logbook::reader::LogReader;
use tempo_core::logbook::sqlite::{LogDb, Resolved};
use tempo_core::logbook::writer::LogWriter;
use tempo_core::logbook::Logbook;

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
fn peak_during(f: impl FnOnce()) -> isize {
    let base = LIVE.load(Ordering::SeqCst);
    PEAK.store(base, Ordering::SeqCst);
    f();
    PEAK.load(Ordering::SeqCst) - base
}

/// A log of `n` contacts the size a real one runs to — a few hundred bytes each in ADIF, with
/// the country, a note and a foreign tag most real rows carry.
fn log_text(n: usize) -> String {
    let mut s = tempo_core::logbook::adif_header();
    for i in 0..n {
        // A call of its own for each contact: an import keeps one of two identical contacts.
        let call = format!("K{i}AB");
        let tag = |name: &str, v: &str| format!("<{name}:{}>{v}", v.len());
        s.push_str(&tag("CALL", &call));
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
    }
    s
}

#[test]
fn the_mirror_holds_one_chunk_while_it_writes_and_a_whole_log_string_would_not_pass() {
    const CONTACTS: usize = 20_000;
    const CHUNK: usize = 256;
    let dir = std::env::temp_dir().join(format!("nexus-mirror-peak-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("log.sqlite3");
    let log_path = dir.join("log.adi");

    let mut log = Logbook::new();
    log.import_adif(&log_text(CONTACTS));
    let records: Vec<Arc<tempo_core::logbook::QsoRecord>> = log.records().to_vec();
    assert_eq!(records.len(), CONTACTS, "premise: every contact imported");
    {
        let mut db = LogDb::open(&db_path).unwrap();
        db.insert_all(records.iter().map(|r| (&**r, Resolved::default())))
            .unwrap();
    }
    let whole = mirror_adif(&records);

    let writer = Arc::new(LogWriter::start(LogDb::open(&db_path).unwrap()));
    let reader = Arc::new(LogReader::new(&db_path));
    let lane = MirrorWriter::with_timing(
        log_path.clone(),
        Arc::new(StoreSource::with_chunk(reader, Arc::clone(&writer), CHUNK)),
        Duration::from_millis(5),
        Duration::from_millis(20),
    );
    // A first write settles what is made once — the read connection, the lane's own state —
    // and gives the next write the right length to expect.
    lane.dirty(0);
    assert_eq!(lane.flush(Duration::from_secs(60)).writes, 1);
    // The second takes the day's snapshot of the file it replaces into the backup ring, which
    // reads that file whole — once a day, as every save of `log.adi` always has. Measured and
    // reported, not bounded: it is the ring's cost, outside the write loop.
    let with_snapshot = peak_during(|| {
        lane.dirty(0);
        assert_eq!(lane.flush(Duration::from_secs(60)).writes, 2);
    });
    assert!(
        dir.join("backups").is_dir(),
        "premise: the day's snapshot was taken"
    );

    // Every other write of the day: the one the bound is about.
    let streamed = peak_during(|| {
        lane.dirty(0);
        assert_eq!(lane.flush(Duration::from_secs(60)).writes, 3);
    });
    assert_eq!(
        std::fs::read(&log_path).unwrap(),
        whole.as_bytes(),
        "the streamed file is the mirror of the log, byte for byte"
    );

    // The positive control: the whole log as one `String`, as the lane built it before C15.
    let as_one_string = peak_during(|| {
        let text = mirror_adif(&records);
        assert_eq!(text.len(), whole.len());
    });

    // The bound: a quarter of the file. One chunk is 256 of 20,000 contacts — about 1% of the
    // log, decoded — plus the lane's 64 KiB buffer; the whole file is ~8 MB.
    let bound = (whole.len() / 4) as isize;
    eprintln!(
        "mirror peak: streamed {} KiB, with the day's snapshot {} KiB, one String {} KiB, \
         file {} KiB, bound {} KiB",
        streamed / 1024,
        with_snapshot / 1024,
        as_one_string / 1024,
        whole.len() / 1024,
        bound / 1024
    );
    assert!(
        streamed <= bound,
        "the lane held {streamed} bytes while writing, over the bound of {bound}"
    );
    assert!(
        as_one_string > bound,
        "positive control: the whole log as one String ({as_one_string} bytes) must trip the \
         bound ({bound}), or the bound proves nothing"
    );
    drop(lane);
    drop(writer);
    let _ = std::fs::remove_dir_all(&dir);
}
