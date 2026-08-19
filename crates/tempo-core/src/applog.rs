//! The application diagnostic log — one plain-text file an operator can find, read, and
//! attach to a bug report.
//!
//! # Why this exists
//!
//! Before this module Nexus logged **nothing about itself**. No `env_logger`, no `tracing`,
//! no `simplelog`; and the desktop binary is built `windows_subsystem = "windows"`, so every
//! `eprintln!` in the tree goes to a stderr no window is attached to and nobody ever sees.
//! The only files Nexus could write about its own health were `nexus-crash.txt` (access
//! violations only, from the vectored handler in `main.rs`) and the CAT diagnostic. When the
//! Greek-Windows operator reported "it will not launch at all" in 2026-08, there was nothing
//! for them to send — not even the name of the last startup step that ran. This is the
//! missing thing: a timestamped, human-readable trail of the events that explain a failure.
//!
//! # The four rules it is built around
//!
//! 1. **Rotation is O(1) — a RENAME, never a rewrite.** When the active file passes
//!    [`SIZE_CAP`], it is renamed over the single previous generation and a fresh, empty file
//!    is opened. The old file is never read, never trimmed, never compacted — and nothing is
//!    ever compacted "at startup". That is the whole point: the operator's constraint on this
//!    work was that a big log must not make launch slow, and a log that trims itself by
//!    rewriting is exactly the big startup write that was ruled out. Two generations, each
//!    capped, so the worst case is a deterministic ~8 MB — a number, not a feeling.
//! 2. **Never block the caller.** The radio loop and the startup path hand lines to a bounded
//!    channel and return immediately. If the channel is full — a disk stall, a slow spinning
//!    drive — the line is **DROPPED and counted**, never waited on. The writer thread notes
//!    the count in the file itself, so a reader can see that the log has a hole rather than
//!    believing a quiet stretch was quiet. Losing a diagnostic line is a paper cut; stalling
//!    the audio loop behind a disk is a transmission fault.
//! 3. **Never log a secret.** This file is DESIGNED to be emailed to a stranger. Every line —
//!    without exception, because there is exactly one function that writes to the file and it
//!    applies this before anything reaches the buffer — goes through [`redact`], which masks
//!    credential-shaped text. See that function for what "structurally hard" means here and
//!    what it does not mean.
//! 4. **Log what diagnoses a failure, not what happened.** Startup milestones, CAT and audio
//!    open/failure, updater events, panics, webview failures. **No per-decode or per-tick
//!    traffic**: a log that fills with routine activity is useless *and* defeats the size
//!    bound by pushing the interesting lines out of the previous generation.
//!
//! # Shape on disk
//!
//! ```text
//! nexus-diag.log     ← active
//! nexus-diag.1.log   ← the previous generation, whole and untouched
//! ```
//!
//! Both live beside `nexus-crash.txt`, so there is ONE folder to tell an operator to look in.
//!
//! Until [`init`] is called this module is inert: every logging call is an atomic load and a
//! return, and no file is created. That keeps it out of unit tests and off any target that
//! has no business writing files.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::OnceLock;

/// Bytes the active file may reach before it is renamed away. 4 MiB is roughly 40,000 lines
/// at the ~100 bytes a line here runs to. A healthy session writes tens of lines, so this is
/// thousands of sessions in one file and rotation should essentially never fire; the cap
/// exists to bound the pathological case (a CAT port flapping open/closed for an hour), not
/// the normal one. Two generations ⇒ ~8 MB worst case on disk, deterministically.
pub const SIZE_CAP: u64 = 4 * 1024 * 1024;

/// Queued lines. Deep enough that a burst of startup milestones never touches the bound, and
/// small enough that a wedged disk cannot grow it without limit.
const QUEUE_DEPTH: usize = 512;

/// How long the writer waits for a line before flushing what it has. An error-level line
/// flushes immediately; everything else is at most this far behind the disk, which matters
/// because the interesting case is a process that is about to die.
const FLUSH_EVERY: std::time::Duration = std::time::Duration::from_millis(500);

