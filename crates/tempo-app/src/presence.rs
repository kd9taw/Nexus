//! Parsec presence — is the operator's remote session still there?
//!
//! An operator who runs the shack over Parsec (a remote-desktop stream) gives Nexus no presence
//! signal of its own: to Nexus the remote mouse and keyboard look exactly like local ones. So if
//! the link drops while a Phone PTT is latched, RTTY/PSK continuous TX is up or Tune is running,
//! the rig stays keyed until the wall-clock TX watchdog trips (Tune has its own `MAX_TUNE_MS`).
//! Presence mode closes that gap by reading Parsec's own record of who is connected, and this
//! module is the part of it that can be tested without a Parsec install: the log-line parser,
//! the file follower and the state machine. The engine half, which decides what to stop and
//! stops it, is `engine/parsec_presence.rs`.
//!
//! # What Parsec actually tells us, and where each fact comes from
//!
//! There is no official presence API for the Parsec app. The Parsec SDK (a library for
//! embedding hosting in your OWN program, not a window onto the Parsec app's sessions) has been
//! withdrawn: its documentation now redirects to parsec.app/410. Parsec for Teams has audit
//! logs, but they are a paid cloud service behind credentials, not a local signal. What is left
//! is the host's own log file:
//!
//! - **Where it is** (official, support.parsec.app, "Parsec App for Windows"):
//!   `%appdata%\Parsec\log.txt` for a Per User install and `%programdata%\Parsec\log.txt` for a
//!   Per Computer (formerly "Shared") install. A community tool (ParsecHooks) reports a per-machine
//!   install still logging into `%APPDATA%`, so both are probed and the newest file wins.
//! - **What is in it** (official, "All Advanced Configuration Options"): `app_log_level` sets
//!   what is written to "the log.txt file in Help > Log File"; the console "can also show …
//!   whether a client connected or disconnected from the stream".
//! - **The lines** (NOT in the official docs; from real host logs posted by users and parsed by
//!   community tools): `[I 2023-06-20 10:44:26] name#1234567 connected.` and the matching
//!   `… disconnected.`. The guest is named by Parsec account (`name#user-id`), not by machine. A
//!   restart writes `[F …] ===== Parsec: Started =====`, after which no earlier session exists.
//! - **The traps** (community, measured against a 5151-line real log): `[D …] IPC AS Client
//!   Connected.` appears dozens of times per session and is Parsec talking to itself;
//!   `UPNP: … reported as not connected` is about the router. Another tool skips `IPC#…` tokens.
//! - **Rotation** (community): the file is renamed to `log.1.txt` at roughly 1 MB and a fresh
//!   `log.txt` begins.
//! - **How long Parsec takes to notice a pulled cable** (official, "Error Codes - 12007"): "We
//!   give your internet 60 seconds to reconnect by default". That, not this code, is the
//!   dominant term in how fast a hard drop unkeys — which is why this is a backstop for the
//!   watchdog and not a replacement for it.
//!
//! So this is a BEST-EFFORT parse of a log format Parsec does not document, and every rule
//! below is written so that its failure is a stop rather than a stuck transmitter.
//!
//! # The rules
//!
//! - **Parsing** ([`parse_line`]): a connect counts only in the exact shape every real log
//!   shows — info level, `name#digits`, ` connected.` — because a false connect is what could
//!   make presence mode stop a LOCAL operator. A disconnect is accepted more loosely (any level,
//!   trailing detail allowed): it can only ever remove a guest that a connect line added, so
//!   leniency there cannot invent anything.
//! - **Following** ([`ParsecLog`]): one read of the whole file when it is first found (so
//!   starting Nexus mid-session sees the guest who is already on), then only the new bytes on
//!   each poll. A file that got shorter was rotated: the unread tail of `log.1.txt` is read
//!   first, so a disconnect written just before a rotation is not lost.
//! - **Deciding** ([`PresenceMachine`]): presence is LOST only on the edge from connected — to
//!   a disconnect, or to a log that can no longer be read. Never connected, then unreadable,
//!   is an operator sitting at the shack, and nothing happens. No transition, no action.

use std::collections::BTreeSet;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The name Parsec's host log has in every install directory.
pub const LOG_FILE_NAME: &str = "log.txt";

