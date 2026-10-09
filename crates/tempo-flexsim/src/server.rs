//! The simulated radio: a SmartSDR TCP API server on loopback, with VITA-49 over UDP, that
//! replays a [`Session`] and injects [`Fault`]s.
//!
//! **TCP.** On connect the simulator sends the session's prologue and a client handle (`V…`, then
//! `H<8 hex digits>`; the first connection gets the session's handle and each later one the next
//! number). Each command line `C[D]<seq>|<command>` is answered by the session rule that matches
//! it, with the client's own sequence number in the reply: `R<seq>|<code>|<message>`, exactly one
//! reply per command (SmartSDR-TCPIP-API). A command no rule matches gets the session's default
//! code. A line that is not a well-formed command is logged and not answered.
//!
//! **UDP.** One socket stands in for both of the radio's UDP ports (4991 for VITA-49, 4992 for
//! the client's registration datagram). The client's address becomes known when it sends the
//! one-byte registration datagram (the plan's §4.2, from AetherSDR's behaviour) or
//! `client udpport <port>`, whichever is latest. Synthetic [`Stream`]s and a session's `vita`
//! items are sent there; every datagram a client sends (DAX TX audio included) is logged.
//!
//! **What the radio remembers.** Who holds the transmitter: a successful `xmit 1` keys it under
//! the connection's handle and a successful `xmit 0` releases it, unless a fault says otherwise.
//! The tune carrier the same way: `transmit tune 1` and `transmit tune 0`. That is what lets a
//! reconnect see a transmitter still keyed, or still tuning, under a dropped handle. And the
//! transmitter's DAX source, which is radio-wide: after a successful `transmit set dax=<0|1>`,
//! every later `sub tx all` reports it, so a reconnect sees what an earlier session left.
//! Everything else a client hears comes from the session's rules: the simulator models no slices
//! or pans of its own.
//!
//! **The log.** Every connection, command, line sent, datagram and fault action is logged in order
//! with its time since the simulator started (monotonic clock). Tests read it, and wait on it with
//! [`Simulator::wait_for`] instead of sleeping.
//!
//! The simulator is test support. It binds 127.0.0.1 only and never talks to a radio.

use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::fault::{interlock_line, withholds, Fault, REFUSED};
use crate::line::{self, LineBuf};
use crate::session::{Item, Session};
use crate::vita::{self, Start, Stream};

/// How often a blocked thread looks up to check for shutdown.
const POLL: Duration = Duration::from_millis(20);

/// What to run besides the session.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub faults: Vec<Fault>,
    pub streams: Vec<Stream>,
    /// How long the radio waits for a ping after `keepalive enable` before it closes the session.
    /// The radio's own value is 15 s (TCPIP-keepalive); tests shorten it.
    pub keepalive_timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            faults: Vec::new(),
            streams: Vec::new(),
            keepalive_timeout: Duration::from_secs(15),
        }
    }
}

/// Who ended a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closer {
    /// The client closed it, or the socket failed.
    Client,
    /// A fault closed it ([`Fault::DisconnectMidOver`]).
    Simulator,
    /// The radio's keepalive closed it: no ping within the timeout.
    Keepalive,
    /// The simulator was dropped.
    Stopped,
}

