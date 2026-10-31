//! The server over real sockets (tungstenite's own client, and raw TCP for what a client library
//! will not send), and the dispatch table without them.
//!
//! What a server sends is checked by [`strict`], a validator written here from the TCI 2.0
//! document and eesdr-tci's command table (`tci.py`, our strictest real client parser), not from
//! the codec: a codec bug cannot hide behind its own reading.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use proptest::prelude::*;
use tungstenite::client::IntoClientRequest;
use tungstenite::handshake::client::Request;
use tungstenite::http::HeaderValue;
use tungstenite::{HandshakeError, Message, WebSocket};

use super::dispatch::{self, Action, Cx, Sink};
use super::mock::{Call, Mock};
use super::state::{FormatSet, Session, Value};
use super::*;
use crate::tci::frame::{AudioRate, Channels, Chrono, SampleFormat, HEADER_LEN};
use crate::tci::wire::{self as tciwire, Flag, ServerText, Word};
use crate::tci::{Channel, EXPERT_SDR3};

// ── An independent check of what a server sends ─────────────────────────────────────────────

/// Every name a server may send, with its argument count: the TCI 2.0 document's, and eesdr-tci's
/// table where the document has none (`rx_enable`, `rx_smeter`, `tx_power`, `tx_swr`). -1: any
/// number. Written from those two sources, not from the codec.
const NAMES: &[(&str, i32)] = &[
    ("vfo_limits", 2),
    ("if_limits", 2),
    ("trx_count", 1),
    ("channels_count", 1),
    ("device", 1),
    ("receive_only", 1),
    ("modulations_list", -1),
    ("protocol", 2),
    ("ready", 0),
    ("start", 0),
    ("stop", 0),
    ("dds", 2),
    ("if", 3),
    ("vfo", 3),
    ("modulation", 2),
    ("trx", 2),
    ("tune", 2),
    ("drive", 2),
    ("tune_drive", 2),
    ("rit_enable", 2),
    ("xit_enable", 2),
    ("split_enable", 2),
    ("rit_offset", 2),
    ("xit_offset", 2),
    ("rx_channel_enable", 3),
    ("rx_filter_band", 3),
    ("cw_macros_speed", 1),
    ("cw_macros_delay", 1),
    ("cw_keyer_speed", 1),
    ("volume", 1),
    ("mute", 1),
    ("rx_mute", 2),
    ("rx_volume", 3),
    ("rx_balance", 3),
    ("mon_volume", 1),
    ("mon_enable", 1),
    ("agc_mode", 2),
    ("agc_gain", 2),
    ("rx_nb_enable", 2),
    ("rx_nb_param", 3),
    ("rx_bin_enable", 2),
    ("rx_nr_enable", 2),
    ("rx_anc_enable", 2),
    ("rx_anf_enable", 2),
    ("rx_apf_enable", 2),
    ("rx_dse_enable", 2),
    ("rx_nf_enable", 2),
    ("lock", 2),
    ("sql_enable", 2),
    ("sql_level", 2),
    ("digl_offset", 1),
    ("digu_offset", 1),
    ("tx_enable", 2),
    ("audio_samplerate", 1),
    ("audio_start", 1),
    ("audio_stop", 1),
    ("audio_stream_sample_type", 1),
    ("audio_stream_channels", 1),
    ("audio_stream_samples", 1),
    ("tx_stream_audio_buffering", 1),
    ("clicked_on_spot", 2),
    ("rx_clicked_on_spot", 4),
    ("tx_frequency", 1),
    ("rx_sensors", 2),
    ("tx_sensors", 5),
    ("vfo_lock", 3),
    ("rx_channel_sensors", 3),
    ("cw_terminal", 1),
    ("cw_macros_empty", 0),
    ("callsign_send", 1),
    ("rx_enable", 2),
    ("rx_smeter", 3),
    ("tx_power", 1),
    ("tx_swr", 1),
];

/// The arguments WSJT-X and JTDX split at the `.` and read the fraction of (power, SWR and
/// levels; `TCITransceiver.cpp:654-668`): each must hold digits, a point, digits.
const DECIMALS: &[(&str, &[usize])] = &[
    ("tx_sensors", &[1, 2, 3, 4]),
    ("tx_power", &[0]),
    ("tx_swr", &[0]),
    ("rx_sensors", &[1]),
    ("rx_channel_sensors", &[2]),
    ("rx_smeter", &[2]),
];

fn decimal(arg: &str) -> bool {
    let arg = arg.strip_prefix('-').unwrap_or(arg);
    matches!(arg.split_once('.'), Some((w, f))
        if !w.is_empty() && !f.is_empty()
            && w.bytes().all(|b| b.is_ascii_digit())
            && f.bytes().all(|b| b.is_ascii_digit()))
}

/// What eesdr-tci's listener does to a message (`listener.py:104-136`), and stricter: exactly one
/// command, ended by `;`; a name it knows; exactly its arguments; a decimal point where WSJT-X
/// splits a value at one.
fn strict(message: &str) -> Result<(), String> {
    let body = message
        .strip_suffix(';')
        .ok_or_else(|| format!("{message:?}: no `;` at the end"))?;
    if body.contains(';') {
        return Err(format!("{message:?}: more than one command in one message"));
    }
    let (name, args) = match body.split_once(':') {
        Some((n, a)) => (n, a.split(',').collect::<Vec<_>>()),
        None => (body, Vec::new()),
    };
    let want = NAMES
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .ok_or_else(|| format!("{message:?}: a name no client knows"))?
        .1;
    if want >= 0 && args.len() as i32 != want {
        return Err(format!("{message:?}: {} arguments, not {want}", args.len()));
    }
    if let Some((_, at)) = DECIMALS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        for &i in *at {
            if !args.get(i).copied().is_some_and(decimal) {
                return Err(format!("{message:?}: argument {i} has no decimal point"));
            }
        }
    }
    Ok(())
}

#[test]
fn the_strict_validator_refuses_what_eesdr_tci_refuses_and_takes_what_it_takes() {
    // The positive control for every check below: each refusal must be for its own reason.
    for good in [
        "vfo:0,0,14074000;",
        "ready;",
        "modulations_list:am,lsb,usb;",
        "tx_sensors:0,-27.0,47.0,47.5,1.25;",
        "rx_enable:0,true;",
    ] {
        assert_eq!(strict(good), Ok(()), "{good}");
    }
    for (bad, why) in [
        ("vfo:0,0,14074000;modulation:0,usb;", "more than one"),
        ("vfo:0,0,14074000", "no `;`"),
        ("channel_count:2;", "no client knows"),
        ("mic_level:0,1;", "no client knows"),
        ("trx:0,true,tci;", "3 arguments"),
        ("modulation:0,0,usb;", "3 arguments"),
        (
            "tx_sensors:0,-27,47.0,47.5,1.25;",
            "argument 1 has no decimal",
        ),
        ("tx_swr:1;", "argument 0 has no decimal"),
        ("tx_power:.5;", "argument 0 has no decimal"),
    ] {
        let got = strict(bad);
        assert!(
            got.as_ref().is_err_and(|e| e.contains(why)),
            "{bad}: {got:?}, wanted {why:?}"
        );
    }
}

// ── The bench: a server on an ephemeral loopback port, and a mock backend ─────────────────

fn hz(n: i64) -> tciwire::Hz {
    tciwire::Hz::new(n).expect("a frequency")
}

fn word(w: &str) -> Word {
    Word::new(w).expect("a word")
}

fn snapshot() -> Snapshot {
    Snapshot {
        radio: 1,
        vfo_limits: (hz(1_800_000), hz(54_000_000)),
        receive_only: false,
        dial: hz(14_074_000),
        mode: word("digu"),
        drive: 50,
        tune_drive: 20,
        trx: false,
        tune: false,
        owner: None,
        tx_enable: Vec::new(),
        cw_speed: 25,
        tx_buffering_ms: 100,
        rx_dbm: Some(-73.5),
        tx: None,
    }
}

/// Clocks short enough for a test.
fn quick() -> Limits {
    Limits {
        handshake: Duration::from_millis(800),
        ping: Duration::from_millis(100),
        silence: Duration::from_millis(600),
        keyed_ping: Duration::from_millis(50),
        keyed_silence: Duration::from_millis(300),
        flood: Duration::from_secs(1),
        ..Limits::default()
    }
}

/// Long enough for a loaded box; the tests that measure time say so.
const WAIT: Duration = Duration::from_secs(5);

struct Bench {
    server: Option<Server>,
    mock: Arc<Mock>,
    handle: Handle,
    port: u16,
}

impl Bench {
    fn new() -> Bench {
        Bench::with(Limits::default())
    }

    fn with(limits: Limits) -> Bench {
        let mock = Arc::new(Mock::default());
        let server = Server::start(
            Config { port: 0, limits },
            Arc::clone(&mock) as Arc<dyn TciBackend>,
            snapshot(),
        )
        .expect("a loopback port");
        let handle = server.handle();
        let port = server.port();
        Bench {
            server: Some(server),
            mock,
            handle,
            port,
        }
    }

    /// A client through its init burst.
    fn client(&self) -> Client {
        let mut c = Client::connect(self.port).expect("a handshake the gate takes");
        c.burst();
        c
    }

    fn server(&self) -> &Server {
        self.server.as_ref().expect("running")
    }

    /// The id of the program that connected `n`th (from 1).
    fn id(&self, n: u64) -> ClientId {
        ClientId::new(n)
    }
}

/// A TCI client: tungstenite's, over a plain socket.
struct Client {
    ws: WebSocket<TcpStream>,
}

fn request(port: u16) -> Request {
    format!("ws://127.0.0.1:{port}/")
        .into_client_request()
        .expect("a request")
}

impl Client {
    fn connect(port: u16) -> Result<Client, tungstenite::Error> {
        Client::connect_with(port, request(port))
    }

    fn connect_with(port: u16, req: Request) -> Result<Client, tungstenite::Error> {
        let stream = TcpStream::connect((Ipv4Addr::LOCALHOST, port))?;
        Client::over(stream, req)
    }

    /// The handshake with a patient socket, then a short timeout so a read can give up.
    fn over(stream: TcpStream, req: Request) -> Result<Client, tungstenite::Error> {
        stream.set_read_timeout(Some(WAIT))?;
        match tungstenite::client::client(req, stream) {
            Ok((ws, _)) => {
                ws.get_ref()
                    .set_read_timeout(Some(Duration::from_millis(20)))?;
                Ok(Client { ws })
            }
            Err(HandshakeError::Failure(e)) => Err(e),
            Err(HandshakeError::Interrupted(_)) => Err(tungstenite::Error::Io(
                std::io::ErrorKind::WouldBlock.into(),
            )),
        }
    }

    fn send(&mut self, text: &str) {
        self.ws
            .send(Message::text(text.to_string()))
            .expect("the server is reading");
    }

    fn send_binary(&mut self, bytes: Vec<u8>) {
        self.ws
            .send(Message::binary(bytes))
            .expect("the server is reading");
    }

    /// The next message, answering pings on the way (tungstenite does, as it reads).
    fn next(&mut self, within: Duration) -> Option<Message> {
        let until = Instant::now() + within;
        while Instant::now() < until {
            match self.ws.read() {
                Ok(Message::Ping(_) | Message::Pong(_)) => {}
                Ok(m) => return Some(m),
                Err(tungstenite::Error::Io(e))
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) => {}
                Err(_) => return None,
            }
        }
        None
    }

    fn text(&mut self, within: Duration) -> Option<String> {
        loop {
            match self.next(within)? {
                Message::Text(t) => return Some(t.as_str().to_string()),
                Message::Close(_) => return None,
                _ => {}
            }
        }
    }

    /// Every text message for `within`.
    fn texts(&mut self, within: Duration) -> Vec<String> {
        let until = Instant::now() + within;
        let mut out = Vec::new();
        while let Some(t) = self.text(until.saturating_duration_since(Instant::now())) {
            out.push(t);
        }
        out
    }

    /// The init burst, through `start;`.
    fn burst(&mut self) -> Vec<String> {
        let mut out = Vec::new();
        while let Some(t) = self.text(WAIT) {
            let end = t == "start;";
            out.push(t);
            if end {
                return out;
            }
        }
        panic!("the burst ended without start; after {out:?}");
    }

    /// Ask, and wait for the line that answers.
    fn ask(&mut self, ask: &str, want: &str) {
        self.send(ask);
        let got = self.until(want);
        assert!(got, "no {want:?} after {ask:?}");
    }

    /// Read until `want` arrives. False when it never does.
    fn until(&mut self, want: &str) -> bool {
        let until = Instant::now() + WAIT;
        while let Some(t) = self.text(until.saturating_duration_since(Instant::now())) {
            if t == want {
                return true;
            }
        }
        false
    }

    /// Read until the server's close frame; its code.
    fn close_code(&mut self, within: Duration) -> Option<u16> {
        let until = Instant::now() + within;
        while Instant::now() < until {
            if let Message::Close(frame) =
                self.next(until.saturating_duration_since(Instant::now()))?
            {
                return frame.map(|f| u16::from(f.code));
            }
        }
        None
    }

    /// The connection still serves: a read is answered.
    fn alive(&mut self) -> bool {
        self.send("trx:0;");
        self.until("trx:0,false;")
    }
}