/// The name the log is rotated to (community-observed, see the module header).
pub const ROTATED_FILE_NAME: &str = "log.1.txt";

/// What Parsec writes when it starts. Every session before it is over.
const STARTED_MARKER: &str = "===== Parsec: Started =====";

/// How much of each file the first read replays. Parsec rotates at about 1 MB, so this is the
/// whole file in practice; the cap only keeps a log that never rotated from being read whole.
const RECONCILE_TAIL_BYTES: u64 = 4 * 1024 * 1024;

/// An unterminated line longer than this is not a log line, and is dropped rather than grown.
const MAX_LINE_BYTES: usize = 64 * 1024;

/// What one look at Parsec's host log says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// At least one Parsec guest is connected.
    Connected,
    /// The log was read and no guest is connected.
    Disconnected,
    /// The log could not be found or read.
    Unreadable,
}

/// A line of the host log that bears on presence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogEvent {
    /// A guest (`name#user-id`) connected.
    Connected(String),
    /// A guest disconnected.
    Disconnected(String),
    /// Parsec (re)started: every earlier session is over.
    Started,
}

/// Classify one line of Parsec's host log. `None` for everything that is not a connect, a
/// disconnect or the restart marker — which is nearly every line.
pub fn parse_line(line: &str) -> Option<LogEvent> {
    let (level, message) = split_prefix(line.trim_end_matches(['\r', '\n']))?;
    if message.starts_with(STARTED_MARKER) {
        return Some(LogEvent::Started);
    }
    // A CONNECT is taken only in the exact shape real logs show: info level, nothing after the
    // period. It is the direction that can do harm — a false one arms presence mode for an
    // operator who is not remote at all.
    if let Some(who) = message.strip_suffix(" connected.") {
        return if level == 'I' {
            guest(who).map(LogEvent::Connected)
        } else {
            None
        };
    }
    // A DISCONNECT at any level, with any detail after it (an error code, say). It only ever
    // removes a guest a connect line added, so reading it loosely cannot invent a session — and
    // missing one is the direction that leaves a transmitter keyed.
    let at = message.find(" disconnected")?;
    let after = &message[at + " disconnected".len()..];
    if after.chars().next().is_some_and(char::is_alphanumeric) {
        return None;
    }
    guest(&message[..at]).map(LogEvent::Disconnected)
}

/// `[L YYYY-MM-DD HH:MM:SS] message` → `(L, message)`, or `None` for anything not in that shape.
fn split_prefix(line: &str) -> Option<(char, &str)> {
    const STAMP: &[u8; 19] = b"dddd-dd-dd dd:dd:dd";
    let b = line.as_bytes();
    if b.len() < 24 || b[0] != b'[' || !b[1].is_ascii_uppercase() || b[2] != b' ' {
        return None;
    }
    if b[22] != b']' || b[23] != b' ' {
        return None;
    }
    let stamp_ok = b[3..22].iter().zip(STAMP).all(|(c, want)| {
        if *want == b'd' {
            c.is_ascii_digit()
        } else {
            c == want
        }
    });
    // Bytes 0..24 are all ASCII by now, so 24 is a character boundary.
    stamp_ok.then(|| (char::from(b[1]), &line[24..]))
}

/// A guest token is `name#user-id`: a name with no surrounding whitespace, then digits. Parsec's
/// own IPC endpoint is not a guest, whatever it is written as.
fn guest(token: &str) -> Option<String> {
    let (name, id) = token.rsplit_once('#')?;
    let ok = !name.is_empty()
        && name.trim() == name
        && !name.chars().any(char::is_control)
        && !name.eq_ignore_ascii_case("IPC")
        && !id.is_empty()
        && id.bytes().all(|c| c.is_ascii_digit());
    ok.then(|| token.to_string())
}

/// The two places a Windows Parsec host writes its log, per user first, then per computer —
/// from `%APPDATA%` and `%ProgramData%`. A directory that is missing or empty is skipped.
pub fn log_candidates(appdata: Option<PathBuf>, programdata: Option<PathBuf>) -> Vec<PathBuf> {
    [appdata, programdata]
        .into_iter()
        .flatten()
        .filter(|dir| !dir.as_os_str().is_empty())
        .map(|dir| dir.join("Parsec").join(LOG_FILE_NAME))
        .collect()
}