/// One entry in the simulator's log. `conn` numbers connections from 0.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    Connected {
        conn: usize,
        handle: u32,
    },
    /// A well-formed command from the client.
    Command {
        conn: usize,
        seq: u32,
        text: String,
    },
    /// A client line that is not a command. Nothing was answered.
    Malformed {
        conn: usize,
        line: String,
    },
    /// A whole line the simulator sent, after template expansion, in the order written.
    Sent {
        conn: usize,
        line: String,
    },
    /// The pieces [`Fault::SplitLine`] cut a line into; its [`Event::Sent`] follows the last one.
    Split {
        conn: usize,
        pieces: Vec<String>,
    },
    /// [`Fault::ReorderReply`] held this reply back.
    ReplyHeld {
        conn: usize,
        seq: u32,
    },
    /// [`Fault::DropPings`] treated this ping as lost.
    PingDropped {
        conn: usize,
        seq: u32,
    },
    /// [`Fault::StuckTransmit`] withheld this many status lines of the command's rule.
    StatusWithheld {
        conn: usize,
        command: String,
        lines: usize,
    },
    Closed {
        conn: usize,
        by: Closer,
    },
    /// A datagram from a client: the registration byte, DAX TX audio, anything.
    UdpIn {
        from: SocketAddr,
        bytes: Vec<u8>,
    },
    /// A VITA packet sent, or with `dropped` withheld by [`Fault::Vita`]. `index` is the
    /// packet's place in its synthetic stream, `None` for a session's `vita` item.
    VitaOut {
        conn: usize,
        stream_id: u32,
        index: Option<usize>,
        count: u8,
        dropped: bool,
    },
    /// A session's `vita` item had nowhere to go: the client had not registered a UDP address.
    VitaUndelivered {
        conn: usize,
    },
    /// A synthetic stream that had started sent its last packet: its ticks ran out, its `until`
    /// command was answered, or the connection ended. Nothing more of it follows.
    StreamEnded {
        conn: usize,
        stream_id: u32,
    },
}

/// A logged event and when it happened, measured from the simulator's start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Logged {
    pub at: Duration,
    pub event: Event,
}

/// A running simulator. Dropping it closes every connection and joins its threads.
pub struct Simulator {
    tcp_addr: SocketAddr,
    udp_addr: SocketAddr,
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

struct Shared {
    session: Session,
    config: Config,
    started: Instant,
    stop: AtomicBool,
    udp: UdpSocket,
    log: Mutex<Vec<Logged>>,
    logged: Condvar,
    radio: Mutex<Radio>,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

#[derive(Default)]
struct Radio {
    /// Connections accepted so far: the next connection's number.
    accepted: usize,
    /// The handle the transmitter is keyed under, if any.
    keyed_by: Option<u32>,
    /// The handle the tune carrier is up under, if any.
    tuned_by: Option<u32>,
    /// [`Fault::DisconnectMidOver`] fires once per simulator.
    disconnect_fired: bool,
    /// The transmitter's DAX source, once a client has set it.
    dax: Option<bool>,
    live: Vec<Arc<Conn>>,
}

struct Conn {
    id: usize,
    handle: u32,
    peer: SocketAddr,
    /// A clone of the socket, for shutting it down from any thread.
    tcp: TcpStream,
    out: Mutex<Outbox>,
    out_cv: Condvar,
    gate: Mutex<Gate>,
    gate_cv: Condvar,
    closed: AtomicBool,
}

/// What the writer thread sends, ordered by due time, then by insertion.
struct Outbox {
    queue: Vec<Out>,
    /// When the last status of the latest rule is due: the API's ordering rule starts each later
    /// rule's statuses no earlier than this.
    last_status_due: Instant,
    /// Commands received so far. A split line's later pieces wait for this to move.
    commands: u64,
}

struct Out {
    due: Instant,
    what: What,
}

enum What {
    Line(String),
    Vita(Vec<u8>),
    /// Start (`true`) or stop a synthetic stream: sent after the reply that triggers it.
    Gate(usize, bool),
    Disconnect,
}

/// Where the streams may send, and which of them a command has started or stopped.
struct Gate {
    dest: Option<SocketAddr>,
    started: Vec<bool>,
    stopped: Vec<bool>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Simulator {
    /// Bind TCP and UDP on 127.0.0.1 (ephemeral ports, read back) and start serving.
    pub fn start(session: Session, config: Config) -> io::Result<Simulator> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let udp = UdpSocket::bind("127.0.0.1:0")?;
        udp.set_read_timeout(Some(POLL))?;
        let tcp_addr = listener.local_addr()?;
        let udp_addr = udp.local_addr()?;
        let shared = Arc::new(Shared {
            session,
            config,
            started: Instant::now(),
            stop: AtomicBool::new(false),
            udp,
            log: Mutex::new(Vec::new()),
            logged: Condvar::new(),
            radio: Mutex::new(Radio::default()),
            workers: Mutex::new(Vec::new()),
        });
        let accept = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("flexsim-accept".into())
                .spawn(move || accept_loop(&shared, listener))?
        };
        let udp_rx = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("flexsim-udp".into())
                .spawn(move || udp_loop(&shared))?
        };
        Ok(Simulator {
            tcp_addr,
            udp_addr,
            shared,
            threads: vec![accept, udp_rx],
        })
    }

