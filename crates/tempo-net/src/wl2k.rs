//! The Winlink CMS telnet transport — one session to `server.winlink.org:8772`, and no more.
//!
//! Same *shape* as [`cluster`](crate::cluster) and [`aprsis`](crate::aprsis): a blocking thread
//! with a read timeout around a [`pump`] that is pure over any `Read`/`Write`, so the whole
//! transport unit-tests from a pair of buffers and a `127.0.0.1:0` listener. Two things make it
//! deliberately *unlike* both, and each is a rule rather than an accident:
//!
//! 1. **One session, never a reconnect loop.** `aprsis::run` reconnects with backoff forever
//!    because a dropped APRS feed costs nothing. A Winlink session is a mail transaction: a
//!    reconnect re-runs the whole handshake against a CMS that has already handed some messages
//!    over, so the cost of a silent retry is duplicated or half-delivered mail. [`run`] connects
//!    once, pumps until the session says it is done, and returns what happened. The operator
//!    reconnects; this file does not.
//! 2. **No polling, ever** (programme spec: *auto-polling on any transport — no unattended radio
//!    sessions, permanently*). Nothing here has a timer, and nothing here may grow one.
//!
//! # The division of labour
//!
//! This file owns the bytes on the socket: the connect, the read timeout, the telnet pre-login,
//! the stop flag, the capture tap. It owns **nothing** about B2F. It cannot: `tempo-net` is a leaf
//! over `std` sockets with no `tempo-core` dependency (see the crate header), and the B2F engine
//! lives in `tempo_core::winlink::b2f`. The two are joined one level up, in
//! `tempo_app::winlink`, through [`ByteSession`] — which is why the same pump will later drive an
//! ARDOP link with no change here.
//!
//! **Lines are bytes, not `String`.** The B2F stream that follows the pre-login carries SOH/STX/EOT
//! binary and an LZHUF body; decoding it as UTF-8 would corrupt it inbound and — worse — outbound.
//! The pre-login below reads *prompts* as text because a telnet prompt is text, and it hands every
//! byte it did not consume back untouched.
//!
//! # ⚠️ The pre-login is a READING, not a citation
//!
//! No document in hand specifies the CMS telnet greeting. What is implemented is one reading,
//! confined to three items so a live connect settles it by editing them and nothing else:
//! [`is_callsign_prompt`], [`password_prompt_end`] and [`CMS_TELNET_PASSWORD`]. **Until a real CMS
//! capture exists this pre-login is unproven**, exactly as `b2f.rs`'s six readings are — and the
//! same is true of every wire reading in `b2f.rs` itself, because its golden transcript is a
//! constructed one that agrees with whatever that implementation does. Nothing in this crate may
//! be cited as evidence about a real CMS until that capture lands.

/// The public CMS telnet host. Named rather than inlined so a task that wants to point at a test
/// server changes one constant.
pub const CMS_HOST: &str = "server.winlink.org";
/// The CMS telnet port (programme spec §1).
pub const CMS_PORT: u16 = 8772;
/// The fixed string the CMS telnet front door wants at its `Password :` prompt.
///
/// ⚠️ **This is not the operator's Winlink account password and must never be confused with it.**
/// The account password is used exactly once, by `tempo_core::winlink::secure::pr_response` inside
/// the B2F session, to answer the `;PQ:` challenge — and it never crosses this pre-login. This
/// constant is a shared doorway token, which is why it is a compile-time constant in a public
/// repository and why [`Login::new`] still takes it as a parameter rather than reading it here (a
/// test, and a future private CMS, pass their own).
///
/// **UNPROVEN — see the module header.** Settled by a real capture.
pub const CMS_TELNET_PASSWORD: &str = "CMSTelnet";

