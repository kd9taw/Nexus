//! The CMS telnet transport against a fake CMS on `127.0.0.1:0`.
//!
//! A `:0` bind so the test never squats on a port and never touches the real network — the same
//! rule every socket test in this crate follows. The fake speaks the pre-login and then a
//! deliberately trivial "protocol" (echo a token), because what is under test here is the
//! transport: the prompt handover, the stop flag, the byte counters, the single-session rule.
//! The real B2F conversation is tested where it lives, over a `Vec<u8>`, with no socket at all.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tempo_net::wl2k::{self, ByteSession, Login, Outcome, SessionState, CMS_TELNET_PASSWORD};

/// A `ByteSession` that answers `PING\r` with `PONG\r` and closes after one exchange.
struct Pong {
    saw: Vec<u8>,
    done: bool,
}

impl Pong {
    fn new() -> Pong {
        Pong {
            saw: Vec::new(),
            done: false,
        }
    }
}

impl ByteSession for Pong {
    fn feed(&mut self, chunk: &[u8]) -> Vec<Vec<u8>> {
        self.saw.extend_from_slice(chunk);
        if self.saw.windows(5).any(|w| w == b"PING\r") && !self.done {
            self.done = true;
            return vec![b"PONG\r".to_vec()];
        }
        Vec::new()
    }
    fn wants_close(&self) -> bool {
        self.done
    }
}

/// Drains a socket to EOF, returning everything read. Deterministic where a fixed number of
/// `read` calls is not: TCP may split or coalesce any write, so a fake that reads exactly three
/// times is a flake waiting for a busy machine.
fn drain(sock: &mut std::net::TcpStream, into: &mut Vec<u8>) {
    let mut buf = [0u8; 1024];
    loop {
        match sock.read(&mut buf) {
            Ok(0) | Err(_) => return,
            Ok(n) => into.extend_from_slice(&buf[..n]),
        }
    }
}

/// Spawns a fake CMS that runs the pre-login then sends `PING\r`, and returns its address plus a
/// handle yielding everything the client sent.
///
/// ⚠️ The password prompt and the first protocol bytes go out in **one write**, because that is
/// the boundary this transport exists to get right: a pre-login that swallowed the rest of the
/// chunk would lose the peer's opening bytes (for a real CMS, its SID).
fn fake_cms() -> (std::net::SocketAddr, std::thread::JoinHandle<Vec<u8>>) {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let handle = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept");
        let mut got = Vec::new();
        let mut buf = [0u8; 1024];
        sock.write_all(b"[WL2K Central CMS]\r\nCallsign :").unwrap();
        let n = sock.read(&mut buf).unwrap();
        got.extend_from_slice(&buf[..n]);
        sock.write_all(b"\r\nPassword :PING\r").unwrap();
        drain(&mut sock, &mut got);
        got
    });
    (addr, handle)
}

#[test]
fn a_whole_session_runs_end_to_end_over_a_real_socket() {
    let (addr, server) = fake_cms();
    let stop = AtomicBool::new(false);
    let state = SessionState::default();
    let mut sess = Pong::new();
    let outcome = wl2k::run(
        &addr.ip().to_string(),
        addr.port(),
        Login::new("N0CALL", CMS_TELNET_PASSWORD),
        &mut sess,
        &stop,
        &state,
        None,
    );
    assert_eq!(outcome, Outcome::Complete, "session should finish cleanly");
    let sent = server.join().expect("server thread");
    let sent = String::from_utf8_lossy(&sent).into_owned();
    assert!(sent.contains("N0CALL\r"), "callsign not sent: {sent:?}");
    assert!(
        sent.contains(&format!("{CMS_TELNET_PASSWORD}\r")),
        "telnet password not sent: {sent:?}"
    );
    assert!(
        sent.contains("PONG\r"),
        "the session's own bytes never reached the wire: {sent:?}"
    );
    // `connected` and `logged_in` are LIVE state for a status chip, and `run` clears both as it
    // returns — a chip still reading "logged in" after a disconnect is a lie about the socket.
    // So the durable evidence that the pre-login handover happened is above: the telnet password
    // reached the wire and so did the session's own PONG. That the latch itself latches is
    // covered where it is set, over a pair of buffers, by
    // `wl2k::tests::pump_latches_logged_in_at_the_handover_and_carries_the_shared_bytes`.
    assert!(
        !state.connected.load(Ordering::Relaxed),
        "connected must be cleared when the session ends"
    );
    assert!(
        !state.logged_in.load(Ordering::Relaxed),
        "logged_in must be cleared when the session ends"
    );
    assert!(state.bytes_in.load(Ordering::Relaxed) > 0);
    assert!(state.bytes_out.load(Ordering::Relaxed) > 0);
}

