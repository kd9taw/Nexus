//! The APRS-IS socket client — connect, log in, stream TNC2 lines, optionally upload.
//!
//! Same shape as [`cluster`](crate::cluster), because it is the same problem: a long-lived telnet
//! session against a public ham service that must survive the network going away. A blocking
//! thread with a read timeout, exponential reconnect backoff, and a [`pump`] that is pure over any
//! `Read`/`Write` so the session logic is testable without a socket.
//!
//! The division of labour: this file owns the bytes on the wire — framing, telling server chatter
//! from packets, the login handshake, reconnect. Everything about the APRS *protocol* it carries —
//! passcode, filter strings, the login line's contents, and every iGate gating rule — lives in
//! `tempo_core::aprs::is`, unit-tested against real captured traffic. The caller joins the two.
//! (The dependency cannot point the other way: `tempo-core` reaches this crate through `modes`.)
//!
//! **Lines are bytes, not `String`.** Real APRS traffic carries non-UTF-8 in the information field
//! (a raw `0x1C` Mic-E type byte, a `0xB0` degree sign), and decoding the stream as UTF-8 would
//! corrupt those packets on the way in and — far worse — on the way back out to the network.
//!
//! Receive-only by design: [`run`] cannot key a radio, and the only bytes it ever transmits are
//! the login line and packets the caller has already passed through
//! [`gate_check`](tempo_core::aprs::is::gate_check).

use std::collections::VecDeque;
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Longest line APRS-IS permits, including CR/LF. A stream that never sends a newline is a fault,
/// not a packet; the buffer resets rather than growing without bound.
const MAX_LINE: usize = 512;

/// What the session wants done with received bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Write these bytes back to the server (the login line).
    Send(Vec<u8>),
    /// A complete TNC2 packet line, CR/LF stripped, raw bytes.
    Packet(Vec<u8>),
    /// The server answered the login. `true` = verified, so uploads are permitted.
    LoggedIn(bool),
}

/// Drives the APRS-IS handshake and line extraction over an incremental byte stream.
///
/// Unlike a DX cluster, APRS-IS does not prompt — the client sends its login unasked, immediately
/// on connect, and the server answers with a `# logresp` line. So the login goes out on the first
/// [`Session::feed`] regardless of what (if anything) arrived.
pub struct Session {
    login: Vec<u8>,
    sent_login: bool,
    /// Set when the server's `# logresp` line said *verified*. Uploading before this is refused by
    /// the server anyway; gating on it keeps a read-only login from ever trying.
    pub verified: bool,
    buf: Vec<u8>,
}

impl Session {
    /// A session that will log in with `login_line` (already CRLF-terminated).
    pub fn new(login_line: &str) -> Self {
        Session {
            login: login_line.as_bytes().to_vec(),
            sent_login: false,
            verified: false,
            buf: Vec::new(),
        }
    }

    /// Feed received bytes; returns what to do. Call once with an empty chunk right after connect
    /// to emit the login.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<Action> {
        let mut out = Vec::new();
        if !self.sent_login {
            self.sent_login = true;
            out.push(Action::Send(self.login.clone()));
        }
        self.buf.extend_from_slice(chunk);
        while let Some(nl) = self.buf.iter().position(|&b| b == b'\n') {
            let mut line: Vec<u8> = self.buf.drain(..=nl).collect();
            while matches!(line.last(), Some(b'\r' | b'\n')) {
                line.pop();
            }
            if line.is_empty() {
                continue;
            }
            if is_server_line(&line) {
                // Banners, keepalives (one every 20 s per socket), and the login response.
                if let Some(v) = login_response(&line) {
                    self.verified = v;
                    out.push(Action::LoggedIn(v));
                }
                continue;
            }
            out.push(Action::Packet(line));
        }
        if self.buf.len() > MAX_LINE * 2 {
            self.buf.clear();
        }
        out
    }
}

/// Is this line server chatter rather than a packet? Banners, the login response and the 20-second
/// keepalives all arrive on the same socket, and all begin with `#`.
pub fn is_server_line(line: &[u8]) -> bool {
    line.first() == Some(&b'#')
}

/// Read the server's login response: `Some(true)` verified (uplink permitted), `Some(false)`
/// unverified (receive-only), `None` if this is not a `# logresp` line.
pub fn login_response(line: &[u8]) -> Option<bool> {
    let s = std::str::from_utf8(line).ok()?;
    let rest = s.strip_prefix('#')?.trim_start().strip_prefix("logresp")?;
    // "# logresp CALL verified, server NINTH" — the status is the second word after the keyword.
    let mut words = rest.split_whitespace();
    let _call = words.next()?;
    Some(
        words
            .next()?
            .trim_end_matches(',')
            .eq_ignore_ascii_case("verified"),
    )
}