/// Longest run of bytes the pre-login will hold while looking for a prompt.
///
/// A prompt is short and arrives with at most a banner in front of it. Without this a server that
/// streamed forever without prompting would grow this buffer without bound — the same door the
/// B2F session bounds in `b2f::Session::feed`, at the same place: where the bytes come in. Over
/// the limit the buffer keeps only its **tail**, because a prompt is by construction at the end
/// of what has arrived; dropping the head loses nothing a prompt could be in.
pub const MAX_PRELOGIN_BUF: usize = 8 * 1024;

/// A byte-oriented session the transport drives, knowing nothing about what it speaks.
///
/// This is the seam between spec Layer A (the protocol, in `tempo-core`) and Layer B (the socket,
/// here). The implementor for Winlink is `tempo_app::winlink::Driver`, which owns a
/// `tempo_core::winlink::b2f::Session`; the ARDOP transport will drive the identical trait with
/// the identical driver, which is the whole reason the telnet path can settle the wire questions
/// with no radio.
pub trait ByteSession {
    /// Feed received bytes; return the byte strings to write back, in order.
    fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>>;
    /// Whether the transport may close: the session is finished or has failed.
    fn wants_close(&self) -> bool;
}

/// What the pre-login wants done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// Write these bytes to the server.
    Send(Vec<u8>),
    /// Pre-login is over. These are the bytes that arrived after the answered prompt and belong
    /// to the B2F session — handed back rather than consumed, because the CMS's SID routinely
    /// shares a read with its last prompt. Emitted exactly once, and always last.
    Ready(Vec<u8>),
}

/// The CMS telnet pre-login: answer `Callsign :`, answer `Password :`, then get out of the way.
pub struct Login {
    /// The callsign to answer the callsign prompt with, upper-cased at construction.
    callsign: Vec<u8>,
    /// The doorway token to answer the password prompt with — see [`CMS_TELNET_PASSWORD`].
    telnet_password: Vec<u8>,
    /// Bytes seen since the last prompt was answered, scanned for the next one.
    buf: Vec<u8>,
    /// Whether a callsign prompt has been answered at least once.
    answered_call: bool,
    /// Whether [`Step::Ready`] has been emitted. After this, every byte is the session's.
    done: bool,
}

impl Login {
    /// A pre-login that will answer with `callsign` and `telnet_password`.
    pub fn new(callsign: &str, telnet_password: &str) -> Self {
        Login {
            callsign: callsign.trim().to_ascii_uppercase().into_bytes(),
            telnet_password: telnet_password.as_bytes().to_vec(),
            buf: Vec::new(),
            answered_call: false,
            done: false,
        }
    }

    /// Whether the pre-login has finished. After this, every byte is the session's.
    pub fn done(&self) -> bool {
        self.done
    }

    /// Bytes held on the server's behalf — the bound this type is responsible for.
    ///
    /// `capacity`, not `len`: the charge is retained heap, which is what a memory bound is about.
    /// (Charging wire bytes instead is the accounting error the B2F session shipped with and had
    /// to have corrected — 2.9 MB of legal wire read as 157.7 MiB of heap.)
    pub fn held_bytes(&self) -> usize {
        self.buf.capacity()
    }