    /// Where clients connect (the radio's TCP 4992).
    pub fn tcp_addr(&self) -> SocketAddr {
        self.tcp_addr
    }

    /// Where clients send UDP: the registration datagram and DAX TX (the radio's 4992 and 4991).
    pub fn udp_addr(&self) -> SocketAddr {
        self.udp_addr
    }

    /// When the simulator started: the zero of every [`Logged::at`], so a test can place the log
    /// on its own clock.
    pub fn started(&self) -> Instant {
        self.shared.started
    }

    /// The log so far, with times.
    pub fn log(&self) -> Vec<Logged> {
        lock(&self.shared.log).clone()
    }

    /// The log so far, without times.
    pub fn events(&self) -> Vec<Event> {
        lock(&self.shared.log)
            .iter()
            .map(|l| l.event.clone())
            .collect()
    }

    /// Every command text received so far, on any connection, in order.
    pub fn commands(&self) -> Vec<String> {
        lock(&self.shared.log)
            .iter()
            .filter_map(|l| match &l.event {
                Event::Command { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// Wait until `done` holds for the log, or `timeout` passes. Returns whether it held.
    pub fn wait_for(&self, timeout: Duration, mut done: impl FnMut(&[Logged]) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        let mut log = lock(&self.shared.log);
        loop {
            if done(&log) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            log = self
                .shared
                .logged
                .wait_timeout(log, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        // Wake the accept loop: it checks the flag after each accept.
        let _ = TcpStream::connect_timeout(&self.tcp_addr, Duration::from_secs(1));
        let live: Vec<Arc<Conn>> = lock(&self.shared.radio).live.clone();
        for conn in live {
            close(&self.shared, &conn, Closer::Stopped);
        }
        for t in self.threads.drain(..) {
            let _ = t.join();
        }
        let workers = std::mem::take(&mut *lock(&self.shared.workers));
        for w in workers {
            let _ = w.join();
        }
    }
}

impl Shared {
    fn record(&self, event: Event) {
        let at = self.started.elapsed();
        lock(&self.log).push(Logged { at, event });
        self.logged.notify_all();
    }

    fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn stuck(&self) -> bool {
        self.config
            .faults
            .iter()
            .any(|f| matches!(f, Fault::StuckTransmit))
    }

    fn stuck_tune(&self) -> bool {
        self.config
            .faults
            .iter()
            .any(|f| matches!(f, Fault::StuckTune))
    }

    fn disconnect_mid_over(&self) -> Option<(Duration, bool)> {
        self.config.faults.iter().find_map(|f| match f {
            Fault::DisconnectMidOver {
                after,
                radio_stays_keyed,
            } => Some((*after, *radio_stays_keyed)),
            _ => None,
        })
    }
}

impl Conn {
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    fn push(&self, due: Instant, what: What) {
        let mut out = lock(&self.out);
        // After everything due at the same time or earlier: equal times keep insertion order.
        let at = out.queue.partition_point(|o| o.due <= due);
        out.queue.insert(at, Out { due, what });
        drop(out);
        self.out_cv.notify_all();
    }

    fn set_dest(&self, dest: SocketAddr) {
        lock(&self.gate).dest = Some(dest);
        self.gate_cv.notify_all();
    }

    /// Expand a session template: `{h}` is the handle as eight hex digits, `{peer}` the client's
    /// IP address.
    fn expand(&self, text: &str) -> String {
        text.replace("{h}", &format!("{:08X}", self.handle))
            .replace("{peer}", &self.peer.ip().to_string())
    }

    /// Queue items from `base`, honouring their waits. Returns when the last one is due.
    fn push_items(&self, base: Instant, items: &[Item]) -> Instant {
        let mut due = base;
        for item in items {
            match item {
                Item::Send(text) => self.push(due, What::Line(self.expand(text))),
                Item::Wait(ms) => due += Duration::from_millis(*ms),
                Item::Vita(bytes) => self.push(due, What::Vita(bytes.clone())),
            }
        }
        due
    }
}

/// End a connection once, whoever notices first.
fn close(shared: &Shared, conn: &Conn, by: Closer) {
    if conn.closed.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = conn.tcp.shutdown(Shutdown::Both);
    {
        let mut radio = lock(&shared.radio);
        radio.live.retain(|c| c.id != conn.id);
        if radio.keyed_by == Some(conn.handle) {
            let stays = by == Closer::Simulator
                && shared.disconnect_mid_over().is_some_and(|(_, stays)| stays);
            if !shared.stuck() && !stays {
                radio.keyed_by = None;
            }
        }
        if radio.tuned_by == Some(conn.handle) && !shared.stuck_tune() {
            radio.tuned_by = None;
        }
    }
    conn.out_cv.notify_all();
    conn.gate_cv.notify_all();
    shared.record(Event::Closed { conn: conn.id, by });
}

fn accept_loop(shared: &Arc<Shared>, listener: TcpListener) {
    for incoming in listener.incoming() {
        if shared.stopping() {
            break;
        }
        let Ok(tcp) = incoming else { continue };
        let Ok(peer) = tcp.peer_addr() else { continue };
        let worker = {
            let shared = Arc::clone(shared);
            thread::Builder::new()
                .name("flexsim-conn".into())
                .spawn(move || serve(&shared, tcp, peer))
        };
        if let Ok(worker) = worker {
            lock(&shared.workers).push(worker);
        }
    }
}

fn udp_loop(shared: &Shared) {
    let mut buf = vec![0u8; 65_536];
    while !shared.stopping() {
        let Ok((n, from)) = shared.udp.recv_from(&mut buf) else {
            continue; // the read timeout: look at the stop flag again
        };
        shared.record(Event::UdpIn {
            from,
            bytes: buf[..n].to_vec(),
        });
        if n == 1 {
            // The registration datagram: VITA goes back to where it came from. It is matched to
            // the newest live connection from the same address.
            let conn = lock(&shared.radio)
                .live
                .iter()
                .rev()
                .find(|c| c.peer.ip() == from.ip())
                .cloned();
            if let Some(conn) = conn {
                conn.set_dest(from);
            }
        }
    }
}

fn serve(shared: &Arc<Shared>, tcp: TcpStream, peer: SocketAddr) {
    let _ = tcp.set_nodelay(true);
    let _ = tcp.set_read_timeout(Some(POLL));
    let (Ok(shutdown_half), Ok(write_half)) = (tcp.try_clone(), tcp.try_clone()) else {
        return;
    };
    let (id, handle) = {
        let mut radio = lock(&shared.radio);
        let id = radio.accepted;
        radio.accepted += 1;
        let handle = match shared.session.handle.wrapping_add(id as u32) {
            0 => 1,
            h => h,
        };
        (id, handle)
    };
    let opened = Instant::now();
    let streams = shared.config.streams.len();
    let conn = Arc::new(Conn {
        id,
        handle,
        peer,
        tcp: shutdown_half,
        out: Mutex::new(Outbox {
            queue: Vec::new(),
            last_status_due: opened,
            commands: 0,
        }),
        out_cv: Condvar::new(),
        gate: Mutex::new(Gate {
            dest: None,
            started: vec![false; streams],
            stopped: vec![false; streams],
        }),
        gate_cv: Condvar::new(),
        closed: AtomicBool::new(false),
    });
    lock(&shared.radio).live.push(Arc::clone(&conn));
    shared.record(Event::Connected { conn: id, handle });

    let mut helpers = Vec::new();
    {
        let (shared, conn) = (Arc::clone(shared), Arc::clone(&conn));
        helpers.push(thread::spawn(move || {
            writer_loop(&shared, &conn, write_half)
        }));
    }
    for index in 0..streams {
        let (shared, conn) = (Arc::clone(shared), Arc::clone(&conn));
        helpers.push(thread::spawn(move || stream_loop(&shared, &conn, index)));
    }

    conn.push(opened, What::Line(shared.session.prologue.clone()));
    conn.push(opened, What::Line(format!("H{handle:08X}")));
    for (ms, items) in &shared.session.timed {
        conn.push_items(opened + Duration::from_millis(*ms), items);
    }

    let mut reader = Reader {
        shared,
        conn: &conn,
        uses: HashMap::new(),
        pings: 0,
        keepalive: false,
        last_ping: Instant::now(),
        held_reply: None,
        dax_rx_creates: 0,
    };
    let mut tcp = tcp;
    let mut lines = LineBuf::default();
    let mut buf = [0u8; 4096];
    while !shared.stopping() && !conn.is_closed() {
        match tcp.read(&mut buf) {
            Ok(0) => close(shared, &conn, Closer::Client),
            Ok(n) => match lines.push(&buf[..n]) {
                Ok(complete) => {
                    for l in complete {
                        reader.on_line(l);
                    }
                }
                Err(line::TooLong) => {
                    shared.record(Event::Malformed {
                        conn: id,
                        line: format!("<more than {} bytes without a terminator>", line::MAX_LINE),
                    });
                    close(shared, &conn, Closer::Simulator);
                }
            },
            Err(e) if line::read_again(&e) => {}
            Err(_) => close(shared, &conn, Closer::Client),
        }
        if reader.keepalive && reader.last_ping.elapsed() > shared.config.keepalive_timeout {
            close(shared, &conn, Closer::Keepalive);
        }
    }
    close(shared, &conn, Closer::Stopped); // a no-op unless the simulator is stopping
    for h in helpers {
        let _ = h.join();
    }
}

/// The per-connection command handler, run on the connection's reading thread.
struct Reader<'a> {
    shared: &'a Shared,
    conn: &'a Arc<Conn>,
    /// How many times each rule group has answered on this connection.
    uses: HashMap<usize, usize>,
    pings: usize,
    keepalive: bool,
    last_ping: Instant,
    held_reply: Option<String>,
    /// `stream create type=dax_rx` commands so far on this connection.
    dax_rx_creates: usize,
}

impl Reader<'_> {
    fn on_line(&mut self, raw: String) {
        let (shared, conn) = (self.shared, self.conn);
        let Some(cmd) = line::parse_command(&raw) else {
            shared.record(Event::Malformed {
                conn: conn.id,
                line: raw,
            });
            return;
        };
        shared.record(Event::Command {
            conn: conn.id,
            seq: cmd.seq,
            text: cmd.text.clone(),
        });
        lock(&conn.out).commands += 1;
        conn.out_cv.notify_all();
        let text = cmd.text.as_str();
        let now = Instant::now();

        if text == "ping" {
            self.pings += 1;
            let lost = shared.config.faults.iter().any(|f| match f {
                Fault::DropPings { first, count } => (*first..first + count).contains(&self.pings),
                _ => false,
            });
            if lost {
                shared.record(Event::PingDropped {
                    conn: conn.id,
                    seq: cmd.seq,
                });
                return;
            }
            self.last_ping = now;
        }
        match text {
            "keepalive enable" => {
                self.keepalive = true;
                self.last_ping = now;
            }
            "keepalive disable" => self.keepalive = false,
            _ => {}
        }
        if let Some(port) = text
            .strip_prefix("client udpport ")
            .and_then(|p| p.trim().parse::<u16>().ok())
        {
            conn.set_dest(SocketAddr::new(conn.peer.ip(), port));
        }

        let session = &shared.session;
        let (code, message, mut items) = match session.lookup(text) {
            Some(group) => {
                let uses = self.uses.entry(group).or_insert(0);
                let rule = session.rule(group, *uses);
                *uses += 1;
                (rule.code.clone(), rule.message.clone(), rule.items.clone())
            }
            None => (session.default_code.clone(), String::new(), Vec::new()),
        };
        let refused = text == "stream create type=dax_tx"
            && shared
                .config
                .faults
                .iter()
                .any(|f| matches!(f, Fault::DaxTxRefused));
        let (code, message) = if refused {
            items.clear();
            (REFUSED.to_string(), String::new())
        } else {
            (code, message)
        };
        let ok = line::is_success(&code);
        let reply = format!("R{}|{}|{}", cmd.seq, code, conn.expand(&message));
        self.reply(text, cmd.seq, reply, now);

        let stuck = withholds(&shared.config.faults, text);
        if stuck {
            let lines = items.iter().filter(|i| matches!(i, Item::Send(_))).count();
            shared.record(Event::StatusWithheld {
                conn: conn.id,
                command: text.to_string(),
                lines,
            });
            items.clear();
        }
        self.augment(text, &mut items);
        let end = {
            let base = lock(&conn.out).last_status_due.max(now);
            let end = conn.push_items(base, &items);
            let mut out = lock(&conn.out);
            out.last_status_due = out.last_status_due.max(end);
            end
        };

        if ok && text == "xmit 1" {
            let after = {
                let mut radio = lock(&shared.radio);
                radio.keyed_by = Some(conn.handle);
                match shared.disconnect_mid_over() {
                    Some((after, _)) if !radio.disconnect_fired => {
                        radio.disconnect_fired = true;
                        Some(after)
                    }
                    _ => None,
                }
            };
            if let Some(after) = after {
                conn.push(end + after, What::Disconnect);
            }
        }
        if ok {
            match text {
                "transmit set dax=1" => lock(&shared.radio).dax = Some(true),
                "transmit set dax=0" => lock(&shared.radio).dax = Some(false),
                _ => {}
            }
        }
        if ok && text == "xmit 0" && !stuck {
            let mut radio = lock(&shared.radio);
            if radio.keyed_by == Some(conn.handle) {
                radio.keyed_by = None;
            }
        }
        if ok && text == "transmit tune 1" {
            lock(&shared.radio).tuned_by = Some(conn.handle);
        }
        if ok && text == "transmit tune 0" && !stuck {
            let mut radio = lock(&shared.radio);
            if radio.tuned_by == Some(conn.handle) {
                radio.tuned_by = None;
            }
        }

        for (index, stream) in shared.config.streams.iter().enumerate() {
            if matches!(&stream.start, Start::After(p) if text.starts_with(p.as_str())) {
                conn.push(now, What::Gate(index, true));
            }
            if matches!(&stream.until, Some(p) if text.starts_with(p.as_str())) {
                conn.push(now, What::Gate(index, false));
            }
        }
    }

    /// Queue a reply, unless [`Fault::ReorderReply`] holds it for after the next one.
    fn reply(&mut self, text: &str, seq: u32, reply: String, now: Instant) {
        let hold = self
            .shared
            .config
            .faults
            .iter()
            .any(|f| matches!(f, Fault::ReorderReply { command } if command == text));
        if hold && self.held_reply.is_none() {
            self.shared.record(Event::ReplyHeld {
                conn: self.conn.id,
                seq,
            });
            self.held_reply = Some(reply);
            return;
        }
        self.conn.push(now, What::Line(reply));
        if let Some(held) = self.held_reply.take() {
            self.conn.push(now, What::Line(held));
        }
    }

    /// What the faults and the radio's memory add to a rule's items.
    fn augment(&mut self, text: &str, items: &mut Vec<Item>) {
        if text.starts_with("stream create type=dax_rx") {
            self.dax_rx_creates += 1;
        }
        for fault in &self.shared.config.faults {
            match fault {
                Fault::ForeignDaxTx(dax) if text == "sub client all" => {
                    items.extend(dax.lines().into_iter().map(Item::Send));
                }
                Fault::DropDaxRx { stream_id, after }
                    if self.dax_rx_creates == 1
                        && text.starts_with("stream create type=dax_rx") =>
                {
                    items.push(Item::Wait(
                        u64::try_from(after.as_millis()).unwrap_or(u64::MAX),
                    ));
                    items.push(Item::Send(format!("S0|stream 0x{stream_id:08X} removed")));
                }
                _ => {}
            }
            let Fault::ForeignClient(foreign) = fault else {
                continue;
            };
            if let Some(topic) = text
                .strip_prefix("sub ")
                .and_then(|t| t.strip_suffix(" all"))
            {
                items.extend(foreign.lines(topic).into_iter().map(Item::Send));
            }
            if foreign.tx_slice && text.starts_with("slice create") {
                // One TX slice per radio: ours is created without the flag.
                for item in items.iter_mut() {
                    if let Item::Send(line) = item {
                        if line.contains("|slice ") {
                            *line = line.replace(" tx=1 ", " tx=0 ");
                        }
                    }
                }
            }
        }
        if text == "sub tx all" {
            let (other, tuning, dax) = {
                let radio = lock(&self.shared.radio);
                let other = |h: &u32| *h != self.conn.handle;
                (
                    radio.keyed_by.filter(other),
                    radio.tuned_by.filter(other),
                    radio.dax,
                )
            };
            if let Some(owner) = other {
                items.push(Item::Send(interlock_line(owner, "TRANSMITTING", "SW")));
            }
            if let Some(owner) = tuning {
                // The tune profile's source (the bundled session, from AetherSDR's notes).
                items.push(Item::Send("S0|transmit tune=1".to_string()));
                items.push(Item::Send(interlock_line(owner, "TRANSMITTING", "TUNE")));
            }
            if let Some(dax) = dax {
                items.push(Item::Send(format!("S0|transmit dax={}", u8::from(dax))));
            }
        }
    }
}

fn writer_loop(shared: &Shared, conn: &Conn, mut tcp: TcpStream) {
    loop {
        let item = {
            let mut out = lock(&conn.out);
            loop {
                if shared.stopping() || conn.is_closed() {
                    return;
                }
                let now = Instant::now();
                let front = out.queue.first().map(|o| o.due);
                match front {
                    Some(due) if due <= now => break out.queue.remove(0),
                    Some(due) => {
                        out = conn
                            .out_cv
                            .wait_timeout(out, (due - now).min(POLL))
                            .unwrap_or_else(|e| e.into_inner())
                            .0;
                    }
                    None => {
                        out = conn
                            .out_cv
                            .wait_timeout(out, POLL)
                            .unwrap_or_else(|e| e.into_inner())
                            .0;
                    }
                }
            }
        };
        match item.what {
            What::Line(text) => {
                if write_line(shared, conn, &mut tcp, &text).is_err() {
                    close(shared, conn, Closer::Client);
                    return;
                }
            }
            What::Vita(bytes) => {
                let dest = lock(&conn.gate).dest;
                match dest {
                    Some(dest) => {
                        let _ = shared.udp.send_to(&bytes, dest);
                        let h = vita::header(&bytes);
                        shared.record(Event::VitaOut {
                            conn: conn.id,
                            stream_id: h.map_or(0, |h| h.stream_id),
                            index: None,
                            count: h.map_or(0, |h| h.count),
                            dropped: false,
                        });
                    }
                    None => shared.record(Event::VitaUndelivered { conn: conn.id }),
                }
            }
            What::Gate(index, start) => {
                {
                    let mut gate = lock(&conn.gate);
                    if start {
                        gate.started[index] = true;
                    } else {
                        gate.stopped[index] = true;
                    }
                }
                conn.gate_cv.notify_all();
            }
            What::Disconnect => {
                close(shared, conn, Closer::Simulator);
                return;
            }
        }
    }
}

/// Write one line, in pieces when [`Fault::SplitLine`] matches it.
fn write_line(shared: &Shared, conn: &Conn, tcp: &mut TcpStream, text: &str) -> io::Result<()> {
    let bytes = format!("{text}\n").into_bytes();
    let split = shared.config.faults.iter().find_map(|f| match f {
        Fault::SplitLine {
            containing,
            cuts,
            max_hold,
        } if text.contains(containing.as_str()) => Some((cuts, *max_hold)),
        _ => None,
    });
    match split {
        None => {
            tcp.write_all(&bytes)?;
            tcp.flush()?;
        }
        Some((cuts, max_hold)) => {
            let pieces = cut(&bytes, cuts);
            shared.record(Event::Split {
                conn: conn.id,
                pieces: pieces
                    .iter()
                    .map(|p| String::from_utf8_lossy(p).into_owned())
                    .collect(),
            });
            // The count is read before each piece is written, so a command the client sends after
            // reading that piece always moves past it.
            let mut seen = lock(&conn.out).commands;
            for (i, piece) in pieces.iter().enumerate() {
                if i > 0 {
                    if !wait_for_command(shared, conn, seen, max_hold) {
                        return Ok(());
                    }
                    seen = lock(&conn.out).commands;
                }
                tcp.write_all(piece)?;
                tcp.flush()?;
            }
        }
    }
    shared.record(Event::Sent {
        conn: conn.id,
        line: text.to_string(),
    });
    Ok(())
}

/// Cut `bytes` at the given offsets (none: the middle). Offsets outside the line are ignored.
fn cut(bytes: &[u8], cuts: &[usize]) -> Vec<Vec<u8>> {
    let mut points: Vec<usize> = if cuts.is_empty() {
        vec![bytes.len() / 2]
    } else {
        cuts.to_vec()
    };
    points.retain(|&c| c > 0 && c < bytes.len());
    points.sort_unstable();
    points.dedup();
    let mut pieces = Vec::with_capacity(points.len() + 1);
    let mut from = 0;
    for p in points {
        pieces.push(bytes[from..p].to_vec());
        from = p;
    }
    pieces.push(bytes[from..].to_vec());
    pieces
}

/// Wait until the client has sent a command since `seen` was read, or `max_hold` passes. `false`
/// when the connection ends first.
fn wait_for_command(shared: &Shared, conn: &Conn, seen: u64, max_hold: Duration) -> bool {
    let deadline = Instant::now() + max_hold;
    let mut out = lock(&conn.out);
    loop {
        if shared.stopping() || conn.is_closed() {
            return false;
        }
        if out.commands > seen {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        out = conn
            .out_cv
            .wait_timeout(out, (deadline - now).min(POLL))
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

fn stream_loop(shared: &Shared, conn: &Conn, index: usize) {
    let spec = &shared.config.streams[index];
    let (drop, swap) = shared
        .config
        .faults
        .iter()
        .find_map(|f| match f {
            Fault::Vita {
                stream_id,
                drop,
                swap,
            } if *stream_id == spec.stream_id => Some((drop.as_slice(), swap.as_slice())),
            _ => None,
        })
        .unwrap_or((&[], &[]));

    // Wait for the client's UDP address and the stream's start.
    {
        let mut gate = lock(&conn.gate);
        loop {
            if shared.stopping() || conn.is_closed() || gate.stopped[index] {
                return;
            }
            let started = match &spec.start {
                Start::Registered => true,
                Start::After(_) => gate.started[index],
            };
            if started && gate.dest.is_some() {
                break;
            }
            gate = conn
                .gate_cv
                .wait_timeout(gate, POLL)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    let send = |bytes: &[u8], packet: usize, count: u8| {
        let dest = lock(&conn.gate).dest;
        if let Some(dest) = dest {
            let _ = shared.udp.send_to(bytes, dest);
        }
        shared.record(Event::VitaOut {
            conn: conn.id,
            stream_id: spec.stream_id,
            index: Some(packet),
            count,
            dropped: false,
        });
    };
    let mut next = Instant::now();
    let mut packet = 0usize;
    let mut count = 0u8;
    let mut held: Option<(Vec<u8>, usize, u8)> = None;
    let mut tick = 0usize;
    while spec.ticks.is_none_or(|max| tick < max) {
        if !sleep_until(shared, conn, next) || lock(&conn.gate).stopped[index] {
            break;
        }
        next += spec.period;
        for (packet_type, class, payload) in spec.tick(tick) {
            let (this, this_count) = (packet, count);
            packet += 1;
            count = (count + 1) & 0xF;
            let bytes = vita::packet(packet_type, spec.stream_id, class, this_count, &payload);
            if drop.contains(&this) {
                shared.record(Event::VitaOut {
                    conn: conn.id,
                    stream_id: spec.stream_id,
                    index: Some(this),
                    count: this_count,
                    dropped: true,
                });
                continue;
            }
            if swap.contains(&this) && held.is_none() {
                held = Some((bytes, this, this_count));
                continue;
            }
            send(&bytes, this, this_count);
            if let Some((late, i, c)) = held.take() {
                send(&late, i, c);
            }
        }
        tick += 1;
    }
    if let Some((late, i, c)) = held.take() {
        send(&late, i, c);
    }
    shared.record(Event::StreamEnded {
        conn: conn.id,
        stream_id: spec.stream_id,
    });
}

/// Sleep until `deadline` in short steps; `false` if the connection ends first.
fn sleep_until(shared: &Shared, conn: &Conn, deadline: Instant) -> bool {
    loop {
        if shared.stopping() || conn.is_closed() {
            return false;
        }
        let now = Instant::now();
        if now >= deadline {
            return true;
        }
        thread::sleep((deadline - now).min(POLL));
    }
}

#[cfg(test)]
mod tests;