/// Of the candidates that exist, the one written most recently — the log Parsec is keeping now.
/// The first listed wins a tie.
fn newest(candidates: &[PathBuf]) -> Option<PathBuf> {
    let mut best: Option<(std::time::SystemTime, &PathBuf)> = None;
    for path in candidates {
        let Ok(meta) = std::fs::metadata(path) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let when = meta.modified().unwrap_or(std::time::UNIX_EPOCH);
        if best.is_none_or(|(t, _)| when > t) {
            best = Some((when, path));
        }
    }
    best.map(|(_, path)| path.clone())
}

/// Follows Parsec's `log.txt` from one poll to the next and keeps the set of connected guests.
#[derive(Debug)]
pub struct ParsecLog {
    candidates: Vec<PathBuf>,
    /// The file being followed, once one has been found.
    path: Option<PathBuf>,
    /// Bytes of `path` already consumed.
    offset: u64,
    /// The unterminated end of the last read, completed by the next one.
    partial: Vec<u8>,
    /// Who the log says is connected right now, as `name#user-id`.
    guests: BTreeSet<String>,
}

impl ParsecLog {
    pub fn new(candidates: Vec<PathBuf>) -> Self {
        Self {
            candidates,
            path: None,
            offset: 0,
            partial: Vec::new(),
            guests: BTreeSet::new(),
        }
    }

    /// The log being followed, once one has been found.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Read whatever Parsec has written since the last poll and say what it means.
    ///
    /// A followed file that has gone missing is `Unreadable` and is NOT forgotten: the guests
    /// and the read position are kept, so if it comes back as a fresh file (a rotation caught
    /// between its rename and its new file) the rest of the old one is still read.
    pub fn poll(&mut self) -> Verdict {
        let read = match self.path.clone() {
            Some(path) => self.follow(&path),
            None => match newest(&self.candidates) {
                Some(path) => {
                    let first = self.reconcile(&path);
                    if first.is_ok() {
                        self.path = Some(path);
                    }
                    first
                }
                None => Err(std::io::ErrorKind::NotFound.into()),
            },
        };
        match read {
            Err(_) => Verdict::Unreadable,
            Ok(()) if self.guests.is_empty() => Verdict::Disconnected,
            Ok(()) => Verdict::Connected,
        }
    }

    /// First sight of a log: replay it — the rotated half of a long session first, then the
    /// current file — so a guest who connected before Nexus started is seen, and park at its
    /// end. A restart marker in the replay clears everything before it, as it does live.
    fn reconcile(&mut self, path: &Path) -> std::io::Result<()> {
        self.guests.clear();
        self.partial.clear();
        if let Ok((older, _)) = read_tail(&path.with_file_name(ROTATED_FILE_NAME)) {
            self.feed(&older);
            self.end_of_file();
        }
        let (bytes, end) = read_tail(path)?;
        self.feed(&bytes);
        self.offset = end;
        Ok(())
    }

    /// A later poll: the bytes written since the last one.
    fn follow(&mut self, path: &Path) -> std::io::Result<()> {
        let mut file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        if len < self.offset {
            // Shorter than what was already read: the file was rotated (or truncated). What was
            // written to the old file after the last poll — a disconnect, say — is only in the
            // rotated copy, so read that first.
            self.finish_rotated(path);
            self.offset = 0;
        }
        file.seek(SeekFrom::Start(self.offset))?;
        let mut bytes = Vec::new();
        let n = file.read_to_end(&mut bytes)?;
        self.offset += n as u64;
        self.feed(&bytes);
        Ok(())
    }

    /// Read the rest of the file that was just rotated away, from where the last poll stopped.
    /// Only if `log.1.txt` is at least as long as what was read — otherwise it is not the file
    /// that was being followed, and the file simply shrank.
    fn finish_rotated(&mut self, path: &Path) {
        let rest = std::fs::File::open(path.with_file_name(ROTATED_FILE_NAME)).and_then(|mut f| {
            if f.metadata()?.len() < self.offset {
                return Ok(Vec::new());
            }
            f.seek(SeekFrom::Start(self.offset))?;
            let mut rest = Vec::new();
            f.read_to_end(&mut rest)?;
            Ok(rest)
        });
        if let Ok(rest) = rest {
            self.feed(&rest);
        }
        self.end_of_file();
    }