    /// Feed received bytes. Returns the answers to send and, once, the leftover B2F bytes.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Step> {
        let mut out = Vec::new();
        if self.done {
            // Nothing after `Ready` belongs here; the caller must stop feeding this type. Being
            // silent rather than panicking keeps a caller bug from taking the socket down.
            return out;
        }
        self.buf.extend_from_slice(chunk);
        loop {
            // One branch covers the first prompt AND every re-prompt: the CMS asks for the
            // callsign again after refusing one, and answering only the first wedges the session
            // forever — open TCP, no data, no retry. cluster.rs paid for that lesson.
            //
            // ⚠️ ASSUMPTION, and it is the one thing here that could wedge a session: the two
            // prompts never share a read. That holds because the server cannot write `Password :`
            // before it has read our callsign, so TCP has nothing to coalesce them from. If a
            // future CMS or a proxy ever does send both at once, the callsign prompt is no longer
            // the buffer's tail, this suffix test misses it, and the pre-login sits forever. The
            // password prompt is NOT exposed to that risk — it is found by substring search
            // ([`password_prompt_end`]) precisely because the SID legitimately shares its read.
            if is_callsign_prompt(&self.buf) {
                self.answered_call = true;
                let mut line = self.callsign.clone();
                line.push(b'\r');
                out.push(Step::Send(line));
                // The prompt and everything before it is consumed; the buffer restarts empty so
                // the next scan cannot re-match the prompt we just answered. Nothing is lost:
                // the CMS does not write past this prompt until it has read our answer, so the
                // buffer's tail IS the prompt.
                self.buf.clear();
                continue;
            }
            if self.answered_call {
                if let Some(at) = password_prompt_end(&self.buf) {
                    let mut line = self.telnet_password.clone();
                    line.push(b'\r');
                    out.push(Step::Send(line));
                    self.done = true;
                    // Everything past the prompt is B2F's. Split rather than dropped: the SID
                    // shares this chunk more often than not.
                    let rest = self.buf.split_off(at);
                    self.buf = Vec::new();
                    out.push(Step::Ready(rest));
                    return out;
                }
            }
            break;
        }
        if self.buf.len() > MAX_PRELOGIN_BUF {
            // Keep the tail: a prompt is at the end of what has arrived, so the head cannot hold
            // one. `drain` on a `Vec` memmoves, and this runs at most once per oversized read.
            let drop = self.buf.len() - MAX_PRELOGIN_BUF;
            self.buf.drain(..drop);
            self.buf.shrink_to(MAX_PRELOGIN_BUF);
        }
        out
    }
}

/// Is the buffer's TRAILING (incomplete) line a callsign prompt?
///
/// **READING — see the module header.** Telnet prompts arrive without a newline, so only the text
/// after the last `\n` is considered, and a prompt terminal is required: a banner or MOTD line
/// that merely *mentions* a callsign must not log us in prematurely, which is exactly the
/// session-wedging bug `cluster::is_login_prompt` was written to stop.
///
/// A **suffix** test rather than [`password_prompt_end`]'s substring search, and the asymmetry is
/// deliberate: nothing follows this prompt until the CMS has read our answer, so there is never a
/// leftover to hand back, and the tighter predicate is the one that keeps a MOTD out.
///
/// ⚠️ **Which half of this actually stops a MOTD — measured by mutation, not reasoned.** The
/// `ends_with` requirement is the load-bearing one: mutating it to `contains` reddens
/// `a_banner_line_mentioning_callsign_does_not_trigger_a_login`. [`prompt_tail`]'s
/// last-line slicing is **redundant while the predicate is a suffix test** — a needle with no
/// newline in it is a suffix of the whole right-trimmed buffer exactly when it is a suffix of the
/// right-trimmed last line — and mutating that half alone leaves the suite green. It is kept as
/// defence in depth (and because `cluster::is_login_prompt`, the shipped version of this idea,
/// has the same shape), not because it is independently doing the work.
fn is_callsign_prompt(buf: &[u8]) -> bool {
    let tail = prompt_tail(buf);
    tail.ends_with("callsign :") || tail.ends_with("callsign:") || tail.ends_with("login:")
}

/// The end offset of a password prompt in the buffer, or `None`.
///
/// Returns an offset rather than a `bool` because the bytes *after* the prompt are the B2F
/// session's and must be handed back — see [`Step::Ready`] — and because the CMS's SID routinely
/// shares the read with this prompt, which means the prompt is frequently **not** the tail. So
/// this is a case-insensitive substring search over the whole buffer for `assword :` / `assword:`,
/// returning the offset just past the match.
///
/// It is only ever called once a callsign prompt has been answered, so the buffer it scans is the
/// CMS's answer to our callsign and not an arbitrary MOTD.
///
/// **READING — see the module header.**
fn password_prompt_end(buf: &[u8]) -> Option<usize> {
    // Lower-cased once; the buffer here is bounded by `MAX_PRELOGIN_BUF`.
    let hay: Vec<u8> = buf.to_ascii_lowercase();
    for needle in [&b"assword :"[..], &b"assword:"[..]] {
        if let Some(at) = hay
            .windows(needle.len())
            .position(|w| w == needle)
            .map(|at| at + needle.len())
        {
            return Some(at);
        }
    }
    None
}