/// What a masked value is replaced with. Deliberately visible: a reader must be able to tell
/// "the value was withheld" from "the field was empty".
const REDACTED: &str = "<redacted>";

/// Severity. Only [`Level::Error`] forces an immediate flush.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        }
    }
}

enum Msg {
    Line {
        level: Level,
        text: String,
    },
    /// Flush and acknowledge — the only synchronous operation, used at quit and by tests.
    Flush(SyncSender<()>),
}

/// The live channel, once [`init`] has run. `None` ⇒ this module is inert.
static TX: OnceLock<SyncSender<Msg>> = OnceLock::new();
/// Where the active file is, for the Settings panel / troubleshooting doc to point at.
static PATH: OnceLock<PathBuf> = OnceLock::new();
/// Lines the bounded channel refused. Reported into the file rather than swallowed.
static DROPPED: AtomicU64 = AtomicU64::new(0);

/// Start the log at `path` (the active file; the previous generation is `<stem>.1.<ext>`).
/// Idempotent — a second call is ignored, so this can be wired defensively. Best-effort: if
/// the file cannot be opened the module simply stays inert and the app runs exactly as
/// before. Nothing here can fail a launch.
pub fn init(path: PathBuf) {
    if TX.get().is_some() {
        return;
    }
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let Some(sink) = Sink::open(&path, SIZE_CAP) else {
        return;
    };
    let (tx, rx) = sync_channel::<Msg>(QUEUE_DEPTH);
    if std::thread::Builder::new()
        .name("nexus-diag".into())
        .spawn(move || pump(sink, rx))
        .is_err()
    {
        return; // no thread ⇒ stay inert rather than logging inline and blocking a caller
    }
    let _ = PATH.set(path);
    let _ = TX.set(tx);
    log(Level::Info, "diag", "diagnostic log opened");
}

/// The active file's path, once [`init`] has run.
pub fn path() -> Option<&'static Path> {
    PATH.get().map(|p| p.as_path())
}

/// Record one event. `event` is a short stable token — `startup`, `cat`, `audio`, `updater`,
/// `webview`, `panic` — and `detail` is the human half.
///
/// **Never blocks.** If the queue is full the line is counted and dropped (see the module
/// header, rule 2).
pub fn log(level: Level, event: &str, detail: &str) {
    let Some(tx) = TX.get() else {
        return; // inert until init()
    };
    let text = format!("{event}: {detail}");
    if tx.try_send(Msg::Line { level, text }).is_err() {
        DROPPED.fetch_add(1, Ordering::Relaxed);
    }
}

/// A startup milestone / ordinary event.
pub fn info(event: &str, detail: &str) {
    log(Level::Info, event, detail);
}

/// Something degraded but survivable — a device that would not open, a fetch that failed.
pub fn warn(event: &str, detail: &str) {
    log(Level::Warn, event, detail);
}

/// A failure the operator will feel. Flushed to disk immediately, because the next thing
/// that happens may be the process dying.
pub fn error(event: &str, detail: &str) {
    log(Level::Error, event, detail);
}

/// Push everything queued to disk and wait for it. Call at quit; a dropped/inert log returns
/// at once. Bounded by a timeout so a wedged writer can never hold up an exit.
pub fn flush() {
    let Some(tx) = TX.get() else {
        return;
    };
    let (ack_tx, ack_rx) = sync_channel::<()>(1);
    if tx.try_send(Msg::Flush(ack_tx)).is_ok() {
        let _ = ack_rx.recv_timeout(std::time::Duration::from_secs(2));
    }
}

/// The writer thread. Owns the file; nothing else in the process may touch it.
fn pump(mut sink: Sink, rx: Receiver<Msg>) {
    loop {
        match rx.recv_timeout(FLUSH_EVERY) {
            Ok(Msg::Line { level, text }) => {
                sink.note_drops(DROPPED.swap(0, Ordering::Relaxed));
                sink.write(level, &text);
                if level == Level::Error {
                    sink.flush();
                }
            }
            Ok(Msg::Flush(ack)) => {
                sink.note_drops(DROPPED.swap(0, Ordering::Relaxed));
                sink.flush();
                let _ = ack.try_send(());
            }
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => sink.flush(),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                sink.flush();
                return;
            }
        }
    }
}

