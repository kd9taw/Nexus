//! The simulated SDR program: a TCI server on 127.0.0.1, on an ephemeral port read back from the
//! socket, serving the [`crate::radio`] model to every connection.
//!
//! **Threads.** One accepts connections. Two serve each connection: a reader that only moves the
//! bytes its socket gives into the connection's link, and the connection's own thread, which
//! does the WebSocket handshake and then all of the protocol (tungstenite's WebSocket cannot be read
//! and written from two threads, so only this one uses it). That thread waits on one condvar for
//! whichever comes first, bytes from the socket or the next message due out, so a frame goes out at
//! its time and a client's message is read as it arrives. (Waiting in a read instead held a due
//! frame until the read's timeout ended, and a 250 Hz kernel stretches a 2 ms timeout to about
//! 8 ms.) One more thread runs the radio's timers: frames, TX_CHRONO, sensor pushes, delayed echoes,
//! faults. The radio's state is behind one lock, which every thread takes only to call the model
//! and carry out its effects, never while writing to a socket.
//!
//! **A radio too slow for its streams** (a process starved of CPU) still serves its clients and
//! stops. The timer thread works in passes: a pass runs the timers due when it began, once each,
//! and looks at the stop flag before each one, so the lock is let go between passes and a
//! connection waiting for it gets its turn. (It once ran every timer due, and each stream set its
//! next one in the past: a radio that fell behind never let go.) A stream that falls more than
//! [`crate::radio::BEHIND`] behind gives up what it missed and goes on at its own time, as an SDR
//! program does ([`Event::Lost`]). Dropping the simulator waits a bounded time ([`JOIN`]) for its
//! threads, so it returns even when one cannot run.
//!
//! **The log.** Every connection, message, command, frame, decision and fault action is logged with
//! its time since the simulator started (monotonic clock), in time order. A message the simulator
//! sends is stamped just before its bytes go out, on the thread that writes them, so nothing a
//! client sends in answer to it can be logged ahead of it. Tests read the log and wait on it with
//! [`Simulator::wait_for`] instead of sleeping.
//!
//! **No process-wide state.** Every simulator binds its own port and owns everything it uses, so
//! any number run side by side in one test binary: nothing
//! here binds a port it was told, and no port is released and bound again.
//!
//! The simulator is test support. It binds 127.0.0.1 only and never talks to a radio.

use std::collections::{BTreeMap, VecDeque};
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use tungstenite::protocol::WebSocketConfig;
use tungstenite::{Bytes, HandshakeError, Message, WebSocket};

use crate::dialect::Dialect;
use crate::event::{Closer, Event, Logged};
use crate::fault::{self, Cut, Fault};
use crate::frame::Header;
use crate::radio::{Action, Effect, Meters, Radio, Snapshot, To};
use crate::synth::Receive;

/// The longest a thread sleeps with nothing due before it looks at the stop flag again.
const IDLE: Duration = Duration::from_millis(50);
/// How long dropping the simulator waits for its threads to end. A thread still running then
/// (starved of CPU, or waiting on a lock it cannot get) is left to end on its own: it holds only
/// the simulator's shared state, and the stop flag is up. So a test on a starved machine fails
/// instead of hanging in the drop.
pub const JOIN: Duration = Duration::from_secs(2);
/// How long a client has to finish its WebSocket handshake.
const HANDSHAKE: Duration = Duration::from_secs(5);
/// The largest client message read; larger ones end the connection.
const MAX_CLIENT_MESSAGE: usize = 1 << 20;

/// What to simulate.
#[derive(Debug, Clone, PartialEq)]
pub struct Config {
    pub dialect: Dialect,
    pub faults: Vec<Fault>,
    pub receive: Receive,
    pub meters: Meters,
    /// Transmit audio samples (channel 0) kept per connection for [`Simulator::tx_audio`]; the
    /// oldest go first. The default holds two minutes at 48 kHz.
    pub tx_audio_cap: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            dialect: Dialect::expert_sdr3(),
            faults: Vec::new(),
            receive: Receive::default(),
            meters: Meters::default(),
            tx_audio_cap: 48_000 * 120,
        }
    }
}