/// The trailing, newline-less text of the buffer, lowercased and right-trimmed — the only part a
/// prompt can be in. Lossy UTF-8 on purpose: this is a *text* judgement about a *prompt*, the
/// result is a `bool`, and no byte of it reaches the wire or the session.
fn prompt_tail(buf: &[u8]) -> String {
    let from = buf.iter().rposition(|&b| b == b'\n').map_or(0, |at| at + 1);
    String::from_utf8_lossy(&buf[from..])
        .trim_end()
        .to_ascii_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sends(steps: &[Step]) -> Vec<String> {
        steps
            .iter()
            .filter_map(|s| match s {
                Step::Send(b) => Some(String::from_utf8_lossy(b).into_owned()),
                Step::Ready(_) => None,
            })
            .collect()
    }

    fn ready(steps: &[Step]) -> Option<Vec<u8>> {
        steps.iter().find_map(|s| match s {
            Step::Ready(rest) => Some(rest.clone()),
            Step::Send(_) => None,
        })
    }

    #[test]
    fn the_callsign_prompt_is_answered_and_then_the_password_prompt() {
        let mut l = Login::new("N0CALL", CMS_TELNET_PASSWORD);
        // The banner and the prompt arrive together, and the prompt has NO newline.
        let a = l.feed(b"[WL2K Central CMS]\r\nCallsign :");
        assert_eq!(sends(&a), vec!["N0CALL\r".to_string()], "{a:?}");
        assert!(ready(&a).is_none(), "pre-login is not over yet: {a:?}");
        assert!(!l.done());

        let b = l.feed(b"\r\nPassword :");
        assert_eq!(sends(&b), vec![format!("{CMS_TELNET_PASSWORD}\r")], "{b:?}");
        assert_eq!(
            ready(&b),
            Some(Vec::new()),
            "answering the password ends pre-login with no leftover: {b:?}"
        );
        assert!(l.done());
    }

    #[test]
    fn the_b2f_stream_that_shares_the_password_prompts_chunk_is_handed_back() {
        // THE BOUNDARY BUG THIS TYPE EXISTS TO NOT HAVE. The CMS's SID can arrive in the same
        // read as the password prompt; a pre-login that swallowed it would lose the peer's SID
        // and the B2F session would fail as "FBB command before the peer's SID".
        let mut l = Login::new("N0CALL", CMS_TELNET_PASSWORD);
        let _ = l.feed(b"Callsign :");
        let steps = l.feed(b"\r\nPassword :\r\n[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r");
        assert_eq!(sends(&steps), vec![format!("{CMS_TELNET_PASSWORD}\r")]);
        assert_eq!(
            ready(&steps).map(|r| String::from_utf8_lossy(&r).into_owned()),
            Some("\r\n[WL2K-5.0-B2FWIHJM$]\r;PQ: 41913235\r".to_string()),
            "every byte after the answered prompt belongs to B2F: {steps:?}"
        );
    }

    #[test]
    fn a_banner_line_mentioning_callsign_does_not_trigger_a_login() {
        // cluster.rs's exact lesson, and it wedged a session there: only the TRAILING,
        // newline-less prompt logs in. A MOTD body line that says the word must not.
        //
        // TWO shapes, because they are stopped by two different halves of the predicate and a
        // test that only carries the first is neutralised by either half alone (found by the
        // positive control: a `contains("callsign")` mutation left the one-shape version green,
        // because `prompt_tail` had already thrown the completed line away).
        let mut l = Login::new("N0CALL", CMS_TELNET_PASSWORD);
        // (a) a COMPLETE line — stopped by `prompt_tail` looking only past the last newline.
        assert!(
            l.feed(b"Enter your callsign at the prompt below.\r\n")
                .is_empty(),
            "a complete line is never a prompt"
        );
        // (b) an INCOMPLETE trailing line that mentions the word — it IS the tail, so only the
        // prompt-terminal requirement stops it. This is the half a naive `contains` would lose.
        assert!(
            l.feed(b"Waiting for your callsign now").is_empty(),
            "a trailing line that merely mentions the word is not a prompt"
        );
        assert_eq!(
            sends(&l.feed(b"\r\nCallsign :")),
            vec!["N0CALL\r".to_string()]
        );
    }

    #[test]
    fn a_re_prompt_is_re_answered() {
        // cluster.rs answers EVERY prompt, not just the first, because a node that rejects a
        // call re-prompts and answering once wedged the session forever (open TCP, no data,
        // no retry). Same failure is available here.
        let mut l = Login::new("N0CALL", CMS_TELNET_PASSWORD);
        assert_eq!(sends(&l.feed(b"Callsign :")), vec!["N0CALL\r".to_string()]);
        let again = l.feed(b"\r\n*** Unknown callsign\r\nCallsign :");
        assert_eq!(sends(&again), vec!["N0CALL\r".to_string()], "{again:?}");
        assert!(!l.done(), "a re-prompt means we are not logged in");
    }

    #[test]
    fn the_prompt_is_found_across_a_chunk_boundary() {
        let mut l = Login::new("N0CALL", CMS_TELNET_PASSWORD);
        let mut steps = Vec::new();
        for byte in b"Callsign :" {
            steps.extend(l.feed(&[*byte]));
        }
        assert_eq!(sends(&steps), vec!["N0CALL\r".to_string()], "{steps:?}");
    }

    /// A `Read` that hands over one queued chunk per call, then reports EOF — a socket's
    /// framing, which `std::io::Cursor` deliberately does not have.
    struct Chunks(Vec<Vec<u8>>);

    impl Read for Chunks {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if self.0.is_empty() {
                return Ok(0);
            }
            let next = self.0.remove(0);
            let n = next.len().min(buf.len());
            buf[..n].copy_from_slice(&next[..n]);
            Ok(n)
        }
    }

    /// A `ByteSession` that answers `PING\r` with `PONG\r` and then wants to close.
    struct Pong {
        saw: Vec<u8>,
        done: bool,
    }

    impl ByteSession for Pong {
        fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
            self.saw.extend_from_slice(chunk);
            if !self.done && self.saw.windows(5).any(|w| w == b"PING\r") {
                self.done = true;
                return vec![b"PONG\r".to_vec()];
            }
            Vec::new()
        }
        fn wants_close(&self) -> bool {
            self.done
        }
    }

    #[test]
    fn pump_latches_logged_in_at_the_handover_and_carries_the_shared_bytes() {
        // `pump` is pure over Read/Write, so the latch is observable here where the loopback test
        // cannot see it: `run` clears the live state as it returns (a status chip must not read
        // "logged in" after a disconnect), so the loopback test asserts the durable evidence
        // instead and this one asserts the latch itself.
        //
        // The reader hands the password prompt and the first protocol bytes over in ONE chunk,
        // which is the boundary `Step::Ready` exists for: without the carry, `PING` is swallowed
        // by the pre-login and the session never speaks.
        // One chunk per `read`, like a socket — and unlike a `Cursor`, which would hand the
        // whole script over in one call and put both prompts in one buffer, a shape a real CMS
        // cannot produce (see the assumption noted at `Login::feed`).
        let script = Chunks(vec![
            b"Callsign :".to_vec(),
            b"\r\nPassword :PING\r".to_vec(),
        ]);
        let mut written: Vec<u8> = Vec::new();
        let stop = std::sync::atomic::AtomicBool::new(false);
        let state = SessionState::default();
        let mut sess = Pong {
            saw: Vec::new(),
            done: false,
        };
        let outcome = pump(
            script,
            &mut written,
            Login::new("N0CALL", CMS_TELNET_PASSWORD),
            &mut sess,
            &stop,
            &state,
            &|| 1_700_000_000,
        );
        assert_eq!(outcome, Outcome::Complete, "written: {written:?}");
        assert!(
            state.logged_in.load(std::sync::atomic::Ordering::Relaxed),
            "logged_in never latched at the handover"
        );
        let sent = String::from_utf8_lossy(&written).into_owned();
        assert_eq!(
            sent,
            format!("N0CALL\r{CMS_TELNET_PASSWORD}\rPONG\r"),
            "{sent:?}"
        );
        assert_eq!(
            state.bytes_out.load(std::sync::atomic::Ordering::Relaxed),
            written.len() as u64,
            "the byte counter drifted from the writes"
        );
        assert_eq!(
            state
                .last_byte_unix
                .load(std::sync::atomic::Ordering::Relaxed),
            1_700_000_000,
            "the clock the caller supplied is the one recorded"
        );
    }

    #[test]
    fn a_flood_with_no_prompt_in_it_does_not_grow_without_bound() {
        let mut l = Login::new("N0CALL", CMS_TELNET_PASSWORD);
        for _ in 0..4096 {
            assert!(l.feed(&[b'x'; 1024]).is_empty());
        }
        assert!(
            l.held_bytes() <= MAX_PRELOGIN_BUF,
            "pre-login held {} bytes",
            l.held_bytes()
        );
    }
}

