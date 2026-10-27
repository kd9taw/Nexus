//! Nexus as a TCI server: programs on this computer (WSJT-X, JTDX, MSHV, loggers, amplifier and
//! tuner controllers) connect to Nexus as they would to an SDR program, and drive the instance's
//! active radio as TCI receiver 0.
//!
//! This module is the protocol side: the listener, the handshake gate, the connections and their
//! bounds, the closed table of what a command does, the state programs are told, and the
//! [`TciBackend`] the app implements. It never touches the radio: every change and every key is a
//! question to the backend.
//!
//! | Module | What it is |
//! |---|---|
//! | `listener` | loopback-only listeners (IPv4 and IPv6), the port, the accept loop |
//! | `handshake` | the gate: no `Origin`, a loopback `Host`, WebSocket 13, the deadline, the client cap |
//! | `conn` | one program: reader and writer threads, the outbox, keepalive, the rate, transmit audio |
//! | `dispatch` | what each command does, a closed enum |
//! | `state` | the [`Snapshot`], what each program was told, the init burst |
//! | `backend` | [`TciBackend`] and its types |
//!
//! # Security
//!
//! - **Loopback only.** `127.0.0.1` and `::1`; never a wildcard, never a LAN address.
//! - **No web pages.** A browser can open a WebSocket to loopback from any page; it must send
//!   `Origin`, and native TCI clients do not, so any handshake carrying one is refused (403).
//!   `Host` must name this computer's loopback and the port, which closes DNS rebinding.
//! - **Bounded.** [`Limits`]: programs, handshakes in progress, the handshake's deadline, message
//!   and command sizes, the command rate, the outbox, transmit audio, keepalive.
//! - **No raw path.** A command reaches the backend only as a typed call from the closed table; an
//!   unknown command is refused by the codec and counted.
//!
//! # Transmit
//!
//! The server never decides that a program may transmit: [`TciBackend::key`],
//! [`TciBackend::tune_on`] and [`TciBackend::cw`] are verdicts, every Nexus gate first. No command
//! arms anything (`tx_enable:…,true` from a program is answered with the verdict, never applied).
//! A program's transmit audio is taken only while the backend admitted its `tci` key, and is
//! dropped (never queued) otherwise. A program that disconnects, goes silent or is closed has its
//! transmissions ended first ([`TciBackend::disconnected`]), before anything else happens to its
//! connection; stopping the server ends every program's first, then tells each `trx:0,false;`,
//! then closes.
//!
//! # Locks
//!
//! The server's locks are taken in one order: the snapshot, then the list of programs, then one
//! program's session, then its outbox. The backend is never called with any of them held, and
//! [`Handle::publish`] never calls the backend, so the app may publish under its own engine lock
//! (and must: [`Snapshot`] says why).
//!
//! Each program also has a transmit gate (`conn.rs`), held only around its calls that can transmit,
//! its spots and its mode asks, and by its end, which tells the backend under it: so nothing the
//! program sends transmits after its end began, and no spot or mode ask of its is kept after it. [`Handle::disconnect`] and
//! [`Server::stop`] take it and call the backend, so the app never calls them under its backend's
//! lock, nor from inside a backend call.

mod backend;
mod conn;
mod dispatch;
mod handshake;
mod listener;
mod state;

#[cfg(test)]
mod mock;
#[cfg(test)]
mod tests;

pub use backend::{
    AudioFormat, ClientId, Closed, Cw, CwSpeed, Event, HandshakeRefusal, KeyKind, Mic, Refusal,
    Refused, Spot, TciBackend,
};
pub use conn::Counts;
pub use listener::PORTS_TRIED;
pub use state::{Snapshot, TxMeters, DEVICE, MODULATIONS, PROTOCOL, UNKNOWN_LIMITS};

use std::io;
use std::net::{SocketAddr, TcpListener};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::tci::frame::{BlockKind, Chrono, RawHeader};
use crate::tci::wire::{Call, Hz, Notice};
use crate::tci::{Channel, Trx};