/// The file, its byte counter, and the rotation rule. **The only thing in the process that
/// writes to the log** — which is what makes [`redact`] unconditional rather than a
/// convention call sites have to remember.
struct Sink {
    file: Option<std::io::BufWriter<std::fs::File>>,
    path: PathBuf,
    /// The single previous generation. Rotation renames `path` over this.
    prev: PathBuf,
    /// Bytes in the ACTIVE file. Seeded from its length at open — one `stat`, never a read.
    written: u64,
    cap: u64,
}

impl Sink {
    fn open(path: &Path, cap: u64) -> Option<Self> {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .ok()?;
        // The length is a `metadata` call, not a read of the file: reopening a 4 MB log must
        // not cost 4 MB of IO on the startup path.
        let written = file.metadata().map(|m| m.len()).unwrap_or(0);
        Some(Sink {
            file: Some(std::io::BufWriter::new(file)),
            path: path.to_path_buf(),
            prev: previous_generation(path),
            written,
            cap,
        })
    }

    /// Write one line. Timestamped, redacted, and rotated if it took the file over the cap.
    fn write(&mut self, level: Level, text: &str) {
        let (y, mo, d, h, mi, s) = crate::logbook::datetime_utc(now_unix());
        let line = format!(
            "{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02}Z  {}  {}\n",
            level.tag(),
            redact(&trim_build_paths(text))
        );
        let Some(f) = self.file.as_mut() else {
            return;
        };
        if f.write_all(line.as_bytes()).is_err() {
            return;
        }
        self.written += line.len() as u64;
        if self.written >= self.cap {
            self.rotate();
        }
    }

    /// Say, in the file, that lines were lost — so a gap reads as a gap.
    fn note_drops(&mut self, n: u64) {
        if n > 0 {
            self.write(
                Level::Warn,
                &format!("diag: {n} line(s) dropped — the writer could not keep up"),
            );
        }
    }

    /// **O(1) rotation.** Flush, close, rename the active file over the previous generation,
    /// open a fresh empty one. No read of either file, no trim, no compaction — the cost does
    /// not scale with how big the log got, which is the entire reason it is a rename.
    fn rotate(&mut self) {
        if let Some(mut f) = self.file.take() {
            let _ = f.flush();
        } // handle dropped here — Windows will not rename an open file
          // Windows `rename` fails when the destination exists; POSIX replaces it. Removing
          // first makes both platforms do the same thing.
        let _ = std::fs::remove_file(&self.prev);
        if std::fs::rename(&self.path, &self.prev).is_err() {
            // Could not rotate (a virus scanner holding the file, say). Reopen and keep
            // going: an oversized log is far better than a silent one.
            self.file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .ok()
                .map(std::io::BufWriter::new);
            return;
        }
        self.file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&self.path)
            .ok()
            .map(std::io::BufWriter::new);
        self.written = 0;
    }

    fn flush(&mut self) {
        if let Some(f) = self.file.as_mut() {
            let _ = f.flush();
        }
    }
}

/// `nexus-diag.log` → `nexus-diag.1.log`. The generation number goes BEFORE the extension so
/// the file still opens in a text editor by double-click.
fn previous_generation(path: &Path) -> PathBuf {
    match (path.file_stem(), path.extension()) {
        (Some(stem), Some(ext)) => path.with_file_name(format!(
            "{}.1.{}",
            stem.to_string_lossy(),
            ext.to_string_lossy()
        )),
        _ => {
            let mut p = path.as_os_str().to_os_string();
            p.push(".1");
            PathBuf::from(p)
        }
    }
}

fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

// ── Redaction ────────────────────────────────────────────────────────────────────────────