// ---------------------------------------------------------------------------------------------
// The socket half: one bounded connect, one pump, one session. See the module header's rule 1.
// ---------------------------------------------------------------------------------------------

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::Duration;

/// Bounded connect — `flexcat.rs`'s rule, for its reason: a `TcpStream::connect` with no timeout
/// can sit for the OS's own SYN budget (over two minutes on Linux) with the UI showing nothing.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The socket read timeout. It is not a protocol timeout — it exists so the pump loop periodically
/// wakes to observe `stop`, which is what makes Disconnect prompt on a silent socket.
const READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Live state of one session, for the status chip. Written by the socket thread, read by the UI
/// poll. Atomics rather than a lock for `aprsis::FeedState`'s reason: a stalled reader must never
/// be able to block a status read.
#[derive(Debug, Default)]
pub struct SessionState {
    /// The TCP connection is up.
    pub connected: AtomicBool,
    /// The telnet pre-login finished — the B2F session has begun.
    pub logged_in: AtomicBool,
    /// Bytes received from the peer this session.
    pub bytes_in: AtomicU64,
    /// Bytes written to the peer this session.
    pub bytes_out: AtomicU64,
    /// Unix seconds of the most recent byte in either direction.
    pub last_byte_unix: AtomicI64,
}

/// How a session ended. Not a `Result`, because three of the four are ordinary outcomes an
/// operator needs told apart, and only the fourth is an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// The session said it was finished and we closed. The good ending.
    Complete,
    /// The stop flag was set: the operator disconnected.
    Stopped,
    /// The peer closed the socket before the session was finished.
    PeerClosed,
    /// A socket error, rendered. **Never carries anything the caller passed in** — see [`run`].
    Io(String),
}