use conn::{Closing, Conn};
use state::Out;

/// The port asked for first: the default of WSJT-X, JTDX, the Go TCI library and MacLoggerDX.
pub const DEFAULT_PORT: u16 = 40001;

/// The bounds a server puts on programs. [`Limits::default`] is what ships; the
/// tests shorten the clocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Programs connected at once (AetherSDR and piHPSDR cap theirs at 8).
    pub clients: usize,
    /// Handshakes in progress at once.
    pub pending: usize,
    /// The whole handshake must arrive within this.
    pub handshake: Duration,
    /// The largest text message.
    pub text_message: usize,
    /// The largest message of any kind (a binary one): WSJT-X sends 2 × `length` floats for 2048
    /// values, 16 KiB, plus the header.
    pub binary_message: usize,
    /// Commands read from one message.
    pub commands: usize,
    /// The longest command, in bytes.
    pub command_bytes: usize,
    /// Arguments one command may carry.
    pub args: usize,
    /// Commands a second, sustained.
    pub rate: u32,
    /// Commands at once.
    pub burst: u32,
    /// A program over the rate for this long is closed.
    pub flood: Duration,
    /// Unsplittable messages in a row that close a program.
    pub garbage: u32,
    /// What may wait for one program, in bytes; receive audio goes first past it.
    pub outbox: usize,
    /// Text waiting for one program past this means it stopped reading: it is closed.
    pub text_backlog: usize,
    /// How often a program is pinged, and how long it may go unheard.
    pub ping: Duration,
    pub silence: Duration,
    /// The same while it holds a key (a dead owner unkeys within 3 s).
    pub keyed_ping: Duration,
    pub keyed_silence: Duration,
    /// Transmit audio taken at most this many times real time.
    pub tx_audio_speed: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            clients: 8,
            pending: 8,
            handshake: Duration::from_secs(2),
            text_message: 16 * 1024,
            binary_message: 32 * 1024 + 64,
            commands: 64,
            command_bytes: 1024,
            args: 16,
            rate: 100,
            burst: 400,
            flood: Duration::from_secs(10),
            garbage: 3,
            outbox: 1024 * 1024,
            text_backlog: 256 * 1024,
            ping: Duration::from_secs(5),
            silence: Duration::from_secs(15),
            keyed_ping: Duration::from_secs(1),
            keyed_silence: Duration::from_secs(3),
            tx_audio_speed: 2,
        }
    }
}

/// How a server is started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Config {
    /// The port asked for; the next is taken when it is in use ([`PORTS_TRIED`]). 0 asks the
    /// system for any free one.
    pub port: u16,
    pub limits: Limits,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            port: DEFAULT_PORT,
            limits: Limits::default(),
        }
    }
}

/// A count with a ceiling, taken and given back.
#[derive(Debug)]
pub(crate) struct Slots {
    taken: AtomicUsize,
    cap: usize,
}

impl Slots {
    fn new(cap: usize) -> Slots {
        Slots {
            taken: AtomicUsize::new(0),
            cap,
        }
    }

    pub(super) fn take(&self) -> bool {
        self.taken
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| {
                (n < self.cap).then_some(n + 1)
            })
            .is_ok()
    }

    pub(super) fn give(&self) {
        let _ = self
            .taken
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1));
    }
}

/// What every thread of one server shares.
pub(crate) struct Shared {
    backend: Arc<dyn TciBackend>,
    limits: Limits,
    port: u16,
    model: Mutex<Snapshot>,
    conns: Mutex<Vec<Arc<Conn>>>,
    /// Programs connected, and handshakes in progress.
    slots: Slots,
    pending: Slots,
    next_id: AtomicU64,
    stopping: AtomicBool,
    /// Handshake refusals already reported: each kind and value once.
    reported: Mutex<Vec<HandshakeRefusal>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// How many distinct handshake refusals are remembered for reporting once.
const REPORTED_CAP: usize = 64;

impl Shared {
    fn stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    fn next_id(&self) -> ClientId {
        ClientId::new(self.next_id.fetch_add(1, Ordering::SeqCst) + 1)
    }

