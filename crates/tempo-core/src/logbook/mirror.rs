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
//! Once the database is the log, anything written into `log.adi` by something other than this
//! mirror is not in the database. The sharpest case is a 1.13 instance and a 1.14 instance
//! sharing one data folder — or the same machine going back to 1.13 for a day and forward again:
//! the old build appends its contacts to `log.adi`, and the next mirror write would replace the
//! file with the database's picture and take those contacts with it.
//!
//! **So the mirror never replaces a file it cannot account for.** Three halves, none silent:
//! - Every mirror write is SELF-DESCRIBING: its header carries [`MIRROR_FIELD`], whose value is
//!   the byte length of everything after `<EOH>`. A file whose header says so and whose length
//!   agrees is a picture some Nexus mirror took ([`mirror_state`] → [`MirrorState::Pristine`]),
//!   whichever process wrote it. A 1.13 save drops the marker (it writes its own header), and a
//!   1.13 append leaves the marker and changes the length, so both read as
//!   [`MirrorState::Foreign`].
//! - The writer replaces only a file that is absent, pristine, the file the store has already
//!   taken in ([`MirrorOptions::accepted`] — the operator's own `log.adi` just converted or
//!   imported), or the one this mirror last wrote. Anything else is LEFT WHERE IT IS, reported
//!   ([`Status::foreign_write`]) and logged, and mirroring stops until the store has taken the
//!   file in — the caller imports it at the next open.
//! - The header also says, in words, that the file is generated.
//!
//! A mirror that stops is a convenience lost for a while. A mirror that overwrote would be
//! contacts lost for good, and nothing else in the app still holds them.

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

/// The ADIF header field that marks a file as written by the mirror. Its VALUE is the byte
/// length of everything after `<EOH>`, which is what lets [`mirror_state`] tell a picture the
/// mirror took from one somebody has since appended to.
///
/// An `APP_` field, which every other logger ignores by specification, so marking the file
/// cannot change how anything else reads it.
pub const MIRROR_FIELD: &str = "APP_NEXUS_MIRROR";

/// The header the mirror writes for a body of `body_len` bytes (everything after `<EOH>`): the
/// ordinary one, plus a line a person can read and a field a program can test.
pub fn mirror_header(body_len: usize) -> String {
    let len = body_len.to_string();
    format!(
        "Nexus logbook — GENERATED. This file is written from the Nexus logbook database.\n\
         Changes made here are replaced the next time Nexus saves; edit the log in Nexus.\n\
         <ADIF_VER:5>3.1.4\n<PROGRAMID:5>Nexus\n<{MIRROR_FIELD}:{}>{len}\n<EOH>",
        len.len()
    )
}

/// The whole mirror file for `records`.
pub fn mirror_adif(records: &[Arc<QsoRecord>]) -> String {
    let mut body = String::from("\n");
    for r in records {
        body.push_str(&super::adif_record_own_log(r));
    }
    let mut out = mirror_header(body.len());
    out.push_str(&body);
    out
}

/// What the file at a `log.adi` path is, as far as the mirror is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MirrorState {
    /// Nothing there.
    Absent,
    /// A picture a Nexus mirror took, unchanged since: the marker is in the header and the
    /// length it records is the length the file has.
    Pristine,
    /// Anything else — an operator's own log, a 1.13 save (its header carries no marker), a
    /// mirror something has appended to since (the length no longer agrees), or a file that
    /// cannot be read. The store must take it in before anything may replace it.
    Foreign,
}

/// Classify the file at `path`. Reads only the head of the file and its length — the body is
/// the 100 MB part, and a length is all that an append changes.
pub fn mirror_state(path: &Path) -> MirrorState {
    let Ok(meta) = std::fs::metadata(path) else {
        return MirrorState::Absent;
    };
    // Not a file at all (a directory in the way): nothing a rename could destroy — the write
    // fails, loudly, on its own.
    if !meta.is_file() {
        return MirrorState::Absent;
    }
    let Some((eoh_end, recorded)) = read_marker(path) else {
        return MirrorState::Foreign;
    };
    if meta.len() == eoh_end + recorded {
        MirrorState::Pristine
    } else {
        MirrorState::Foreign
    }
}