/// A raw request's answer head, for what a client library will not send.
fn raw(port: u16, request: &str) -> String {
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connect");
    s.set_read_timeout(Some(WAIT)).expect("timeout");
    s.write_all(request.as_bytes()).expect("write");
    let mut out = Vec::new();
    let mut buf = [0u8; 1024];
    while !out.windows(4).any(|w| w == b"\r\n\r\n") {
        match s.read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn handshake(host: &str, extra: &str) -> String {
    format!(
        "GET / HTTP/1.1\r\nHost: {host}\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n{extra}\r\n"
    )
}

fn status(head: &str) -> &str {
    head.split("\r\n").next().unwrap_or("")
}

fn refused(mock: &Mock, f: impl Fn(&HandshakeRefusal) -> bool) -> bool {
    mock.wait_event(WAIT, |e| matches!(e, Event::Refused { why, .. } if f(why)))
        .is_some()
}

// ── Where it listens ─────────────────────────────────────────────────────────────────────────

/// Never a wildcard, never a LAN address.
#[test]
fn the_listeners_are_loopback_only_on_one_port() {
    let b = Bench::new();
    let addrs = b.server().local_addrs();
    assert!(!addrs.is_empty());
    assert!(
        addrs.iter().all(|a| a.ip().is_loopback()),
        "{addrs:?} are not all loopback"
    );
    assert!(addrs.iter().any(SocketAddr::is_ipv4), "{addrs:?}: no IPv4");
    assert!(
        addrs.iter().all(|a| a.port() == b.port),
        "{addrs:?}: not all on {}",
        b.port
    );
    // Positive control: the port takes a client on 127.0.0.1.
    assert!(b.client().alive());
}

/// A program that resolves `localhost` to `::1` first still reaches the server.
#[test]
fn a_program_reaches_the_server_over_ipv6_loopback() {
    let b = Bench::new();
    if !b.server().local_addrs().iter().any(SocketAddr::is_ipv6) {
        // No ::1 on this computer: nothing to reach.
        return;
    }
    let stream = TcpStream::connect(("::1", b.port)).expect("::1");
    let req = format!("ws://[::1]:{}/", b.port)
        .into_client_request()
        .expect("a request");
    let mut c = Client::over(stream, req).expect("the gate takes [::1]");
    assert_eq!(c.burst().last().map(String::as_str), Some("start;"));
}

/// A port below the ephemeral range, free on both loopback addresses, with the one after it
/// free too (never one an ephemeral bind can be handed).
fn two_free_ports() -> u16 {
    let v6 = TcpListener::bind(("::1", 0)).is_ok();
    for p in (20_000u16..30_000).step_by(37) {
        let free = [p, p + 1].iter().all(|&q| {
            TcpListener::bind((Ipv4Addr::LOCALHOST, q)).is_ok()
                && (!v6 || TcpListener::bind(("::1", q)).is_ok())
        });
        if free {
            return p;
        }
    }
    panic!("no two free ports below the ephemeral range");
}

#[test]
fn a_second_window_takes_the_next_port_and_a_third_is_refused() {
    let port = two_free_ports();
    let start = || {
        Server::start(
            Config {
                port,
                limits: Limits::default(),
            },
            Arc::new(Mock::default()) as Arc<dyn TciBackend>,
            snapshot(),
        )
    };
    let first = start().expect("the configured port");
    assert_eq!(first.port(), port);
    let second = start().expect("the next port");
    assert_eq!(
        second.port(),
        port + 1,
        "a second window takes the next one"
    );
    let third = start();
    assert_eq!(
        third.as_ref().err().map(std::io::Error::kind),
        Some(std::io::ErrorKind::AddrInUse),
        "only {PORTS_TRIED} ports are tried"
    );
    // Stopping frees the port at once, on both addresses.
    first.stop();
    let again = start().expect("the configured port, freed");
    assert_eq!(again.port(), port);
    drop(second);
}

// ── The handshake gate ───────────────────────────────────────────────────────────────────────

fn with_header(port: u16, name: &'static str, value: &'static str) -> Request {
    let mut req = request(port);
    req.headers_mut()
        .insert(name, HeaderValue::from_static(value));
    req
}

fn http_status(e: &tungstenite::Error) -> Option<u16> {
    match e {
        tungstenite::Error::Http(response) => Some(response.status().as_u16()),
        _ => None,
    }
}

/// A web page's handshake (it must send `Origin`) is refused, 403, and logged once.
#[test]
fn a_handshake_carrying_origin_is_refused_403() {
    let b = Bench::new();
    for (name, value) in [
        ("Origin", "https://example.com"),
        ("Origin", "null"),
        ("Origin", "http://127.0.0.1:8080"),
        ("Sec-WebSocket-Origin", "https://example.com"),
    ] {
        let got = Client::connect_with(b.port, with_header(b.port, name, value));
        assert_eq!(
            got.as_ref().err().and_then(http_status),
            Some(403),
            "{name}: {value}"
        );
    }
    assert!(refused(&b.mock, |w| *w
        == HandshakeRefusal::Origin("https://example.com".into())));
    // An empty Origin is still a browser's.
    let head = raw(
        b.port,
        &handshake(&format!("127.0.0.1:{}", b.port), "Origin: \r\n"),
    );
    assert_eq!(status(&head), "HTTP/1.1 403 Forbidden", "{head}");
    // Positive control: the same request without it is taken.
    assert!(Client::connect(b.port).is_ok());
    assert!(!b.mock.calls().iter().any(|c| matches!(c, Call::Key(..))));
}

/// DNS rebinding. `Host` must name this computer's loopback and the port, once.
#[test]
fn a_handshake_whose_host_is_not_loopback_is_refused_403() {
    let b = Bench::new();
    let p = b.port;
    for host in [
        format!("evil.example:{p}"),
        format!("127.0.0.1:{}", p.wrapping_add(1)),
        "127.0.0.1".to_string(),
        format!("127.0.0.2:{p}"),
        format!("localhost.:{p}"),
        format!("192.168.1.10:{p}"),
    ] {
        let head = raw(p, &handshake(&host, ""));
        assert_eq!(
            status(&head),
            "HTTP/1.1 403 Forbidden",
            "Host: {host}: {head}"
        );
    }
    let twice = handshake(
        &format!("127.0.0.1:{p}"),
        &format!("Host: 127.0.0.1:{p}\r\n"),
    );
    assert_eq!(
        status(&raw(p, &twice)),
        "HTTP/1.1 403 Forbidden",
        "two Host headers"
    );
    let none = "GET / HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n";
    assert_eq!(status(&raw(p, none)), "HTTP/1.1 403 Forbidden", "no Host");
    assert!(refused(&b.mock, |w| *w
        == HandshakeRefusal::Host(format!("evil.example:{p}"))));
    // Positive controls: each loopback name with the port, in any case.
    for host in [
        format!("127.0.0.1:{p}"),
        format!("localhost:{p}"),
        format!("LOCALHOST:{p}"),
        format!("[::1]:{p}"),
    ] {
        let head = raw(p, &handshake(&host, ""));
        assert_eq!(
            status(&head),
            "HTTP/1.1 101 Switching Protocols",
            "Host: {host}"
        );
    }
}

/// The version: WebSocket 13 only, and anything else is told so (426).
#[test]
fn a_handshake_of_another_websocket_version_is_answered_426() {
    let b = Bench::new();
    let host = format!("127.0.0.1:{}", b.port);
    let eight = handshake(&host, "").replace("Version: 13", "Version: 8");
    let head = raw(b.port, &eight);
    assert_eq!(status(&head), "HTTP/1.1 426 Upgrade Required", "{head}");
    assert!(head.contains("Sec-WebSocket-Version: 13"), "{head}");
    assert!(refused(&b.mock, |w| *w == HandshakeRefusal::Version));
    // Positive control: 13 is taken.
    assert_eq!(
        status(&raw(b.port, &handshake(&host, ""))),
        "HTTP/1.1 101 Switching Protocols"
    );
}

#[test]
fn a_request_that_is_not_a_websocket_handshake_is_answered_400() {
    let b = Bench::new();
    let host = format!("127.0.0.1:{}", b.port);
    for req in [
        format!("GET / HTTP/1.1\r\nHost: {host}\r\n\r\n"),
        handshake(&host, "").replacen("GET", "POST", 1),
        handshake(&host, "").replace("dGhlIHNhbXBsZSBub25jZQ==", "short"),
        handshake(&host, "").replace("HTTP/1.1", "HTTP/1.0"),
    ] {
        let head = raw(b.port, &req);
        assert_eq!(status(&head), "HTTP/1.1 400 Bad Request", "{req:?}: {head}");
    }
    assert!(Client::connect(b.port).is_ok(), "the server still serves");
}

/// The whole handshake must arrive within the deadline, however it trickles in.
#[test]
fn a_silent_connection_is_closed_at_the_handshake_deadline() {
    let b = Bench::with(quick());
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, b.port)).expect("connect");
    s.set_read_timeout(Some(Duration::from_secs(10)))
        .expect("timeout");
    let t0 = Instant::now();
    let mut buf = [0u8; 16];
    let n = s.read(&mut buf).unwrap_or(0);
    let took = t0.elapsed();
    assert_eq!(n, 0, "closed, unanswered");
    assert!(
        took >= Duration::from_millis(700) && took < Duration::from_secs(4),
        "closed after {took:?}, the deadline is 800 ms"
    );
    assert!(refused(&b.mock, |w| *w == HandshakeRefusal::Timeout));
}

#[test]
fn a_trickled_handshake_is_closed_at_the_deadline() {
    let b = Bench::with(quick());
    let req = handshake(&format!("127.0.0.1:{}", b.port), "");
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, b.port)).expect("connect");
    let t0 = Instant::now();
    let mut closed_at = None;
    // One byte every 100 ms: each read sees data well inside any per-read timeout, and the
    // whole request would take ~16 s.
    for byte in req.bytes() {
        if s.write_all(&[byte]).is_err() {
            closed_at = Some(t0.elapsed());
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
        if t0.elapsed() > Duration::from_secs(6) {
            break;
        }
    }
    let closed_at = closed_at.expect("the server closed a trickled handshake");
    assert!(
        closed_at < Duration::from_secs(4),
        "closed after {closed_at:?}, the deadline is 800 ms"
    );
    // Positive control: the same request at once is taken.
    assert!(Client::connect(b.port).is_ok());
}

/// Eight programs at once; the ninth is told to come back later.
#[test]
fn a_ninth_program_is_refused_503() {
    let b = Bench::new();
    let mut eight: Vec<Client> = (0..8).map(|_| b.client()).collect();
    let ninth = Client::connect(b.port);
    assert_eq!(ninth.as_ref().err().and_then(http_status), Some(503));
    assert!(refused(&b.mock, |w| *w == HandshakeRefusal::Full));
    // Positive control: when one leaves, the next is taken.
    let gone = eight.pop().expect("eight");
    drop(gone);
    let until = Instant::now() + WAIT;
    let mut taken = Client::connect(b.port);
    while taken.is_err() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(20));
        taken = Client::connect(b.port);
    }
    assert!(taken.is_ok(), "a slot came free");
}

// ── Message bounds ───────────────────────────────────────────────────────────────────────────

/// A text message over 16 KiB, and any message over tungstenite's cap, close with 1009.
#[test]
fn a_text_message_over_the_cap_closes_with_1009() {
    let b = Bench::new();
    let mut c = b.client();
    // Positive control: under the cap, a long command is refused and counted, nothing more.
    c.send(&format!("vfo:0,0,{};", "1".repeat(15 * 1024)));
    assert!(c.alive());
    c.send(&"a".repeat(20 * 1024));
    assert_eq!(c.close_code(WAIT), Some(1009));
    assert!(b
        .mock
        .wait_event(WAIT, |e| matches!(
            e,
            Event::Closed {
                why: Closed::TooBig,
                ..
            }
        ))
        .is_some());
}

#[test]
fn a_binary_message_over_the_cap_closes_with_1009() {
    let b = Bench::new();
    let mut c = b.client();
    c.send_binary(vec![0u8; 1 << 20]);
    assert_eq!(c.close_code(WAIT), Some(1009));
    let mut d = b.client();
    d.send(&"b".repeat(1 << 20));
    assert_eq!(d.close_code(WAIT), Some(1009));
}

/// Commands past 64 in a message, past 16 arguments, past 1 KiB, are refused and counted.
#[test]
fn a_command_over_its_bounds_is_refused_and_counted() {
    let b = Bench::new();
    let mut c = b.client();
    c.send(&format!("vfo:0,0,{};", vec!["1"; 17].join(",")));
    c.send(&"trx:0;".repeat(65));
    let replies = c.texts(Duration::from_millis(500));
    assert_eq!(
        replies.iter().filter(|t| *t == "trx:0,false;").count(),
        64,
        "64 reads answered, the rest refused"
    );
    let counts = b.handle.programs()[0].counts;
    assert_eq!(counts.unreadable, 2, "{counts:?}");
}

// ── The rate ─────────────────────────────────────────────────────────────────────────────────

fn slow(rate: u32, burst: u32) -> Limits {
    Limits {
        rate,
        burst,
        ..quick()
    }
}