    fn conns(&self) -> Vec<Arc<Conn>> {
        lock(&self.conns).clone()
    }

    fn find(&self, client: ClientId) -> Option<Arc<Conn>> {
        lock(&self.conns).iter().find(|c| c.id == client).cloned()
    }

    fn register(&self, conn: Arc<Conn>) {
        lock(&self.conns).push(conn);
    }

    /// Remove a program and give back its slot.
    fn unregister(&self, client: ClientId) {
        let removed = {
            let mut conns = lock(&self.conns);
            let before = conns.len();
            conns.retain(|c| c.id != client);
            before != conns.len()
        };
        if removed {
            self.slots.give();
        }
    }

    fn event(&self, event: Event) {
        self.backend.event(event);
    }

    /// Report a handshake refusal the first time it is seen (a page reloading, a scanner).
    fn refused(&self, event: Event) {
        if let Event::Refused { why, .. } = &event {
            let mut seen = lock(&self.reported);
            if seen.contains(why) {
                return;
            }
            if seen.len() >= REPORTED_CAP {
                seen.remove(0);
            }
            seen.push(why.clone());
        }
        self.event(event);
    }

    /// State changes from one program's command, to every program.
    fn deliver(&self, sender: ClientId, outs: &[Out]) {
        if outs.is_empty() {
            return;
        }
        for conn in self.conns() {
            conn::deliver(&conn, sender, outs);
        }
    }
}

/// A running server. Dropping it stops it, as [`Server::stop`] does.
pub struct Server {
    shared: Arc<Shared>,
    accept: Option<JoinHandle<()>>,
    addrs: Vec<SocketAddr>,
}

/// One connected program, as the app sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Program {
    pub id: ClientId,
    pub peer: SocketAddr,
    /// Its stream settings.
    pub format: AudioFormat,
    /// Its receive audio is started.
    pub audio: bool,
    /// A key this server admitted for it and has not seen end.
    pub key: Option<KeyKind>,
    pub counts: Counts,
}

impl Server {
    /// Bind this computer's loopback on `config.port` (or the next, [`PORTS_TRIED`]) and start
    /// serving, with `snapshot` as the state until the app publishes another.
    pub fn start(
        config: Config,
        backend: Arc<dyn TciBackend>,
        snapshot: Snapshot,
    ) -> io::Result<Server> {
        let listeners: Vec<TcpListener> = listener::bind(config.port)?;
        let addrs = listener::addrs(&listeners);
        let port = addrs.first().map_or(config.port, SocketAddr::port);
        let shared = Arc::new(Shared {
            backend,
            limits: config.limits,
            port,
            model: Mutex::new(snapshot),
            conns: Mutex::new(Vec::new()),
            slots: Slots::new(config.limits.clients),
            pending: Slots::new(config.limits.pending),
            next_id: AtomicU64::new(0),
            stopping: AtomicBool::new(false),
            reported: Mutex::new(Vec::new()),
        });
        let worker = Arc::clone(&shared);
        let accept = std::thread::Builder::new()
            .name("tci-accept".into())
            .spawn(move || listener::accept_loop(worker, listeners))?;
        Ok(Server {
            shared,
            accept: Some(accept),
            addrs,
        })
    }

    /// The port the server got.
    pub fn port(&self) -> u16 {
        self.shared.port
    }

    /// The addresses it listens on: loopback, IPv4 and (where this computer has it) IPv6.
    pub fn local_addrs(&self) -> &[SocketAddr] {
        &self.addrs
    }

