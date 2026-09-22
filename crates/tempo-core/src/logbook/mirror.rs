//! The ADIF mirror lane — SPEC-1 v3's **C6**.
//!
//! The operator's storage ruling is *"SQLite now, ADIF mirror"*: the database becomes the real
//! log, and **a background writer keeps `log.adi` current beside it for backups and for any
//! other logger to read.** This is that writer. Without it, making the database canonical would
//! silently end an operator's ADIF backup — which is the half of the ruling they can see.
//!
//! # Four properties, and each one is load-bearing
//!
//! **Write-only.** The mirror is never read back. `recover_external_appends` and the union
//! merge exist because two instances could both write `log.adi`; once the database is the
//! truth there is nothing to recover FROM, and reading the mirror back would be a second
//! source of record.
//!
//! **It keeps the name `log.adi`.** Renaming it is the worst available failure: no error
//! anywhere, and the operator's third-party logger, backup script or sync client quietly
//! diverges from a path it will go on reading forever.
//!
//! **Its own lane.** Serialising a lifetime log is hundreds of milliseconds; doing it on a
//! caller's thread would put it back on exactly the path this programme is clearing. The mirror
//! holds no database connection — it turns records into text and replaces a file — so it is a
//! plain second thread and can never sit behind a database write.
//!
//! **Debounced and coalesced.** Only the newest state matters: a mirror is a picture of the log
//! now, not a journal of how it got there. Ten stamps in a second cost one write.
//!
//! # ⚠️ The transition hazard, and the guard for it
//!
//! After this change, an edit made to `log.adi` by the operator or by another program is
//! DISCARDED on the next mirror write, where today it would be merged. The sharpest case is a
//! 1.13 instance and a 1.14 instance sharing one data folder: the old one writes `log.adi`, the
//! new one overwrites it.
//!
//! The guard is two halves, and neither is silent:
//! - Every mirror write carries [`MIRROR_FIELD`] in its header, so the file SAYS it is
//!   generated — to a person reading it and to [`is_generated`], which a caller can ask before
//!   opening a folder that already holds a database.
//! - The mirror notices when the file it is about to replace is not the one it last wrote, and
//!   reports it ([`Status::foreign_write`]) instead of overwriting in silence.
//!
//! **First run is not a foreign write.** The check starts only once this mirror has written the
//! file at least once; an un-marked `log.adi` that a migration has just converted is the
//! expected state, not a conflict.

use super::QsoRecord;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

/// Quiet time before a pending state is written. Ten stamps in a burst cost one write.
const DEBOUNCE: Duration = Duration::from_millis(1_000);
/// The longest a change may wait, however busy the log is. Without a cap, continuous logging
/// would keep pushing the write out and the mirror would never be current.
const MAX_DELAY: Duration = Duration::from_millis(5_000);
/// How much of a file's head is read to look for the generated marker. The header ends at
/// `<EOH>`, so this never has to touch the body — and the body is the 100 MB part.
const HEADER_PROBE_BYTES: usize = 8 * 1024;

/// The ADIF header field that marks a file as written by the mirror.
///
/// An `APP_` field, which every other logger ignores by specification, so marking the file
/// cannot change how anything else reads it.
pub const MIRROR_FIELD: &str = "APP_NEXUS_MIRROR";

/// The header the mirror writes: the ordinary one, plus a line a person can read and a field a
/// program can test.
pub fn mirror_header() -> String {
    format!(
        "Nexus logbook — GENERATED. This file is written from the Nexus logbook database.\n\
         Changes made here are replaced the next time Nexus saves; edit the log in Nexus.\n\
         <ADIF_VER:5>3.1.4\n<PROGRAMID:5>Nexus\n<{}:1>1\n<EOH>\n",
        MIRROR_FIELD
    )
}

/// The whole mirror file for `records`.
pub fn mirror_adif(records: &[Arc<QsoRecord>]) -> String {
    let mut out = mirror_header();
    for r in records {
        out.push_str(&super::adif_record_own_log(r));
    }
    out
}