impl Config {
    /// The default configuration on `dialect`.
    pub fn dialect(dialect: Dialect) -> Config {
        Config {
            dialect,
            ..Config::default()
        }
    }

    /// This configuration with `faults` added.
    pub fn with(mut self, faults: impl IntoIterator<Item = Fault>) -> Config {
        self.faults.extend(faults);
        self
    }
}

/// A running simulator. Dropping it closes every connection and joins its threads.
pub struct Simulator {
    addr: SocketAddr,
    shared: Arc<Shared>,
    threads: Vec<JoinHandle<()>>,
}

struct Shared {
    started: Instant,
    stop: AtomicBool,
    /// The faults the writers apply ([`Fault::SplitConcat`], [`Fault::Slow`]).
    cut: Option<Option<Cut>>,
    slow: Duration,
    log: Mutex<Vec<Logged>>,
    logged: Condvar,
    state: Mutex<State>,
    /// Wakes the timer thread when a timer is set.
    wake: Condvar,
    workers: Mutex<Vec<JoinHandle<()>>>,
    /// Every socket still open, so a drop can end them without the radio's lock.
    sockets: Mutex<Vec<(u64, TcpStream)>>,
}

struct State {
    radio: Radio,
    conns: BTreeMap<usize, Arc<Conn>>,
    next_conn: usize,
    timers: BTreeMap<(Instant, u64), Action>,
    timer_seq: u64,
    socket_seq: u64,
}

struct Conn {
    id: usize,
    /// A clone of the socket, for shutting it down from any thread.
    tcp: TcpStream,
    /// Its outbox, and what its socket gave.
    link: Arc<Link>,
    /// [`Fault::HalfOpen`] silenced it.
    silent: AtomicBool,
    /// [`Fault::SplitConcat`]'s cut has been made on this connection.
    cut_done: AtomicBool,
}

/// A connection's two directions in one place, so that its thread can wait for either.
#[derive(Default)]
struct Link {
    queues: Mutex<Queues>,
    /// Notified when a message is queued to send, bytes arrive, or the socket's reading ends.
    ready: Condvar,
}

#[derive(Default)]
struct Queues {
    /// What is queued to send, in the order it is due.
    out: VecDeque<Out>,
    /// Bytes the socket gave that the WebSocket has not read yet.
    inbox: VecDeque<u8>,
    /// How the socket's reading ended: at the end of the stream (`None`) or with an error.
    ended: Option<Option<io::ErrorKind>>,
}

struct Out {
    due: Instant,
    what: What,
}

enum What {
    Text(String),
    Frame(Vec<u8>),
    Ping,
    Drop,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

impl Conn {
    /// Queue `what` at `due`, after everything due at the same time or earlier.
    fn push(&self, due: Instant, what: What) {
        let mut q = lock(&self.link.queues);
        let at = q.out.partition_point(|o| o.due <= due);
        q.out.insert(at, Out { due, what });
        drop(q);
        self.link.ready.notify_all();
    }