    /// Take in bytes: every complete line is applied, the unterminated end is kept.
    fn feed(&mut self, bytes: &[u8]) {
        self.partial.extend_from_slice(bytes);
        let mut start = 0;
        while let Some(nl) = self.partial[start..].iter().position(|&b| b == b'\n') {
            let line = String::from_utf8_lossy(&self.partial[start..start + nl]).into_owned();
            self.apply(&line);
            start += nl + 1;
        }
        self.partial.drain(..start);
        if self.partial.len() > MAX_LINE_BYTES {
            self.partial.clear();
        }
    }

    /// A file that has ended for good: its last line is complete even without a newline.
    fn end_of_file(&mut self) {
        if !self.partial.is_empty() {
            let line = String::from_utf8_lossy(&std::mem::take(&mut self.partial)).into_owned();
            self.apply(&line);
        }
    }

    fn apply(&mut self, line: &str) {
        match parse_line(line) {
            Some(LogEvent::Connected(guest)) => {
                self.guests.insert(guest);
            }
            Some(LogEvent::Disconnected(guest)) => {
                self.guests.remove(&guest);
            }
            Some(LogEvent::Started) => self.guests.clear(),
            None => {}
        }
    }
}

/// The last [`RECONCILE_TAIL_BYTES`] of a file, starting at a line boundary, and the offset its
/// end sits at.
fn read_tail(path: &Path) -> std::io::Result<(Vec<u8>, u64)> {
    let mut file = std::fs::File::open(path)?;
    let start = file.metadata()?.len().saturating_sub(RECONCILE_TAIL_BYTES);
    file.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    let n = file.read_to_end(&mut bytes)?;
    if start > 0 {
        // Started mid-line: the fragment before the first newline is not a line.
        let cut = bytes
            .iter()
            .position(|&b| b == b'\n')
            .map_or(bytes.len(), |nl| nl + 1);
        bytes.drain(..cut);
    }
    Ok((bytes, start + n as u64))
}

/// Presence as the machine holds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Presence {
    /// Not yet known, or the log could not be read.
    #[default]
    Unknown,
    Connected,
    Disconnected,
}

/// How many polls in a row the log must be unreadable, after a session was seen, before that
/// counts as the session being lost.
pub const UNREADABLE_POLLS_TO_LOSE: u8 = 2;

/// The presence state machine: connected / disconnected / unknown, and the one edge that
/// matters.
#[derive(Debug, Clone, Default)]
pub struct PresenceMachine {
    state: Presence,
    /// Consecutive unreadable polls while connected.
    unreadable: u8,
}

impl PresenceMachine {
    /// Take one verdict. `true` exactly when presence is LOST on it: connected, then a
    /// disconnect — or connected, then a log that stays unreadable for
    /// [`UNREADABLE_POLLS_TO_LOSE`] polls in a row. One failed read is retried by the next poll
    /// rather than acted on, so a rotation caught between its rename and its new file does not
    /// read as a drop; two in a row are a lost session, which is the fail-safe the operator
    /// signed off ("if it's unreadable, presence counts as lost").
    pub fn observe(&mut self, verdict: Verdict) -> bool {
        match verdict {
            Verdict::Connected => {
                self.state = Presence::Connected;
                self.unreadable = 0;
                false
            }
            Verdict::Disconnected => {
                let lost = self.state == Presence::Connected;
                self.state = Presence::Disconnected;
                self.unreadable = 0;
                lost
            }
            Verdict::Unreadable if self.state == Presence::Connected => {
                self.unreadable = self.unreadable.saturating_add(1);
                if self.unreadable < UNREADABLE_POLLS_TO_LOSE {
                    return false;
                }
                self.state = Presence::Unknown;
                self.unreadable = 0;
                true
            }
            // Never connected (or already gone): an operator at the shack. Nothing to do.
            Verdict::Unreadable => {
                self.state = Presence::Unknown;
                false
            }
        }
    }