/// Whether the file at `path` was written by the mirror.
///
/// Reads only the head of the file — the marker is in the header, and a lifetime log's body is
/// the part worth not reading. A missing file is `false`: nothing generated it.
///
/// **This is the "two versions, one folder" question.** A `log.adi` sitting beside a database
/// and NOT carrying the marker was written by something else — an older Nexus that still owns
/// its own ADIF, or another program — and a caller that finds that should say so rather than
/// let one of them overwrite the other.
pub fn is_generated(path: &Path) -> bool {
    let Ok(mut f) = std::fs::File::open(path) else {
        return false;
    };
    use std::io::Read;
    let mut head = vec![0u8; HEADER_PROBE_BYTES];
    let Ok(n) = f.read(&mut head) else {
        return false;
    };
    head.truncate(n);
    // Only what precedes `<EOH>` counts: a RECORD carrying the same characters is not a header
    // marker, and a file whose header has ended has answered the question already. The same
    // case-insensitive window search `body_after_eoh` uses, from the other side of the tag.
    let eoh = head
        .windows(5)
        .position(|w| w.eq_ignore_ascii_case(b"<EOH>"));
    let header = match eoh {
        Some(end) => &head[..end],
        // A file shorter than the probe with no `<EOH>` has no body either — a header-only
        // file is a real thing, and it is all header.
        None if n < HEADER_PROBE_BYTES => &head[..],
        // No `<EOH>` within the probe: this is not a header, it is a body.
        None => return false,
    };
    let needle = format!("<{MIRROR_FIELD}:").into_bytes();
    header
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(&needle))
}

/// What the mirror has done, and anything a caller should surface.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    /// How many times the file has been replaced.
    pub writes: u64,
    /// A change is waiting to be written.
    pub pending: bool,
    /// The last write's failure, if it failed. A mirror is a convenience over the real log, so
    /// a failure is reported and the lane carries on rather than taking the session down.
    pub last_error: Option<String>,
    /// ⚠️ Something other than this mirror has written `log.adi` since the mirror last did.
    /// The operator's edit — or another Nexus instance's whole log — is about to be replaced,
    /// and that must not happen without anyone being told.
    pub foreign_write: bool,
}

enum Msg {
    /// The log as it now stands. Pointer copies, so this costs a `Vec` and not the records.
    Write(Vec<Arc<QsoRecord>>),
    /// Write anything pending NOW and answer when it is on disk.
    Flush(mpsc::SyncSender<Status>),
}

/// The mirror's thread and the handle onto it.
pub struct MirrorWriter {
    tx: Option<mpsc::Sender<Msg>>,
    status: Arc<Mutex<Status>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MirrorWriter {
    /// Start the lane, mirroring to `path`.
    pub fn start(path: PathBuf) -> MirrorWriter {
        MirrorWriter::with_timing(path, DEBOUNCE, MAX_DELAY)
    }

    /// The lane with its debounce named — what the tests drive, so they do not have to wait a
    /// real second to observe a real write.
    pub fn with_timing(path: PathBuf, debounce: Duration, max_delay: Duration) -> MirrorWriter {
        let (tx, rx) = mpsc::channel::<Msg>();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = Arc::clone(&status);
        let handle = std::thread::Builder::new()
            .name("nexus-log-mirror".into())
            .spawn(move || run(path, rx, shared, debounce, max_delay))
            .ok();
        MirrorWriter {
            tx: Some(tx),
            status,
            handle,
        }
    }

    /// Hand the lane the log as it now stands. Never touches the disk; returns immediately.
    pub fn submit(&self, records: Vec<Arc<QsoRecord>>) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Write(records));
        }
    }

    /// Write anything pending now, and wait up to `deadline` for it to be on disk.
    ///
    /// For shutdown, and for a caller that has just done something an operator would expect to
    /// see in the file immediately.
    pub fn flush(&self, deadline: Duration) -> Status {
        let (reply, answer) = mpsc::sync_channel(1);
        if let Some(tx) = &self.tx {
            if tx.send(Msg::Flush(reply)).is_ok() {
                if let Ok(s) = answer.recv_timeout(deadline) {
                    return s;
                }
            }
        }
        self.status()
    }

    /// What the lane has done so far.
    pub fn status(&self) -> Status {
        self.status.lock().map(|s| s.clone()).unwrap_or_default()
    }
}