/// Commands past the bucket are dropped and counted.
#[test]
fn commands_over_the_rate_are_dropped_and_counted() {
    let b = Bench::with(slow(20, 10));
    let mut c = b.client();
    let t0 = Instant::now();
    for _ in 0..200 {
        c.send("trx:0;");
    }
    let sent_in = t0.elapsed();
    let replies = c.texts(Duration::from_millis(500));
    let answered = replies.iter().filter(|t| *t == "trx:0,false;").count();
    let most = 10.0 + 20.0 * (sent_in.as_secs_f64() + 0.6);
    assert!(
        (answered as f64) <= most && answered >= 10,
        "{answered} answered in {sent_in:?}; at most {most}"
    );
    let counts = b.handle.programs()[0].counts;
    assert_eq!(counts.rate_dropped as usize, 200 - answered, "{counts:?}");
}

/// A program over the rate without a break closes, 1008.
#[test]
fn a_program_over_the_rate_for_the_flood_time_is_closed() {
    let b = Bench::with(slow(20, 10));
    let mut c = b.client();
    let t0 = Instant::now();
    let mut code = None;
    while t0.elapsed() < Duration::from_secs(4) {
        if c.ws.send(Message::text("trx:0;")).is_err() {
            break;
        }
        if let Ok(Message::Close(Some(f))) = c.ws.read() {
            code = Some(u16::from(f.code));
            break;
        }
    }
    let code = code.or_else(|| c.close_code(WAIT));
    assert_eq!(code, Some(1008), "closed for the flood");
    assert!(
        t0.elapsed() >= Duration::from_millis(900),
        "not before the flood time"
    );
}

/// A program's own stop is never lost to the rate: one of each kind gets through every 100 ms.
#[test]
fn a_stop_goes_through_while_over_the_rate() {
    let b = Bench::with(slow(1, 1));
    let mut c = b.client();
    c.ask("trx:0;", "trx:0,false;");
    // The bucket is empty now; a write is dropped…
    c.send("vfo:0,0,14075000;");
    // …and a stop still goes through.
    c.send("trx:0,false;");
    assert!(b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::Release(_)))
        .is_some());
    assert!(!b
        .mock
        .calls()
        .iter()
        .any(|call| matches!(call, Call::SetDial(..))));
}

// ── Not TCI at all ───────────────────────────────────────────────────────────────────────────

#[test]
fn three_unsplittable_messages_in_a_row_close_the_program() {
    let b = Bench::new();
    let mut c = b.client();
    // Positive control: commands the codec does not know are not garbage.
    for _ in 0..5 {
        c.send("no_such_command:1;");
    }
    assert!(c.alive());
    for _ in 0..3 {
        c.send("vfo:0,0,\u{7};");
    }
    assert_eq!(c.close_code(WAIT), Some(1008));
}

// ── Keepalive ────────────────────────────────────────────────────────────────────────────────

#[test]
fn a_program_that_never_answers_a_ping_is_closed_and_ended() {
    let b = Bench::with(quick());
    let mut c = Client::connect(b.port).expect("handshake");
    // It never reads, so it never answers.
    let t0 = Instant::now();
    let gone = b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::Disconnected(_)));
    assert!(gone.is_some(), "the silent program was ended");
    assert!(
        t0.elapsed() >= Duration::from_millis(500),
        "not before its silence ran out ({:?})",
        t0.elapsed()
    );
    assert!(b
        .mock
        .wait_event(WAIT, |e| matches!(
            e,
            Event::Closed {
                why: Closed::Silent,
                ..
            }
        ))
        .is_some());
    // Positive control: one that reads (and so answers) stays.
    let mut d = b.client();
    let until = Instant::now() + Duration::from_millis(1500);
    while Instant::now() < until {
        let _ = d.next(Duration::from_millis(50));
    }
    assert!(d.alive(), "a program answering pings stays");
    let _ = c.ws.close(None);
}

#[test]
fn a_program_holding_a_key_is_ended_sooner_when_it_goes_silent() {
    let b = Bench::with(Limits {
        silence: Duration::from_secs(30),
        ..quick()
    });
    let mut c = b.client();
    c.ask("trx:0,true;", "trx:0,true;");
    let t0 = Instant::now();
    // Silent from here.
    let gone = b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::Disconnected(_)));
    assert!(gone.is_some(), "the key holder was ended");
    let took = t0.elapsed();
    assert!(
        took < Duration::from_secs(3),
        "ended after {took:?}: the keyed silence is 300 ms, the other 30 s"
    );
}

/// The writer's side: a program that takes what the server sends a few bytes at a time, and the
/// control, one that takes it at once through the same buffers. Linux only: they make the server's
/// send buffer small through the kernel's own sizing of it ([`SMALL_MSS`]), which other systems do
/// their own way.
#[cfg(target_os = "linux")]
mod slow_readers {
    use std::os::fd::AsRawFd;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    use super::*;
    use crate::tci::server::conn::WRITE_STALL;

    /// The MSS a program with small buffers announces. Linux sizes the server's send buffer for
    /// that connection from it, 25,600 bytes on this box where loopback's own MSS gives megabytes,
    /// so one write of the writer's (up to 64 KiB of frames) fills the buffers between the server
    /// and a program that does not read it.
    const SMALL_MSS: libc::c_int = 384;

    /// How much of what the server sends a slow program reads at a time, every [`SLOW_GAP`]: in a
    /// whole [`WRITE_STALL`], about a sixth of one write of the writer's, yet something every time,
    /// so each write of the socket goes on a little before a timeout on it would end it.
    const SLOW_READ: usize = 200;

    /// The gap between a slow program's reads.
    const SLOW_GAP: Duration = Duration::from_millis(100);

    /// How late the farewell may come after the stall: the reader's wake and a margin for a loaded
    /// box. With a timeout on each write of the socket alone, it never came while the program read
    /// on.
    const PAST_THE_STALL: Duration = Duration::from_secs(1);

    /// Receive audio as large as one write of the writer's, in one frame: eight of the largest
    /// frames a program may stream ([`rx_frame`]) make such a write, but the writer gathers them as
    /// they come, so its first writes of them can be one frame each. One frame of the same size is
    /// the first write, from the moment it is sent, and more than the buffers hold.
    fn one_whole_write() -> Vec<u8> {
        let mut f = tx_frame(16 * 1024, 16 * 1024);
        f[24] = 1;
        f
    }

    /// The control's receive audio: [`BURST`] frames at a time, one write of the writer's and more
    /// than the buffers hold, every [`BURST_GAP`].
    const BURST: usize = 8;
    const BURST_GAP: Duration = Duration::from_millis(200);

    /// A program's socket with small buffers, connected to the server on `port`: it announces
    /// [`SMALL_MSS`], and its receive buffer is the smallest the kernel allows.
    fn small_buffers(port: u16) -> TcpStream {
        let socket = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None)
            .expect("a socket");
        socket
            .set_recv_buffer_size(2048)
            .expect("a small receive buffer");
        let mss = SMALL_MSS;
        // SAFETY: the option's value is a c_int that outlives the call, and its length is given.
        let set = unsafe {
            libc::setsockopt(
                socket.as_raw_fd(),
                libc::IPPROTO_TCP,
                libc::TCP_MAXSEG,
                (&mss as *const libc::c_int).cast(),
                std::mem::size_of::<libc::c_int>() as libc::socklen_t,
            )
        };
        assert_eq!(set, 0, "TCP_MAXSEG: {}", std::io::Error::last_os_error());
        socket
            .connect(&SocketAddr::from((Ipv4Addr::LOCALHOST, port)).into())
            .expect("connected");
        socket.into()
    }

    /// A program over [`small_buffers`] through its init burst, its receive audio started and its
    /// `tci` key admitted, as WSJT-X's is for an over.
    fn keyed_through_small_buffers(b: &Bench) -> Client {
        let mut c = Client::over(small_buffers(b.port), request(b.port))
            .expect("a handshake the gate takes");
        c.burst();
        c.ask("audio_start:0;", "audio_start:0;");
        c.ask("trx:0,true,tci;", "trx:0,true;");
        c
    }

    impl Client {
        /// The rest of the connection as a program that reads slowly: at most [`SLOW_READ`] bytes
        /// of what the server sends every [`SLOW_GAP`], below the WebSocket (no frame is whole at
        /// this pace), counted in `read`, with a command of its own each time so it is never
        /// silent; until the server ends the connection or `limit` passes.
        fn read_slowly(mut self, limit: Duration, read: &AtomicUsize) {
            let mut tcp = self.ws.get_ref().try_clone().expect("a second handle");
            let mut buf = [0u8; SLOW_READ];
            let until = Instant::now() + limit;
            while Instant::now() < until && self.ws.send(Message::text("trx:0;")).is_ok() {
                let _ = tcp.set_read_timeout(Some(SLOW_GAP));
                match tcp.read(&mut buf) {
                    Ok(0) => return,
                    Ok(n) => {
                        read.fetch_add(n, Ordering::Relaxed);
                    }
                    Err(e)
                        if matches!(
                            e.kind(),
                            std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                        ) => {}
                    Err(_) => return,
                }
                thread::sleep(SLOW_GAP);
            }
        }
    }

    /// ⭐ A PROGRAM THAT READS SLOWLY CANNOT HOLD OFF ITS FAREWELL: a program holding a `tci` key,
    /// through small buffers, is sent one write of receive audio, more than the buffers hold, then
    /// reads [`SLOW_READ`] bytes of it every [`SLOW_GAP`], with a command of its own each time so it
    /// is never silent. Each write of the socket goes on a little before a timeout on it would end
    /// it, so with a timeout on each write alone the writer's write lasted as long as the slow read
    /// went on, and the farewell that ends the program's transmissions never came meanwhile. Each
    /// write of the writer's has [`WRITE_STALL`] in all: past it the program is taken for gone, its
    /// transmissions end, and it is closed as lost, as one that stopped reading is.
    #[test]
    fn a_program_that_reads_slowly_cannot_hold_off_its_farewell() {
        let b = Bench::new();
        let c = keyed_through_small_buffers(&b);
        let id = b.id(1);
        let sent = Instant::now();
        assert!(
            b.handle.send_rx_audio(id, one_whole_write()),
            "its audio is started"
        );
        let read = Arc::new(AtomicUsize::new(0));
        let reader = {
            let read = Arc::clone(&read);
            thread::spawn(move || c.read_slowly(WRITE_STALL * 3, &read))
        };
        let farewell = b.mock.wait_call(WRITE_STALL + PAST_THE_STALL, |call| {
            *call == Call::Disconnected(id)
        });
        let took = sent.elapsed();
        assert!(
            farewell.is_some(),
            "no farewell {took:?} after the audio was sent, the program reading on ({} bytes \
             read); still served: {:?}",
            read.load(Ordering::Relaxed),
            b.handle
                .programs()
                .iter()
                .map(|p| (p.id, p.key))
                .collect::<Vec<_>>()
        );
        assert!(
            took >= WRITE_STALL,
            "taken for gone after {took:?}, inside the stall"
        );
        assert!(
            b.mock
                .wait_event(WAIT, |e| *e
                    == Event::Closed {
                        client: id,
                        why: Closed::Lost
                    })
                .is_some(),
            "closed as lost: {:?}",
            b.mock.events()
        );
        wait_gone(&b);
        assert_eq!(transmit_after_end(&b, id), [], "transmitting after its end");
        reader.join().expect("the program's reads");
    }

    /// The control: a program with the same small buffers that reads what it is sent at once
    /// takes every frame of receive audio sent to it, [`BURST`] at a time every [`BURST_GAP`] for
    /// longer than [`WRITE_STALL`], and stays. The stall is each write's own, so a program whose
    /// writes go out is never taken for gone.
    #[test]
    fn a_program_that_keeps_up_takes_all_its_audio_and_stays() {
        let b = Bench::new();
        let mut c = keyed_through_small_buffers(&b);
        let id = b.id(1);
        let sender = {
            let handle = b.handle.clone();
            thread::spawn(move || {
                let started = Instant::now();
                let mut sent = 0;
                while started.elapsed() < WRITE_STALL + BURST_GAP * 5 {
                    for _ in 0..BURST {
                        sent += usize::from(handle.send_rx_audio(id, rx_frame()));
                    }
                    thread::sleep(BURST_GAP);
                }
                (sent, started.elapsed())
            })
        };
        let audio = |c: &mut Client| {
            usize::from(matches!(
                c.next(Duration::from_millis(100)),
                Some(Message::Binary(_))
            ))
        };
        let mut got = 0;
        while !sender.is_finished() {
            got += audio(&mut c);
        }
        let (sent, took) = sender.join().expect("the audio was sent");
        let until = Instant::now() + WAIT;
        while got < sent && Instant::now() < until {
            got += audio(&mut c);
        }
        assert!(took > WRITE_STALL, "the audio ran only {took:?}");
        assert!(
            !b.mock.calls().contains(&Call::Disconnected(id)),
            "taken for gone while it kept up: {:?}",
            b.mock.events()
        );
        assert_eq!(got, sent, "every frame reached the program");
        assert!(c.alive(), "the program is still served");
        let _ = c.ws.close(None);
    }
}

// ── The init burst ───────────────────────────────────────────────────────────────────────────