/// Drive one pre-login and one [`ByteSession`] over a connected duplex until the session is
/// finished, the peer closes, or `stop` is set.
///
/// Pure over any `Read`/`Write`, so the whole path is testable from a pair of buffers.
pub fn pump<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    mut login: Login,
    session: &mut dyn ByteSession,
    stop: &AtomicBool,
    state: &SessionState,
    now_unix: &dyn Fn() -> i64,
) -> Outcome {
    let mut buf = [0u8; 4096];
    // Bytes that arrived with the last prompt and belong to the session, not the pre-login.
    let mut carry: Vec<u8> = Vec::new();
    loop {
        if stop.load(Ordering::Relaxed) {
            return Outcome::Stopped;
        }
        if login.done() && session.wants_close() {
            return Outcome::Complete;
        }
        if !carry.is_empty() {
            let chunk = std::mem::take(&mut carry);
            for out in session.feed(&chunk) {
                if let Err(e) = write_all(&mut writer, &out, state) {
                    return Outcome::Io(e.to_string());
                }
            }
            continue;
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => {
                return if login.done() && session.wants_close() {
                    Outcome::Complete
                } else {
                    Outcome::PeerClosed
                }
            }
            Ok(n) => n,
            // The read timeout wakes the loop so `stop` is observed on a quiet socket.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                continue
            }
            Err(e) => return Outcome::Io(e.to_string()),
        };
        let chunk = &buf[..n];
        state.bytes_in.fetch_add(n as u64, Ordering::Relaxed);
        state.last_byte_unix.store(now_unix(), Ordering::Relaxed);

        if !login.done() {
            for step in login.feed(chunk) {
                match step {
                    Step::Send(bytes) => {
                        if let Err(e) = write_all(&mut writer, &bytes, state) {
                            return Outcome::Io(e.to_string());
                        }
                    }
                    Step::Ready(rest) => {
                        state.logged_in.store(true, Ordering::Relaxed);
                        carry = rest;
                    }
                }
            }
            continue;
        }
        for out in session.feed(chunk) {
            if let Err(e) = write_all(&mut writer, &out, state) {
                return Outcome::Io(e.to_string());
            }
        }
    }
}