    /// Everything due by `now`, in order.
    fn take_due(&self, now: Instant) -> Vec<What> {
        let mut q = lock(&self.link.queues);
        let n = q.out.partition_point(|o| o.due <= now);
        q.out.drain(..n).map(|o| o.what).collect()
    }
}

impl Link {
    /// Wait until bytes arrive, the socket's reading ends, the first message queued is due, or
    /// `until` passes. Whether one of the first three happened.
    fn wait(&self, until: Instant) -> bool {
        let mut q = lock(&self.queues);
        loop {
            let now = Instant::now();
            let first = q.out.front().map(|o| o.due);
            if !q.inbox.is_empty() || q.ended.is_some() || first.is_some_and(|due| due <= now) {
                return true;
            }
            if now >= until {
                return false;
            }
            let wake = first.map_or(until, |due| due.min(until));
            q = self
                .ready
                .wait_timeout(q, wake - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Drop everything that arrived and everything queued; whether the socket's reading ended.
    fn discard(&self) -> bool {
        let mut q = lock(&self.queues);
        q.inbox.clear();
        q.out.clear();
        q.ended.is_some()
    }
}

/// The stream under a connection's WebSocket. Reading takes what the reader thread got from the
/// socket and never waits (`WouldBlock` when nothing is there); writing goes to the socket.
struct Wire {
    tcp: TcpStream,
    link: Arc<Link>,
}

impl Read for Wire {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let mut q = lock(&self.link.queues);
        if !q.inbox.is_empty() {
            return q.inbox.read(buf);
        }
        match q.ended {
            None => Err(io::ErrorKind::WouldBlock.into()),
            Some(None) => Ok(0),
            Some(Some(kind)) => Err(kind.into()),
        }
    }
}

impl Write for Wire {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.tcp.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}

/// A connection's reader: every byte its socket gives goes into the link, until the socket ends
/// (the client closed it, or the simulator shut it down).
fn read_loop(mut tcp: TcpStream, link: &Link) {
    let mut buf = vec![0u8; 64 * 1024];
    loop {
        let read = tcp.read(&mut buf);
        let mut q = lock(&link.queues);
        match read {
            Ok(0) => q.ended = Some(None),
            Ok(n) => q.inbox.extend(&buf[..n]),
            Err(e) if would_block(&e) => continue,
            Err(e) => q.ended = Some(Some(e.kind())),
        }
        let ended = q.ended.is_some();
        drop(q);
        link.ready.notify_all();
        if ended {
            return;
        }
    }
}

impl Simulator {
    /// Bind 127.0.0.1 on an ephemeral port and start serving.
    pub fn start(config: Config) -> io::Result<Simulator> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let addr = listener.local_addr()?;
        let started = Instant::now();
        let cut = fault::find(&config.faults, |f| match f {
            Fault::SplitConcat { cut } => Some(cut.clone()),
            _ => None,
        });
        let slow = fault::find(&config.faults, |f| match f {
            Fault::Slow { delay } => Some(*delay),
            _ => None,
        })
        .unwrap_or(Duration::ZERO);
        let radio = Radio::new(
            config.dialect,
            config.faults,
            config.receive,
            config.meters,
            config.tx_audio_cap,
            started,
        );
        let shared = Arc::new(Shared {
            started,
            stop: AtomicBool::new(false),
            cut,
            slow,
            log: Mutex::new(Vec::new()),
            logged: Condvar::new(),
            state: Mutex::new(State {
                radio,
                conns: BTreeMap::new(),
                next_conn: 0,
                timers: BTreeMap::new(),
                timer_seq: 0,
                socket_seq: 0,
            }),
            wake: Condvar::new(),
            workers: Mutex::new(Vec::new()),
            sockets: Mutex::new(Vec::new()),
        });
        let accept = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("tcisim-accept".into())
                .spawn(move || accept_loop(&shared, listener))?
        };
        let timers = {
            let shared = Arc::clone(&shared);
            thread::Builder::new()
                .name("tcisim-radio".into())
                .spawn(move || timer_loop(&shared))?
        };
        Ok(Simulator {
            addr,
            shared,
            threads: vec![accept, timers],
        })
    }

    /// Where clients connect.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// The WebSocket URL clients connect to: `ws://127.0.0.1:<port>/`.
    pub fn url(&self) -> String {
        format!("ws://{}/", self.addr)
    }

    /// When the simulator started: the zero of every [`Logged::at`].
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