const BURST: [&str; 31] = [
    "vfo_limits:1800000,54000000;",
    "if_limits:-24000,24000;",
    "trx_count:1;",
    "channels_count:2;",
    "device:Nexus;",
    "receive_only:false;",
    "modulations_list:am,lsb,usb,cw,nfm,digl,digu;",
    "protocol:ExpertSDR3,2.0;",
    "vfo:0,0,14074000;",
    "vfo:0,1,14074000;",
    "dds:0,14074000;",
    "modulation:0,digu;",
    "rx_enable:0,true;",
    "split_enable:0,false;",
    "rit_enable:0,false;",
    "xit_enable:0,false;",
    "rit_offset:0,0;",
    "xit_offset:0,0;",
    "drive:0,50;",
    "tune_drive:0,20;",
    "trx:0,false;",
    "tune:0,false;",
    "tx_enable:0,false;",
    "rx_mute:0,false;",
    "audio_samplerate:48000;",
    "audio_stream_sample_type:float32;",
    "audio_stream_channels:2;",
    "audio_stream_samples:2048;",
    "tx_stream_audio_buffering:100;",
    "ready;",
    "start;",
];

/// The init burst, line for line, each line its own WebSocket message, each one the strict
/// validator (eesdr-tci's rules) takes.
#[test]
fn the_init_burst_is_one_command_a_message() {
    let b = Bench::new();
    let mut c = Client::connect(b.port).expect("handshake");
    let mut got = Vec::new();
    while let Some(m) = c.next(WAIT) {
        match m {
            Message::Text(t) => {
                let t = t.as_str().to_string();
                assert_eq!(strict(&t), Ok(()), "message {}", got.len());
                let end = t == "start;";
                got.push(t);
                if end {
                    break;
                }
            }
            other => panic!("not text: {other:?}"),
        }
    }
    assert_eq!(got, BURST);
}

// ── What programs are told after it ─────────────────────────────────────────────────────────

#[test]
fn a_snapshot_sends_each_program_only_what_changed() {
    let b = Bench::new();
    let mut c = b.client();
    b.handle.publish(snapshot());
    assert!(
        c.texts(Duration::from_millis(300)).is_empty(),
        "nothing changed"
    );
    let mut s = snapshot();
    s.dial = hz(7_074_000);
    s.mode = word("digl");
    b.handle.publish(s.clone());
    assert_eq!(
        c.texts(Duration::from_millis(300)),
        [
            "vfo:0,0,7074000;",
            "vfo:0,1,7074000;",
            "dds:0,7074000;",
            "tx_frequency:7074000;",
            "modulation:0,digl;",
        ]
    );
    s.radio = 2;
    s.vfo_limits = (hz(144_000_000), hz(148_000_000));
    s.dial = hz(144_174_000);
    b.handle.publish(s);
    let lines = c.texts(Duration::from_millis(300));
    assert_eq!(
        lines[..4],
        [
            "device:Nexus;",
            "vfo_limits:144000000,148000000;",
            "receive_only:false;",
            "modulations_list:am,lsb,usb,cw,nfm,digl,digu;",
        ],
        "a radio switch sends the device lines first: {lines:?}"
    );
    for want in [
        "vfo:0,0,144174000;",
        "modulation:0,digl;",
        "trx:0,false;",
        "tx_enable:0,false;",
    ] {
        assert!(
            lines.iter().any(|l| l == want),
            "{want} after a radio switch: {lines:?}"
        );
    }
    for line in &lines {
        assert_eq!(strict(line), Ok(()));
    }
}

#[test]
fn the_sender_always_gets_its_echo_and_the_others_only_a_change() {
    let b = Bench::new();
    let mut a = b.client();
    let mut other = b.client();
    // The dial it already has: WSJT-X still waits for the echo.
    a.ask("vfo:0,0,14074000;", "vfo:0,0,14074000;");
    assert!(other.texts(Duration::from_millis(300)).is_empty());
    a.ask("vfo:0,0,14075000;", "vfo:0,0,14075000;");
    assert!(other.until("vfo:0,0,14075000;"));
    assert!(b.mock.calls().contains(&Call::SetDial(b.id(1), 14_075_000)));
}

/// At the server: every power and SWR value has a decimal point, even a whole one.
#[test]
fn power_and_swr_carry_a_decimal_point() {
    let b = Bench::new();
    let mut c = b.client();
    c.send("tx_sensors_enable:true,30;");
    c.send("rx_sensors_enable:true,30;");
    let mut s = snapshot();
    s.trx = true;
    s.rx_dbm = Some(-73.0);
    s.tx = Some(TxMeters {
        mic_dbm: -27.0,
        rms_w: 47.0,
        peak_w: 50.0,
        swr: 1.0,
    });
    b.handle.publish(s);
    let mut seen = std::collections::BTreeSet::new();
    let until = Instant::now() + WAIT;
    while seen.len() < 5 && Instant::now() < until {
        let Some(t) = c.text(Duration::from_millis(200)) else {
            continue;
        };
        let name = t.split([':', ';']).next().unwrap_or("").to_string();
        if DECIMALS.iter().any(|(n, _)| *n == name) {
            assert_eq!(strict(&t), Ok(()), "{t}");
            seen.insert(name);
        }
    }
    assert_eq!(
        seen.into_iter().collect::<Vec<_>>(),
        [
            "rx_channel_sensors",
            "rx_sensors",
            "tx_power",
            "tx_sensors",
            "tx_swr"
        ]
    );
}

// ── Keys ─────────────────────────────────────────────────────────────────────────────────────

#[test]
fn an_admitted_key_tells_every_program_at_once() {
    let b = Bench::new();
    let mut a = b.client();
    let mut other = b.client();
    a.ask("audio_start:0;", "audio_start:0;");
    a.ask("trx:0,true,tci;", "trx:0,true;");
    assert!(other.until("trx:0,true;"), "the other program is told too");
    assert!(b
        .mock
        .calls()
        .contains(&Call::Key(b.id(1), KeyKind::Tci, true)));
    assert_eq!(b.handle.programs()[0].key, Some(KeyKind::Tci));
}

#[test]
fn a_refused_key_answers_trx_false_and_logs_its_gate_once() {
    let b = Bench::new();
    b.mock.answers().transmit = Err(Refusal::Disarmed);
    let mut c = b.client();
    c.ask("trx:0,true,tci;", "trx:0,false;");
    c.ask("trx:0,true,tci;", "trx:0,false;");
    let refusals = b
        .mock
        .events()
        .into_iter()
        .filter(|e| {
            matches!(
                e,
                Event::Refusal {
                    refusal: Refusal::Disarmed,
                    ..
                }
            )
        })
        .count();
    assert_eq!(refusals, 1, "logged once until the answer changes");
    // Not audio-started, as the backend was told.
    assert!(b
        .mock
        .calls()
        .contains(&Call::Key(b.id(1), KeyKind::Tci, false)));
    assert_eq!(b.handle.programs()[0].key, None);
}

/// What "Last refused" and the connection log say when a program's own audio comes before it asked
/// a voice mode, as the app's hint and the guide say it: the advice whole, and that WSJT-X and
/// JTDX ask for a mode only when it differs from Nexus's (set to USB on a station already in USB,
/// they ask for nothing and are refused).
#[test]
fn the_voice_mode_refusal_says_some_programs_ask_only_when_the_mode_differs() {
    assert_eq!(
        Refusal::VoiceMode.text(),
        "a program may transmit its own audio only after it asks for a voice mode (USB, LSB, AM or \
         FM): set the program's Radio mode, or use Nexus for FT8; some programs, WSJT-X and JTDX \
         among them, ask for their mode only when it differs from Nexus's"
    );
}

/// The forms MSHV writes on its first `vfo` (`network.cpp:2482-2491`): none of them arms.
#[test]
fn mshvs_first_writes_are_answered_and_arm_nothing() {
    let b = Bench::new();
    let mut c = b.client();
    c.ask("tx_enable:0,true;", "tx_enable:0,false;");
    c.ask("rx_enable:0,true;", "rx_enable:0,true;");
    c.ask("rx_mute:0,false;", "rx_mute:0,false;");
    c.ask("split_enable:0,false;", "split_enable:0,false;");
    c.ask("drive:0,25;", "drive:0,25;");
    let calls = b.mock.calls();
    assert!(
        !calls.iter().any(Call::can_transmit),
        "nothing that transmits: {calls:?}"
    );
    assert!(calls.contains(&Call::SetDrive(b.id(1), 25)));
}

/// WSJT-X's start-up (`TCITransceiver.cpp:400-480`, `:1084-1150`), end to end.
#[test]
fn wsjtx_start_up_key_and_release() {
    let b = Bench::new();
    let mut c = b.client();
    c.ask("split_enable:false;", "split_enable:0,false;");
    c.ask("audio_start:0;", "audio_start:0;");
    c.send("rx_sensors_enable:true,500;");
    c.ask("vfo:0,0,14076000;", "vfo:0,0,14076000;");
    c.ask("trx:0,true,tci;", "trx:0,true;");
    c.send("trx:0,false,tci;");
    assert!(b
        .mock
        .wait_call(WAIT, |call| *call == Call::Release(b.id(1)))
        .is_some());
    assert_eq!(b.handle.programs()[0].key, None);
    // The radio unkeys: the next snapshot tells it.
    b.handle.publish(snapshot());
    assert!(c.until("trx:0,false;"));
}

/// A program's release: the server stops taking its transmit audio before the backend hears it,
/// never after.
#[test]
fn the_server_stops_taking_audio_before_the_backend_hears_a_release() {
    let b = Bench::new();
    let mut c = keyed(&b);
    b.mock.pause_at("release");
    c.send("trx:0,false,tci;");
    assert!(b
        .mock
        .wait_call(WAIT, |call| *call == Call::Release(b.id(1)))
        .is_some());
    let key = b.handle.programs()[0].key;
    b.mock.release();
    assert_eq!(
        key, None,
        "still taking its audio while the backend hears the release"
    );
}

/// JTDX's "Rig" split: refused cleanly, the dial told back, nothing retuned for B.
#[test]
fn jtdx_rig_split_is_refused_cleanly() {
    let b = Bench::new();
    let mut c = b.client();
    c.ask("vfo:0,1,14080000;", "vfo:0,1,14074000;");
    c.ask("split_enable:0,true;", "split_enable:0,false;");
    assert!(!b
        .mock
        .calls()
        .iter()
        .any(|call| matches!(call, Call::SetDial(..))));
}

// ── Transmit audio ───────────────────────────────────────────────────────────────────────────

/// A transmit-audio frame as WSJT-X sends it: `length` values, float32 stereo at 48 kHz, in a
/// payload of `payload` values.
fn tx_frame(length: u32, payload: usize) -> Vec<u8> {
    let mut f = Vec::with_capacity(HEADER_LEN + payload * 4);
    for v in [0u32, 48_000, 3, 0, 0, length, 2, 0] {
        f.extend_from_slice(&v.to_le_bytes());
    }
    f.resize(HEADER_LEN, 0);
    f.resize(HEADER_LEN + payload * 4, 0);
    f
}

fn keyed(b: &Bench) -> Client {
    let mut c = b.client();
    c.ask("audio_start:0;", "audio_start:0;");
    c.ask("trx:0,true,tci;", "trx:0,true;");
    c
}

/// A short frame is refused and counted; nothing reaches the backend; the program goes on.
#[test]
fn a_short_binary_frame_is_refused_counted_and_harmless() {
    let b = Bench::new();
    let mut c = keyed(&b);
    c.send_binary(vec![2u8; 10]);
    c.send_binary(tx_frame(2048, 2048)[..HEADER_LEN - 1].to_vec());
    assert!(c.alive());
    let counts = b.handle.programs()[0].counts;
    assert_eq!(counts.bad_frames, 2, "{counts:?}");
    assert!(!b
        .mock
        .calls()
        .iter()
        .any(|call| matches!(call, Call::TxAudio(..))));
    // Positive control: a whole frame from the key holder reaches it.
    c.send_binary(tx_frame(2048, 2048));
    assert!(b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::TxAudio(..)))
        .is_some());
}

/// Transmit audio only from the key holder; others' dropped, never queued.
#[test]
fn transmit_audio_from_a_program_without_the_key_is_dropped() {
    let b = Bench::new();
    let mut holder = keyed(&b);
    let mut other = b.client();
    other.send_binary(tx_frame(2048, 2048));
    other.send_binary(tx_frame(2048, 4096));
    assert!(other.alive());
    assert_eq!(b.handle.programs()[1].counts.tx_dropped, 2);
    // Receive audio, IQ, a chrono, another receiver: not transmit audio, refused.
    let mut wrong = tx_frame(2048, 2048);
    wrong[24] = 1;
    holder.send_binary(wrong);
    let mut receiver = tx_frame(2048, 2048);
    receiver[0] = 1;
    holder.send_binary(receiver);
    assert!(holder.alive());
    assert_eq!(b.handle.programs()[0].counts.bad_frames, 2);
    assert!(!b
        .mock
        .calls()
        .iter()
        .any(|call| matches!(call, Call::TxAudio(..))));
}

/// At most twice real time, counting `length` (WSJT-X's payload is twice what it fills).
#[test]
fn transmit_audio_over_twice_real_time_is_dropped() {
    let b = Bench::new();
    let mut c = keyed(&b);
    // 2048 values stereo at 48 kHz: 21.3 ms a frame. 150 frames are 3.2 s of audio.
    let t0 = Instant::now();
    for _ in 0..150 {
        c.send_binary(tx_frame(2048, 4096));
    }
    let sent_in = t0.elapsed();
    assert!(c.alive());
    let counts = b.handle.programs()[0].counts;
    let most = (0.5 + 2.0 * (sent_in.as_secs_f64() + 0.3)) / (1024.0 / 48_000.0);
    assert!(
        (counts.tx_frames as f64) <= most + 1.0 && counts.tx_frames >= 23,
        "{counts:?} in {sent_in:?}: at most {most:.0} frames"
    );
    assert_eq!(counts.tx_frames + counts.tx_dropped, 150);
}