    pub fn state(&self) -> Presence {
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    const FIXTURE: &str = include_str!("../tests/fixtures/parsec/host-log.txt");

    fn connected(g: &str) -> Option<LogEvent> {
        Some(LogEvent::Connected(g.to_string()))
    }
    fn disconnected(g: &str) -> Option<LogEvent> {
        Some(LogEvent::Disconnected(g.to_string()))
    }

    // ----- the parser -----

    #[test]
    fn parses_the_connect_and_disconnect_lines_real_logs_carry() {
        assert_eq!(
            parse_line("[I 2023-06-20 10:44:26] someone#7777777 connected."),
            connected("someone#7777777")
        );
        assert_eq!(
            parse_line("[I 2023-06-20 10:44:45] someone#7777777 disconnected."),
            disconnected("someone#7777777")
        );
        assert_eq!(
            parse_line("[F 2026-08-11 17:23:28] ===== Parsec: Started ====="),
            Some(LogEvent::Started)
        );
        // Windows line endings, as the file is written on the host.
        assert_eq!(
            parse_line("[I 2026-09-27 08:01:10] shackop#1234567 connected.\r"),
            connected("shackop#1234567")
        );
        // A name with a space in it is still one guest.
        assert_eq!(
            parse_line("[I 2026-09-27 08:01:10] shack op#1234567 connected."),
            connected("shack op#1234567")
        );
    }

    /// Everything here looks like a connection to a careless matcher. A false CONNECT is the
    /// dangerous one: it is what would let presence mode stop an operator sitting at the shack.
    #[test]
    fn rejects_the_lines_that_only_look_like_connections() {
        for line in [
            // Parsec talking to itself, dozens of times a session.
            "[D 2026-09-27 07:58:18] IPC AS Client Connected.",
            // The router, not a guest.
            "[D 2026-09-27 08:02:00] UPNP: 192.0.2.1 reported as not connected",
            "[D 2026-09-27 08:02:00] Client 'shackop#1234567' went dormant",
            // Parsec's own IPC endpoint written in guest form is still not a guest.
            "[I 2026-09-27 08:01:10] IPC#3 connected.",
            "[I 2026-09-27 08:01:10] ipc#3 connected.",
            // A connect at any level but info is not the shape real logs show.
            "[D 2026-09-27 08:01:10] shackop#1234567 connected.",
            // No `#user-id`, a non-numeric id, an empty name.
            "[I 2026-09-27 08:01:10] shackop connected.",
            "[I 2026-09-27 08:01:10] shackop#12a4 connected.",
            "[I 2026-09-27 08:01:10] #1234567 connected.",
            "[I 2026-09-27 08:01:10]  shackop#1234567 connected.",
            // Anything after the period.
            "[I 2026-09-27 08:01:10] shackop#1234567 connected. again",
            // Not a log line at all.
            "shackop#1234567 connected.",
            "[I 2026-09-27] shackop#1234567 connected.",
            "[i 2026-09-27 08:01:10] shackop#1234567 connected.",
            "[I 2026-09-27 08:01:1x] shackop#1234567 connected.",
            "[0] FPS:59.9/0, L:2.5/7.1, B:3.2/10.0, N:0/0/0",
            "",
            "\u{0}\u{1}\u{fffd}garbage",
        ] {
            assert_eq!(parse_line(line), None, "{line:?} must not parse");
        }
    }

    /// A disconnect can only remove a guest a connect line added, so it is read loosely: Parsec
    /// says a client disconnects "with which error if applicable", and the line for a hard drop
    /// may carry one.
    #[test]
    fn a_disconnect_with_trailing_detail_still_counts() {
        for line in [
            "[I 2026-09-27 08:05:45] shackop#1234567 disconnected. (-12007)",
            "[W 2026-09-27 08:05:45] shackop#1234567 disconnected (-12007)",
            "[E 2026-09-27 08:05:45] shackop#1234567 disconnected: network",
        ] {
            assert_eq!(
                parse_line(line),
                disconnected("shackop#1234567"),
                "{line:?}"
            );
        }
        // …but still only for a guest-shaped token.
        assert_eq!(
            parse_line("[I 2026-09-27 08:05:45] IPC#3 disconnected."),
            None
        );
        assert_eq!(
            parse_line("[I 2026-09-27 08:05:45] shackop#1234567 disconnectedly"),
            None
        );
    }

    #[test]
    fn the_fixture_log_balances() {
        let events: Vec<LogEvent> = FIXTURE.lines().filter_map(parse_line).collect();
        assert_eq!(
            events,
            vec![
                LogEvent::Started,
                LogEvent::Connected("shackop#1234567".into()),
                LogEvent::Connected("visitor#7654321".into()),
                LogEvent::Disconnected("visitor#7654321".into()),
                LogEvent::Disconnected("shackop#1234567".into()),
            ]
        );
    }

    #[test]
    fn candidates_are_per_user_then_per_computer() {
        let c = log_candidates(
            Some(PathBuf::from("appdata")),
            Some(PathBuf::from("programdata")),
        );
        assert_eq!(
            c,
            vec![
                Path::new("appdata").join("Parsec").join("log.txt"),
                Path::new("programdata").join("Parsec").join("log.txt"),
            ]
        );
        assert_eq!(
            log_candidates(None, Some(PathBuf::from("pd"))),
            vec![Path::new("pd").join("Parsec").join("log.txt")]
        );
        assert!(log_candidates(Some(PathBuf::new()), None).is_empty());
    }

    // ----- the follower -----

    /// A scratch Parsec directory with a `log.txt` in it, removed on drop.
    struct Scratch {
        dir: PathBuf,
    }
    impl Scratch {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("nexus-parsec-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self { dir }
        }
        fn log(&self) -> PathBuf {
            self.dir.join(LOG_FILE_NAME)
        }
        fn rotated(&self) -> PathBuf {
            self.dir.join(ROTATED_FILE_NAME)
        }
        fn append(&self, text: &str) {
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.log())
                .unwrap();
            f.write_all(text.as_bytes()).unwrap();
        }
        fn follower(&self) -> ParsecLog {
            ParsecLog::new(vec![self.log()])
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    const STARTED: &str = "[F 2026-09-27 07:58:12] ===== Parsec: Started =====\n";
    const NOISE: &str = "[D 2026-09-27 07:58:18] IPC AS Client Connected.\n\
                         [D 2026-09-27 08:01:17] [0] FPS:59.9/0, L:2.5/7.1, B:3.2/10.0, N:0/0/0\n";
    const OP_ON: &str = "[I 2026-09-27 08:01:10] shackop#1234567 connected.\n";
    const OP_OFF: &str = "[I 2026-09-27 08:05:45] shackop#1234567 disconnected.\n";
    const VISITOR_ON: &str = "[I 2026-09-27 08:03:30] visitor#7654321 connected.\n";
    const VISITOR_OFF: &str = "[I 2026-09-27 08:04:02] visitor#7654321 disconnected.\n";

    #[test]
    fn a_session_is_connected_until_its_own_disconnect_line() {
        let s = Scratch::new("session");
        s.append(STARTED);
        s.append(NOISE);
        let mut log = s.follower();
        assert_eq!(
            log.poll(),
            Verdict::Disconnected,
            "a readable log, nobody on"
        );
        assert_eq!(log.path(), Some(s.log().as_path()));
        s.append(OP_ON);
        assert_eq!(log.poll(), Verdict::Connected);
        s.append(NOISE);
        assert_eq!(log.poll(), Verdict::Connected, "noise changes nothing");
        // A second guest comes and goes; the operator is still on.
        s.append(VISITOR_ON);
        s.append(VISITOR_OFF);
        assert_eq!(log.poll(), Verdict::Connected);
        s.append(OP_OFF);
        assert_eq!(log.poll(), Verdict::Disconnected);
    }

    #[test]
    fn starting_mid_session_finds_the_guest_already_on() {
        let s = Scratch::new("midsession");
        s.append(STARTED);
        s.append(OP_ON);
        s.append(NOISE);
        let mut log = s.follower();
        assert_eq!(log.poll(), Verdict::Connected);
        // …and a session that ended before Nexus started is not one.
        let t = Scratch::new("ended");
        t.append(STARTED);
        t.append(OP_ON);
        t.append(OP_OFF);
        assert_eq!(t.follower().poll(), Verdict::Disconnected);
    }

    /// A restart marker ends every session before it — while following, and when an old
    /// session's connect line is still in the file at startup.
    #[test]
    fn a_restart_marker_ends_every_session() {
        let s = Scratch::new("restart");
        s.append(STARTED);
        s.append(OP_ON);
        let mut log = s.follower();
        assert_eq!(log.poll(), Verdict::Connected);
        s.append(STARTED);
        assert_eq!(log.poll(), Verdict::Disconnected);

        let t = Scratch::new("restart-history");
        t.append(STARTED);
        t.append(OP_ON); // Parsec died without a disconnect line…
        t.append(STARTED); // …and came back.
        assert_eq!(t.follower().poll(), Verdict::Disconnected);
    }

    /// The disconnect is written into the old file, the file is rotated, and a fresh log.txt
    /// begins — all between two polls. The disconnect must not be lost with the rotation.
    #[test]
    fn a_disconnect_written_just_before_a_rotation_is_not_lost() {
        let s = Scratch::new("rotation");
        s.append(STARTED);
        s.append(NOISE);
        s.append(OP_ON);
        let mut log = s.follower();
        assert_eq!(log.poll(), Verdict::Connected);
        s.append(NOISE);
        s.append(OP_OFF);
        std::fs::rename(s.log(), s.rotated()).unwrap();
        s.append("[D 2026-09-27 09:00:00] display_hz    = 60\n");
        assert_eq!(log.poll(), Verdict::Disconnected);

        // CONTROL: the same rotation with the operator still on stays connected, and what the
        // fresh file says next is read from its first byte.
        let t = Scratch::new("rotation-still-on");
        t.append(STARTED);
        t.append(NOISE);
        t.append(OP_ON);
        let mut log = t.follower();
        assert_eq!(log.poll(), Verdict::Connected);
        t.append(NOISE);
        std::fs::rename(t.log(), t.rotated()).unwrap();
        t.append(NOISE);
        assert_eq!(log.poll(), Verdict::Connected, "a rotation is not a drop");
        t.append(OP_OFF);
        assert_eq!(log.poll(), Verdict::Disconnected);
    }

    /// A Nexus started after a rotation still sees a session whose connect line is in the
    /// rotated file.
    #[test]
    fn a_session_that_began_before_a_rotation_is_found_at_startup() {
        let s = Scratch::new("rotated-history");
        s.append(STARTED);
        s.append(OP_ON);
        std::fs::rename(s.log(), s.rotated()).unwrap();
        s.append(NOISE);
        assert_eq!(s.follower().poll(), Verdict::Connected);
    }

    /// A file that shrank with no rotated file beside it is read again from its first byte.
    #[test]
    fn a_truncated_log_is_read_again_from_the_start() {
        let s = Scratch::new("truncated");
        s.append(STARTED);
        s.append(NOISE);
        s.append(NOISE);
        s.append(OP_ON);
        let mut log = s.follower();
        assert_eq!(log.poll(), Verdict::Connected);
        std::fs::write(s.log(), STARTED).unwrap();
        assert_eq!(log.poll(), Verdict::Disconnected);
    }

    #[test]
    fn a_line_split_across_two_reads_is_completed_not_dropped() {
        let s = Scratch::new("partial");
        s.append(STARTED);
        let mut log = s.follower();
        assert_eq!(log.poll(), Verdict::Disconnected);
        let (head, tail) = OP_ON.split_at(30);
        s.append(head);
        assert_eq!(
            log.poll(),
            Verdict::Disconnected,
            "half a line is nothing yet"
        );
        s.append(tail);
        assert_eq!(log.poll(), Verdict::Connected);
    }

    #[test]
    fn garbage_is_ignored_and_changes_nothing() {
        let s = Scratch::new("garbage");
        std::fs::write(
            s.log(),
            b"\x00\xff\xfe binary \x01\x02\n[I nonsense\n\n\r\n[D 2026-09-27 07:58:18] IPC AS Client Connected.\n",
        )
        .unwrap();
        let mut log = s.follower();
        assert_eq!(log.poll(), Verdict::Disconnected);
        s.append(OP_ON);
        assert_eq!(log.poll(), Verdict::Connected);
        s.append("\u{fffd}\u{fffd} not a line\n");
        assert_eq!(log.poll(), Verdict::Connected);
    }

    #[test]
    fn a_missing_log_is_unreadable_and_a_vanished_one_too() {
        let s = Scratch::new("missing");
        let mut log = s.follower();
        assert_eq!(log.poll(), Verdict::Unreadable, "no file at all");
        assert_eq!(log.path(), None);
        s.append(STARTED);
        s.append(OP_ON);
        assert_eq!(log.poll(), Verdict::Connected, "found once it appears");
        std::fs::remove_file(s.log()).unwrap();
        assert_eq!(
            log.poll(),
            Verdict::Unreadable,
            "the followed file vanished"
        );
        assert!(log.poll() == Verdict::Unreadable);
    }

    /// Both installs' logs exist (the operator switched install type once): the one Parsec is
    /// writing now — the newer — is the one that counts.
    #[test]
    fn the_newest_candidate_is_the_one_followed() {
        let old = Scratch::new("cand-old");
        let new = Scratch::new("cand-new");
        old.append(STARTED);
        old.append(OP_ON);
        new.append(STARTED);
        let when = std::time::SystemTime::now();
        std::fs::File::options()
            .write(true)
            .open(old.log())
            .unwrap()
            .set_modified(when - std::time::Duration::from_secs(3600))
            .unwrap();
        std::fs::File::options()
            .write(true)
            .open(new.log())
            .unwrap()
            .set_modified(when)
            .unwrap();
        let mut log = ParsecLog::new(vec![old.log(), new.log()]);
        assert_eq!(log.poll(), Verdict::Disconnected);
        assert_eq!(log.path(), Some(new.log().as_path()));
        // CONTROL: listed the other way round, the newer file still wins.
        let mut log = ParsecLog::new(vec![new.log(), old.log()]);
        log.poll();
        assert_eq!(log.path(), Some(new.log().as_path()));
    }

    // ----- the state machine -----

    use Verdict::{Connected as C, Disconnected as D, Unreadable as U};

    /// Feed a run of verdicts; the indices at which presence was lost.
    fn losses(run: &[Verdict]) -> Vec<usize> {
        let mut m = PresenceMachine::default();
        run.iter()
            .enumerate()
            .filter_map(|(i, v)| m.observe(*v).then_some(i))
            .collect()
    }

    #[test]
    fn presence_is_lost_only_on_the_edge_from_connected() {
        assert_eq!(losses(&[C, C, D]), vec![2]);
        assert_eq!(
            losses(&[C, D, D, D]),
            vec![1],
            "once, not on every poll after"
        );
        assert_eq!(losses(&[C, D, C, D]), vec![1, 3], "each session's own drop");
        assert_eq!(
            losses(&[C, C, C, C]),
            Vec::<usize>::new(),
            "no transition, no action"
        );
    }

    #[test]
    fn never_connected_then_unreadable_does_nothing() {
        assert_eq!(losses(&[U, U, U, U]), Vec::<usize>::new());
        assert_eq!(losses(&[D, U, U, D, U, U]), Vec::<usize>::new());
        let mut m = PresenceMachine::default();
        assert_eq!(m.state(), Presence::Unknown);
        m.observe(D);
        assert_eq!(m.state(), Presence::Disconnected);
    }

    #[test]
    fn unreadable_after_connected_counts_as_lost() {
        assert_eq!(losses(&[C, U, U]), vec![2]);
        let mut m = PresenceMachine::default();
        m.observe(C);
        assert_eq!(m.state(), Presence::Connected);
        assert!(!m.observe(U), "one failed read is retried on the next poll");
        assert!(m.observe(U), "two in a row are a lost session");
        assert_eq!(m.state(), Presence::Unknown);
        assert!(!m.observe(U), "and it is lost once");
    }

    /// The guard's other direction: a single failed read between good ones — a rotation caught
    /// between its rename and its new file — is not a dropped session.
    #[test]
    fn one_unreadable_poll_between_connected_polls_is_not_a_drop() {
        assert_eq!(losses(&[C, U, C, U, C, U, C]), Vec::<usize>::new());
        assert_eq!(
            losses(&[C, U, D]),
            vec![2],
            "a real disconnect still counts at once"
        );
    }
}