/// Live state of the feed, for the status chip. Written by the socket thread, read by the UI poll.
#[derive(Debug, Default)]
pub struct FeedState {
    /// A session is up AND the server answered the login.
    pub connected: AtomicBool,
    /// The login was accepted as verified — the precondition for any upload.
    pub verified: AtomicBool,
    /// Packet lines received since startup.
    pub packets: std::sync::atomic::AtomicU64,
    /// Unix seconds of the most recent packet line.
    pub last_packet_unix: std::sync::atomic::AtomicI64,
    /// Packets uploaded to APRS-IS since startup (always 0 with the uplink off).
    pub uploaded: std::sync::atomic::AtomicU64,
}

/// Drive a session over a connected duplex until EOF or `stop`.
///
/// Pure over any `Read`/`Write`, so the whole login + line-extraction + upload path is testable
/// from a pair of buffers. `outbox` carries lines the caller has ALREADY gated; this function
/// applies no policy of its own beyond refusing to send on an unverified login.
#[allow(clippy::too_many_arguments)]
fn pump<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    login_line: &str,
    on_packet: &mut dyn FnMut(&[u8]),
    stop: &AtomicBool,
    state: &FeedState,
    outbox: &Mutex<VecDeque<Vec<u8>>>,
    now_unix: &dyn Fn() -> i64,
) -> std::io::Result<()> {
    let mut session = Session::new(login_line);
    let mut buf = [0u8; 4096];
    let mut chunk: &[u8] = &[];
    loop {
        if stop.load(Ordering::Relaxed) {
            return Ok(());
        }
        for action in session.feed(chunk) {
            match action {
                Action::Send(bytes) => writer.write_all(&bytes)?,
                Action::LoggedIn(verified) => {
                    state.connected.store(true, Ordering::Relaxed);
                    state.verified.store(verified, Ordering::Relaxed);
                }
                Action::Packet(line) => {
                    state.packets.fetch_add(1, Ordering::Relaxed);
                    state.last_packet_unix.store(now_unix(), Ordering::Relaxed);
                    on_packet(&line);
                }
            }
        }
        // Flush gated uploads. Only on a VERIFIED login: an unverified (`pass -1`) client is
        // refused by the server, and pushing at it anyway risks the session being dropped.
        if session.verified {
            let queued: Vec<Vec<u8>> = {
                let mut q = outbox.lock().unwrap_or_else(|e| e.into_inner());
                q.drain(..).collect()
            };
            for mut line in queued {
                line.extend_from_slice(b"\r\n");
                writer.write_all(&line)?;
                state.uploaded.fetch_add(1, Ordering::Relaxed);
            }
        }
        let n = match reader.read(&mut buf) {
            Ok(0) => return Ok(()), // server closed
            Ok(n) => n,
            // The read timeout wakes the loop so `stop` and the outbox are observed even on a
            // quiet socket.
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                chunk = &[];
                continue;
            }
            Err(e) => return Err(e),
        };
        chunk = &buf[..n];
    }
}

/// Connect to an APRS-IS server, log in, and call `on_packet` for every TNC2 line, reconnecting
/// with backoff until `stop` is set. The thin live-socket wrapper around [`pump`].
///
/// `addr` is `host:port` (the client port is 14580, the user-defined filter port). `login_line`
/// comes from [`tempo_core::aprs::is::login_line`]. Lines placed in `outbox` are uploaded verbatim
/// — the caller owns every gating decision.
pub fn run(
    addr: &str,
    login_line: &str,
    mut on_packet: impl FnMut(&[u8]),
    stop: &AtomicBool,
    state: &FeedState,
    outbox: &Mutex<VecDeque<Vec<u8>>>,
) {
    const BASE: Duration = Duration::from_secs(3);
    const MAX: Duration = Duration::from_secs(120);
    let now = || {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    };
    let mut backoff = BASE;
    while !stop.load(Ordering::Relaxed) {
        let started = Instant::now();
        if let Some(stream) = connect(addr) {
            if let Ok(reader) = stream.try_clone() {
                let _ = pump(
                    reader,
                    stream,
                    login_line,
                    &mut on_packet,
                    stop,
                    state,
                    outbox,
                    &now,
                );
            }
        }
        state.connected.store(false, Ordering::Relaxed);
        state.verified.store(false, Ordering::Relaxed);
        // A session that stayed up resets the backoff; a fast failure (server down, or a login the
        // server refuses) backs off to two minutes so we never hammer a public ham service.
        backoff = if started.elapsed() > Duration::from_secs(20) {
            BASE
        } else {
            (backoff * 2).min(MAX)
        };
        if !sleep_interruptible(backoff, stop) {
            return;
        }
    }
}