    /// What the app uses while the server runs.
    pub fn handle(&self) -> Handle {
        Handle {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Stop: every program's transmissions end first, then each is told
    /// `trx:0,false;`, then the connections close. The port is free when this returns.
    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        if self.shared.stopping.swap(true, Ordering::SeqCst) {
            return;
        }
        let conns = self.shared.conns();
        for c in &conns {
            c.end_transmit(&self.shared);
        }
        for c in &conns {
            conn::tell(c, Out::Trx(false));
        }
        for c in &conns {
            c.close(Closing::new(Closed::Stopped));
        }
        if let Some(accept) = self.accept.take() {
            let _ = accept.join();
        }
        // The connections finish on their own threads; give them a moment, never forever.
        let until = Instant::now() + Duration::from_secs(3);
        while !lock(&self.shared.conns).is_empty() && Instant::now() < until {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// What the app uses while the server runs: publish the station, send audio, see the programs.
/// Cheap to clone; every call is quick and none waits on a program.
#[derive(Clone)]
pub struct Handle {
    shared: Arc<Shared>,
}

impl Handle {
    /// The station now. Each program is sent what changed for it. Never calls the backend, so the
    /// app may (and should) publish under the lock its backend's commands take.
    pub fn publish(&self, snapshot: Snapshot) {
        let mut model = lock(&self.shared.model);
        *model = snapshot;
        for conn in self.shared.conns() {
            conn::publish(&conn, &model);
        }
    }

    /// The connected programs.
    pub fn programs(&self) -> Vec<Program> {
        self.shared
            .conns()
            .iter()
            .map(|c| {
                let session = lock(&c.session);
                Program {
                    id: c.id,
                    peer: c.peer,
                    format: session.format,
                    audio: session.audio,
                    key: session.key,
                    counts: c.counts(),
                }
            })
            .collect()
    }

    /// Send one receive-audio frame (type 1, receiver 0, in the program's format) to a program
    /// whose audio is started. `false` when it is not, or the frame is not receive audio. The
    /// oldest frames are dropped when the program is not reading.
    pub fn send_rx_audio(&self, client: ClientId, frame: Vec<u8>) -> bool {
        let rx_audio = RawHeader::read(&frame)
            .is_some_and(|h| h.kind == BlockKind::RxAudio.code() && h.receiver == 0);
        let Some(conn) = self.shared.find(client) else {
            return false;
        };
        if !rx_audio || !lock(&conn.session).audio {
            return false;
        }
        conn.push_audio(frame);
        true
    }

    /// Ask the program holding an admitted `tci` key for transmit audio (TX_CHRONO, receiver 0).
    /// `false`, and nothing sent, to any other program: no chrono without an armed over, and none
    /// to a program that does not own it.
    pub fn send_chrono(&self, client: ClientId, chrono: &Chrono) -> bool {
        let Some(conn) = self.shared.find(client) else {
            return false;
        };
        if chrono.trx.get() != 0 || lock(&conn.session).key != Some(KeyKind::Tci) {
            return false;
        }
        let Ok(bytes) = chrono.encode() else {
            return false;
        };
        conn.push_chrono(bytes);
        true
    }

    /// The operator clicked a spot in Nexus: every program is told, in both spellings.
    /// Tuning is Nexus's; a click never transmits.
    pub fn spot_clicked(&self, call: &Call, hz: Hz) {
        let (Some(trx), Some(channel)) = (Trx::new(0), Channel::new(0)) else {
            return;
        };
        let lines = || {
            vec![
                Notice::ClickedOnSpot {
                    call: call.clone(),
                    hz,
                }
                .render(),
                Notice::RxClickedOnSpot {
                    trx,
                    channel,
                    call: call.clone(),
                    hz,
                }
                .render(),
            ]
        };
        for conn in self.shared.conns() {
            let session = lock(&conn.session);
            if session.inited {
                conn.push_lines(lines());
            }
        }
    }

    /// The operator's Disconnect: the program's transmissions end (after any of its transmit
    /// calls in flight returns), then it is closed. Nothing it sends transmits after this returns.
    pub fn disconnect(&self, client: ClientId) {
        if let Some(conn) = self.shared.find(client) {
            conn.end_transmit(&self.shared);
            conn.close(Closing::new(Closed::Disconnected));
        }
    }

    /// The port the server got.
    pub fn port(&self) -> u16 {
        self.shared.port
    }
}