/// A key the snapshot shows ended (Stop, the watchdog) stops the server taking its audio.
#[test]
fn a_key_the_snapshot_shows_ended_takes_no_more_audio() {
    let b = Bench::new();
    let mut c = keyed(&b);
    let mut s = snapshot();
    s.trx = true;
    s.owner = Some(b.id(1));
    b.handle.publish(s.clone());
    assert_eq!(b.handle.programs()[0].key, Some(KeyKind::Tci));
    s.trx = false;
    s.owner = None;
    b.handle.publish(s);
    assert!(c.until("trx:0,false;"));
    assert_eq!(b.handle.programs()[0].key, None);
    c.send_binary(tx_frame(2048, 2048));
    assert!(c.alive());
    assert_eq!(b.handle.programs()[0].counts.tx_dropped, 1);
}

// ── Audio out ────────────────────────────────────────────────────────────────────────────────

fn rx_frame() -> Vec<u8> {
    let mut f = tx_frame(2048, 2048);
    f[24] = 1;
    f
}

#[test]
fn receive_audio_goes_only_to_a_program_that_started_it() {
    let b = Bench::new();
    let mut c = b.client();
    assert!(!b.handle.send_rx_audio(b.id(1), rx_frame()), "not started");
    c.ask("audio_start:0;", "audio_start:0;");
    assert!(
        !b.handle.send_rx_audio(b.id(1), tx_frame(2048, 2048)),
        "not receive audio"
    );
    assert!(b.handle.send_rx_audio(b.id(1), rx_frame()));
    let got = loop {
        match c.next(WAIT) {
            Some(Message::Binary(bytes)) => break bytes,
            Some(_) => {}
            None => panic!("no audio"),
        }
    };
    assert_eq!(got.len(), rx_frame().len());
    c.ask("audio_stop:0;", "audio_stop:0;");
    assert!(!b.handle.send_rx_audio(b.id(1), rx_frame()), "stopped");
}

fn chrono() -> Chrono {
    Chrono {
        trx: crate::tci::Trx::new(0).expect("0"),
        sample_rate: 48_000,
        format: SampleFormat::Float32,
        channels: Channels::Two,
        length: 2048,
    }
}

/// At the server: TX_CHRONO only to the program holding an admitted `tci` key.
#[test]
fn a_chrono_goes_only_to_the_tci_key_holder() {
    let b = Bench::new();
    let mut holder = keyed(&b);
    let mut other = b.client();
    other.ask("trx:0,true;", "trx:0,true;");
    assert!(
        !b.handle.send_chrono(b.id(2), &chrono()),
        "a microphone key"
    );
    assert!(b.handle.send_chrono(b.id(1), &chrono()));
    let got = loop {
        match holder.next(WAIT) {
            Some(Message::Binary(bytes)) => break bytes,
            Some(_) => {}
            None => panic!("no chrono"),
        }
    };
    assert_eq!(&got[24..28], &3u32.to_le_bytes(), "a TX_CHRONO");
    holder.send("trx:0,false,tci;");
    assert!(b
        .mock
        .wait_call(WAIT, |c| *c == Call::Release(b.id(1)))
        .is_some());
    assert!(holder.alive());
    assert!(!b.handle.send_chrono(b.id(1), &chrono()), "released");
}

/// The outbox: receive audio goes first when a program does not read, and a text backlog closes
/// it. The publisher never waits.
#[test]
fn a_program_that_does_not_read_loses_audio_first_and_never_blocks_the_publisher() {
    let b = Bench::with(Limits {
        outbox: 64 * 1024,
        text_backlog: 32 * 1024,
        ..Limits::default()
    });
    let mut c = Client::connect(b.port).expect("handshake");
    c.send("audio_start:0;");
    let until = Instant::now() + WAIT;
    while b.handle.programs().first().is_some_and(|p| !p.audio) && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    // It never reads from here. 64 MiB of audio cannot fit in any socket's buffers.
    let t0 = Instant::now();
    for _ in 0..4096 {
        b.handle.send_rx_audio(b.id(1), rx_frame());
    }
    assert!(
        t0.elapsed() < Duration::from_secs(3),
        "the pump waited on the program: {:?}",
        t0.elapsed()
    );
    let until = Instant::now() + WAIT;
    while b.handle.programs()[0].counts.audio_dropped == 0 && Instant::now() < until {
        b.handle.send_rx_audio(b.id(1), rx_frame());
    }
    assert!(b.handle.programs()[0].counts.audio_dropped > 0);
    // Text that cannot be dropped: past the backlog the program is closed.
    let mut s = snapshot();
    for i in 0..20_000i64 {
        s.dial = hz(14_000_000 + i);
        b.handle.publish(s.clone());
        if b.handle.programs().is_empty() {
            break;
        }
    }
    assert!(b
        .mock
        .wait_event(WAIT, |e| matches!(
            e,
            Event::Closed {
                why: Closed::Backlog,
                ..
            }
        ))
        .is_some());
}

// ── Ending ───────────────────────────────────────────────────────────────────────────────────

/// A lost socket ends the program's transmissions, first.
#[test]
fn a_lost_socket_ends_the_programs_transmissions() {
    let b = Bench::new();
    let c = keyed(&b);
    let t0 = Instant::now();
    drop(c);
    assert!(b
        .mock
        .wait_call(WAIT, |call| *call == Call::Disconnected(b.id(1)))
        .is_some());
    assert!(
        t0.elapsed() < Duration::from_secs(1),
        "ended after {:?}",
        t0.elapsed()
    );
    let calls = b.mock.calls();
    let ended = calls
        .iter()
        .position(|c| *c == Call::Disconnected(b.id(1)))
        .expect("ended");
    assert_eq!(ended, calls.len() - 1, "nothing after it: {calls:?}");
    // Its slot is freed after its transmissions end, not before.
    let until = Instant::now() + WAIT;
    while !b.handle.programs().is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(b.handle.programs().is_empty());
}

/// Stopping ends every key first, then `trx:0,false;`, then the close. The order is
/// read where it shows: when a program hears `trx:0,false;`, its transmissions have already ended.
#[test]
fn stopping_ends_keys_then_tells_trx_false_then_closes() {
    let mut b = Bench::new();
    let mut c = keyed(&b);
    let mut other = b.client();
    let server = b.server.take().expect("running");
    let stopper = std::thread::spawn(move || server.stop());
    for (client, id) in [(&mut c, b.id(1)), (&mut other, b.id(2))] {
        let mut last = None;
        let code = loop {
            match client.next(WAIT) {
                Some(Message::Text(t)) => {
                    if t.as_str() == "trx:0,false;" {
                        assert!(
                            b.mock.calls().contains(&Call::Disconnected(id)),
                            "{id}: told trx:0,false; before its transmissions ended"
                        );
                    }
                    last = Some(t.as_str().to_string());
                }
                Some(Message::Close(f)) => break f.map(|f| u16::from(f.code)),
                Some(_) => {}
                None => break None,
            }
        };
        assert_eq!(last.as_deref(), Some("trx:0,false;"));
        assert_eq!(code, Some(1001));
    }
    stopper.join().expect("stopped");
    assert_eq!(
        b.mock
            .calls()
            .iter()
            .filter(|c| matches!(c, Call::Disconnected(_)))
            .count(),
        2
    );
    // The port is free.
    assert!(TcpListener::bind((Ipv4Addr::LOCALHOST, b.port)).is_ok());
}

#[test]
fn the_operators_disconnect_ends_the_program() {
    let b = Bench::new();
    let mut c = keyed(&b);
    b.handle.disconnect(b.id(1));
    assert_eq!(c.close_code(WAIT), Some(1000));
    assert!(b.mock.calls().contains(&Call::Disconnected(b.id(1))));
    let until = Instant::now() + WAIT;
    while !b.handle.programs().is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(b.handle.programs().is_empty());
}

// ── A program's end, and what it may still send ─────────────────────────────────────────────

/// Wait until no program is connected.
fn wait_gone(b: &Bench) {
    let until = Instant::now() + WAIT;
    while !b.handle.programs().is_empty() && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(b.handle.programs().is_empty(), "the program is gone");
}

/// The backend's calls after `Disconnected(client)`.
fn after_end(b: &Bench, client: ClientId) -> Vec<Call> {
    let calls = b.mock.calls();
    let ended = calls
        .iter()
        .position(|c| *c == Call::Disconnected(client))
        .expect("the program was ended");
    calls[ended + 1..].to_vec()
}

/// Those that could transmit: there must be none.
fn transmit_after_end(b: &Bench, client: ClientId) -> Vec<Call> {
    after_end(b, client)
        .into_iter()
        .filter(Call::can_transmit)
        .collect()
}

/// The operator's Disconnect while one of the program's messages is still running: nothing later
/// in that message reaches the backend, so nothing it sends can transmit once its end began
/// (found in a security review).
#[test]
fn nothing_after_the_operators_disconnect_can_transmit() {
    let b = Bench::new();
    let mut c = b.client();
    c.ask("audio_start:0;", "audio_start:0;");
    b.mock.pause_at("set_drive");
    // One message: a write the backend holds, then a retune, a key, a Tune and CW behind it.
    c.send("drive:0,25;vfo:0,0,14075000;trx:0,true,tci;tune:0,true;cw_macros:0,CQ;");
    assert!(b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::SetDrive(..)))
        .is_some());
    b.handle.disconnect(b.id(1));
    b.mock.release();
    wait_gone(&b);
    assert_eq!(
        after_end(&b, b.id(1)),
        [],
        "nothing reaches the backend after the end"
    );
}

/// The same while the server stops.
#[test]
fn nothing_after_a_stop_begins_can_transmit() {
    let mut b = Bench::new();
    let mut c = b.client();
    c.ask("audio_start:0;", "audio_start:0;");
    b.mock.pause_at("set_drive");
    c.send("drive:0,25;vfo:0,0,14075000;trx:0,true,tci;tune:0,true;cw_macros:0,CQ;");
    assert!(b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::SetDrive(..)))
        .is_some());
    let server = b.server.take().expect("running");
    let stopper = std::thread::spawn(move || server.stop());
    assert!(b
        .mock
        .wait_call(WAIT, |call| *call == Call::Disconnected(b.id(1)))
        .is_some());
    b.mock.release();
    stopper.join().expect("stopped");
    assert_eq!(
        after_end(&b, b.id(1)),
        [],
        "nothing reaches the backend after the end"
    );
}

/// A key the backend is admitting when the program's end begins: the end waits for the call and
/// then ends what it admitted. Told first, the backend would end nothing, and the key would outlive
/// it.
#[test]
fn a_disconnect_waits_for_a_key_in_flight_and_then_ends_it() {
    let b = Bench::new();
    let mut c = b.client();
    b.mock.pause_at("key");
    c.send("trx:0,true;");
    assert!(b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::Key(..)))
        .is_some());
    let handle = b.handle.clone();
    let id = b.id(1);
    let ender = std::thread::spawn(move || handle.disconnect(id));
    let early = b.mock.wait_call(Duration::from_millis(500), |call| {
        *call == Call::Disconnected(id)
    });
    b.mock.release();
    ender.join().expect("disconnected");
    assert_eq!(early, None, "ended while its key was in flight");
    let calls = b.mock.calls();
    let key = calls.iter().position(|c| matches!(c, Call::Key(..)));
    let ended = calls.iter().position(|c| *c == Call::Disconnected(id));
    assert!(key < ended && ended.is_some(), "{calls:?}");
    wait_gone(&b);
    assert_eq!(transmit_after_end(&b, id), []);
}

/// A spot the backend is keeping when the program's end begins: the end waits for the call, so
/// the backend drops the program's spots after it and none outlives the program; and a spot
/// behind it is never asked once the end began.
#[test]
fn a_disconnect_waits_for_a_spot_in_flight_and_no_spot_comes_after_it() {
    let b = Bench::new();
    let mut c = b.client();
    b.mock.pause_at("spot");
    c.send("spot:K1ABC,CW,14025000,4294901760,CQ;");
    assert!(b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::Spot(..)))
        .is_some());
    let handle = b.handle.clone();
    let id = b.id(1);
    let ender = std::thread::spawn(move || handle.disconnect(id));
    let early = b.mock.wait_call(Duration::from_millis(500), |call| {
        *call == Call::Disconnected(id)
    });
    // Another spot behind it: it may not reach the backend once the end began.
    c.send("spot:W1AW,CW,14030000,4294901760,QST;");
    b.mock.release();
    ender.join().expect("disconnected");
    assert_eq!(early, None, "ended while its spot was in flight");
    let calls = b.mock.calls();
    let spot = calls.iter().position(|c| matches!(c, Call::Spot(..)));
    let ended = calls.iter().position(|c| *c == Call::Disconnected(id));
    assert!(spot < ended && ended.is_some(), "{calls:?}");
    wait_gone(&b);
    assert_eq!(
        after_end(&b, id),
        [],
        "nothing reaches the backend after the end"
    );
}