/// Sleep `dur` in 100 ms steps, returning false early if `stop` is set.
fn sleep_interruptible(dur: Duration, stop: &AtomicBool) -> bool {
    let steps = (dur.as_millis() / 100).max(1);
    for _ in 0..steps {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    true
}

fn connect(addr: &str) -> Option<TcpStream> {
    let sa = addr.to_socket_addrs().ok()?.next()?;
    let stream = TcpStream::connect_timeout(&sa, Duration::from_secs(10)).ok()?;
    // A read timeout so the pump loop periodically observes `stop` and drains the outbox.
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    // APRS-IS asks bidirectional clients to disable Nagle: an uploaded packet held back waiting
    // for more data arrives at the network seconds late.
    let _ = stream.set_nodelay(true);
    Some(stream)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn packets(actions: &[Action]) -> Vec<String> {
        actions
            .iter()
            .filter_map(|a| match a {
                Action::Packet(p) => Some(String::from_utf8_lossy(p).into_owned()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn the_login_goes_out_unprompted_on_the_first_feed() {
        // APRS-IS never prompts. A client that waits for one waits forever.
        let mut s = Session::new("user KD9TAW pass -1 vers Nexus 0.21.1\r\n");
        let out = s.feed(b"");
        assert_eq!(
            out,
            vec![Action::Send(
                b"user KD9TAW pass -1 vers Nexus 0.21.1\r\n".to_vec()
            )]
        );
        assert!(s.feed(b"").is_empty(), "and only once");
    }

    #[test]
    fn server_lines_are_told_from_packets() {
        assert!(is_server_line(b"# aprsc 2.1.15-gc67551b"));
        assert!(is_server_line(
            b"# aprsc 2.1.15 29 Jul 2026 16:52:51 GMT T2OREGON 199.204.38.35:14580"
        ));
        assert!(!is_server_line(
            b"XE2N-10>APDW16,qAR,XE2N-10:!2537.90NR10014.20W&"
        ));
    }

    #[test]
    fn login_response_reads_the_verify_status() {
        assert_eq!(
            login_response(b"# logresp ab0oo verified, server NINTH"),
            Some(true)
        );
        assert_eq!(
            login_response(b"# logresp N0CALL unverified, server T2OREGON"),
            Some(false)
        );
        assert_eq!(login_response(b"# aprsc 2.1.15-gc67551b"), None);
        assert_eq!(login_response(b"KD9TAW>APRS:!hi"), None);
    }

    #[test]
    fn banners_and_keepalives_are_not_packets() {
        let mut s = Session::new("login\r\n");
        let out = s.feed(
            b"# aprsc 2.1.15-gc67551b\r\n\
              # logresp KD9TAW verified, server T2OREGON\r\n\
              XE2N-10>APDW16,qAR,XE2N-10:!2537.90NR10014.20W&\r\n\
              # aprsc 2.1.15 29 Jul 2026 16:52:51 GMT T2OREGON 199.204.38.35:14580\r\n",
        );
        assert_eq!(
            packets(&out),
            vec!["XE2N-10>APDW16,qAR,XE2N-10:!2537.90NR10014.20W&"]
        );
        assert!(out.contains(&Action::LoggedIn(true)));
        assert!(s.verified);
    }

    #[test]
    fn an_unverified_login_is_recorded_as_such() {
        let mut s = Session::new("login\r\n");
        s.feed(b"# logresp N0CALL unverified, server T2OREGON\r\n");
        assert!(
            !s.verified,
            "a read-only login may receive but never upload"
        );
    }

    #[test]
    fn lines_split_across_reads_are_reassembled() {
        let mut s = Session::new("login\r\n");
        assert!(packets(&s.feed(b"# aprsc 2.1\r\nSP3VN>URRT29,WIDE")).is_empty());
        assert!(packets(&s.feed(b"1-1,qAR,SP3IZN-1:`,Rl")).is_empty());
        let out = s.feed(b"\"R[/`\"4k}hello_0\r\nNEXT>APRS:!hi\r\n");
        assert_eq!(
            packets(&out),
            vec![
                "SP3VN>URRT29,WIDE1-1,qAR,SP3IZN-1:`,Rl\"R[/`\"4k}hello_0",
                "NEXT>APRS:!hi"
            ]
        );
    }

    #[test]
    fn a_non_utf8_information_field_reaches_the_caller_intact() {
        let mut s = Session::new("login\r\n");
        let mut wire = b"OH7LZB-13>SX15S6,TCPIP*,qAC,FOURTH:'I',l ".to_vec();
        wire.push(0x1C);
        wire.extend_from_slice(b">/]\r\n");
        let out = s.feed(&wire);
        match &out[..] {
            [Action::Send(_), Action::Packet(p)] => {
                assert!(p.contains(&0x1C), "the raw byte must survive the stream");
            }
            other => panic!("expected login + one packet, got {other:?}"),
        }
    }

    #[test]
    fn a_server_that_never_sends_a_newline_cannot_grow_the_buffer() {
        let mut s = Session::new("login\r\n");
        for _ in 0..100 {
            s.feed(&[b'x'; 64]);
        }
        assert!(s.buf.len() <= MAX_LINE * 2);
    }

    #[test]
    fn pump_logs_in_reads_packets_and_uploads_only_once_verified() {
        let wire: Vec<u8> = b"# aprsc 2.1.15\r\n\
                              # logresp KD9TAW verified, server T2OREGON\r\n\
                              XE2N-10>APDW16,qAR,XE2N-10:!2537.90NR10014.20W&\r\n"
            .to_vec();
        let mut got: Vec<String> = Vec::new();
        let mut sent: Vec<u8> = Vec::new();
        let stop = AtomicBool::new(false);
        let state = FeedState::default();
        let outbox = Mutex::new(VecDeque::from(vec![
            b"SP3VN>URRT29,WIDE1-1,qAO,KD9TAW:!hi".to_vec()
        ]));
        pump(
            &wire[..],
            &mut sent,
            "user KD9TAW pass 12345 vers Nexus 0.21.1\r\n",
            &mut |line| got.push(String::from_utf8_lossy(line).into_owned()),
            &stop,
            &state,
            &outbox,
            &|| 1_700_000_000,
        )
        .unwrap();

        assert_eq!(got, vec!["XE2N-10>APDW16,qAR,XE2N-10:!2537.90NR10014.20W&"]);
        let sent = String::from_utf8_lossy(&sent);
        assert!(
            sent.starts_with("user KD9TAW pass 12345 vers Nexus 0.21.1\r\n"),
            "{sent}"
        );
        assert!(
            sent.ends_with("SP3VN>URRT29,WIDE1-1,qAO,KD9TAW:!hi\r\n"),
            "the gated line is uploaded CRLF-terminated: {sent}"
        );
        assert_eq!(state.packets.load(Ordering::Relaxed), 1);
        assert_eq!(state.uploaded.load(Ordering::Relaxed), 1);
        assert_eq!(
            state.last_packet_unix.load(Ordering::Relaxed),
            1_700_000_000
        );
    }

    #[test]
    fn pump_never_uploads_on_an_unverified_login() {
        // `pass -1` receives normally and must never push — the read-only feed has to work for an
        // operator who has not enabled the iGate, without ever touching the network's write side.
        let wire: Vec<u8> = b"# logresp N0CALL unverified, server T2OREGON\r\n\
                              XE2N-10>APDW16,qAR,XE2N-10:!2537.90NR10014.20W&\r\n"
            .to_vec();
        let mut sent: Vec<u8> = Vec::new();
        let outbox = Mutex::new(VecDeque::from(vec![b"SRC>DEST,qAO,KD9TAW:!hi".to_vec()]));
        let state = FeedState::default();
        pump(
            &wire[..],
            &mut sent,
            "user N0CALL pass -1 vers Nexus 0.21.1\r\n",
            &mut |_| {},
            &AtomicBool::new(false),
            &state,
            &outbox,
            &|| 0,
        )
        .unwrap();
        let sent = String::from_utf8_lossy(&sent);
        assert_eq!(sent, "user N0CALL pass -1 vers Nexus 0.21.1\r\n");
        assert_eq!(state.uploaded.load(Ordering::Relaxed), 0);
        assert!(!state.verified.load(Ordering::Relaxed));
        assert_eq!(
            state.packets.load(Ordering::Relaxed),
            1,
            "but it still RECEIVES"
        );
    }

    #[test]
    fn pump_returns_immediately_when_stopped() {
        let stop = AtomicBool::new(true);
        let mut sent: Vec<u8> = Vec::new();
        pump(
            &b"KD9TAW>APRS:!hi\r\n"[..],
            &mut sent,
            "login\r\n",
            &mut |_| panic!("must not decode after stop"),
            &stop,
            &FeedState::default(),
            &Mutex::new(VecDeque::new()),
            &|| 0,
        )
        .unwrap();
        assert!(sent.is_empty());
    }
}