/// The marker's value, and the offset just past `<EOH>`: `None` when the header carries no
/// marker, or one whose value is not a length.
fn read_marker(path: &Path) -> Option<(u64, u64)> {
    let head = read_head(path)?;
    let eoh = head
        .windows(5)
        .position(|w| w.eq_ignore_ascii_case(b"<EOH>"))?;
    let header = &head[..eoh];
    let needle = format!("<{MIRROR_FIELD}:").into_bytes();
    let at = header
        .windows(needle.len())
        .position(|w| w.eq_ignore_ascii_case(&needle))?;
    // `<APP_NEXUS_MIRROR:N>value` — N is the length of the value, per ADIF.
    let rest = &header[at + needle.len()..];
    let close = rest.iter().position(|&b| b == b'>')?;
    let n: usize = std::str::from_utf8(&rest[..close]).ok()?.parse().ok()?;
    let value = rest.get(close + 1..close + 1 + n)?;
    let recorded: u64 = std::str::from_utf8(value).ok()?.parse().ok()?;
    Some((eoh as u64 + 5, recorded))
}

fn read_head(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut head = vec![0u8; HEADER_PROBE_BYTES];
    let mut n = 0;
    // `read` may return short; the header is small, so read until it is full or the file ends.
    while n < head.len() {
        match f.read(&mut head[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(_) => return None,
        }
    }
    head.truncate(n);
    Some(head)
}

/// A file's identity as far as the mirror can tell without reading it: length and mtime.
pub type FileStamp = (u64, SystemTime);

/// The stamp of the file at `path`, if it can be read.
pub fn file_stamp(path: &Path) -> Option<FileStamp> {
    std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok().map(|t| (m.len(), t)))
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
    /// ⚠️ `log.adi` holds something the mirror cannot account for — not a pristine mirror, not
    /// the file the store took in, not this mirror's own last write — so it was LEFT AS IT IS
    /// and mirroring has stopped. Set until the store takes the file in and says so
    /// ([`MirrorWriter::accept`]).
    pub foreign_write: bool,
    /// The file the refusal was about, so the store can take in exactly that one.
    pub foreign_stamp: Option<FileStamp>,
}

enum Msg {
    /// The log as it now stands. Pointer copies, so this costs a `Vec` and not the records.
    Write(Vec<Arc<QsoRecord>>),
    /// Write anything pending NOW and answer when it is on disk.
    Flush(mpsc::SyncSender<Status>),
    /// The store has taken in the file with this stamp: it may be replaced from now on.
    Accept(FileStamp),
}

/// How a mirror lane is set up.
#[derive(Debug, Clone)]
pub struct MirrorOptions {
    /// Quiet time before a pending state is written.
    pub debounce: Duration,
    /// The longest a change may wait, however busy the log is.
    pub max_delay: Duration,
    /// The stamp of a `log.adi` the store has ALREADY TAKEN IN — the operator's own file, just
    /// converted, or a foreign one just imported. It may be replaced even though it is not a
    /// pristine mirror, for exactly as long as it still has this stamp.
    pub accepted: Option<FileStamp>,
}

impl Default for MirrorOptions {
    fn default() -> Self {
        MirrorOptions {
            debounce: DEBOUNCE,
            max_delay: MAX_DELAY,
            accepted: None,
        }
    }
}

/// The mirror's thread and the handle onto it.
pub struct MirrorWriter {
    tx: Option<mpsc::Sender<Msg>>,
    status: Arc<Mutex<Status>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

impl MirrorWriter {
    /// Start the lane, mirroring to `path`, with nothing accepted: only an absent file, a
    /// pristine mirror or this lane's own write may be replaced.
    pub fn start(path: PathBuf) -> MirrorWriter {
        MirrorWriter::with_options(path, MirrorOptions::default())
    }