/// A mode ask the backend is taking when the program's end begins: the end waits for the call, then
/// the backend forgets the ask with the program. The backend keys a program's own audio only after
/// that program asked a voice mode, and each server numbers its programs from 1 again: an ask kept
/// past its program's end would stand for the next server's program with that number, which asked
/// nothing.
#[test]
fn a_disconnect_waits_for_a_mode_ask_in_flight_and_then_forgets_it() {
    let mut b = Bench::new();
    let mut c = b.client();
    b.mock.pause_at("set_mode");
    c.send("modulation:0,usb;");
    assert!(b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::SetMode(..)))
        .is_some());
    let handle = b.handle.clone();
    let id = b.id(1);
    let ender = std::thread::spawn(move || handle.disconnect(id));
    let early = b.mock.wait_call(Duration::from_millis(500), |call| {
        *call == Call::Disconnected(id)
    });
    b.mock.release();
    ender.join().expect("disconnected");
    wait_gone(&b);
    let kept = b.mock.asked(id);
    // The next server, on the same backend: its first program is program 1 again.
    b.server.take().expect("running").stop();
    let next = Server::start(
        Config {
            port: 0,
            limits: Limits::default(),
        },
        Arc::clone(&b.mock) as Arc<dyn TciBackend>,
        snapshot(),
    )
    .expect("a loopback port");
    let mut newcomer = Client::connect(next.port()).expect("a handshake the gate takes");
    newcomer.burst();
    let numbered: Vec<ClientId> = next.handle().programs().iter().map(|p| p.id).collect();
    assert_eq!(numbered, [id], "the next server numbers from 1 again");
    assert_eq!(
        (kept, b.mock.asked(id)),
        (None, None),
        "the ask kept after its program's end, and for the next program 1, which asked nothing"
    );
    assert_eq!(early, None, "ended while its mode ask was in flight");
    let calls = b.mock.calls();
    let ask = calls.iter().position(|c| matches!(c, Call::SetMode(..)));
    let ended = calls.iter().position(|c| *c == Call::Disconnected(id));
    assert!(ask < ended && ended.is_some(), "{calls:?}");
}

/// Transmit audio the backend is taking when the program's end begins: the same.
#[test]
fn a_disconnect_waits_for_transmit_audio_in_flight() {
    let b = Bench::new();
    let mut c = keyed(&b);
    b.mock.pause_at("tx_audio");
    c.send_binary(tx_frame(2048, 4096));
    assert!(b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::TxAudio(..)))
        .is_some());
    let handle = b.handle.clone();
    let id = b.id(1);
    let ender = std::thread::spawn(move || handle.disconnect(id));
    let early = b.mock.wait_call(Duration::from_millis(500), |call| {
        *call == Call::Disconnected(id)
    });
    // More audio behind it: none of it may reach the backend once the end began.
    c.send_binary(tx_frame(2048, 4096));
    b.mock.release();
    ender.join().expect("disconnected");
    assert_eq!(early, None, "ended while its audio was in flight");
    wait_gone(&b);
    assert_eq!(transmit_after_end(&b, id), [], "{:?}", b.mock.calls());
}

/// The gate on its own: a program whose end has begun but whose close has not (the moment between
/// the two inside a Disconnect or a stop) still has its frames read, and none reaches the backend.
#[test]
fn an_ended_programs_transmit_audio_never_reaches_the_backend() {
    let b = Bench::new();
    let mut c = keyed(&b);
    let id = b.id(1);
    let conn = b.handle.shared.find(id).expect("connected");
    conn.end_transmit(&b.handle.shared);
    assert!(b.mock.calls().contains(&Call::Disconnected(id)));
    c.send_binary(tx_frame(2048, 4096));
    let until = Instant::now() + WAIT;
    while b.handle.programs()[0].counts.tx_dropped == 0 && Instant::now() < until {
        std::thread::sleep(Duration::from_millis(10));
    }
    let counts = b.handle.programs()[0].counts;
    assert_eq!((counts.tx_dropped, counts.tx_frames), (1, 0), "{counts:?}");
    assert_eq!(transmit_after_end(&b, id), []);
}

/// And the table's half of it: once a program's end has begun, a key, a Tune, CW or a mode ask (the
/// backend keys a program's own audio only after it asked a voice mode) asks the backend nothing
/// and is answered nothing.
#[test]
fn an_ended_program_asks_the_backend_nothing_that_transmits() {
    for text in [
        "trx:0,true,tci;",
        "trx:0,true;",
        "tune:0,true;",
        "cw_macros:0,CQ;",
        "modulation:0,usb;",
    ] {
        let mock = Mock::default();
        let model = Mutex::new(snapshot());
        let session = Mutex::new(Session::default());
        let lines = Lines::default();
        let fx = dispatch::run(
            classify(text),
            &Cx {
                client: ClientId::new(1),
                backend: &mock,
                model: &model,
                session: &session,
                sink: &lines,
                gate: &Mutex::new(true),
            },
        );
        assert_eq!(mock.calls(), [], "{text}: the backend");
        assert!(fx.all.is_empty(), "{text}: told no one");
        assert!(
            lines.0.lock().expect("lines").is_empty(),
            "{text}: answered"
        );
    }
    // Positive control: with the gate open the same commands ask.
    let mock = Mock::default();
    let (_, calls, _) = run_one("trx:0,true,tci;", &mock);
    assert_eq!(calls, ["Key(Tci)"]);
    let mock = Mock::default();
    let (_, calls, _) = run_one("modulation:0,usb;", &mock);
    assert_eq!(calls, ["SetMode(usb)"]);
}

// ── Transmit audio: what a frame claims against what it carries ─────────────────────────────

/// A transmit frame's header (float32 stereo at 48 kHz, as WSJT-X sends it) claiming `length`
/// values, then `payload` bytes.
fn claimed(length: u32, payload: usize) -> Vec<u8> {
    let mut f = tx_frame(length, 0);
    f.resize(HEADER_LEN + payload, 0);
    f
}

/// Found in a security review: a frame whose `length` and payload disagree is refused, counted, and
/// never reaches the backend. A claim of 0 (or one too small for its payload) used to pay nothing.
#[test]
fn a_transmit_frame_whose_length_and_payload_disagree_is_refused() {
    let b = Bench::new();
    let mut c = keyed(&b);
    let frames = [
        // Claims nothing, carries nothing: a header alone.
        claimed(0, 0),
        // Claims nothing, carries 32 KiB.
        claimed(0, 32 * 1024),
        // Claims half a stereo frame.
        claimed(1, 4),
        // Claims 2048 values, carries 100.
        claimed(2048, 400),
        // Carries more than twice its claim.
        claimed(100, 4 * 201),
        // Not a whole number of samples.
        claimed(2048, 2048 * 4 + 3),
    ];
    let n = frames.len() as u64;
    for f in frames {
        c.send_binary(f);
    }
    assert!(c.alive());
    let counts = b.handle.programs()[0].counts;
    assert_eq!((counts.bad_frames, counts.tx_frames), (n, 0), "{counts:?}");
    assert!(!b
        .mock
        .calls()
        .iter()
        .any(|call| matches!(call, Call::TxAudio(..))));
    // Positive control: WSJT-X's own shape, twice its claim, is taken.
    c.send_binary(tx_frame(2048, 4096));
    assert!(b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::TxAudio(..)))
        .is_some());
}

/// A flood of tiny frames pays at least the smallest frame a program may stream (100 values) each,
/// so it cannot buy unbounded backend calls with a small claim.
#[test]
fn tiny_transmit_frames_pay_at_least_the_smallest_stream_frame() {
    let b = Bench::new();
    let mut c = keyed(&b);
    let t0 = Instant::now();
    for _ in 0..3000 {
        // One stereo frame claimed, two carried: a valid claim, 21 microseconds of audio.
        c.send_binary(tx_frame(2, 4));
    }
    assert!(c.alive());
    let sent_in = t0.elapsed();
    let counts = b.handle.programs()[0].counts;
    // Charged 100 values (50 stereo frames) each: half a second at once, then twice real time.
    let most = (0.5 + 2.0 * (sent_in.as_secs_f64() + 0.3)) / (50.0 / 48_000.0);
    assert!(
        (counts.tx_frames as f64) <= most + 1.0,
        "{counts:?} in {sent_in:?}: at most {most:.0}"
    );
    assert_eq!(counts.tx_frames + counts.tx_dropped, 3000, "{counts:?}");
}

/// The backend is handed the `length` values that were paid for, not the rest of the payload.
#[test]
fn the_backend_gets_only_the_values_that_were_paid_for() {
    let b = Bench::new();
    let mut c = keyed(&b);
    // WSJT-X's shape: 2048 values claimed in a payload of 4096.
    c.send_binary(tx_frame(2048, 4096));
    let got = b
        .mock
        .wait_call(WAIT, |call| matches!(call, Call::TxAudio(..)));
    assert_eq!(got, Some(Call::TxAudio(b.id(1), HEADER_LEN + 2048 * 4)));
}

// ── Refused outright ─────────────────────────────────────────────────────────────────────────

/// A unique directory under the system's temporary one, removed at the end.
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let p = std::env::temp_dir().join(format!(
            "tci-{tag}-{}-{nanos}-{}",
            std::process::id(),
            N.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).expect("a temporary directory");
        TempDir(p)
    }

    fn entries(&self) -> usize {
        std::fs::read_dir(&self.0).map_or(0, |d| d.count())
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn line_out_recorder_save_is_refused_and_writes_nothing() {
    let b = Bench::new();
    let dir = TempDir::new("recorder");
    // Positive control for the emptiness check: it sees a file where one is.
    let control = TempDir::new("control");
    std::fs::write(control.0.join("x.wav"), b"x").expect("write");
    assert_eq!(control.entries(), 1);
    let mut c = b.client();
    let path = dir.0.join("rec.wav");
    let path = path.to_string_lossy();
    c.send("line_out_recorder_start:0,250;");
    c.send(&format!("line_out_recorder_save:0,{path};"));
    c.send("line_out_recorder_break:0;");
    // Answered nothing: the next line is the read's.
    c.send("trx:0;");
    assert_eq!(c.text(WAIT).as_deref(), Some("trx:0,false;"));
    assert_eq!(dir.entries(), 0, "nothing written");
    assert!(b.mock.events().contains(&Event::CommandRefused {
        client: b.id(1),
        what: Refused::Recorder
    }));
    let calls = b.mock.calls();
    assert_eq!(calls, [Call::Connected(b.id(1))], "the backend saw nothing");
}

#[test]
fn keyer_is_refused_and_keys_nothing() {
    let b = Bench::new();
    let mut c = b.client();
    c.send("keyer:0,true,100;");
    c.send("keyer:0,false,100;");
    c.send("trx:0;");
    assert_eq!(c.text(WAIT).as_deref(), Some("trx:0,false;"));
    assert!(!b.mock.calls().iter().any(Call::can_transmit));
    assert!(b.mock.events().contains(&Event::CommandRefused {
        client: b.id(1),
        what: Refused::Keyer
    }));
}

// ── The dispatch table, without sockets ─────────────────────────────────────────────────────

/// The sender's lines, collected.
#[derive(Default)]
struct Lines(Mutex<Vec<String>>);

impl Sink for Lines {
    fn send(&self, lines: Vec<ServerText>) {
        self.0
            .lock()
            .expect("lines")
            .extend(lines.into_iter().map(ServerText::into_string));
    }
}

fn classify(text: &str) -> Action {
    let mut all: Vec<_> = tciwire::inbound(text, &EXPERT_SDR3).collect();
    assert_eq!(all.len(), 1, "{text}");
    dispatch::classify(all.remove(0).expect(text))
}

/// The arm an action is, by name. Exhaustive: an arm added to [`Action`] does not compile here
/// until the table below covers it.
fn arm(a: &Action) -> &'static str {
    match a {
        Action::Answer(_) => "Answer",
        Action::Format(_) => "Format",
        Action::Sensors { .. } => "Sensors",
        Action::Dial(_) => "Dial",
        Action::SubDial(_) => "SubDial",
        Action::Mode(_) => "Mode",
        Action::Drive(_) => "Drive",
        Action::TuneDrive(_) => "TuneDrive",
        Action::Key(_) => "Key",
        Action::Release => "Release",
        Action::TuneOn => "TuneOn",
        Action::TuneOff => "TuneOff",
        Action::EndOwn => "EndOwn",
        Action::AudioStart => "AudioStart",
        Action::AudioStop => "AudioStop",
        Action::Cw(_) => "Cw",
        Action::CwStop => "CwStop",
        Action::CwSpeed(_) => "CwSpeed",
        Action::Spot(_) => "Spot",
        Action::SpotDelete(_) => "SpotDelete",
        Action::SpotClear => "SpotClear",
        Action::Refuse(..) => "Refuse",
        Action::Ignore(_) => "Ignore",
    }
}

const ARMS: [&str; 23] = [
    "Answer",
    "Format",
    "Sensors",
    "Dial",
    "SubDial",
    "Mode",
    "Drive",
    "TuneDrive",
    "Key",
    "Release",
    "TuneOn",
    "TuneOff",
    "EndOwn",
    "AudioStart",
    "AudioStop",
    "Cw",
    "CwStop",
    "CwSpeed",
    "Spot",
    "SpotDelete",
    "SpotClear",
    "Refuse",
    "Ignore",
];

/// One command per row: what it is, what the backend is asked, and what the program
/// that sent it is told. Every arm of the table, each by a command a real client sends.
struct Row {
    text: &'static str,
    arm: &'static str,
    calls: &'static [&'static str],
    reply: &'static [&'static str],
}

