//! JS8Call's API for other programs on THIS computer: the listeners. TCP on 127.0.0.1 (JS8Call's
//! port 2442) and UDP datagrams from 127.0.0.1 to 127.0.0.1 (JS8Call's 2242), both OFF until the
//! operator turns one on (Settings ▸ Digital ▸ JS8). What each line says is
//! `tempo_app::engine::js8_api`; this file moves lines, and holds the limits.
//!
//! WHY IT IS SHAPED THIS WAY (a listener is a door; these are the locks):
//! - LOOPBACK ONLY, whatever is configured. The settings hold a port and nothing else, every bind
//!   names 127.0.0.1 ([`bind_tcp`], [`bind_udp`]), every datagram goes to 127.0.0.1, and a peer
//!   that is not loopback is dropped on arrival anyway. (JS8Call's own UDP socket binds every
//!   interface, MessageClient.cpp:169-171, so its requests can come from the network; these cannot.)
//! - READ-ONLY BY CONSTRUCTION. A request is answered from a snapshot copied out of the Engine once
//!   a tick ([`Hub::snapshot`]): the threads that read requests hold no Engine and take no lock, so
//!   what a request can do is bounded by `answer`'s signature — it borrows a copy and returns a
//!   line. Everything that is not a read is dropped unanswered, as JS8Call drops a request it does
//!   not accept.
//! - REQUESTS ONLY WHEN ACCEPTED. Each side has JS8Call's "Accept requests" switch, off by
//!   default ([`Hub::accepts`]): off, the side still sends what the station says, and a request
//!   on it goes no further, as in JS8Call (mainwindow.cpp:6717-6739). A TCP line that is no
//!   request is told why (`API.ERROR`) whatever the switch says, as JS8Call's server tells it
//!   before the switch; a datagram that is no request is told nothing, as in JS8Call.
//! - BOUNDED. One TCP connection (JS8Call's own limit; the next is told "Connections Full" and
//!   closed), a request line or datagram past `MAX_REQUEST_BYTES` is not read (a TCP line past it
//!   closes the connection), each client's outgoing queue is bounded and a client that stops reading
//!   is dropped, and the engine caps what waits between ticks.
//! - NOT REMOTE. The hosted Remote and the LAN page reach neither these sockets nor these settings:
//!   the settings are off the Remote document's allow-list and its write allow-list
//!   (`remote_service/operations/js8_api_tests.rs`).
//!
//! One manager thread polls the two switches once a tick and hot-applies them, in the CAT broker's
//! mold: turning a side on starts it with no restart, off stops it, a new port rebinds.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use tempo_app::engine::engine_lock;
use tempo_app::engine::js8_api::{
    self, answer, read_tcp_line, Js8ApiSnapshot, Outgoing, Request, StateWatch, TcpLine, Via,
    MAX_REQUEST_BYTES, MAX_TCP_CONNECTIONS, PING_INTERVAL_MS,
};

use crate::SharedEngine;

/// How often the manager looks at the engine while a side is on: the events' latency.
const TICK: Duration = Duration::from_millis(250);
/// …and while both are off: only the switches are read.
const IDLE_TICK: Duration = Duration::from_millis(1000);
/// Lines waiting for one TCP client; a client this far behind has stopped reading.
const CLIENT_QUEUE: usize = 512;
/// A client that takes longer than this to accept one line has stopped reading.
const WRITE_TIMEOUT: Duration = Duration::from_secs(5);
/// A bind that failed on a port is tried again on that port after this (a new port at once).
const BIND_RETRY: Duration = Duration::from_secs(30);

fn now_ms() -> u64 {
    tempo_core::timing::now_unix_ms() as u64
}

/// The TCP side's listener: 127.0.0.1, always.
fn bind_tcp(port: u16) -> std::io::Result<TcpListener> {
    let at = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    #[cfg(test)]
    if let Some(held) = tests::held(at) {
        return Ok(held);
    }
    TcpListener::bind(at)
}

/// The UDP side's own socket: 127.0.0.1 on a port the system picks; datagrams go out from it to
/// the configured port and requests come back to it.
fn bind_udp() -> std::io::Result<UdpSocket> {
    UdpSocket::bind((Ipv4Addr::LOCALHOST, 0))
}

/// One connected TCP program.
struct Client {
    tx: SyncSender<Arc<str>>,
    alive: Arc<AtomicBool>,
    /// Lines handed to the writer and not yet written.
    queued: Arc<AtomicUsize>,
    /// A handle on the socket, to close it from outside its threads.
    socket: TcpStream,
}

impl Client {
    /// Queue `line` (newline included). A client whose queue is full has stopped reading: it is
    /// closed, never waited on.
    fn send(&self, line: &Arc<str>) {
        if !self.alive.load(Ordering::Relaxed) {
            return;
        }
        self.queued.fetch_add(1, Ordering::Relaxed);
        if self.tx.try_send(line.clone()).is_err() {
            self.queued.fetch_sub(1, Ordering::Relaxed);
            self.close();
        }
    }

    fn close(&self) {
        self.alive.store(false, Ordering::Relaxed);
        let _ = self.socket.shutdown(Shutdown::Both);
    }
}

/// The UDP side: its socket and where datagrams go.
struct Udp {
    socket: UdpSocket,
    to: SocketAddr,
    /// JS8Call never sends the same datagram twice in a row (MessageClient.cpp:111-121).
    last: Option<String>,
}

/// What the servers share: the clients, the UDP side, the snapshot reads are answered from, and
/// each side's "Accept requests" switch.
#[derive(Default)]
pub(crate) struct Hub {
    clients: Mutex<Vec<Client>>,
    udp: Mutex<Option<Udp>>,
    snapshot: Mutex<Arc<Js8ApiSnapshot>>,
    tcp_accept: AtomicBool,
    udp_accept: AtomicBool,
}

impl Hub {
    /// Whether a request on `via` goes on to be answered: that side's "Accept requests" switch,
    /// as the manager last read it.
    fn accepts(&self, via: Via) -> bool {
        match via {
            Via::Tcp => self.tcp_accept.load(Ordering::Relaxed),
            Via::Udp => self.udp_accept.load(Ordering::Relaxed),
        }
    }