    /// Every well-formed command received, on any connection, in order, as it arrived (no `;`).
    pub fn commands(&self) -> Vec<String> {
        lock(&self.shared.log)
            .iter()
            .filter_map(|l| match &l.event {
                Event::Command { text, .. } => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The commands received on one connection.
    pub fn commands_from(&self, conn: usize) -> Vec<String> {
        lock(&self.shared.log)
            .iter()
            .filter_map(|l| match &l.event {
                Event::Command { conn: c, text, .. } if *c == conn => Some(text.clone()),
                _ => None,
            })
            .collect()
    }

    /// The text messages sent to one connection, in order.
    pub fn sent_to(&self, conn: usize) -> Vec<String> {
        lock(&self.shared.log)
            .iter()
            .filter_map(|l| match &l.event {
                Event::TextOut { conn: c, text } if *c == conn => Some(text.clone()),
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

    /// The radio's state now.
    pub fn radio(&self) -> Snapshot {
        lock(&self.shared.state).radio.snapshot()
    }

    /// The live connections.
    pub fn connections(&self) -> Vec<usize> {
        lock(&self.shared.state).conns.keys().copied().collect()
    }

    /// The transmit audio a connection has sent: channel 0 of every transmit frame, in order.
    pub fn tx_audio(&self, conn: usize) -> Vec<f32> {
        lock(&self.shared.state).radio.tx_audio(conn)
    }

    /// Apply commands as the SDR's own operator (or another program): a key, a retune, a mode,
    /// `stop;`. Broadcast to every client like any change; the operator always wins.
    pub fn operator(&self, commands: &str) {
        self.with_radio(|radio, now| radio.operator(commands, now));
    }

    /// The SDR's operator clicks a spot at `hz` on the panorama.
    pub fn click_spot(&self, call: &str, hz: u64) {
        self.with_radio(|radio, now| radio.click(call, hz, now));
    }

    /// The SDR's operator clicks a spot at `hz` on transceiver `trx`'s panorama, in its receive
    /// channel `channel` (0 = A, 1 = B), in each spelling the dialect sends.
    pub fn click_spot_on(&self, trx: u32, channel: u32, call: &str, hz: u64) {
        self.with_radio(|radio, now| radio.click_on(trx, channel, call, hz, now));
    }

    /// Set what the meters read from now on.
    pub fn set_meters(&self, meters: Meters) {
        self.with_radio(|radio, _| radio.set_meters(meters));
    }

    /// Drop a connection's TCP with no close frame, as a lost network would.
    pub fn drop_connection(&self, conn: usize) {
        let state = lock(&self.shared.state);
        if let Some(c) = state.conns.get(&conn) {
            c.push(Instant::now(), What::Drop);
        }
    }

    fn with_radio(&self, f: impl FnOnce(&mut Radio, Instant)) {
        let mut state = lock(&self.shared.state);
        f(&mut state.radio, Instant::now());
        let fx = state.radio.take();
        carry_out(&self.shared, &mut state, fx);
    }
}

impl Drop for Simulator {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::SeqCst);
        self.shared.wake.notify_all();
        // Wake the accept loop: it looks at the flag after each accept.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_secs(1));
        // Every socket's reader sees its end, and its connection's thread with it.
        for (_, tcp) in lock(&self.shared.sockets).iter() {
            let _ = tcp.shutdown(Shutdown::Both);
        }
        let deadline = Instant::now() + JOIN;
        let mut left = join_by(self.threads.drain(..).collect(), deadline);
        let workers = std::mem::take(&mut *lock(&self.shared.workers));
        left += join_by(workers, deadline);
        if left > 0 {
            eprintln!(
                "tempo-tcisim: {left} thread(s) still running {JOIN:?} after the drop, left to end on their own"
            );
        }
    }
}

/// Join each of `threads` that ends by `deadline`; the rest are left running. How many were left.
fn join_by(threads: Vec<JoinHandle<()>>, deadline: Instant) -> usize {
    let mut left = 0;
    for t in threads {
        while !t.is_finished() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(1));
        }
        if t.is_finished() {
            let _ = t.join();
        } else {
            left += 1;
        }
    }
    left
}

impl Shared {
    fn stopping(&self) -> bool {
        self.stop.load(Ordering::SeqCst)
    }

    fn record(&self, event: Event) {
        // The time is read under the lock, so two threads appending cannot land out of order.
        let mut log = lock(&self.log);
        let at = self.started.elapsed();
        log.push(Logged { at, event });
        drop(log);
        self.logged.notify_all();
    }

    /// Log `event` at `at`, a time taken earlier on this thread, in its place by time.
    fn record_at(&self, at: Duration, event: Event) {
        let mut log = lock(&self.log);
        let place = log.iter().rposition(|l| l.at <= at).map_or(0, |i| i + 1);
        log.insert(place, Logged { at, event });
        drop(log);
        self.logged.notify_all();
    }
}

/// Carry out the radio's effects, under the state lock.
fn carry_out(shared: &Shared, state: &mut State, fx: Vec<Effect>) {
    let now = Instant::now();
    for effect in fx {
        match effect {
            Effect::Text { to, text } => {
                let due = now + shared.slow;
                let targets: Vec<Arc<Conn>> = match to {
                    To::All => state.conns.values().cloned().collect(),
                    To::One(c) => state.conns.get(&c).cloned().into_iter().collect(),
                };
                for conn in targets {
                    if !conn.silent.load(Ordering::SeqCst) {
                        conn.push(due, What::Text(text.clone()));
                    }
                }
            }
            Effect::Frame { to, bytes } => {
                if let Some(conn) = state.conns.get(&to) {
                    if !conn.silent.load(Ordering::SeqCst) {
                        conn.push(now, What::Frame(bytes));
                    }
                }
            }
            Effect::Timer { at, action } => {
                state.timer_seq += 1;
                state.timers.insert((at, state.timer_seq), action);
                shared.wake.notify_all();
            }
            Effect::Log(event) => shared.record(event),
            Effect::Drop(c) => {
                if let Some(conn) = state.conns.get(&c) {
                    conn.push(now, What::Drop);
                }
            }
            Effect::Silence(c) => {
                if let Some(conn) = state.conns.get(&c) {
                    conn.silent.store(true, Ordering::SeqCst);
                    lock(&conn.link.queues).out.clear();
                }
            }
            Effect::Ping(c) => {
                if let Some(conn) = state.conns.get(&c) {
                    conn.push(now, What::Ping);
                }
            }
        }
    }
}

fn accept_loop(shared: &Arc<Shared>, listener: TcpListener) {
    for incoming in listener.incoming() {
        if shared.stopping() {
            break;
        }
        let Ok(tcp) = incoming else { continue };
        let worker = {
            let shared = Arc::clone(shared);
            thread::Builder::new()
                .name("tcisim-conn".into())
                .spawn(move || serve(&shared, tcp))
        };
        if let Ok(worker) = worker {
            lock(&shared.workers).push(worker);
        }
    }
}

fn timer_loop(shared: &Shared) {
    let mut state = lock(&shared.state);
    loop {
        // A pass: the timers due when it began, each once, in order. What they set waits for a
        // later pass, so a pass ends however far behind the radio is.
        let began = Instant::now();
        let later = state.timers.split_off(&(began, u64::MAX));
        let due = std::mem::replace(&mut state.timers, later);
        for action in due.into_values() {
            if shared.stopping() {
                return;
            }
            state.radio.on_action(action, Instant::now());
            let fx = state.radio.take();
            carry_out(shared, &mut state, fx);
        }
        if shared.stopping() {
            return;
        }
        let wait = state
            .timers
            .first_key_value()
            .map_or(IDLE, |((at, _), _)| {
                at.saturating_duration_since(Instant::now())
            })
            .min(IDLE);
        state = shared
            .wake
            .wait_timeout(state, wait)
            .unwrap_or_else(|e| e.into_inner())
            .0;
    }
}

fn would_block(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut | io::ErrorKind::Interrupted
    )
}

/// A new socket: its reader, then its connection, then its end.
fn serve(shared: &Arc<Shared>, tcp: TcpStream) {
    let _ = tcp.set_nodelay(true);
    let (Ok(reading), Ok(listed)) = (tcp.try_clone(), tcp.try_clone()) else {
        return;
    };
    let token = {
        let mut state = lock(&shared.state);
        state.socket_seq += 1;
        state.socket_seq
    };
    {
        let mut sockets = lock(&shared.sockets);
        // A drop has already ended every socket it knew of, and would not see this one.
        if shared.stopping() {
            return;
        }
        sockets.push((token, listed));
    }
    let link = Arc::new(Link::default());
    let reader = {
        let link = Arc::clone(&link);
        thread::Builder::new()
            .name("tcisim-read".into())
            .spawn(move || read_loop(reading, &link))
    };
    if let Ok(reader) = reader {
        lock(&shared.workers).push(reader);
        connection(shared, tcp, &link);
    }
    // However the connection ended, its socket ends too, and its reader with it.
    let mut sockets = lock(&shared.sockets);
    if let Some(at) = sockets.iter().position(|(t, _)| *t == token) {
        let _ = sockets.swap_remove(at).1.shutdown(Shutdown::Both);
    }
}

/// A connection's thread: the WebSocket handshake (within [`HANDSHAKE`]), then all of its I/O.
fn connection(shared: &Arc<Shared>, tcp: TcpStream, link: &Arc<Link>) {
    let Ok(shutdown) = tcp.try_clone() else {
        return;
    };
    let config = WebSocketConfig::default()
        .max_message_size(Some(MAX_CLIENT_MESSAGE))
        .max_frame_size(Some(MAX_CLIENT_MESSAGE));
    let deadline = Instant::now() + HANDSHAKE;
    let wire = Wire {
        tcp,
        link: Arc::clone(link),
    };
    let mut handshake = tungstenite::accept_with_config(wire, Some(config));
    let mut ws = loop {
        match handshake {
            Ok(ws) => break ws,
            // The request is not all here yet.
            Err(HandshakeError::Interrupted(mid)) if !shared.stopping() && link.wait(deadline) => {
                handshake = mid.handshake();
            }
            Err(e) => {
                if !shared.stopping() {
                    shared.record(Event::Rejected { why: e.to_string() });
                }
                return;
            }
        }
    };
    if shared.stopping() {
        return;
    }
    let conn = {
        let mut state = lock(&shared.state);
        let id = state.next_conn;
        state.next_conn += 1;
        let conn = Arc::new(Conn {
            id,
            tcp: shutdown,
            link: Arc::clone(link),
            silent: AtomicBool::new(false),
            cut_done: AtomicBool::new(false),
        });
        state.conns.insert(id, Arc::clone(&conn));
        shared.record(Event::Connected { conn: id });
        state.radio.connect(id, Instant::now());
        let fx = state.radio.take();
        carry_out(shared, &mut state, fx);
        conn
    };
    let closer = run(shared, &conn, &mut ws);
    let _ = conn.tcp.shutdown(Shutdown::Both);
    let mut state = lock(&shared.state);
    state.conns.remove(&conn.id);
    shared.record(Event::Closed {
        conn: conn.id,
        by: closer,
    });
    state.radio.disconnect(conn.id, closer, Instant::now());
    let fx = state.radio.take();
    carry_out(shared, &mut state, fx);
}

/// A connection's I/O, until it ends; returns who ended it.
fn run(shared: &Shared, conn: &Conn, ws: &mut WebSocket<Wire>) -> Closer {
    loop {
        if shared.stopping() {
            return Closer::Stopped;
        }
        if conn.silent.load(Ordering::SeqCst) {
            // Half open: read and discard everything, answer nothing, not even a ping.
            if conn.link.discard() {
                return Closer::Dropped;
            }
            conn.link.wait(Instant::now() + IDLE);
            continue;
        }
        match write_due(shared, conn, ws) {
            Ok(true) => {}
            Ok(false) => return Closer::Simulator,
            Err(_) => return Closer::Dropped,
        }
        match ws.read() {
            Ok(Message::Text(text)) => {
                let text = text.as_str().to_string();
                shared.record(Event::TextIn {
                    conn: conn.id,
                    text: text.clone(),
                });
                let mut state = lock(&shared.state);
                state.radio.on_text(conn.id, &text, Instant::now());
                let fx = state.radio.take();
                carry_out(shared, &mut state, fx);
            }
            Ok(Message::Binary(bytes)) => {
                let mut state = lock(&shared.state);
                state.radio.on_binary(conn.id, &bytes, Instant::now());
                let fx = state.radio.take();
                carry_out(shared, &mut state, fx);
            }
            Ok(Message::Close(_)) => {
                let _ = ws.flush();
                return Closer::Client;
            }
            Ok(_) => {} // ping (answered by tungstenite), pong
            // Nothing whole has arrived: wait for more bytes, or for the next message due out.
            Err(tungstenite::Error::Io(e)) if would_block(&e) => {
                conn.link.wait(Instant::now() + IDLE);
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Closer::Client
            }
            Err(_) => return Closer::Dropped,
        }
    }
}

/// Write everything due. `Ok(false)` when a fault or the test dropped the connection.
fn write_due(shared: &Shared, conn: &Conn, ws: &mut WebSocket<Wire>) -> tungstenite::Result<bool> {
    loop {
        let due = conn.take_due(Instant::now());
        if due.is_empty() {
            return Ok(true);
        }
        let mut texts: Vec<String> = Vec::new();
        for what in due {
            if let What::Text(text) = what {
                texts.push(text);
                continue;
            }
            flush_texts(shared, conn, ws, &mut texts)?;
            match what {
                What::Text(_) => {}
                What::Frame(bytes) => {
                    let at = shared.started.elapsed();
                    let header = Header::decode(&bytes);
                    let len = bytes.len();
                    ws.send(Message::binary(bytes))?;
                    shared.record_at(
                        at,
                        Event::FrameOut {
                            conn: conn.id,
                            header,
                            bytes: len,
                        },
                    );
                }
                What::Ping => ws.send(Message::Ping(Bytes::new()))?,
                What::Drop => {
                    let _ = conn.tcp.shutdown(Shutdown::Both);
                    return Ok(false);
                }
            }
        }
        flush_texts(shared, conn, ws, &mut texts)?;
    }
}

/// Send the queued text messages: one each, or under [`Fault::SplitConcat`] all in one, cut where
/// it says.
fn flush_texts(
    shared: &Shared,
    conn: &Conn,
    ws: &mut WebSocket<Wire>,
    texts: &mut Vec<String>,
) -> tungstenite::Result<()> {
    if texts.is_empty() {
        return Ok(());
    }
    let messages: Vec<String> = match &shared.cut {
        None => std::mem::take(texts),
        Some(cut) => {
            let joined = texts.concat();
            texts.clear();
            match cut.as_ref().and_then(|c| {
                if conn.cut_done.load(Ordering::SeqCst) {
                    None
                } else {
                    cut_point(&joined, c)
                }
            }) {
                Some(at) => {
                    conn.cut_done.store(true, Ordering::SeqCst);
                    vec![joined[..at].to_string(), joined[at..].to_string()]
                }
                None => vec![joined],
            }
        }
    };
    for text in messages {
        let at = shared.started.elapsed();
        ws.send(Message::text(text.clone()))?;
        shared.record_at(
            at,
            Event::TextOut {
                conn: conn.id,
                text,
            },
        );
    }
    Ok(())
}

/// Where [`Fault::SplitConcat`] cuts `text`: inside the first command that starts with
/// `cut.starting`, `cut.at` bytes in (kept strictly inside the command, on a character boundary).
fn cut_point(text: &str, cut: &Cut) -> Option<usize> {
    let mut start = 0;
    for command in text.split_inclusive(';') {
        let lower = command.to_ascii_lowercase();
        if !cut.starting.is_empty()
            && lower.trim_start().starts_with(&cut.starting)
            && command.len() > 1
        {
            let mut at = start + cut.at.clamp(1, command.len() - 1);
            while !text.is_char_boundary(at) {
                at += 1;
            }
            return (at < start + command.len()).then_some(at);
        }
        start += command.len();
    }
    None
}

#[cfg(test)]
mod tests;