const ROWS: &[Row] = &[
    Row {
        text: "start;",
        arm: "Answer",
        calls: &[],
        reply: &["start;"],
    },
    Row {
        text: "vfo:0,0;",
        arm: "Answer",
        calls: &[],
        reply: &["vfo:0,0,14074000;"],
    },
    Row {
        text: "vfo:0,1;",
        arm: "Answer",
        calls: &[],
        reply: &["vfo:0,1,14074000;"],
    },
    Row {
        text: "dds:0;",
        arm: "Answer",
        calls: &[],
        reply: &["dds:0,14074000;"],
    },
    Row {
        text: "if:0,0;",
        arm: "Answer",
        calls: &[],
        reply: &["if:0,0,0;"],
    },
    Row {
        text: "modulation:0;",
        arm: "Answer",
        calls: &[],
        reply: &["modulation:0,digu;"],
    },
    Row {
        text: "trx:0;",
        arm: "Answer",
        calls: &[],
        reply: &["trx:0,false;"],
    },
    Row {
        text: "tune:0;",
        arm: "Answer",
        calls: &[],
        reply: &["tune:0,false;"],
    },
    Row {
        text: "drive:0;",
        arm: "Answer",
        calls: &[],
        reply: &["drive:0,50;"],
    },
    Row {
        text: "drive;",
        arm: "Answer",
        calls: &[],
        reply: &["drive:0,50;"],
    },
    Row {
        text: "tune_drive:0;",
        arm: "Answer",
        calls: &[],
        reply: &["tune_drive:0,20;"],
    },
    Row {
        text: "split_enable:0;",
        arm: "Answer",
        calls: &[],
        reply: &["split_enable:0,false;"],
    },
    Row {
        text: "split_enable:false;",
        arm: "Answer",
        calls: &[],
        reply: &["split_enable:0,false;"],
    },
    Row {
        text: "split_enable:0,false;",
        arm: "Answer",
        calls: &[],
        reply: &["split_enable:0,false;"],
    },
    Row {
        text: "rx_mute:0,false;",
        arm: "Answer",
        calls: &[],
        reply: &["rx_mute:0,false;"],
    },
    Row {
        text: "rx_enable:0,true;",
        arm: "Answer",
        calls: &[],
        reply: &["rx_enable:0,true;"],
    },
    Row {
        text: "rx_enable:0;",
        arm: "Answer",
        calls: &[],
        reply: &["rx_enable:0,true;"],
    },
    Row {
        text: "rit_offset:0;",
        arm: "Answer",
        calls: &[],
        reply: &["rit_offset:0,0;"],
    },
    Row {
        text: "rx_channel_enable:0,1;",
        arm: "Answer",
        calls: &[],
        reply: &["rx_channel_enable:0,1,false;"],
    },
    Row {
        text: "vfo_lock:0,0;",
        arm: "Answer",
        calls: &[],
        reply: &["vfo_lock:0,0,false;"],
    },
    Row {
        text: "rx_smeter:0,0;",
        arm: "Answer",
        calls: &[],
        reply: &["rx_smeter:0,0,-73.5;"],
    },
    Row {
        text: "cw_macros_speed;",
        arm: "Answer",
        calls: &[],
        reply: &["cw_macros_speed:25;"],
    },
    Row {
        text: "cw_terminal:false;",
        arm: "Answer",
        calls: &[],
        reply: &["cw_terminal:false;"],
    },
    Row {
        text: "mute;",
        arm: "Answer",
        calls: &[],
        reply: &["mute:false;"],
    },
    Row {
        text: "tx_stream_audio_buffering:50;",
        arm: "Answer",
        calls: &[],
        reply: &["tx_stream_audio_buffering:100;"],
    },
    Row {
        text: "audio_samplerate:24000;",
        arm: "Format",
        calls: &[],
        reply: &["audio_samplerate:24000;"],
    },
    Row {
        text: "audio_stream_sample_type:int16;",
        arm: "Format",
        calls: &[],
        reply: &["audio_stream_sample_type:int16;"],
    },
    Row {
        text: "audio_stream_channels:1;",
        arm: "Format",
        calls: &[],
        reply: &["audio_stream_channels:1;"],
    },
    Row {
        text: "audio_stream_samples:512;",
        arm: "Format",
        calls: &[],
        reply: &["audio_stream_samples:512;"],
    },
    Row {
        text: "audio_stream_samples:4096;",
        arm: "Format",
        calls: &[],
        reply: &["audio_stream_samples:2048;"],
    },
    Row {
        text: "rx_sensors_enable:true,500;",
        arm: "Sensors",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "tx_sensors_enable:false;",
        arm: "Sensors",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "vfo:0,0,14075000;",
        arm: "Dial",
        calls: &["SetDial(14075000)"],
        reply: &[
            "vfo:0,0,14075000;",
            "vfo:0,1,14075000;",
            "dds:0,14075000;",
            "tx_frequency:14075000;",
        ],
    },
    Row {
        text: "vfo:0,0,60000000;",
        arm: "Dial",
        calls: &[],
        reply: &[
            "vfo:0,0,14074000;",
            "vfo:0,1,14074000;",
            "dds:0,14074000;",
            "tx_frequency:14074000;",
        ],
    },
    Row {
        text: "vfo:0,1,14074000;",
        arm: "SubDial",
        calls: &[],
        reply: &["vfo:0,1,14074000;"],
    },
    Row {
        text: "vfo:0,1,14080000;",
        arm: "SubDial",
        calls: &[],
        reply: &["vfo:0,1,14074000;"],
    },
    Row {
        text: "modulation:0,usb;",
        arm: "Mode",
        calls: &["SetMode(usb)"],
        reply: &["modulation:0,usb;"],
    },
    Row {
        text: "modulation:0,0,lsb;",
        arm: "Mode",
        calls: &["SetMode(lsb)"],
        reply: &["modulation:0,lsb;"],
    },
    Row {
        text: "modulation:0,wfm;",
        arm: "Mode",
        calls: &[],
        reply: &["modulation:0,digu;"],
    },
    Row {
        text: "drive:0,25;",
        arm: "Drive",
        calls: &["SetDrive(25)"],
        reply: &["drive:0,25;"],
    },
    Row {
        text: "tune_drive:0,10;",
        arm: "TuneDrive",
        calls: &["SetTuneDrive(10)"],
        reply: &["tune_drive:0,10;"],
    },
    Row {
        text: "trx:0,true,tci;",
        arm: "Key",
        calls: &["Key(Tci)"],
        reply: &["trx:0,true;"],
    },
    Row {
        text: "trx:0,true;",
        arm: "Key",
        calls: &["Key(Mic(Selected))"],
        reply: &["trx:0,true;"],
    },
    Row {
        text: "trx:0,true,mic2;",
        arm: "Key",
        calls: &["Key(Mic(Mic2))"],
        reply: &["trx:0,true;"],
    },
    Row {
        text: "trx:0,false;",
        arm: "Release",
        calls: &["Release"],
        reply: &["trx:0,false;"],
    },
    Row {
        text: "trx:0,false,tci;",
        arm: "Release",
        calls: &["Release"],
        reply: &["trx:0,false;"],
    },
    Row {
        text: "tune:0,true;",
        arm: "TuneOn",
        calls: &["TuneOn"],
        reply: &["tune:0,true;"],
    },
    Row {
        text: "tune:0,false;",
        arm: "TuneOff",
        calls: &["TuneOff"],
        reply: &["tune:0,false;"],
    },
    Row {
        text: "stop;",
        arm: "EndOwn",
        calls: &["EndOwn"],
        reply: &[],
    },
    Row {
        text: "audio_start:0;",
        arm: "AudioStart",
        calls: &["AudioStart"],
        reply: &["audio_start:0;"],
    },
    Row {
        text: "audio_stop:0;",
        arm: "AudioStop",
        calls: &["AudioStop"],
        reply: &["audio_stop:0;"],
    },
    Row {
        text: "cw_macros:0,CQ CQ DE K1ABC;",
        arm: "Cw",
        calls: &["Cw"],
        reply: &[],
    },
    Row {
        text: "cw_msg:0,TU,K1ABC,5NN;",
        arm: "Cw",
        calls: &["Cw"],
        reply: &[],
    },
    Row {
        text: "cw_macros_stop;",
        arm: "CwStop",
        calls: &["CwStop"],
        reply: &[],
    },
    Row {
        text: "cw_macros_speed:30;",
        arm: "CwSpeed",
        calls: &["CwSpeed"],
        reply: &["cw_macros_speed:25;"],
    },
    Row {
        text: "cw_macros_speed_up:2;",
        arm: "CwSpeed",
        calls: &["CwSpeed"],
        reply: &["cw_macros_speed:25;"],
    },
    Row {
        text: "spot:K1ABC,FT8,14074000,4294901760,hello;",
        arm: "Spot",
        calls: &["Spot(K1ABC)"],
        reply: &[],
    },
    Row {
        text: "spot_delete:K1ABC;",
        arm: "SpotDelete",
        calls: &["SpotDelete(K1ABC)"],
        reply: &[],
    },
    Row {
        text: "spot_clear;",
        arm: "SpotClear",
        calls: &["SpotClear"],
        reply: &[],
    },
    Row {
        text: "dds:0,14000000;",
        arm: "Refuse",
        calls: &[],
        reply: &["dds:0,14074000;"],
    },
    Row {
        text: "if:0,0,1500;",
        arm: "Refuse",
        calls: &[],
        reply: &["if:0,0,0;"],
    },
    Row {
        text: "split_enable:0,true;",
        arm: "Refuse",
        calls: &[],
        reply: &["split_enable:0,false;"],
    },
    Row {
        text: "split_enable:true;",
        arm: "Refuse",
        calls: &[],
        reply: &["split_enable:0,false;"],
    },
    Row {
        text: "rit_enable:0,true;",
        arm: "Refuse",
        calls: &[],
        reply: &["rit_enable:0,false;"],
    },
    Row {
        text: "rx_nb_enable:0,true;",
        arm: "Refuse",
        calls: &[],
        reply: &["rx_nb_enable:0,false;"],
    },
    Row {
        text: "xit_offset:0,500;",
        arm: "Refuse",
        calls: &[],
        reply: &["xit_offset:0,0;"],
    },
    Row {
        text: "rx_channel_enable:0,1,true;",
        arm: "Refuse",
        calls: &[],
        reply: &["rx_channel_enable:0,1,false;"],
    },
    Row {
        text: "rx_enable:0,false;",
        arm: "Refuse",
        calls: &[],
        reply: &["rx_enable:0,true;"],
    },
    Row {
        text: "tx_enable:0,true;",
        arm: "Refuse",
        calls: &[],
        reply: &["tx_enable:0,false;"],
    },
    Row {
        text: "cw_terminal:true;",
        arm: "Refuse",
        calls: &[],
        reply: &["cw_terminal:false;"],
    },
    Row {
        text: "mon_enable:true;",
        arm: "Refuse",
        calls: &[],
        reply: &["mon_enable:false;"],
    },
    Row {
        text: "trx:0,true,line;",
        arm: "Refuse",
        calls: &[],
        reply: &["trx:0,false;"],
    },
    Row {
        text: "trx:1,true,tci;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "trx:1,false;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "vfo:1,0,14074000;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "vfo:0,2,14074000;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "modulation:0,1,usb;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "rx_filter_band:0,100,2900;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "volume:-10;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "agc_mode:0,fast;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "digu_offset:1500;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "cw_macros_delay:100;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "iq_start:0;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "iq_samplerate:96000;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "line_out_start:0;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "line_out_recorder_save:0,/tmp/x.wav;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "set_in_focus;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "keyer:0,true,100;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "keyer:0,false;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
    Row {
        text: "cw_msg:K1ABD;",
        arm: "Ignore",
        calls: &[],
        reply: &[],
    },
];

/// A backend call, named as the rows name it.
fn named(call: &Call) -> String {
    match call {
        Call::SetDial(_, hz) => format!("SetDial({hz})"),
        Call::SetMode(_, m) => format!("SetMode({m})"),
        Call::SetDrive(_, l) => format!("SetDrive({l})"),
        Call::SetTuneDrive(_, l) => format!("SetTuneDrive({l})"),
        Call::Key(_, kind, _) => format!("Key({kind:?})"),
        Call::Release(_) => "Release".into(),
        Call::TuneOn(_) => "TuneOn".into(),
        Call::TuneOff(_) => "TuneOff".into(),
        Call::EndOwn(_) => "EndOwn".into(),
        Call::AudioStart(_) => "AudioStart".into(),
        Call::AudioStop(_) => "AudioStop".into(),
        Call::Cw(..) => "Cw".into(),
        Call::CwStop(_) => "CwStop".into(),
        Call::CwSpeed(..) => "CwSpeed".into(),
        Call::Spot(_, call) => format!("Spot({call})"),
        Call::SpotDelete(_, call) => format!("SpotDelete({call})"),
        Call::SpotClear(_) => "SpotClear".into(),
        other => format!("{other:?}"),
    }
}

/// Run one command against a fresh session: the backend's calls, and the lines the sender got.
fn run_one(text: &str, mock: &Mock) -> (Action, Vec<String>, Vec<String>) {
    let model = Mutex::new(snapshot());
    let session = Mutex::new(Session::default());
    let lines = Lines::default();
    let client = ClientId::new(1);
    let action = classify(text);
    let fx = dispatch::run(
        action.clone(),
        &Cx {
            client,
            backend: mock,
            model: &model,
            session: &session,
            sink: &lines,
            gate: &Mutex::new(false),
        },
    );
    let mut sent = lines.0.into_inner().expect("lines");
    // What every program is told includes the sender, always.
    let mut s = session.into_inner().expect("session");
    for o in fx.all {
        sent.extend(s.echo(o, true).into_iter().map(ServerText::into_string));
    }
    // The log lines go to the backend, as `conn::run` hands them on.
    for event in fx.events {
        mock.event(event);
    }
    (action, mock.calls().iter().map(named).collect(), sent)
}

#[test]
fn every_arm_of_the_table_does_what_the_table_says() {
    let mut covered = std::collections::BTreeSet::new();
    for row in ROWS {
        let mock = Mock::default();
        let (action, calls, sent) = run_one(row.text, &mock);
        assert_eq!(arm(&action), row.arm, "{}: {action:?}", row.text);
        assert_eq!(calls, row.calls, "{}: the backend", row.text);
        assert_eq!(sent, row.reply, "{}: the reply", row.text);
        for line in &sent {
            assert_eq!(strict(line), Ok(()), "{}", row.text);
        }
        covered.insert(row.arm);
    }
    assert_eq!(
        covered.into_iter().collect::<Vec<_>>(),
        {
            let mut all = ARMS.to_vec();
            all.sort_unstable();
            all
        },
        "every arm of the table has a row"
    );
}

/// A refusal from the backend reaches the program as the command's own answer, with the value in
/// force, and the log names the gate.
#[test]
fn a_backend_refusal_is_answered_with_the_value_in_force() {
    for (text, set, want) in [
        (
            "vfo:0,0,14075000;",
            "dial",
            &[
                "vfo:0,0,14074000;",
                "vfo:0,1,14074000;",
                "dds:0,14074000;",
                "tx_frequency:14074000;",
            ][..],
        ),
        ("modulation:0,usb;", "mode", &["modulation:0,digu;"][..]),
        ("drive:0,90;", "drive", &["drive:0,50;"][..]),
        ("tune:0,true;", "transmit", &["tune:0,false;"][..]),
        ("trx:0,true,tci;", "transmit", &["trx:0,false;"][..]),
        ("audio_start:0;", "audio", &[][..]),
        ("cw_macros_speed:40;", "cw", &["cw_macros_speed:25;"][..]),
    ] {
        let mock = Mock::default();
        {
            let mut a = mock.answers();
            match set {
                "dial" => a.dial = Err(Refusal::OnAir),
                "mode" => a.mode = Err(Refusal::OnAir),
                "drive" => a.drive = Err(Refusal::OnAir),
                "transmit" => a.transmit = Err(Refusal::OnAir),
                "audio" => a.audio = Err(Refusal::OnAir),
                _ => a.cw_speed = Err(Refusal::OnAir),
            }
        }
        let (_, _, sent) = run_one(text, &mock);
        assert_eq!(sent, want, "{text}");
        assert!(
            mock.events().contains(&Event::Refusal {
                client: ClientId::new(1),
                refusal: Refusal::OnAir
            }),
            "{text}: the gate is named"
        );
    }
}

/// A program's stream settings change what it is sent, and its transmit audio is read with them.
#[test]
fn stream_settings_are_the_programs_own() {
    let mut session = Session::default();
    let (line, refused) =
        session.set_format(FormatSet::Rate(AudioRate::new(12_000).expect("a rate")));
    assert_eq!(line.as_str(), "audio_samplerate:12000;");
    assert_eq!(refused, None);
    let (line, refused) = session.set_format(FormatSet::Samples(99));
    assert_eq!(line.as_str(), "audio_stream_samples:2048;");
    assert_eq!(refused, Some(Refused::Samples));
    assert_eq!(session.format.rate.hz(), 12_000);
    assert_eq!(AudioFormat::default().rate.hz(), 48_000);
    // Sensor intervals are clamped into 30–1000 ms; absent, 200 ms.
    session.set_sensors(false, true, Some(5));
    assert_eq!(session.rx_sensors, Some(Duration::from_millis(30)));
    session.set_sensors(true, true, Some(60_000));
    assert_eq!(session.tx_sensors, Some(Duration::from_millis(1000)));
    session.set_sensors(true, true, None);
    assert_eq!(session.tx_sensors, Some(Duration::from_millis(200)));
    session.set_sensors(false, false, Some(100));
    assert_eq!(session.rx_sensors, None);
    // The values a refused switch reports are off, whatever was asked.
    assert_eq!(
        session
            .answer(Value::Flag(Flag::Lock), &snapshot(), ClientId::new(1))
            .map(ServerText::into_string)
            .as_deref(),
        Some("lock:0,false;")
    );
    assert_eq!(
        session.answer(
            Value::Vfo(Channel::new(2).expect("2")),
            &snapshot(),
            ClientId::new(1)
        ),
        None,
        "no channel C"
    );
}

// ── Properties ──────────────────────────────────────────────────────────────────────────────

/// Every command name either side sends, and some no one does.
fn a_name() -> impl Strategy<Value = &'static str> {
    prop::sample::select(
        [
            "start",
            "stop",
            "dds",
            "if",
            "vfo",
            "modulation",
            "trx",
            "tune",
            "drive",
            "tune_drive",
            "rit_enable",
            "xit_enable",
            "split_enable",
            "rit_offset",
            "xit_offset",
            "rx_channel_enable",
            "rx_filter_band",
            "cw_macros_speed",
            "cw_macros_delay",
            "cw_keyer_speed",
            "volume",
            "mute",
            "rx_mute",
            "rx_volume",
            "rx_balance",
            "mon_volume",
            "mon_enable",
            "agc_mode",
            "agc_gain",
            "rx_nb_enable",
            "rx_nb_param",
            "rx_bin_enable",
            "rx_nr_enable",
            "rx_anc_enable",
            "rx_anf_enable",
            "rx_apf_enable",
            "rx_dse_enable",
            "rx_nf_enable",
            "lock",
            "sql_enable",
            "sql_level",
            "digl_offset",
            "digu_offset",
            "tx_enable",
            "cw_macros_speed_up",
            "cw_macros_speed_down",
            "spot",
            "spot_delete",
            "spot_clear",
            "iq_samplerate",
            "audio_samplerate",
            "iq_start",
            "iq_stop",
            "audio_start",
            "audio_stop",
            "line_out_start",
            "line_out_stop",
            "line_out_recorder_start",
            "line_out_recorder_save",
            "line_out_recorder_break",
            "audio_stream_sample_type",
            "audio_stream_channels",
            "audio_stream_samples",
            "tx_stream_audio_buffering",
            "clicked_on_spot",
            "rx_clicked_on_spot",
            "tx_footswitch",
            "tx_frequency",
            "app_focus",
            "set_in_focus",
            "keyer",
            "rx_sensors_enable",
            "tx_sensors_enable",
            "rx_sensors",
            "tx_sensors",
            "vfo_lock",
            "rx_channel_sensors",
            "cw_macros",
            "cw_terminal",
            "cw_macros_empty",
            "cw_msg",
            "callsign_send",
            "cw_macros_stop",
            "rx_enable",
            "rx_smeter",
            "tx_power",
            "tx_swr",
            "vfo_limits",
            "protocol",
            "ready",
            "device",
            "no_such_command",
        ]
        .to_vec(),
    )
}