    fn set_accept(&self, tcp: bool, udp: bool) {
        self.tcp_accept.store(tcp, Ordering::Relaxed);
        self.udp_accept.store(udp, Ordering::Relaxed);
    }

    fn snapshot(&self) -> Arc<Js8ApiSnapshot> {
        self.snapshot
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }

    fn set_snapshot(&self, snap: Js8ApiSnapshot) {
        *self.snapshot.lock().unwrap_or_else(|e| e.into_inner()) = Arc::new(snap);
    }

    /// A message the station sends: as a datagram while UDP is on, and to every TCP client when
    /// its id is 0 or below, JS8Call's rule (`MessageServer::send`, MessageServer.cpp:107-114).
    fn send(&self, out: &Outgoing) {
        self.send_udp(&out.line);
        if out.id <= 0 {
            let line: Arc<str> = format!("{}\n", out.line).into();
            for c in self
                .clients
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
            {
                c.send(&line);
            }
        }
    }

    fn send_udp(&self, line: &str) {
        let mut udp = self.udp.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(u) = udp.as_mut() {
            if u.last.as_deref() != Some(line) {
                // Nothing listening is not an error worth a word: JS8Call stays quiet too.
                let _ = u.socket.send_to(line.as_bytes(), u.to);
                u.last = Some(line.to_string());
            }
        }
    }

    fn set_udp(&self, udp: Option<Udp>) {
        *self.udp.lock().unwrap_or_else(|e| e.into_inner()) = udp;
    }

    /// Close every TCP client.
    fn close_clients(&self) {
        for c in self
            .clients
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            c.close();
        }
    }

    /// Take a new TCP connection, or tell it there is no room and close it.
    fn admit(self: &Arc<Self>, stream: TcpStream, peer: SocketAddr) {
        if !peer.ip().to_canonical().is_loopback() {
            let _ = stream.shutdown(Shutdown::Both);
            return;
        }
        let mut clients = self.clients.lock().unwrap_or_else(|e| e.into_inner());
        clients.retain(|c| c.alive.load(Ordering::Relaxed));
        if clients.len() >= MAX_TCP_CONNECTIONS {
            drop(clients);
            let mut s = stream;
            let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
            let full = js8_api::connections_full(now_ms());
            let _ = s.write_all(format!("{}\n", full.line).as_bytes());
            let _ = s.shutdown(Shutdown::Both);
            return;
        }
        let (Ok(reader), Ok(mut writer), Ok(socket)) =
            (stream.try_clone(), stream.try_clone(), stream.try_clone())
        else {
            return;
        };
        let _ = stream.set_nodelay(true);
        let _ = writer.set_write_timeout(Some(WRITE_TIMEOUT));
        let (tx, rx) = sync_channel::<Arc<str>>(CLIENT_QUEUE);
        let client = Client {
            tx,
            alive: Arc::new(AtomicBool::new(true)),
            queued: Arc::new(AtomicUsize::new(0)),
            socket,
        };
        let (alive, queued) = (client.alive.clone(), client.queued.clone());
        let writes = std::thread::Builder::new()
            .name("js8-api-tcp-out".into())
            .spawn(move || {
                for line in rx {
                    let ok = writer.write_all(line.as_bytes()).is_ok();
                    queued.fetch_sub(1, Ordering::Relaxed);
                    if !ok {
                        alive.store(false, Ordering::Relaxed);
                        let _ = writer.shutdown(Shutdown::Both);
                        break;
                    }
                }
            });
        if writes.is_err() {
            return;
        }
        let hub = Arc::clone(self);
        let (tx, alive, closer) = (
            client.tx.clone(),
            client.alive.clone(),
            client.socket.try_clone(),
        );
        clients.push(client);
        drop(clients);
        let _ = std::thread::Builder::new()
            .name("js8-api-tcp-in".into())
            .spawn(move || {
                read_requests(reader, &hub, &tx);
                alive.store(false, Ordering::Relaxed);
                if let Ok(s) = closer {
                    let _ = s.shutdown(Shutdown::Both);
                }
            });
    }

    /// The program is closing: `STATION.CLOSING` to everyone, the UDP side's `CLOSE`, and a short
    /// wait for the TCP writers to put the line on the wire.
    fn closing(&self) {
        self.send(&js8_api::closing());
        self.send_udp(&js8_api::close(now_ms()).line);
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline
            && self
                .clients
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .iter()
                .any(|c| c.alive.load(Ordering::Relaxed) && c.queued.load(Ordering::Relaxed) > 0)
        {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

/// A TCP client's requests, one per line, until it closes or sends a line past the cap. A line
/// that is no request is told why, to this client. While "Accept TCP requests" is on, each read
/// is answered to this client from the hub's snapshot (and, as JS8Call answers every request on
/// every side that is on, as a datagram while UDP is on); everything else goes unanswered.
fn read_requests(mut stream: TcpStream, hub: &Hub, tx: &SyncSender<Arc<str>>) {
    let mut pending: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        let n = match stream.read(&mut chunk) {
            Ok(0) | Err(_) => return,
            Ok(n) => n,
        };
        pending.extend_from_slice(&chunk[..n]);
        while let Some(end) = pending.iter().position(|&b| b == b'\n') {
            if end > MAX_REQUEST_BYTES {
                return;
            }
            let line: Vec<u8> = pending.drain(..=end).collect();
            let req = match read_tcp_line(&line[..end], now_ms()) {
                TcpLine::Blank => continue,
                TcpLine::Error(why) => {
                    if tx.try_send(format!("{}\n", why.line).into()).is_err() {
                        return;
                    }
                    continue;
                }
                TcpLine::Request(req) => req,
            };
            if !hub.accepts(Via::Tcp) {
                continue;
            }
            if let Some(out) = answer(&req, Via::Tcp, &hub.snapshot(), now_ms()) {
                if tx.try_send(format!("{}\n", out.line).into()).is_err() {
                    return;
                }
                hub.send_udp(&out.line);
            }
        }
        if pending.len() > MAX_REQUEST_BYTES {
            return;
        }
    }
}

/// The TCP listener, until `stop`.
fn serve_tcp(listener: TcpListener, hub: Arc<Hub>, stop: Arc<AtomicBool>) {
    if listener.set_nonblocking(true).is_err() {
        return;
    }
    while !stop.load(Ordering::Relaxed) {
        match listener.accept() {
            Ok((stream, peer)) => {
                let _ = stream.set_nonblocking(false);
                hub.admit(stream, peer);
            }
            // WouldBlock, or a failed accept (out of handles): the listener is still good.
            Err(_) => std::thread::sleep(Duration::from_millis(100)),
        }
    }
}

/// The UDP side's requests, until `stop`. While "Accept UDP requests" is on, answered as datagrams
/// (and over TCP when the request's id makes it so), as JS8Call answers them; a datagram that is
/// no request is answered nothing, as JS8Call's UDP side reports it to nobody
/// (MessageClient.cpp:63-69).
fn serve_udp(socket: UdpSocket, hub: Arc<Hub>, stop: Arc<AtomicBool>) {
    let _ = socket.set_read_timeout(Some(Duration::from_millis(200)));
    let mut buf = vec![0u8; MAX_REQUEST_BYTES + 1];
    while !stop.load(Ordering::Relaxed) {
        match socket.recv_from(&mut buf) {
            Ok((n, from)) => {
                if n > MAX_REQUEST_BYTES
                    || !from.ip().to_canonical().is_loopback()
                    || !hub.accepts(Via::Udp)
                {
                    continue;
                }
                if let Some(out) = Request::parse(&buf[..n])
                    .and_then(|req| answer(&req, Via::Udp, &hub.snapshot(), now_ms()))
                {
                    hub.send(&out);
                }
            }
            Err(e) => {
                // A timeout is the stop check's turn. Anything else is one datagram's report, not
                // the socket's: Windows answers a datagram to a port nobody listens on with
                // ConnectionReset on the next receive, and one past the buffer with WSAEMSGSIZE.
                if !matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) {
                    std::thread::sleep(Duration::from_millis(50));
                }
            }
        }
    }
}