/// Identifier endings that mean "what follows is a credential". Matched on the **whole
/// identifier**, so `clublog_api_key=`, `cloudlogKey:` and `?key=` are all caught while
/// `cw_key_port=COM3` (ends in `port`) is not.
const SENSITIVE_SUFFIXES: [&str; 13] = [
    "password",
    "passwd",
    "pwd",
    "pass",
    "token",
    "secret",
    "apikey",
    "key",
    "credential",
    "credentials",
    "auth",
    "authorization",
    "cookie",
];

/// Mask credential-shaped text in one line.
///
/// # What "structurally hard" means here, honestly
///
/// It is not a type-level proof — nothing stops a caller passing a password as free text.
/// What it is instead: there is exactly **one** function in the process that can write to
/// this file ([`Sink::write`]), it calls this unconditionally on every line, and the file
/// handle is private to the writer thread — so a call site cannot opt out, forget, or take a
/// shortcut around it. Combined with the rule that the settings struct is never handed to
/// this module, a leak needs someone to deliberately format a secret into a shape this does
/// not recognise, rather than merely to be careless.
///
/// Three shapes are masked: `<sensitive-identifier><=|:> value`, `Bearer`/`Basic <value>`,
/// and a URL's `//user:password@host` userinfo.
///
/// It is deliberately over-eager. Masking a harmless value (`lowpass=3000`) costs one
/// diagnostic detail; leaking a credential costs the operator their account.
/// Cut the BUILD MACHINE's absolute paths down to repo-relative ones.
///
/// ⚠️ THIS IS A LEAK FIX, not tidiness. `#[track_caller]` and `panic::Location` both render the
/// path as it was handed to the compiler, which for a locally built binary is an absolute path
/// under the developer's home directory — and this file is the one an operator is asked to
/// EMAIL to a stranger or attach to a public issue. A panic line reading
/// `/home/<someone>/work/.../engine.rs:8864` publishes a username with it, from a file whose
/// whole design goal (see the module header's redaction rule) is that it can be handed over
/// without being read first. Found in a real operator log, 2026-08-19.
///
/// Everything up to and including the last `crates/`, `src-tauri/`, `libtempo/` or `ui/`
/// segment is dropped, leaving `crates/tempo-app/src/engine.rs:8864` — which is what a reader
/// actually wants anyway, and what a CI build (whose paths are already relative) prints.
/// Anything unrecognised is left alone: a path shape we do not know is not one to guess at.
fn trim_build_paths(text: &str) -> String {
    const ROOTS: [&str; 4] = ["/crates/", "/src-tauri/", "/libtempo/", "/ui/"];
    let mut out = text.to_string();
    for root in ROOTS {
        // Repeatedly, because one line can carry two paths (a panic renders location + payload).
        while let Some(i) = out.find(root) {
            // Walk back to the start of this path token so the whole absolute prefix goes, not
            // just the part before `/crates/`.
            let start = out[..i]
                .rfind(|c: char| c.is_whitespace() || c == '(' || c == '\'' || c == '"')
                .map(|p| p + 1)
                .unwrap_or(0);
            out.replace_range(start..=i, "");
        }
    }
    out
}