    /// The lane with its debounce named — what the tests drive, so they do not have to wait a
    /// real second to observe a real write.
    pub fn with_timing(path: PathBuf, debounce: Duration, max_delay: Duration) -> MirrorWriter {
        MirrorWriter::with_options(
            path,
            MirrorOptions {
                debounce,
                max_delay,
                accepted: None,
            },
        )
    }

    /// The lane, set up in full.
    pub fn with_options(path: PathBuf, options: MirrorOptions) -> MirrorWriter {
        let (tx, rx) = mpsc::channel::<Msg>();
        let status = Arc::new(Mutex::new(Status::default()));
        let shared = Arc::clone(&status);
        let handle = std::thread::Builder::new()
            .name("nexus-log-mirror".into())
            .spawn(move || run(path, rx, shared, options))
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

    /// The store has taken in the file with `stamp` (see [`Status::foreign_stamp`]): it may be
    /// replaced, for as long as it still has that stamp.
    pub fn accept(&self, stamp: FileStamp) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Accept(stamp));
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

/// What the lane knows about the file between writes: the file it may replace even though it
/// is not a pristine mirror — its own last write, or the one the store took in.
struct Written {
    ours: Option<FileStamp>,
    accepted: Option<FileStamp>,
    /// The foreign stamp last reported, so a refusal is logged once per file and not once per
    /// debounce.
    reported: Option<FileStamp>,
}

impl Written {
    /// May the file at `path` be replaced? Absent, pristine, our own last write, or the file the
    /// store took in — and nothing else.
    fn may_replace(&self, path: &Path) -> Result<(), Option<FileStamp>> {
        match mirror_state(path) {
            MirrorState::Absent | MirrorState::Pristine => Ok(()),
            MirrorState::Foreign => {
                let now = file_stamp(path);
                match now {
                    Some(s) if Some(s) == self.ours || Some(s) == self.accepted => Ok(()),
                    other => Err(other),
                }
            }
        }
    }
}

fn run(path: PathBuf, rx: mpsc::Receiver<Msg>, status: Arc<Mutex<Status>>, opts: MirrorOptions) {
    super::io_fence::enter_log_lane();
    let (debounce, max_delay) = (opts.debounce, opts.max_delay);
    let mut pending: Option<Vec<Arc<QsoRecord>>> = None;
    let mut first_queued: Option<Instant> = None;
    let mut last_queued = Instant::now();
    let mut written = Written {
        ours: None,
        accepted: opts.accepted,
        reported: None,
    };

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
                Ok(Msg::Accept(stamp)) => written.accepted = Some(stamp),
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
            Ok(Msg::Accept(stamp)) => written.accepted = Some(stamp),
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
///
/// ⛔ **It refuses to replace a file it cannot account for** (see the module header), and the
/// check is HERE, at the write, because the write is what would destroy the other writer's
/// contacts. The dated backup ring is kept too, exactly as the whole-file save kept it: a
/// snapshot of the file about to be replaced, once a day and always before a write that makes
/// it smaller — the copy that catches a purge, or a log that lost rows somewhere upstream.
fn write_once(
    path: &Path,
    records: &[Arc<QsoRecord>],
    status: &Arc<Mutex<Status>>,
    written: &mut Written,
) {
    // Everything below is disk: the head of `log.adi`, the ring snapshot, the temporary file,
    // the rename and the folder sync.
    super::io_fence::on_log_lane("the log.adi mirror's rewrite");
    if let Err(stamp) = written.may_replace(path) {
        if written.reported != stamp {
            written.reported = stamp;
            crate::applog::error(
                "logbook",
                &format!(
                    "{} was changed by something other than Nexus since Nexus last wrote it. It has \
                     been left exactly as it is, and Nexus will take its contacts into the \
                     logbook the next time it starts. Until then the file is not kept up to date.",
                    path.display()
                ),
            );
        }
        set(status, |s| {
            s.foreign_write = true;
            s.foreign_stamp = stamp;
        });
        return;
    }

    let body = mirror_adif(records);
    let tmp = path.with_extension(format!("adi.mirror{}.tmp", std::process::id()));
    let result = (|| -> std::io::Result<Option<(u64, SystemTime)>> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        super::write_sync(&tmp, body.as_bytes())?;
        let stamp = file_stamp(&tmp);
        // The ring: best-effort, and BEFORE the rename publishes the new file, so the snapshot
        // is of the one being replaced.
        super::Logbook::snapshot_before_save(
            path,
            body.len() as u64,
            super::now_unix(),
            super::BACKUP_KEEP,
            super::BACKUP_TOTAL_BYTES,
        );
        std::fs::rename(&tmp, path)?;
        super::sync_parent_dir(path);
        Ok(stamp)
    })();

    match result {
        Ok(stamp) => {
            // The identity of what WE wrote, read back off the file rather than computed, so a
            // filesystem that rounds an mtime cannot make our own write look foreign.
            written.ours = file_stamp(path).or(stamp);
            written.reported = None;
            set(status, |s| {
                s.writes += 1;
                s.last_error = None;
                s.foreign_write = false;
                s.foreign_stamp = None;
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

    /// ⛔ The transition hazard. Another program rewriting `log.adi` — a 1.13 instance saving its
    /// own log — must NOT be overwritten: those contacts are in no database, and replacing the
    /// file would be the last copy of them going away. The mirror leaves the file exactly as it
    /// is, says so, and stops until the store has taken the file in.
    #[test]
    fn a_write_by_something_else_is_left_alone_and_reported() {
        let d = Dir::new("foreign");
        let w = writer(d.log());
        w.submit(rows(5));
        w.flush(Duration::from_secs(10));
        assert!(
            !w.status().foreign_write,
            "our own write is not a foreign one"
        );
        assert_eq!(mirror_state(&d.log()), MirrorState::Pristine);

        // Something else replaces the file — a 1.13 instance saving its own log.
        let theirs = "Someone else's log\n<EOH>\n<CALL:5>W1ABC<EOR>\n";
        std::fs::write(d.log(), theirs).unwrap();
        assert!(!is_generated(&d.log()), "and it does not carry the marker");

        w.submit(rows(6));
        let s = w.flush(Duration::from_secs(10));
        assert!(s.foreign_write, "the refusal is reported");
        assert_eq!(
            s.foreign_stamp,
            file_stamp(&d.log()),
            "naming the file it refused"
        );
        assert_eq!(s.writes, 1, "and the refusal is not counted as a write");
        assert_eq!(
            std::fs::read_to_string(d.log()).unwrap(),
            theirs,
            "the other writer's file is exactly as it left it"
        );

        // The store takes the file in and says so: now it may be replaced.
        w.accept(s.foreign_stamp.expect("stamp"));
        w.submit(rows(6));
        let s = w.flush(Duration::from_secs(10));
        assert!(
            !s.foreign_write,
            "the refusal clears once the file is accepted"
        );
        assert_eq!(s.writes, 2);
        assert_eq!(
            parse_adif(&std::fs::read_to_string(d.log()).unwrap()).len(),
            6
        );
    }

    /// ⛔ The quiet version of the same hazard: a 1.13 instance APPENDING to the mirror. The
    /// marker is still in the header — only the length has moved — and the appended contact
    /// must not be replaced away.
    #[test]
    fn a_contact_appended_to_the_mirror_by_something_else_is_not_replaced_away() {
        let d = Dir::new("appended");
        let w = writer(d.log());
        w.submit(rows(3));
        w.flush(Duration::from_secs(10));
        assert_eq!(mirror_state(&d.log()), MirrorState::Pristine, "control");

        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(d.log())
            .unwrap();
        use std::io::Write as _;
        f.write_all(b"<CALL:5>W9NEW<BAND:3>40m<MODE:2>CW<EOR>\n")
            .unwrap();
        drop(f);
        assert!(is_generated(&d.log()), "the marker is still there");
        assert_eq!(
            mirror_state(&d.log()),
            MirrorState::Foreign,
            "but the length it records no longer agrees"
        );

        w.submit(rows(4));
        let s = w.flush(Duration::from_secs(10));
        assert!(s.foreign_write);
        assert!(
            std::fs::read_to_string(d.log()).unwrap().contains("W9NEW"),
            "the appended contact is still in the file"
        );
    }

    /// A first run is not a conflict when the store has taken the file in: the operator's own
    /// `log.adi`, just converted, has no marker, and the store names it as accepted.
    #[test]
    fn an_accepted_unmarked_log_is_replaced_and_an_unaccepted_one_is_not() {
        let d = Dir::new("firstrun");
        std::fs::write(d.log(), super::super::adif_header()).unwrap();
        let accepted = file_stamp(&d.log());
        let w = MirrorWriter::with_options(
            d.log(),
            MirrorOptions {
                debounce: Duration::from_millis(5),
                max_delay: Duration::from_millis(50),
                accepted,
            },
        );
        w.submit(rows(4));
        w.flush(Duration::from_secs(10));
        assert!(
            !w.status().foreign_write,
            "the log the store already took in is not a foreign write"
        );
        assert_eq!(w.status().writes, 1);

        // The control: the same unmarked file, NOT accepted, is left alone.
        let d2 = Dir::new("firstrun2");
        std::fs::write(d2.log(), super::super::adif_header()).unwrap();
        let w2 = writer(d2.log());
        w2.submit(rows(4));
        let s = w2.flush(Duration::from_secs(10));
        assert!(
            s.foreign_write,
            "an unmarked log nobody took in is not replaced"
        );
        assert_eq!(s.writes, 0);
    }

    /// Two mirror lanes on one file — two Nexus windows on one data folder — accept each
    /// other's output, because a pristine mirror is anybody's to replace.
    #[test]
    fn two_mirror_lanes_accept_each_others_pictures() {
        let d = Dir::new("twolanes");
        let (a, b) = (writer(d.log()), writer(d.log()));
        a.submit(rows(3));
        a.flush(Duration::from_secs(10));
        b.submit(rows(5));
        let sb = b.flush(Duration::from_secs(10));
        assert!(!sb.foreign_write, "B replaces A's pristine picture");
        a.submit(rows(7));
        let sa = a.flush(Duration::from_secs(10));
        assert!(!sa.foreign_write, "and A replaces B's");
        assert_eq!(
            parse_adif(&std::fs::read_to_string(d.log()).unwrap()).len(),
            7
        );
    }

    /// The header records the body's length and it agrees with the file — the property every
    /// other check stands on.
    #[test]
    fn the_marker_records_the_bodys_length() {
        let recs = rows(9);
        let text = mirror_adif(&recs);
        let d = Dir::new("marker");
        std::fs::write(d.log(), &text).unwrap();
        let (eoh_end, recorded) = read_marker(&d.log()).expect("a marker");
        assert_eq!(eoh_end + recorded, text.len() as u64);
        assert_eq!(mirror_state(&d.log()), MirrorState::Pristine);
        // Absent is its own answer, and an ordinary log is foreign.
        assert_eq!(mirror_state(&d.0.join("none.adi")), MirrorState::Absent);
        std::fs::write(d.log(), super::super::adif_header()).unwrap();
        assert_eq!(mirror_state(&d.log()), MirrorState::Foreign);
    }

    /// The dated backup ring survives the move from whole-file saves to the mirror: a write
    /// that makes the file SMALLER — a purge, a delete, a log that lost rows upstream — takes a
    /// snapshot of the file it replaces first.
    #[test]
    fn a_mirror_write_that_shrinks_the_file_snapshots_it_first() {
        let d = Dir::new("ring");
        let w = writer(d.log());
        w.submit(rows(30));
        w.flush(Duration::from_secs(10));
        let before = std::fs::read(d.log()).unwrap();
        w.submit(rows(2));
        w.flush(Duration::from_secs(10));
        let shrinks: Vec<_> = std::fs::read_dir(d.0.join("backups"))
            .expect("the ring exists")
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.to_string_lossy().ends_with("-shrink.adi"))
            .collect();
        assert_eq!(shrinks.len(), 1, "one shrink snapshot: {shrinks:?}");
        assert_eq!(
            std::fs::read(&shrinks[0]).unwrap(),
            before,
            "and it is the file as it was before the shrinking write"
        );
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