/// A server that is running: its port and its stop flag.
struct Running {
    port: u16,
    stop: Arc<AtomicBool>,
}

impl Running {
    fn stop(self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// The manager's state between ticks.
struct Manager {
    hub: Arc<Hub>,
    version: &'static str,
    tcp: Option<Running>,
    udp: Option<Running>,
    /// The last TCP bind that failed, so a taken port is neither retried nor reported every tick.
    tcp_failed: Option<(u16, Instant)>,
    watch: StateWatch,
    next_ping_ms: u64,
}

impl Manager {
    fn new(hub: Arc<Hub>, version: &'static str) -> Manager {
        Manager {
            hub,
            version,
            tcp: None,
            udp: None,
            tcp_failed: None,
            watch: StateWatch::default(),
            next_ping_ms: 0,
        }
    }

    /// One look at the engine: hot-apply the switches, publish the snapshot, send what happened.
    /// False while both sides are off.
    fn tick(&mut self, engine: &SharedEngine) -> bool {
        let poll = engine_lock(engine).js8_api_poll();
        let Some(mut poll) = poll else {
            self.stop_all();
            return false;
        };
        // `STATION.VERSION` names the program, as the UDP side's PING does.
        poll.snapshot.version = self.version.to_string();
        let now = now_ms();
        let (before, after) = self.watch.step(&poll.snapshot, now);
        // Published before a side starts and before anything is announced, so the first request
        // a new connection makes, and one a program makes on hearing an event, both read the
        // state the station is in.
        self.hub.set_snapshot(poll.snapshot);
        self.hub.set_accept(poll.tcp_accept, poll.udp_accept);
        self.apply_tcp(poll.tcp_port.filter(|p| *p != 0));
        self.apply_udp(poll.udp_port.filter(|p| *p != 0));
        for out in &before {
            self.hub.send(out);
        }
        for ev in &poll.events {
            self.hub.send(&ev.to_outgoing());
        }
        for out in &after {
            self.hub.send(out);
        }
        if self.udp.is_some() && now >= self.next_ping_ms {
            self.hub.send(&js8_api::ping(self.version, now));
            self.next_ping_ms = now + PING_INTERVAL_MS;
        }
        true
    }

    fn apply_tcp(&mut self, want: Option<u16>) {
        if want == self.tcp.as_ref().map(|r| r.port) {
            return;
        }
        if let Some(r) = self.tcp.take() {
            r.stop();
            self.hub.close_clients();
        }
        let Some(port) = want else {
            self.tcp_failed = None;
            return;
        };
        if let Some((p, at)) = self.tcp_failed {
            if p == port && at.elapsed() < BIND_RETRY {
                return;
            }
        }
        match bind_tcp(port) {
            Ok(listener) => {
                let stop = Arc::new(AtomicBool::new(false));
                let (hub, flag) = (self.hub.clone(), stop.clone());
                let started = std::thread::Builder::new()
                    .name("js8-api-tcp".into())
                    .spawn(move || serve_tcp(listener, hub, flag));
                if started.is_ok() {
                    self.tcp = Some(Running { port, stop });
                    self.tcp_failed = None;
                    crate::conn_log(
                        "JS8 API",
                        "info",
                        format!("JS8Call's API is on 127.0.0.1:{port} (TCP), read-only"),
                    );
                }
            }
            Err(e) => {
                let repeat = self.tcp_failed.is_some_and(|(p, _)| p == port);
                self.tcp_failed = Some((port, Instant::now()));
                if !repeat {
                    let msg = format!(
                        "JS8Call's API could not use TCP port {port} on this computer ({e}). \
                         Another program may be using it; JS8Call itself uses 2442."
                    );
                    tempo_core::applog::error("js8", &msg);
                    crate::conn_log("JS8 API", "error", msg);
                }
            }
        }
    }

    fn apply_udp(&mut self, want: Option<u16>) {
        if want == self.udp.as_ref().map(|r| r.port) {
            return;
        }
        if let Some(r) = self.udp.take() {
            r.stop();
            self.hub.set_udp(None);
        }
        let Some(port) = want else {
            return;
        };
        let socket = match bind_udp() {
            Ok(s) => s,
            Err(e) => {
                tempo_core::applog::error("js8", &format!("JS8Call's API UDP socket: {e}"));
                return;
            }
        };
        let Ok(reader) = socket.try_clone() else {
            return;
        };
        let stop = Arc::new(AtomicBool::new(false));
        let (hub, flag) = (self.hub.clone(), stop.clone());
        let started = std::thread::Builder::new()
            .name("js8-api-udp".into())
            .spawn(move || serve_udp(reader, hub, flag));
        if started.is_ok() {
            self.hub.set_udp(Some(Udp {
                socket,
                to: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
                last: None,
            }));
            self.udp = Some(Running { port, stop });
            // JS8Call pings as soon as its side is up (MessageClient.cpp:176).
            self.next_ping_ms = 0;
            crate::conn_log(
                "JS8 API",
                "info",
                format!("JS8Call's API sends to 127.0.0.1:{port} (UDP), read-only"),
            );
        }
    }

    fn stop_all(&mut self) {
        self.apply_tcp(None);
        self.apply_udp(None);
        self.watch = StateWatch::default();
        self.hub.set_snapshot(Js8ApiSnapshot::default());
        self.hub.set_accept(false, false);
    }
}

/// The hub the running program's manager serves, for [`closing`].
static HUB: OnceLock<Arc<Hub>> = OnceLock::new();

/// Start the manager. Nothing is bound or sent until the operator turns a side on.
pub(crate) fn spawn(engine: SharedEngine) {
    let hub = HUB.get_or_init(|| Arc::new(Hub::default())).clone();
    let _ = std::thread::Builder::new()
        .name("js8-api".into())
        .spawn(move || {
            let mut m = Manager::new(hub, env!("CARGO_PKG_VERSION"));
            loop {
                let on = m.tick(&engine);
                std::thread::sleep(if on { TICK } else { IDLE_TICK });
            }
        });
}

/// The program is closing: say so, as JS8Call does (`STATION.CLOSING`, and the UDP side's
/// `CLOSE`). Bounded at a few hundred milliseconds; a no-op while the API is off.
pub(crate) fn closing() {
    if let Some(hub) = HUB.get() {
        hub.closing();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::io::{BufRead, BufReader};
    use tempo_app::engine::Engine;
    use tempo_app::settings::Settings;

    const WAIT: Duration = Duration::from_secs(5);

    // Listeners a test holds for its manager (`held_port`), each taken by `bind_tcp` when the manager
    // binds exactly its address. Per thread: each test drives its own manager. (A plain comment: doc
    // comments cannot attach through `thread_local!`.)
    std::thread_local! {
        static HELD: std::cell::RefCell<Vec<TcpListener>> = const { std::cell::RefCell::new(Vec::new()) };
    }

    /// The listener this test holds at `at`, for the manager's bind there.
    pub(super) fn held(at: SocketAddr) -> Option<TcpListener> {
        HELD.with(|held| {
            let mut held = held.borrow_mut();
            let i = held.iter().position(|l| l.local_addr().ok() == Some(at))?;
            Some(held.swap_remove(i))
        })
    }

    /// A port on 127.0.0.1 for the manager, bound already and held until the manager binds it,
    /// when `bind_tcp` takes the listener: nothing can be handed the port in between, as anything
    /// could a port the system picked and the test let go. It is below the ephemeral range too,
    /// so once the manager lets it go no `:0` bind can land there either.
    fn held_port() -> u16 {
        let listener =
            crate::lan_client::tests::listener_below_the_ephemeral_range(Ipv4Addr::LOCALHOST);
        let port = listener.local_addr().unwrap().port();
        HELD.with(|held| held.borrow_mut().push(listener));
        port
    }

    /// This computer's address on its network, if it has one. A UDP `connect` sends nothing: it
    /// only asks the routing table which local address would be used (192.0.2.1 is TEST-NET-1).
    fn lan_address() -> Option<std::net::IpAddr> {
        let s = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
        s.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
        let ip = s.local_addr().ok()?.ip();
        (!ip.is_loopback() && !ip.is_unspecified()).then_some(ip)
    }

    /// A JS8 station whose API side given a port is on and accepts requests; the tests of the
    /// Accept switches turn one off.
    fn engine_with(tcp: Option<u16>, udp: Option<u16>) -> SharedEngine {
        let mut s = Settings {
            mycall: "KD9TAW".into(),
            mygrid: "EN52".into(),
            ..Settings::default()
        };
        if let Some(port) = tcp {
            s.js8_api_tcp = true;
            s.js8_api_tcp_port = port;
            s.js8_api_tcp_accept = true;
        }
        if let Some(port) = udp {
            s.js8_api_udp = true;
            s.js8_api_udp_port = port;
            s.js8_api_udp_accept = true;
        }
        let mut e = Engine::with_settings(s);
        e.js8_enter();
        Arc::new(Mutex::new(e))
    }

    fn set(engine: &SharedEngine, f: impl FnOnce(&mut Settings)) {
        let mut e = engine_lock(engine);
        let mut s = e.settings().clone();
        f(&mut s);
        e.apply_settings(s);
    }

    struct Program {
        out: TcpStream,
        lines: BufReader<TcpStream>,
    }

    impl Program {
        fn connect(port: u16) -> Program {
            let out = TcpStream::connect((Ipv4Addr::LOCALHOST, port)).expect("connects");
            out.set_read_timeout(Some(WAIT)).unwrap();
            let lines = BufReader::new(out.try_clone().unwrap());
            Program { out, lines }
        }

        fn say(&mut self, text: &str) {
            self.out.write_all(text.as_bytes()).unwrap();
            self.out.write_all(b"\n").unwrap();
        }

        /// The next line, `None` once the station has closed the connection.
        fn line(&mut self) -> Option<Value> {
            let mut l = String::new();
            match self.lines.read_line(&mut l) {
                Ok(0) => None,
                Ok(_) => Some(serde_json::from_str(&l).expect("a JSON line")),
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                    ) =>
                {
                    panic!("no line within {WAIT:?}")
                }
                // Closed with our bytes unread: a reset, which is a close too.
                Err(_) => None,
            }
        }

        /// Every line up to and including the answer whose `_ID` is `id`.
        fn until_answer(&mut self, id: i64) -> Vec<Value> {
            let mut seen = Vec::new();
            loop {
                let l = self.line().expect("the connection stays open");
                let done = l["params"]["_ID"] == id;
                seen.push(l);
                if done {
                    return seen;
                }
            }
        }

        /// Ask for the callsign under `id` and return everything that came before its answer.
        fn sentinel(&mut self, id: i64) -> Vec<Value> {
            self.say(&format!(
                r#"{{"params":{{"_ID":{id}}},"type":"STATION.GET_CALLSIGN","value":""}}"#
            ));
            let mut seen = self.until_answer(id);
            let answer = seen.pop().unwrap();
            assert_eq!(answer["value"], "KD9TAW");
            seen
        }
    }

    /// A new connection is taken and answered (the slot a closed one held is free again).
    fn served(port: u16, id: i64) -> bool {
        let Ok(mut s) = TcpStream::connect((Ipv4Addr::LOCALHOST, port)) else {
            return false;
        };
        let _ = s.set_read_timeout(Some(WAIT));
        let ask =
            format!(r#"{{"params":{{"_ID":{id}}},"type":"STATION.GET_CALLSIGN","value":""}}"#);
        if s.write_all(format!("{ask}\n").as_bytes()).is_err() {
            return false;
        }
        let mut l = String::new();
        BufReader::new(s).read_line(&mut l).is_ok() && l.contains(&format!(r#""_ID":{id}"#))
    }

    fn served_soon(port: u16, id: i64) -> bool {
        let deadline = Instant::now() + WAIT;
        while Instant::now() < deadline {
            if served(port, id) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn datagram(s: &UdpSocket) -> Option<(Value, SocketAddr)> {
        let mut buf = [0u8; 65_536];
        let (n, from) = s.recv_from(&mut buf).ok()?;
        Some((
            serde_json::from_slice(&buf[..n]).expect("a JSON datagram"),
            from,
        ))
    }

    /// The listener is on 127.0.0.1 whatever port it is given: reachable there, refused at this
    /// computer's own network address on the same port.
    #[test]
    fn the_api_listens_on_loopback_only() {
        let port = held_port();
        let engine = engine_with(Some(port), None);
        let mut m = Manager::new(Arc::new(Hub::default()), "test");
        assert!(m.tick(&engine));
        let mut p = Program::connect(port);
        p.sentinel(1);
        match lan_address() {
            Some(ip) => {
                let lan = TcpStream::connect_timeout(&SocketAddr::new(ip, port), WAIT);
                assert!(lan.is_err(), "reached the API at {ip}:{port}");
                // Positive control: the same address and port reach a listener bound there.
                let open = TcpListener::bind((ip, 0)).unwrap();
                let at = open.local_addr().unwrap();
                assert!(TcpStream::connect_timeout(&at, WAIT).is_ok());
            }
            None => eprintln!("no network address on this computer; loopback half only"),
        }
        let udp = bind_udp().unwrap();
        assert!(udp.local_addr().unwrap().ip().is_loopback());
        m.stop_all();
    }

    /// JS8Call's limit: one program at a time. The next is told "Connections Full" and closed;
    /// the first is still served; once it leaves, another is taken.
    #[test]
    fn one_program_at_a_time() {
        let port = held_port();
        let engine = engine_with(Some(port), None);
        let mut m = Manager::new(Arc::new(Hub::default()), "test");
        m.tick(&engine);
        let mut first = Program::connect(port);
        first.sentinel(1);
        let mut second = Program::connect(port);
        let full = second.line().expect("told why");
        assert_eq!(
            (&full["type"], &full["value"]),
            (&Value::from("API.ERROR"), &Value::from("Connections Full"))
        );
        assert!(second.line().is_none(), "and closed");
        first.sentinel(2);
        drop(first);
        // Its reader sees the close; then the next program is taken.
        assert!(served_soon(port, 3), "the slot never freed");
        m.stop_all();
    }

    /// A request line past the cap closes that connection and nothing else; a line that is no
    /// request is told why (`API.ERROR`), the connection kept: a fuzz loop over the socket, then a
    /// read still answered.
    #[test]
    fn oversize_and_malformed_requests_are_dropped() {
        let port = held_port();
        let engine = engine_with(Some(port), None);
        let mut m = Manager::new(Arc::new(Hub::default()), "test");
        m.tick(&engine);
        let mut p = Program::connect(port);
        let mut x: u32 = 0x2545_f491;
        let mut next = move || {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x
        };
        let mut junk = Vec::new();
        for i in 0..2000 {
            let line: Vec<u8> = match i % 3 {
                0 => (0..(next() % 120)).map(|_| next() as u8).collect(),
                1 => br#"{"params":{"_ID":9"#.to_vec(),
                _ => format!(
                    r#"{{"type":"TX.SEND_MESSAGE","value":"X{i}","params":{{"_ID":{}}}}}"#,
                    1000 + i
                )
                .into_bytes(),
            };
            junk.extend(line.iter().filter(|b| **b != b'\n'));
            junk.push(b'\n');
        }
        // One API.ERROR for each line that is no request (a blank one is passed over, a refused
        // request draws nothing); the 667 half-objects are all such lines.
        let told = junk
            .split(|b| *b == b'\n')
            .filter(|l| matches!(read_tcp_line(l, 0), TcpLine::Error(_)))
            .count();
        assert!(told >= 667, "{told}");
        // Written from another thread while this one reads, as a program would: one that wrote
        // all of this and read nothing back would fill its own queue and be dropped.
        junk.extend_from_slice(br#"{"params":{"_ID":7},"type":"STATION.GET_CALLSIGN","value":""}"#);
        junk.push(b'\n');
        let mut out = p.out.try_clone().unwrap();
        let writer = std::thread::spawn(move || out.write_all(&junk).unwrap());
        let mut before = p.until_answer(7);
        writer.join().unwrap();
        assert_eq!(before.pop().unwrap()["value"], "KD9TAW");
        assert_eq!(
            before.len(),
            told,
            "one answer for each line that is no request"
        );
        for l in &before {
            assert!(
                l["type"] == "API.ERROR" && l["params"]["_ID"].is_string(),
                "junk drew {l}"
            );
        }
        // Past the cap without a newline: that connection is closed.
        let big = vec![b'A'; MAX_REQUEST_BYTES + 2];
        let _ = p.out.write_all(&big);
        assert!(p.line().is_none(), "closed");
        assert!(served_soon(port, 8), "the API stopped serving");
        m.stop_all();
    }

    /// Settings, the JS8 state, the latch and the dial: everything a request could change.
    fn state(engine: &SharedEngine) -> (String, String, bool, u64) {
        let e = engine_lock(engine);
        (
            serde_json::to_string(e.settings()).unwrap(),
            serde_json::to_string(&e.js8_state()).unwrap(),
            e.tx_enabled(),
            e.settings().dial_hz(),
        )
    }

    /// Over the real socket, every request that would transmit, tune, change the rig or a setting,
    /// or write — each family JS8Call has — draws no answer and changes nothing.
    #[test]
    fn every_write_family_over_the_socket_is_refused_and_changes_nothing() {
        let port = held_port();
        let engine = engine_with(Some(port), None);
        engine_lock(&engine).js8_api_hear_heartbeat("W1AW", "FN31", 1200.0, -7);
        let mut m = Manager::new(Arc::new(Hub::default()), "test");
        m.tick(&engine);
        let mut p = Program::connect(port);
        p.sentinel(1);
        let before = state(&engine);
        let refused = [
            r#""type":"TX.SEND_MESSAGE","value":"W1AW SNR?""#,
            r#""type":"TX.SET_TEXT","value":"CQ""#,
            r#""type":"RIG.SET_FREQ","value":"""#,
            r#""type":"RIG.SET_TUNE","value":"true""#,
            r#""type":"RIG.TX_HALT","value":"""#,
            r#""type":"MODE.SET_SPEED","value":"""#,
            r#""type":"STATION.SET_GRID","value":"FN31""#,
            r#""type":"STATION.SET_STATUS","value":"AWAY""#,
            r#""type":"STATION.SET_SPOT","value":"true""#,
            r#""type":"STATION.SEND_HB","value":"""#,
            r#""type":"STATION.SET_GROUPS","value":"""#,
            r#""type":"INBOX.STORE_MESSAGE","value":"""#,
            r#""type":"RX.SET_FILTER","value":"""#,
            r#""type":"WINDOW.RAISE","value":"""#,
        ];
        for (i, body) in refused.iter().enumerate() {
            p.say(&format!(
                r#"{{"params":{{"_ID":{},"DIAL":14078000,"OFFSET":800,"SPEED":2,"CALLSIGN":"W1AW","TEXT":"HI","GROUPS":["@FUN"],"CENTER":1500}},{body}}}"#,
                100 + i
            ));
        }
        let answers = p.sentinel(2);
        assert!(answers.is_empty(), "a refused request drew: {answers:?}");
        m.tick(&engine);
        assert_eq!(
            state(&engine),
            before,
            "a refused request changed the station"
        );
        m.stop_all();
    }

    /// The reads outside the `GET` families are answered over the socket too, and
    /// `STATION.VERSION` names the version the manager runs, as the PING does.
    #[test]
    fn the_harmless_reads_are_answered_over_the_socket() {
        let port = held_port();
        let engine = engine_with(Some(port), None);
        let mut m = Manager::new(Arc::new(Hub::default()), "9.9.9");
        m.tick(&engine);
        let mut p = Program::connect(port);
        p.sentinel(1);
        for (i, (ask, reply)) in [
            ("RIG.GET_FREQ", "RIG.FREQ"),
            ("RIG.GET_PTT", "RIG.PTT_STATUS"),
            ("MODE.GET_SPEED", "MODE.SPEED"),
            ("TX.GET_TEXT", "TX.TEXT"),
            ("TX.GET_QUEUE_DEPTH", "TX.QUEUE_DEPTH"),
            ("STATION.VERSION", "STATION.VERSION"),
        ]
        .into_iter()
        .enumerate()
        {
            let id = 10 + i as i64;
            p.say(&format!(
                r#"{{"params":{{"_ID":{id}}},"type":"{ask}","value":""}}"#
            ));
            let seen = p.until_answer(id);
            assert_eq!(seen.len(), 1, "{seen:?}");
            assert_eq!(seen[0]["type"], reply);
            if ask == "STATION.VERSION" {
                assert_eq!(seen[0]["params"]["VERSION"], "9.9.9");
            }
        }
        m.stop_all();
    }

    /// What the station hears reaches a connected program as JS8Call sends it, and a read after
    /// it sees what was heard. `STATION.STATUS` is never sent over TCP (its id is fresh).
    #[test]
    fn heard_traffic_reaches_a_connected_program() {
        let port = held_port();
        let engine = engine_with(Some(port), None);
        let mut m = Manager::new(Arc::new(Hub::default()), "test");
        m.tick(&engine);
        let mut p = Program::connect(port);
        p.sentinel(1);
        engine_lock(&engine).js8_api_hear_heartbeat("W1AW", "FN31", 1200.0, -7);
        engine_lock(&engine).set_tx_offset(1750.0);
        m.tick(&engine);
        let seen = p.sentinel(2);
        let kinds: Vec<&str> = seen.iter().map(|l| l["type"].as_str().unwrap()).collect();
        // Spotting is on by default (Nexus's PSK Reporter switch), so the station is spotted too.
        assert_eq!(kinds, ["RX.ACTIVITY", "RX.DIRECTED", "RX.SPOT"], "{seen:?}");
        assert_eq!(seen[1]["params"]["FROM"], "W1AW");
        assert_eq!(seen[1]["value"], "@HB HEARTBEAT \u{2662} ");
        p.say(r#"{"params":{"_ID":3},"type":"RX.GET_CALL_ACTIVITY","value":""}"#);
        let calls = p.until_answer(3).pop().unwrap();
        assert_eq!(calls["params"]["W1AW"]["GRID"], "FN31");
        m.stop_all();
    }

    /// The UDP side sends to 127.0.0.1 on the configured port, from 127.0.0.1: a ping and the
    /// station's state first, then the answers to requests from this computer, at that port. A
    /// request from anywhere else, past the cap, malformed or refused is not answered.
    #[test]
    fn the_udp_side_talks_only_to_this_computer() {
        let tool = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        tool.set_read_timeout(Some(WAIT)).unwrap();
        let engine = engine_with(None, Some(tool.local_addr().unwrap().port()));
        let mut m = Manager::new(Arc::new(Hub::default()), "9.9.9");
        m.tick(&engine);
        let mut kinds = Vec::new();
        let mut station = None;
        while kinds.len() < 3 {
            let (d, from) = datagram(&tool).expect("the first datagrams");
            assert!(from.ip().is_loopback(), "{from}");
            station = Some(from);
            if d["type"] == "PING" {
                assert_eq!(
                    (&d["params"]["NAME"], &d["params"]["VERSION"]),
                    (&Value::from("Nexus"), &Value::from("9.9.9"))
                );
            }
            kinds.push(d["type"].as_str().unwrap().to_string());
        }
        kinds.sort();
        assert_eq!(kinds, ["PING", "RIG.FREQ", "STATION.STATUS"]);
        let station = station.unwrap();
        let ask = |text: &[u8]| tool.send_to(text, station).unwrap();
        // Refused, malformed, past the cap: nothing; then a read, answered with its own id.
        ask(br#"{"params":{"_ID":5},"type":"TX.SEND_MESSAGE","value":"W1AW SNR?"}"#);
        ask(b"{\"params\":{\"_ID\":6");
        let mut big = br#"{"params":{"_ID":7,"PAD":""#.to_vec();
        big.extend(std::iter::repeat_n(b'A', MAX_REQUEST_BYTES));
        big.extend(br#""},"type":"STATION.GET_CALLSIGN","value":""}"#);
        ask(&big);
        ask(br#"{"params":{"_ID":8},"type":"STATION.GET_CALLSIGN","value":""}"#);
        let (d, _) = datagram(&tool).expect("the read's answer");
        assert_eq!(
            (&d["type"], &d["value"], &d["params"]["_ID"]),
            (
                &Value::from("STATION.CALLSIGN"),
                &Value::from("KD9TAW"),
                &Value::from(8)
            )
        );
        // From this computer's network address instead of 127.0.0.1: dropped.
        if let Some(ip) = lan_address() {
            let outside = UdpSocket::bind((ip, 0)).unwrap();
            if outside
                .send_to(
                    br#"{"params":{"_ID":9},"type":"STATION.GET_CALLSIGN","value":""}"#,
                    station,
                )
                .is_ok()
            {
                ask(br#"{"params":{"_ID":10},"type":"STATION.GET_GRID","value":""}"#);
                let (d, _) = datagram(&tool).expect("the sentinel's answer");
                assert_eq!(
                    d["params"]["_ID"], 10,
                    "a datagram from {ip} was answered: {d}"
                );
            }
        }
        // A read with no id is answered with a fresh one, a string, as JS8Call's UDP answer has.
        ask(br#"{"type":"STATION.GET_GRID"}"#);
        let (d, _) = datagram(&tool).unwrap();
        assert!(d["params"]["_ID"].is_string(), "{d}");
        m.stop_all();
    }

    /// "Accept TCP requests" off, JS8Call's default: a connected program hears what the station
    /// sends, and its requests go no further, as JS8Call's do (mainwindow.cpp:6729-6739), so a
    /// read draws nothing. A line that is no request is still told why, as JS8Call's server tells
    /// it before the switch, and is the marker here: TCP keeps order, so an answer to a read
    /// would have come before it. Turned on, a read is answered at once.
    #[test]
    fn with_tcp_requests_not_accepted_the_station_only_speaks() {
        let port = held_port();
        let engine = engine_with(Some(port), None);
        set(&engine, |s| s.js8_api_tcp_accept = false);
        let mut m = Manager::new(Arc::new(Hub::default()), "test");
        m.tick(&engine);
        let mut p = Program::connect(port);
        p.say(r#"{"params":{"_ID":1},"type":"STATION.GET_CALLSIGN","value":""}"#);
        p.say(r#"{"params":{"_ID":2},"type":"RIG.GET_FREQ","value":""}"#);
        p.say(r#"{"params":{"_ID":3},"type":"INBOX.GET_MESSAGES","value":""}"#);
        p.say("{not json");
        let first = p.line().expect("told why");
        assert_eq!(first["type"], "API.ERROR", "a read was answered: {first}");
        assert!(first["value"]
            .as_str()
            .unwrap()
            .ends_with(": json parsing error"));
        engine_lock(&engine).js8_api_hear_heartbeat("W1AW", "FN31", 1200.0, -7);
        m.tick(&engine);
        let heard: Vec<Value> = (0..3).map(|_| p.line().expect("what was heard")).collect();
        let kinds: Vec<&str> = heard.iter().map(|l| l["type"].as_str().unwrap()).collect();
        assert_eq!(kinds, ["RX.ACTIVITY", "RX.DIRECTED", "RX.SPOT"]);
        set(&engine, |s| s.js8_api_tcp_accept = true);
        m.tick(&engine);
        assert!(p.sentinel(4).is_empty());
        m.stop_all();
    }

    /// A JS8 contact logged reaches a program on each side as JS8Call's `LOG.QSO`, as every event
    /// does: to a connected TCP program and as a datagram, with neither side accepting requests,
    /// since it answers none.
    #[test]
    fn a_logged_js8_contact_reaches_both_sides() {
        let port = held_port();
        let tool = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        tool.set_read_timeout(Some(WAIT)).unwrap();
        let engine = engine_with(Some(port), Some(tool.local_addr().unwrap().port()));
        set(&engine, |s| {
            s.js8_api_tcp_accept = false;
            s.js8_api_udp_accept = false;
        });
        let mut m = Manager::new(Arc::new(Hub::default()), "test");
        m.tick(&engine);
        let mut p = Program::connect(port);
        // TCP keeps order: once a line that is no request is told why, the program is connected.
        p.say("{not json");
        assert_eq!(p.line().expect("told why")["type"], "API.ERROR");
        let mut rec = tempo_core::logbook::parse_adif("<CALL:4>W1AW<EOR>").remove(0);
        rec.band = "20m".into();
        rec.freq_mhz = 14.0795;
        rec.mode = "JS8".into();
        rec.when_unix = 1_792_074_600;
        engine_lock(&engine).log_qso(rec);
        m.tick(&engine);
        let heard = p.line().expect("the contact");
        assert_eq!(
            (
                &heard["type"],
                &heard["params"]["CALL"],
                &heard["params"]["_ID"]
            ),
            (
                &Value::from("LOG.QSO"),
                &Value::from("W1AW"),
                &Value::from(-1)
            ),
            "{heard}"
        );
        let mut types = Vec::new();
        let datagram_seen = loop {
            let (d, _) = datagram(&tool).expect("the station's datagrams");
            types.push(d["type"].clone());
            if d["type"] == "LOG.QSO" || types.len() > 8 {
                break d;
            }
        };
        assert_eq!(
            datagram_seen, heard,
            "the same line on both sides: {types:?}"
        );
        m.stop_all();
    }

    /// "Accept UDP requests" off: the station's datagrams still go out, a TCP program's answers
    /// among them (the switch is on requests, not on what is sent), and a request from the UDP
    /// program draws nothing (mainwindow.cpp:6717-6727); turned on, it is answered. A datagram
    /// that is no request draws nothing either way: JS8Call's UDP side reports it to nobody.
    #[test]
    fn with_udp_requests_not_accepted_the_station_only_speaks() {
        let port = held_port();
        let tool = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        tool.set_read_timeout(Some(WAIT)).unwrap();
        let engine = engine_with(Some(port), Some(tool.local_addr().unwrap().port()));
        set(&engine, |s| s.js8_api_udp_accept = false);
        let mut m = Manager::new(Arc::new(Hub::default()), "test");
        m.tick(&engine);
        let mut kinds = Vec::new();
        let mut station = None;
        while kinds.len() < 3 {
            let (d, from) = datagram(&tool).expect("the first datagrams");
            station = Some(from);
            kinds.push(d["type"].as_str().unwrap().to_string());
        }
        kinds.sort();
        assert_eq!(kinds, ["PING", "RIG.FREQ", "STATION.STATUS"]);
        let station = station.unwrap();
        let ask = |text: &[u8]| tool.send_to(text, station).unwrap();
        ask(br#"{"params":{"_ID":5},"type":"STATION.GET_CALLSIGN","value":""}"#);
        ask(br#"{"params":{"_ID":6},"type":"RIG.GET_FREQ","value":""}"#);
        ask(b"{not json");
        tool.set_read_timeout(Some(Duration::from_millis(800)))
            .unwrap();
        assert!(datagram(&tool).is_none(), "a datagram was answered");
        tool.set_read_timeout(Some(WAIT)).unwrap();
        // A TCP program's read is answered there and goes out as a datagram too.
        let mut p = Program::connect(port);
        p.sentinel(8);
        let (d, _) = datagram(&tool).expect("the TCP answer as a datagram");
        assert_eq!(
            (&d["type"], &d["params"]["_ID"]),
            (&Value::from("STATION.CALLSIGN"), &Value::from(8))
        );
        engine_lock(&engine).js8_api_hear_heartbeat("W1AW", "FN31", 1200.0, -7);
        m.tick(&engine);
        let mut heard = Vec::new();
        while heard.len() < 3 {
            let (d, _) = datagram(&tool).expect("what was heard");
            heard.push(d["type"].as_str().unwrap().to_string());
        }
        assert_eq!(heard, ["RX.ACTIVITY", "RX.DIRECTED", "RX.SPOT"]);
        set(&engine, |s| s.js8_api_udp_accept = true);
        m.tick(&engine);
        ask(br#"{"params":{"_ID":9},"type":"STATION.GET_CALLSIGN","value":""}"#);
        let (d, _) = datagram(&tool).expect("answered once accepted");
        assert_eq!(
            (&d["type"], &d["params"]["_ID"]),
            (&Value::from("STATION.CALLSIGN"), &Value::from(9))
        );
        m.stop_all();
    }

    /// Off stops listening; a new port rebinds; the same datagram is never sent twice in a row.
    #[test]
    fn the_switches_apply_at_once() {
        // Both at once, so the new port is never the old one.
        let (port, moved) = (held_port(), held_port());
        let engine = engine_with(Some(port), None);
        let mut m = Manager::new(Arc::new(Hub::default()), "test");
        assert!(m.tick(&engine));
        Program::connect(port).sentinel(1);
        set(&engine, |s| s.js8_api_tcp = false);
        assert!(!m.tick(&engine));
        let deadline = Instant::now() + WAIT;
        while TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_ok() {
            assert!(
                Instant::now() < deadline,
                "still listening on {port} after off"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        set(&engine, |s| {
            s.js8_api_tcp = true;
            s.js8_api_tcp_port = moved;
        });
        assert!(m.tick(&engine));
        Program::connect(moved).sentinel(2);
        assert!(TcpStream::connect((Ipv4Addr::LOCALHOST, port)).is_err());
        m.stop_all();
        // The UDP side's repeat rule.
        let tool = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        tool.set_read_timeout(Some(Duration::from_millis(500)))
            .unwrap();
        let hub = Hub::default();
        hub.set_udp(Some(Udp {
            socket: bind_udp().unwrap(),
            to: tool.local_addr().unwrap(),
            last: None,
        }));
        hub.send_udp("one");
        hub.send_udp("one");
        hub.send_udp("two");
        let mut got = Vec::new();
        let mut buf = [0u8; 16];
        while let Ok(n) = tool.recv(&mut buf) {
            got.push(String::from_utf8_lossy(&buf[..n]).to_string());
        }
        assert_eq!(got, ["one", "two"]);
    }

    /// Closing, the program says so on both sides, as JS8Call does.
    #[test]
    fn closing_is_announced() {
        let port = held_port();
        let tool = UdpSocket::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        tool.set_read_timeout(Some(WAIT)).unwrap();
        let engine = engine_with(Some(port), Some(tool.local_addr().unwrap().port()));
        let hub = Arc::new(Hub::default());
        let mut m = Manager::new(hub.clone(), "test");
        m.tick(&engine);
        let mut p = Program::connect(port);
        p.sentinel(1);
        while datagram(&tool).is_some_and(|(d, _)| d["type"] != "STATION.CALLSIGN") {}
        hub.closing();
        let last = p.line().expect("the closing line");
        assert_eq!(last["type"], "STATION.CLOSING");
        assert_eq!(last["params"]["REASON"], "User closed application");
        let (a, _) = datagram(&tool).unwrap();
        let (b, _) = datagram(&tool).unwrap();
        assert_eq!(
            (&a["type"], &b["type"]),
            (&Value::from("STATION.CLOSING"), &Value::from("CLOSE"))
        );
        m.stop_all();
    }
}