#[test]
fn the_stop_flag_ends_a_session_parked_in_read() {
    // The transport must observe `stop` on a SILENT socket, which is what the read timeout is
    // for. Without it a Disconnect leaves a thread blocked in `read` until the CMS times out —
    // the same class of wedge the 1.10.2 stuck-TX teardown was.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    let server = std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept");
        sock.write_all(b"Callsign :").unwrap();
        // Then say nothing at all, forever, until the client goes away.
        let mut sink = Vec::new();
        let _ = sock.read_to_end(&mut sink);
    });
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        flag.store(true, Ordering::SeqCst);
    });
    // `run` goes on a worker and the answer comes back through a bounded `recv_timeout`, because
    // the failure this test exists to catch (no read timeout, so the pump parks in `read` and
    // never looks at `stop`) is a HANG — and a hang never reaches an `elapsed()` assertion on
    // this thread. Without the channel the assertion below reads like a promptness bound and is
    // really just a comment; with it, the mutation that deletes `set_read_timeout` fails here
    // instead of stalling the suite until CI's own timeout.
    let (tx, rx) = std::sync::mpsc::channel();
    let host = addr.ip().to_string();
    let port = addr.port();
    std::thread::spawn(move || {
        let state = SessionState::default();
        let mut sess = Pong::new();
        let outcome = wl2k::run(
            &host,
            port,
            Login::new("N0CALL", CMS_TELNET_PASSWORD),
            &mut sess,
            &stop,
            &state,
            None,
        );
        let _ = tx.send(outcome);
    });
    let outcome = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("stop was not observed within 10s — is the read timeout gone?");
    assert_eq!(outcome, Outcome::Stopped, "stop must end the pump");
    drop(server);
}

#[test]
fn a_refused_connect_is_an_io_outcome_and_not_a_retry_loop() {
    // THE SINGLE-SESSION RULE. `aprsis::run` would sit here reconnecting with backoff forever;
    // a mail transaction must not. Bind and immediately drop, so the port is (almost certainly)
    // closed, and assert we return promptly with an Io outcome rather than looping.
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").expect("bind");
        l.local_addr().expect("addr").port()
    };
    let stop = AtomicBool::new(false);
    let state = SessionState::default();
    let mut sess = Pong::new();
    let started = std::time::Instant::now();
    let outcome = wl2k::run(
        "127.0.0.1",
        port,
        Login::new("N0CALL", CMS_TELNET_PASSWORD),
        &mut sess,
        &stop,
        &state,
        None,
    );
    assert!(
        matches!(outcome, Outcome::Io(_)),
        "expected an Io outcome, got {outcome:?}"
    );
    assert!(
        started.elapsed() < std::time::Duration::from_secs(15),
        "a refused connect took {:?} — is there a retry loop?",
        started.elapsed()
    );
    assert!(!state.connected.load(Ordering::Relaxed));
}

#[test]
fn a_peer_that_closes_mid_session_is_reported_not_retried() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    std::thread::spawn(move || {
        let (mut sock, _) = listener.accept().expect("accept");
        sock.write_all(b"Callsign :").unwrap();
        let mut b = [0u8; 64];
        let _ = sock.read(&mut b);
        // Hang up without ever prompting for a password.
        drop(sock);
    });
    let stop = AtomicBool::new(false);
    let state = SessionState::default();
    let mut sess = Pong::new();
    let outcome = wl2k::run(
        &addr.ip().to_string(),
        addr.port(),
        Login::new("N0CALL", CMS_TELNET_PASSWORD),
        &mut sess,
        &stop,
        &state,
        None,
    );
    assert_eq!(outcome, Outcome::PeerClosed, "got {outcome:?}");
}