fn redact(line: &str) -> String {
    let c: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < c.len() {
        // `//user:password@host` — the classic URL userinfo leak.
        if c[i] == '/' && i + 1 < c.len() && c[i + 1] == '/' {
            if let Some(at) = userinfo_end(&c, i + 2) {
                out.push_str("//");
                let mut j = i + 2;
                while j < at && c[j] != ':' {
                    out.push(c[j]);
                    j += 1;
                }
                if j < at {
                    out.push(':');
                    out.push_str(REDACTED);
                }
                i = at; // the '@' itself is copied by the normal path
                continue;
            }
        }
        if !is_ident_start(c[i]) {
            out.push(c[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < c.len() && is_ident(c[i]) {
            i += 1;
        }
        let word: String = c[start..i].iter().collect();
        out.push_str(&word);
        let lower = word.to_ascii_lowercase();

        // `Bearer <token>` / `Basic <base64>` — separated by space, not by `=`.
        if lower == "bearer" || lower == "basic" {
            let mut j = i;
            while j < c.len() && c[j] == ' ' {
                j += 1;
            }
            if j > i && j < c.len() && !c[j].is_whitespace() {
                out.extend(c[i..j].iter());
                while j < c.len() && !c[j].is_whitespace() {
                    j += 1;
                }
                out.push_str(REDACTED);
                i = j;
                continue;
            }
        }

        if !is_sensitive(&lower) {
            continue;
        }
        // …optional spaces, a `=` or `:`, optional spaces, then the value.
        let mut j = i;
        while j < c.len() && c[j] == ' ' {
            j += 1;
        }
        if j >= c.len() || (c[j] != '=' && c[j] != ':') {
            continue;
        }
        j += 1;
        while j < c.len() && c[j] == ' ' {
            j += 1;
        }
        out.extend(c[i..j].iter());
        i = j;
        if i >= c.len() {
            continue;
        }
        if c[i] == '"' || c[i] == '\'' {
            let quote = c[i];
            i += 1;
            while i < c.len() && c[i] != quote {
                i += 1;
            }
            if i < c.len() {
                i += 1;
            }
            out.push(quote);
            out.push_str(REDACTED);
            out.push(quote);
        } else {
            let vstart = i;
            while i < c.len() && !matches!(c[i], ' ' | '\t' | ',' | '&' | ';' | ')') {
                i += 1;
            }
            let val: String = c[vstart..i].iter().collect();
            let vl = val.to_ascii_lowercase();
            if vl == "bearer" || vl == "basic" {
                // `Authorization: Bearer <token>` — the identifier IS sensitive, but the value
                // run is only the auth SCHEME. Masking it would hide the harmless half and
                // leave the token in the clear (it did, first time round). Keep the scheme,
                // mask what follows it.
                out.push_str(&val);
                let mut j = i;
                while j < c.len() && c[j] == ' ' {
                    j += 1;
                }
                out.extend(c[i..j].iter());
                i = j;
                while i < c.len() && !c[i].is_whitespace() {
                    i += 1;
                }
                if i > j {
                    out.push_str(REDACTED);
                }
            } else {
                out.push_str(REDACTED);
            }
        }
    }
    out
}

/// The index of the `@` that closes a URL userinfo starting at `from`, if there is one before
/// the authority ends. Bounded: anything with a `/`, whitespace or a quote in it first is not
/// userinfo, it is an ordinary path.
fn userinfo_end(c: &[char], from: usize) -> Option<usize> {
    let mut j = from;
    while j < c.len() {
        match c[j] {
            '@' => return Some(j),
            '/' | ' ' | '\t' | '"' | '\'' => return None,
            _ => j += 1,
        }
    }
    None
}

fn is_ident_start(ch: char) -> bool {
    ch.is_ascii_alphabetic() || ch == '_'
}

fn is_ident(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

/// Whether an identifier names a credential. Normalised first (`api_key`, `api-key` and
/// `apiKey` are one name), then matched on the ENDING so any product prefix is covered.
fn is_sensitive(lower_ident: &str) -> bool {
    let flat: String = lower_ident
        .chars()
        .filter(|c| *c != '_' && *c != '-')
        .collect();
    SENSITIVE_SUFFIXES.iter().any(|s| flat.ends_with(s))
}

#[cfg(test)]
mod tests {

    /// A LOG WE ASK STRANGERS TO READ MUST NOT CARRY THE BUILD MACHINE'S PATHS.
    ///
    /// `#[track_caller]` and `panic::Location` print the path the compiler was given. On a
    /// locally built binary that is absolute and sits under someone's home directory, so a
    /// single panic publishes a username into the file whose entire purpose is to be handed
    /// over unread. Seen in a real operator log, 2026-08-19.
    #[test]
    fn build_paths_are_cut_to_repo_relative() {
        let leaky = "at /home/someone/work/proj/crates/tempo-app/src/engine.rs:8864: panicked";
        let cut = trim_build_paths(leaky);
        assert_eq!(cut, "at crates/tempo-app/src/engine.rs:8864: panicked");
        assert!(
            !cut.contains("/home/"),
            "control: the home directory is gone"
        );

        // Two paths on one line — a panic renders location AND payload.
        let two = "at /a/b/crates/x/src/f.rs:1: panicked at /a/b/crates/x/src/f.rs:1:9: boom";
        assert!(!trim_build_paths(two).contains("/a/b/"));

        // Every root the tree builds from.
        for root in [
            "/src-tauri/src/lib.rs:17",
            "/libtempo/x.f90:3",
            "/ui/src/main.tsx:9",
        ] {
            let s = format!("x /home/u/work/proj{root}");
            assert!(
                !trim_build_paths(&s).contains("/home/u"),
                "{root} still leaked"
            );
        }

        // A path shape we do not recognise is LEFT ALONE rather than mangled — an operator's
        // own settings path is diagnostic and is not ours to guess at.
        let keep = r"settings loaded from C:\Users\op\AppData\Roaming\tempo\settings.json";
        assert_eq!(trim_build_paths(keep), keep);
    }
    use super::*;

    /// A unique scratch directory (OS temp dir — nothing here touches the checkout).
    fn scratch() -> PathBuf {
        use std::sync::atomic::AtomicU32;
        static N: AtomicU32 = AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("nexus_diag_{}_{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_default()
    }

    /// ★ THE FILE IS DESIGNED TO BE EMAILED TO A STRANGER. Every shape a credential reaches a
    /// log line in must come out masked — and it must be masked by the WRITE PATH, not by a
    /// helper a call site could forget, which is why this drives `Sink::write` rather than
    /// `redact` directly.
    #[test]
    fn no_credential_shape_survives_the_write_path() {
        let dir = scratch();
        let path = dir.join("nexus-diag.log");
        let mut sink = Sink::open(&path, SIZE_CAP).unwrap();
        let secrets = [
            ("lotw upload: password=hunter2 user=W1AW", "hunter2"),
            ("eqsl: eqslPassword: s3cr3t-value", "s3cr3t-value"),
            ("clublog: clublog_api_key=abcd1234efgh", "abcd1234efgh"),
            ("cloudlog: cloudlogKey = XYZTOKEN99", "XYZTOKEN99"),
            ("qrz: session_token=\"tok-abc-999\"", "tok-abc-999"),
            (
                "http: Authorization: Bearer eyJhbGciOiJIUzI1",
                "eyJhbGciOiJIUzI1",
            ),
            (
                "cluster: telnet://w1aw:clusterpw@dxc.example.net:7300",
                "clusterpw",
            ),
            (
                "fetch https://api.example.com/v1?key=SEKRIT99&call=W1AW",
                "SEKRIT99",
            ),
        ];
        for (line, _) in secrets {
            sink.write(Level::Info, line);
        }
        sink.flush();
        let body = read(&path);
        for (line, secret) in secrets {
            assert!(
                !body.contains(secret),
                "{secret:?} leaked into the diagnostic log from {line:?}\n--- file ---\n{body}"
            );
        }
        assert!(
            body.contains(REDACTED),
            "…and it says so, rather than dropping the line"
        );
        // Positive control — the redactor must not be a shredder. An ordinary CAT failure,
        // which is the whole reason this file exists, has to survive intact.
        sink.write(
            Level::Error,
            "cat: open /dev/ttyUSB0 failed: permission denied",
        );
        sink.flush();
        let body = read(&path);
        assert!(
            body.contains("cat: open /dev/ttyUSB0 failed: permission denied"),
            "ordinary diagnostics pass through untouched:\n{body}"
        );
        // …and the non-secret half of a credential line is still readable.
        assert!(
            body.contains("user=W1AW"),
            "only the value is masked, not the line"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// ★ ROTATION IS A RENAME. Proven by content: the previous generation must be byte-for-byte
    /// what the active file held before it rotated. A trim-and-rewrite — the "big disk write
    /// while the app is starting" this design exists to avoid — could not produce that. And
    /// there are never more than two generations.
    #[test]
    fn rotation_renames_the_whole_file_and_keeps_exactly_two_generations() {
        let dir = scratch();
        let path = dir.join("nexus-diag.log");
        let prev = dir.join("nexus-diag.1.log");
        let mut sink = Sink::open(&path, 400).unwrap();

        for i in 0..4 {
            sink.write(
                Level::Info,
                &format!("startup: milestone {i} of the first generation"),
            );
        }
        sink.flush();
        let before = read(&path);
        assert!(!before.is_empty() && !prev.exists(), "not rotated yet");

        // Push it over the cap.
        for i in 0..4 {
            sink.write(
                Level::Info,
                &format!("startup: milestone {i} of the second generation"),
            );
        }
        sink.flush();

        assert!(
            prev.exists(),
            "the previous generation exists after rotation"
        );
        let rolled = read(&prev);
        assert!(
            rolled.starts_with(&before),
            "the rolled file is the ORIGINAL bytes — a rename, not a rewrite\n{rolled}"
        );
        assert!(
            rolled.contains("first generation"),
            "the old lines went with it"
        );
        assert!(
            read(&path).len() < rolled.len(),
            "the active file started fresh instead of being trimmed in place"
        );
        let files: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files.len(), 2, "two generations, never a third: {files:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A dropped line leaves a mark. A gap that reads as a quiet stretch would be worse than
    /// no log at all — the reader would draw a conclusion from silence that never happened.
    #[test]
    fn dropped_lines_are_counted_into_the_file_not_swallowed() {
        let dir = scratch();
        let path = dir.join("nexus-diag.log");
        let mut sink = Sink::open(&path, SIZE_CAP).unwrap();
        sink.note_drops(0);
        assert_eq!(read(&path), "", "no note when nothing was dropped");
        sink.note_drops(17);
        sink.flush();
        assert!(
            read(&path).contains("17 line(s) dropped"),
            "the hole is named: {}",
            read(&path)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The identifier rule fires on credentials and holds off ordinary radio settings — both
    /// directions, because a guard shown in only one is half a test.
    #[test]
    fn the_sensitive_identifier_rule_fires_and_holds_off() {
        for yes in [
            "password",
            "passwd",
            "pwd",
            "lotwpassword",
            "clublog_api_key",
            "cloudlogKey",
            "key",
            "apiKey",
            "session_token",
            "Authorization",
            "clientSecret",
            "cookie",
        ] {
            assert!(
                is_sensitive(&yes.to_ascii_lowercase()),
                "{yes} must be masked"
            );
        }
        for no in [
            "cw_key_port",
            "cw_key_line",
            "winkeyer_port",
            "callsign",
            "band",
            "mode",
            "device",
            "port",
            "baud",
            "grid",
            "keyer",
            "frequency",
        ] {
            assert!(
                !is_sensitive(&no.to_ascii_lowercase()),
                "{no} must NOT be masked"
            );
        }
    }

    /// End to end through the real public API: init, log, flush, and the line is on disk.
    /// This is the only test that touches the process-global logger.
    #[test]
    fn the_public_api_writes_a_timestamped_line() {
        let dir = scratch();
        let path = dir.join("nexus-diag.log");
        init(path.clone());
        info("startup", "settings loaded");
        error("cat", "rigctld refused the port");
        flush();
        let body = read(&path);
        assert!(body.contains("startup: settings loaded"), "{body}");
        assert!(
            body.contains("ERROR  cat: rigctld refused the port"),
            "{body}"
        );
        assert!(body.contains('Z'), "lines carry a UTC timestamp: {body}");
        assert_eq!(super::path(), Some(path.as_path()));
        // NOT removed: the writer thread owns this file for the rest of the process.
    }
}
