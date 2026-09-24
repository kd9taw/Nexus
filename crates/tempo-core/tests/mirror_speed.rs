//! SPEC-2 v3 §4.5's target for the streamed mirror: at most 1.5× the time `mirror_adif` took
//! over the log in memory. A measurement, not a gate — ignored by default, and run in a release
//! build:
//!
//! ```text
//! cargo test --release -p tempo-core --test mirror_speed -- --ignored --nocapture
//! NEXUS_MIRROR_BENCH_N=500000 cargo test --release -p tempo-core --test mirror_speed -- --ignored --nocapture
//! ```
//!
//! Its own binary so no other test (or the counting allocator of `mirror_memory`) shares the
//! process with it. It checks the two files are the same bytes, and prints the times.

use std::sync::Arc;
use std::time::{Duration, Instant};

use tempo_core::logbook::mirror::{mirror_adif, MirrorWriter, StoreSource};
use tempo_core::logbook::reader::LogReader;
use tempo_core::logbook::sqlite::{LogDb, Resolved};
use tempo_core::logbook::writer::LogWriter;
use tempo_core::logbook::Logbook;

/// `n` contacts of the size a real log's run to, each distinct.
fn log_text(n: usize) -> String {
    let mut s = tempo_core::logbook::adif_header();
    for i in 0..n {
        let tag = |name: &str, v: &str| format!("<{name}:{}>{v}", v.len());
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
        s.push_str(&tag("STATE", "WI"));
        s.push_str(&tag("RST_SENT", "-10"));
        s.push_str(&tag("RST_RCVD", "-12"));
        s.push_str(&tag("COMMENT", "tnx for the contact, 73 and good DX"));
        s.push_str(&tag("APP_OTHERLOG_ID", &format!("{i:08}")));
        s.push_str(&tag("APP_TEMPO_UL_QRZ", "accepted|1700000000|"));
        s.push_str("<EOR>\n");
    }
    s
}

#[test]
#[ignore = "a measurement: run in release with --ignored"]
fn the_streamed_mirror_against_mirror_adif() {
    let n: usize = std::env::var("NEXUS_MIRROR_BENCH_N")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(150_000);
    let dir = std::env::temp_dir().join(format!("nexus-mirror-speed-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let db_path = dir.join("log.sqlite3");
    let log_path = dir.join("log.adi");

    let mut log = Logbook::new();
    log.import_adif(&log_text(n));
    let records = log.records().to_vec();
    assert_eq!(records.len(), n);
    {
        let mut db = LogDb::open(&db_path).unwrap();
        db.insert_all(records.iter().map(|r| (&**r, Resolved::default())))
            .unwrap();
    }

    let mut in_memory = Vec::new();
    let mut stage1 = Vec::new();
    let mut whole = String::new();
    for _ in 0..3 {
        let t = Instant::now();
        whole = mirror_adif(&records);
        in_memory.push(t.elapsed());
        // Stage 1's whole write, the ring aside: the text, then a written, fsynced, renamed file.
        let t = Instant::now();
        let text = mirror_adif(&records);
        let tmp = dir.join("stage1.tmp");
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp).unwrap();
            f.write_all(text.as_bytes()).unwrap();
            f.sync_all().unwrap();
        }
        std::fs::rename(&tmp, dir.join("stage1.adi")).unwrap();
        stage1.push(t.elapsed());
    }
    // The store's read alone: every record decoded, nothing written.
    let mut decode = Vec::new();
    for _ in 0..3 {
        let db = LogDb::open(&db_path).unwrap();
        let t = Instant::now();
        let mut seen = 0usize;
        db.each_record(tempo_core::logbook::sqlite::RECORD_CHUNK, &mut |_| {
            seen += 1;
            std::ops::ControlFlow::Continue(())
        })
        .unwrap();
        decode.push(t.elapsed());
        assert_eq!(seen, n);
    }

    let writer = Arc::new(LogWriter::start(LogDb::open(&db_path).unwrap()));
    let lane = MirrorWriter::with_timing(
        log_path.clone(),
        Arc::new(StoreSource::new(
            Arc::new(LogReader::new(&db_path)),
            Arc::clone(&writer),
        )),
        Duration::from_millis(1),
        Duration::from_millis(5),
    );
    // Two writes first: the file, then the day's ring snapshot. Then the ordinary ones.
    for w in 1..=2 {
        lane.dirty(0);
        assert_eq!(lane.flush(Duration::from_secs(600)).writes, w);
    }
    let mut streamed = Vec::new();
    for w in 3..=5 {
        let t = Instant::now();
        lane.dirty(0);
        assert_eq!(lane.flush(Duration::from_secs(600)).writes, w);
        streamed.push(t.elapsed());
    }
    assert!(
        std::fs::read(&log_path).unwrap() == whole.as_bytes(),
        "the same bytes"
    );
    let best = |v: &[Duration]| v.iter().min().copied().unwrap_or_default();
    let ms = |v: &[Duration]| best(v).as_secs_f64() * 1e3;
    eprintln!(
        "S2T mirror n={n} bytes={} mirror_adif_ms={:.0} stage1_write_ms={:.0} store_decode_ms={:.0} \
         streamed_write_ms={:.0} streamed/mirror_adif={:.2} streamed/stage1_write={:.2}",
        whole.len(),
        ms(&in_memory),
        ms(&stage1),
        ms(&decode),
        ms(&streamed),
        ms(&streamed) / ms(&in_memory),
        ms(&streamed) / ms(&stage1)
    );
    drop(lane);
    drop(writer);
    let _ = std::fs::remove_dir_all(&dir);
}