/// Hex, lowercase, no separators — the trace encoding, so an assertion can name what it expects
/// in the form the file holds it.
fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[test]
fn a_capture_of_a_whole_session_holds_no_pre_login_byte() {
    // HYGIENE RULE 1, over a real socket rather than a unit fixture. The pre-login carries the
    // callsign and the telnet doorway token in the clear, and a capture file is a thing an
    // operator attaches to a bug report. Recording starts at the B2F handover, so the capture
    // opens with the peer's first post-handover bytes and holds neither prompt nor answer.
    let (addr, server) = fake_cms();
    let dir = std::env::temp_dir().join(format!("wl2k-cap-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("mkdir");
    let path = dir.join("session.trace");
    let stop = AtomicBool::new(false);
    let state = SessionState::default();
    let mut sess = Pong::new();
    let outcome = {
        let mut tap = wl2k::TraceTap::create(&path, "# loopback capture\n").expect("create");
        wl2k::run(
            &addr.ip().to_string(),
            addr.port(),
            Login::new("N0CALL", CMS_TELNET_PASSWORD),
            &mut sess,
            &stop,
            &state,
            Some(&mut tap),
        )
    };
    assert_eq!(outcome, Outcome::Complete);
    let _ = server.join();
    let text = std::fs::read_to_string(&path).expect("read trace");

    // Nothing from before the handover. Checked in HEX, because the file holds hex: grepping it
    // for the ASCII strings would pass whether or not the rule held.
    for forbidden in [
        &b"Callsign :"[..],
        b"N0CALL\r",
        CMS_TELNET_PASSWORD.as_bytes(),
        b"[WL2K Central CMS]",
    ] {
        assert!(
            !text.contains(&hex(forbidden)),
            "a pre-login byte string reached the capture ({:?}):\n{text}",
            String::from_utf8_lossy(forbidden)
        );
    }
    // And the session's own traffic IS there, both directions — otherwise the assertions above
    // would pass on an empty file.
    assert!(
        text.contains(&format!("< {}\n", hex(b"PING\r"))),
        "the peer's post-handover bytes are missing:\n{text}"
    );
    assert!(
        text.contains(&format!("> {}\n", hex(b"PONG\r"))),
        "our post-handover bytes are missing:\n{text}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_stop_that_lands_before_the_thread_starts_opens_no_socket() {
    // `winlink_connect` claims the session slot and then does the keychain read, the mailbox
    // restore and the spawn, so a Disconnect can reach the stop flag before this function runs at
    // all. Honouring the flag only inside the pump would still make a TCP connection to the
    // public CMS for a session nobody wants.
    //
    // Proved without a listener: a *reachable* port would prove nothing, since the pump would
    // stop on its first pass either way and the outcome would look the same. Against a port
    // nothing is listening on, connecting is `Outcome::Io` and not connecting is
    // `Outcome::Stopped`, so the two are distinguishable — and the control is the same call with
    // the flag clear, which must come back `Io`.
    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr");
    drop(listener); // nothing is listening there now
    let host = addr.ip().to_string();

    let state = SessionState::default();
    let mut sess = Pong::new();
    let outcome = wl2k::run(
        &host,
        addr.port(),
        Login::new("N0CALL", CMS_TELNET_PASSWORD),
        &mut sess,
        &AtomicBool::new(true),
        &state,
        None,
    );
    assert_eq!(
        outcome,
        Outcome::Stopped,
        "a session that was already stopped still opened a socket"
    );
    assert!(
        !state.connected.load(Ordering::Relaxed),
        "connected was set for a session that never connected"
    );

    // THE POSITIVE CONTROL: with the flag clear, the same call really does try to connect.
    let mut sess = Pong::new();
    let outcome = wl2k::run(
        &host,
        addr.port(),
        Login::new("N0CALL", CMS_TELNET_PASSWORD),
        &mut sess,
        &AtomicBool::new(false),
        &SessionState::default(),
        None,
    );
    assert!(
        matches!(outcome, Outcome::Io(_)),
        "the control did not reach the socket, so the test above proves nothing: {outcome:?}"
    );
}