fn an_arg() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("0".to_string()),
        Just("1".to_string()),
        Just("true".to_string()),
        Just("false".to_string()),
        Just("tci".to_string()),
        Just("mic1".to_string()),
        Just("usb".to_string()),
        Just("14074000".to_string()),
        Just("-5".to_string()),
        Just("float32".to_string()),
        Just("48000".to_string()),
        "[A-Za-z0-9 ./^~*-]{0,8}",
    ]
}

fn a_command() -> impl Strategy<Value = String> {
    (
        a_name(),
        prop::option::of(prop::collection::vec(an_arg(), 0..6)),
    )
        .prop_map(|(name, args)| match args {
            None => format!("{name};"),
            Some(args) => format!("{name}:{};", args.join(",")),
        })
}

/// The commands that may ask the backend to transmit, and only on receiver 0.
fn may_transmit(text: &str) -> bool {
    let body = text.trim_end_matches(';');
    let (name, args) = body.split_once(':').unwrap_or((body, ""));
    let args: Vec<&str> = args.split(',').map(str::trim).collect();
    let zero = args.first().is_some_and(|a| *a == "0");
    match name.to_ascii_lowercase().as_str() {
        "trx" | "tune" => zero && args.get(1).is_some_and(|a| a.eq_ignore_ascii_case("true")),
        "cw_macros" | "cw_msg" => zero,
        _ => false,
    }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2048, ..ProptestConfig::default() })]

    /// No raw command funnel: whatever a program sends, the only calls that can transmit are a key,
    /// a Tune and CW, each from its own command on receiver 0. Everything the sender is told passes
    /// the strict validator.
    #[test]
    fn only_a_key_tune_or_cw_command_can_ask_to_transmit(text in a_command()) {
        let Some(Ok(inbound)) = tciwire::inbound(&text, &EXPERT_SDR3).next() else {
            return Ok(());
        };
        let mock = Mock::default();
        let model = Mutex::new(snapshot());
        let session = Mutex::new(Session::default());
        let lines = Lines::default();
        let fx = dispatch::run(
            dispatch::classify(inbound),
            &Cx {
                client: ClientId::new(1),
                backend: &mock,
                model: &model,
                session: &session,
                sink: &lines,
                gate: &Mutex::new(false),
            },
        );
        let calls = mock.calls();
        if calls.iter().any(Call::can_transmit) {
            prop_assert!(may_transmit(&text), "{} asked to transmit: {:?}", text, calls);
        }
        let mut sent = lines.0.into_inner().expect("lines");
        let mut s = session.into_inner().expect("session");
        for o in fx.all {
            sent.extend(s.echo(o, true).into_iter().map(ServerText::into_string));
        }
        for line in sent {
            prop_assert_eq!(strict(&line), Ok(()));
        }
    }

    /// The table never panics on any text, and nothing it reads becomes a key unless it says so.
    #[test]
    fn any_text_runs_without_a_panic(text in any::<String>()) {
        let mock = Mock::default();
        let model = Mutex::new(snapshot());
        let session = Mutex::new(Session::default());
        let lines = Lines::default();
        for inbound in tciwire::inbound(&text, &EXPERT_SDR3).flatten() {
            let _ = dispatch::run(
                dispatch::classify(inbound),
                &Cx {
                    client: ClientId::new(1),
                    backend: &mock,
                    model: &model,
                    session: &session,
                    sink: &lines,
                    gate: &Mutex::new(false),
                },
            );
        }
    }
}

/// The reader itself, over a socket: any text and any binary message, and the connection still
/// serves after each (a panic in the reader would drop it).
#[test]
fn any_message_over_a_socket_leaves_the_program_served() {
    let b = Bench::new();
    let c = std::cell::RefCell::new(b.client());
    let mut runner = proptest::test_runner::TestRunner::new(ProptestConfig {
        cases: 96,
        ..ProptestConfig::default()
    });
    let strategy = (
        prop_oneof![
            a_command(),
            "[ -~]{0,200}",
            any::<String>().prop_map(|s| s.chars().take(300).collect()),
        ],
        prop::collection::vec(any::<u8>(), 0..300),
    );
    runner
        .run(&strategy, |(text, bytes)| {
            let mut c = c.borrow_mut();
            // Only what cannot key: a key or a Tune would need a release between cases.
            if !may_transmit(&text) {
                c.send(&text);
            }
            c.send_binary(bytes);
            c.send("trx:0;");
            prop_assert!(c.until("trx:0,false;"), "after {:?}", text);
            Ok(())
        })
        .expect("the program is still served");
    // Garbage before any handshake does not stop the server taking the next one.
    let mut s = TcpStream::connect((Ipv4Addr::LOCALHOST, b.port)).expect("connect");
    let _ = s.write_all(&[0xff, 0x00, 0x13, 0x37, b'\r', b'\n', b'\r', b'\n']);
    drop(s);
    assert!(Client::connect(b.port).is_ok());
    assert!(!b.mock.calls().iter().any(Call::can_transmit));
}

#[test]
fn the_limits_are_the_designs() {
    let l = Limits::default();
    assert_eq!(
        (l.clients, l.handshake, l.text_message, l.binary_message),
        (8, Duration::from_secs(2), 16 * 1024, 32 * 1024 + 64)
    );
    assert_eq!((l.commands, l.command_bytes, l.args), (64, 1024, 16));
    assert_eq!(
        (l.rate, l.burst, l.flood, l.garbage),
        (100, 400, Duration::from_secs(10), 3)
    );
    assert_eq!((l.outbox, l.text_backlog), (1024 * 1024, 256 * 1024));
    assert_eq!(
        (l.ping, l.silence, l.keyed_ping, l.keyed_silence),
        (
            Duration::from_secs(5),
            Duration::from_secs(15),
            Duration::from_secs(1),
            Duration::from_secs(3)
        )
    );
    assert_eq!(l.tx_audio_speed, 2);
    assert_eq!(DEFAULT_PORT, 40001);
    assert_eq!(PROTOCOL, ("ExpertSDR3", "2.0"));
}