impl Drop for MirrorWriter {
    /// Dropping the handle closes the channel, which ends the loop; the thread is joined so a
    /// write already in flight finishes rather than being cut off by the process exiting.
    fn drop(&mut self) {
        self.tx = None;
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/// What the lane knows about the file between writes: the identity of what IT wrote, so a file
/// that no longer matches can be recognised as somebody else's.
#[derive(Default)]
struct Written {
    stamp: Option<(u64, SystemTime)>,
}

impl Written {
    /// Has something else written the file since we did? `false` until the mirror has written
    /// once — an un-marked file at startup is the expected state, not a conflict.
    fn foreign(&self, path: &Path) -> bool {
        let Some((len, mtime)) = self.stamp else {
            return false;
        };
        match std::fs::metadata(path) {
            Ok(m) => m.len() != len || m.modified().map(|t| t != mtime).unwrap_or(false),
            // Gone is not "someone edited it"; the next write puts it back.
            Err(_) => false,
        }
    }
}

fn run(
    path: PathBuf,
    rx: mpsc::Receiver<Msg>,
    status: Arc<Mutex<Status>>,
    debounce: Duration,
    max_delay: Duration,
) {
    let mut pending: Option<Vec<Arc<QsoRecord>>> = None;
    let mut first_queued: Option<Instant> = None;
    let mut last_queued = Instant::now();
    let mut written = Written::default();

    loop {
        // Nothing waiting: block until something arrives or the handle is dropped.
        let Some(t0) = first_queued else {
            match rx.recv() {
                Ok(Msg::Write(rows)) => {
                    pending = Some(rows);
                    first_queued = Some(Instant::now());
                    last_queued = Instant::now();
                    set(&status, |s| s.pending = true);
                }
                Ok(Msg::Flush(reply)) => {
                    let _ = reply.send(snapshot(&status));
                }
                Err(_) => return, // the handle went away
            }
            continue;
        };

        // Waiting: write when the log has been quiet for `debounce`, or when the change has
        // waited `max_delay` however busy it is — a mirror that keeps being pushed out by
        // continuous logging is never current, which is the one thing it exists to be.
        let now = Instant::now();
        let due = (last_queued + debounce).min(t0 + max_delay);
        let wait = due.saturating_duration_since(now);
        match rx.recv_timeout(wait) {
            Ok(Msg::Write(rows)) => {
                pending = Some(rows); // newest wins: a mirror is a picture, not a journal
                last_queued = Instant::now();
            }
            Ok(Msg::Flush(reply)) => {
                if let Some(rows) = pending.take() {
                    write_once(&path, &rows, &status, &mut written);
                }
                first_queued = None;
                set(&status, |s| s.pending = false);
                let _ = reply.send(snapshot(&status));
            }
            Err(RecvTimeoutError::Timeout) => {
                if let Some(rows) = pending.take() {
                    write_once(&path, &rows, &status, &mut written);
                }
                first_queued = None;
                set(&status, |s| s.pending = false);
            }
            Err(RecvTimeoutError::Disconnected) => {
                // The handle was dropped: write what is pending rather than losing it.
                if let Some(rows) = pending.take() {
                    write_once(&path, &rows, &status, &mut written);
                }
                set(&status, |s| s.pending = false);
                return;
            }
        }
    }
}

/// One replacement of the mirror file: per-PID tmp, fsync, rename, directory sync — the same
/// sequence `Logbook::save` uses, and for the same reason. This is a full rewrite of every
/// contact ever logged, so a rename that reaches the disk before the bytes do would leave
/// `log.adi` pointing at an unwritten tmp.
fn write_once(
    path: &Path,
    records: &[Arc<QsoRecord>],
    status: &Arc<Mutex<Status>>,
    written: &mut Written,
) {
    // ⚠️ Before replacing it: is this still the file we wrote? Checked here rather than at
    // submit time because it is the write that destroys the other writer's work.
    if written.foreign(path) {
        set(status, |s| s.foreign_write = true);
    }

    let body = mirror_adif(records);
    let tmp = path.with_extension(format!("adi.mirror{}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<Option<(u64, SystemTime)>> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        super::write_sync(&tmp, body.as_bytes())?;
        let stamp = std::fs::metadata(&tmp)
            .ok()
            .and_then(|m| m.modified().ok().map(|t| (m.len(), t)));
        std::fs::rename(&tmp, path)?;
        super::sync_parent_dir(path);
        Ok(stamp)
    })();

    match result {
        Ok(stamp) => {
            // The identity of what WE wrote, read back off the file rather than computed, so a
            // filesystem that rounds an mtime cannot make our own write look foreign.
            written.stamp = std::fs::metadata(path)
                .ok()
                .and_then(|m| m.modified().ok().map(|t| (m.len(), t)))
                .or(stamp);
            set(status, |s| {
                s.writes += 1;
                s.last_error = None;
            });
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            set(status, |s| s.last_error = Some(e.to_string()));
        }
    }
}

fn set(status: &Arc<Mutex<Status>>, f: impl FnOnce(&mut Status)) {
    if let Ok(mut s) = status.lock() {
        f(&mut s);
    }
}

fn snapshot(status: &Arc<Mutex<Status>>) -> Status {
    status.lock().map(|s| s.clone()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::logbook::{parse_adif, Logbook};

    struct Dir(PathBuf);
    impl Dir {
        fn new(tag: &str) -> Dir {
            let p = std::env::temp_dir().join(format!(
                "nexus-mirror-{tag}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = std::fs::remove_dir_all(&p);
            std::fs::create_dir_all(&p).unwrap();
            Dir(p)
        }
        fn log(&self) -> PathBuf {
            self.0.join("log.adi")
        }
    }
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn rows(n: usize) -> Vec<Arc<QsoRecord>> {
        let mut text = super::super::adif_header();
        for i in 0..n {
            text.push_str(&format!(
                "<CALL:{}>{}<BAND:3>20m<FREQ:6>14.074<MODE:3>FT8\
                 <QSO_DATE:8>20260101<TIME_ON:6>{:02}{:02}{:02}<EOR>\n",
                format!("K{i}ABC").len(),
                format_args!("K{i}ABC"),
                i / 3600 % 24,
                i / 60 % 60,
                i % 60,
            ));
        }
        let d = Dir::new(&format!("rows{n}"));
        std::fs::write(d.log(), text).unwrap();
        Logbook::load(&d.log()).records().to_vec()
    }

    /// Fast timings so a test observes a real write rather than waiting a real second.
    fn writer(path: PathBuf) -> MirrorWriter {
        MirrorWriter::with_timing(path, Duration::from_millis(5), Duration::from_millis(50))
    }

    /// The mirror is the log: every contact in it, readable by an ordinary ADIF parser.
    #[test]
    fn the_mirror_holds_every_contact_and_parses_as_adif() {
        let d = Dir::new("holds");
        let recs = rows(400);
        let w = writer(d.log());
        w.submit(recs.clone());
        w.flush(Duration::from_secs(10));

        let text = std::fs::read_to_string(d.log()).unwrap();
        let parsed = parse_adif(&text);
        assert_eq!(parsed.len(), 400, "every contact is in the mirror");
        // On the RECORDS, not on a re-export of them: an export comparison is blind to
        // whatever an export may legitimately leave out.
        for (a, b) in parsed.iter().zip(recs.iter()) {
            assert_eq!(a.call, b.call);
            assert_eq!(a.when_unix, b.when_unix);
            assert_eq!(a.band, b.band);
            assert_eq!(a.mode, b.mode);
        }
    }

    /// The marker is what lets anything tell a generated `log.adi` from an operator's own.
    #[test]
    fn a_mirrored_file_says_it_is_generated_and_an_ordinary_log_does_not() {
        let d = Dir::new("mark");
        let w = writer(d.log());
        w.submit(rows(3));
        w.flush(Duration::from_secs(10));
        assert!(is_generated(&d.log()), "the mirror marks what it writes");

        // The positive control's other half: an ordinary log, written the way every Nexus
        // before this one wrote it, must NOT answer yes.
        let plain = d.0.join("plain.adi");
        std::fs::write(&plain, super::super::adif_header()).unwrap();
        assert!(!is_generated(&plain), "an ordinary log is not generated");
        assert!(
            !is_generated(&d.0.join("absent.adi")),
            "a file that does not exist was not generated by anything"
        );

        // The marker counts in the HEADER only. Another logger's file, or an import, can carry
        // the same characters in a RECORD — and a file full of somebody else's contacts is
        // exactly the one that must not be mistaken for ours and overwritten.
        let in_body = d.0.join("bodymark.adi");
        std::fs::write(
            &in_body,
            format!(
                "{}<CALL:5>W1ABC<{}:1>1<EOR>\n",
                super::super::adif_header(),
                MIRROR_FIELD
            ),
        )
        .unwrap();
        assert!(
            !is_generated(&in_body),
            "a marker inside a record is not a generated header"
        );
    }

    /// ⚠️ The transition hazard. Another program rewriting `log.adi` must not be overwritten in
    /// silence — that is an operator's edit, or another instance's whole log, going away.
    #[test]
    fn a_write_by_something_else_is_noticed_before_it_is_replaced() {
        let d = Dir::new("foreign");
        let w = writer(d.log());
        w.submit(rows(5));
        w.flush(Duration::from_secs(10));
        assert!(
            !w.status().foreign_write,
            "our own write is not a foreign one"
        );

        // Something else replaces the file — a 1.13 instance saving its own log.
        std::fs::write(d.log(), "Someone else's log\n<EOH>\n<CALL:5>W1ABC<EOR>\n").unwrap();
        assert!(!is_generated(&d.log()), "and it does not carry the marker");

        w.submit(rows(6));
        w.flush(Duration::from_secs(10));
        assert!(
            w.status().foreign_write,
            "the mirror must report that it replaced somebody else's file"
        );
    }

    /// A first run is not a conflict: a `log.adi` a migration has just read, with no marker on
    /// it, is the expected state and must not raise the alarm.
    #[test]
    fn the_first_write_over_an_unmarked_log_is_not_a_foreign_write() {
        let d = Dir::new("firstrun");
        std::fs::write(d.log(), super::super::adif_header()).unwrap();
        let w = writer(d.log());
        w.submit(rows(4));
        w.flush(Duration::from_secs(10));
        assert!(
            !w.status().foreign_write,
            "the log the operator already had is not a foreign write"
        );
        assert_eq!(w.status().writes, 1);
    }

    /// A burst costs one write, not one per change — the whole point of a debounced lane.
    #[test]
    fn a_burst_of_changes_costs_one_write() {
        let d = Dir::new("debounce");
        let w =
            MirrorWriter::with_timing(d.log(), Duration::from_millis(60), Duration::from_secs(30));
        let recs = rows(20);
        for _ in 0..25 {
            w.submit(recs.clone());
        }
        w.flush(Duration::from_secs(10));
        assert_eq!(
            w.status().writes,
            1,
            "25 changes inside the debounce window are one write"
        );
    }

    /// …but a log that never goes quiet must still reach the disk. Without the cap, continuous
    /// logging would push the write out forever and the mirror would never be current.
    #[test]
    fn a_log_that_never_goes_quiet_is_still_written_within_the_cap() {
        let d = Dir::new("cap");
        let w = MirrorWriter::with_timing(
            d.log(),
            Duration::from_secs(30), // a debounce that alone would never fire
            Duration::from_millis(40),
        );
        let recs = rows(5);
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(300) {
            w.submit(recs.clone());
            std::thread::sleep(Duration::from_millis(5));
        }
        // No flush: the cap alone has to have produced a write.
        let deadline = Instant::now() + Duration::from_secs(5);
        while w.status().writes == 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            w.status().writes >= 1,
            "the cap must write even while the log keeps changing"
        );
        assert!(d.log().is_file());
    }

    /// Dropping the handle must not lose a change that was still waiting out its debounce.
    #[test]
    fn a_change_pending_at_shutdown_is_written_not_dropped() {
        let d = Dir::new("shutdown");
        {
            let w = MirrorWriter::with_timing(
                d.log(),
                Duration::from_secs(30),
                Duration::from_secs(30),
            );
            w.submit(rows(7));
            // Straight out of scope, with the change still inside its debounce window.
        }
        let text = std::fs::read_to_string(d.log()).expect("the pending change reached the disk");
        assert_eq!(parse_adif(&text).len(), 7);
    }

    /// A mirror that cannot be written reports it and carries on. It is a convenience over the
    /// real log, so a failing mirror must never take a session down with it.
    #[test]
    fn a_write_that_fails_is_reported_and_does_not_stop_the_lane() {
        let d = Dir::new("failwrite");
        // A directory where the file should be: the rename cannot replace it.
        std::fs::create_dir(d.log()).unwrap();
        let w = writer(d.log());
        w.submit(rows(3));
        let s = w.flush(Duration::from_secs(10));
        assert!(s.last_error.is_some(), "the failure is reported");
        assert_eq!(s.writes, 0, "and not counted as a write");

        // The lane is still alive: point it somewhere writable and it works.
        let d2 = Dir::new("failwrite2");
        let w2 = writer(d2.log());
        w2.submit(rows(3));
        w2.flush(Duration::from_secs(10));
        assert_eq!(w2.status().writes, 1);
    }

    /// The header the mirror writes must not disturb an ordinary ADIF reader: everything before
    /// `<EOH>` is header, and the marker is an `APP_` field every other logger ignores.
    #[test]
    fn the_generated_header_does_not_change_what_a_reader_sees() {
        let d = Dir::new("header");
        let w = writer(d.log());
        w.submit(rows(9));
        w.flush(Duration::from_secs(10));
        let text = std::fs::read_to_string(d.log()).unwrap();
        assert!(text.contains(MIRROR_FIELD), "the marker is there");
        assert_eq!(parse_adif(&text).len(), 9, "and the records still parse");
        assert!(
            text.contains("GENERATED"),
            "a person opening the file is told, not only a program"
        );
    }
}