/// Writes and counts. One function so the counter cannot drift from the write.
fn write_all<W: Write>(w: &mut W, bytes: &[u8], state: &SessionState) -> std::io::Result<()> {
    w.write_all(bytes)?;
    w.flush()?;
    state
        .bytes_out
        .fetch_add(bytes.len() as u64, Ordering::Relaxed);
    Ok(())
}

/// Connect to `host:port`, bounded by [`CONNECT_TIMEOUT`], with [`READ_TIMEOUT`] set.
///
/// A literal IP parses straight through with no name lookup; a hostname falls back to
/// `ToSocketAddrs` (`flexcat::connect`'s shape, and `server.winlink.org` is a hostname, so this
/// path is the normal one here rather than the fallback).
pub fn connect(host: &str, port: u16) -> std::io::Result<TcpStream> {
    let target: SocketAddr = match format!("{host}:{port}").parse() {
        Ok(sa) => sa,
        Err(_) => (host, port).to_socket_addrs()?.next().ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("cannot resolve {host}"),
            )
        })?,
    };
    let stream = TcpStream::connect_timeout(&target, CONNECT_TIMEOUT)?;
    stream.set_read_timeout(Some(READ_TIMEOUT))?;
    // A B2F answer held back by Nagle waiting for more data arrives at the CMS seconds late, and
    // the CMS is waiting for it before it sends anything else.
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

/// Connect once, run one session, return what happened.
///
/// ⚠️ **There is no reconnect loop here and there must never be one** — module header, rule 1.
/// If you are about to add `while !stop.load(...)` around this body because `aprsis::run` has one,
/// stop: that loop is correct for a telemetry feed and wrong for a mail transaction.
pub fn run(
    host: &str,
    port: u16,
    login: Login,
    session: &mut dyn ByteSession,
    stop: &AtomicBool,
    state: &SessionState,
) -> Outcome {
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    };
    let stream = match connect(host, port) {
        Ok(s) => s,
        Err(e) => return Outcome::Io(e.to_string()),
    };
    let reader = match stream.try_clone() {
        Ok(r) => r,
        Err(e) => return Outcome::Io(e.to_string()),
    };
    state.connected.store(true, Ordering::Relaxed);
    let outcome = pump(reader, stream, login, session, stop, state, &now);
    state.connected.store(false, Ordering::Relaxed);
    state.logged_in.store(false, Ordering::Relaxed);
    outcome
}
