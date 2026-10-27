//! One program's connection, after its handshake: two threads, an outbox between them, and every
//! bound the server puts on a program.
//!
//! # Threads
//!
//! The **reader** (the thread that ran the handshake) owns the WebSocket and reads with a short
//! timeout, so it also notices a close asked for elsewhere, a silent peer and a flood. It runs each
//! command ([`super::dispatch`]) and hands a transmit frame on, never blocking on the program. The
//! **writer** owns a second handle on the socket and is the only thread that writes to it: the
//! outbox's frames, the pings, the sensors. Each of its writes has [`WRITE_STALL`] in all, or the
//! program is taken for gone. Nothing else waits on a program: the app's publisher, its audio pump
//! and the other programs' readers only push to the outbox, which is bounded.
//!
//! tungstenite answers a ping or a close itself, from inside the reader's `read`. Its writes go to
//! [`Wire`], which hands each one to the outbox whole (one write is whole frames: tungstenite
//! formats a frame, then writes its buffer in one call), so the writer stays the socket's only
//! writer and a pong never lands inside another frame.
//!
//! # The outbox
//!
//! Control frames first, then TX_CHRONO, then text, then receive audio. At most
//! [`Limits::outbox`] bytes a program: receive audio goes first when it is full. A text backlog
//! over [`Limits::text_backlog`] means the program stopped reading, and it is closed.
//!
//! # Bounds on what a program sends
//!
//! A text message over [`Limits::text_message`], or any message over [`Limits::binary_message`]
//! (tungstenite's own cap), closes the connection with 1009. At most [`Limits::commands`] commands
//! a message, [`Limits::args`] arguments and [`Limits::command_bytes`] bytes a command. Commands
//! cost one token each from a bucket of [`Limits::burst`] refilled at [`Limits::rate`] a second;
//! the excess is dropped and counted, and a program over the rate for [`Limits::flood`] is closed.
//! A stop still goes through while a program is over the rate, at most one of each kind every
//! [`STOP_SPACING`]: dropping a program's own `trx:0,false;` is never the safe way. Messages in a
//! row that hold nothing the codec could even split ([`Limits::garbage`]) close it too.
//!
//! Transmit audio is taken only from the program holding an admitted `tci` key; every other
//! binary frame is dropped and counted, never queued for a later key. A frame
//! must claim at least one whole frame, carry all it claims in whole samples, and carry no more
//! than twice it (WSJT-X sends twice and fills the first half): otherwise its claim and its payload
//! disagree, and it is refused as bad. The backend is handed the claimed values alone, and they are
//! paid for at most [`Limits::tx_audio_speed`] times real time, each frame at least
//! [`MIN_TX_VALUES`] values, so neither a frame's own `length` nor a flood of tiny frames can buy
//! audio or backend calls for nothing.
//!
//! # A program's end
//!
//! Everything that can transmit for one program (a key, a Tune, CW, its transmit audio) reaches the
//! backend through that program's gate ([`Conn::transmit`]), and only while its end has not begun.
//! The end ([`Conn::end_transmit`], from the program's own reader, the operator's Disconnect or
//! the server's stop) takes the same gate to mark it begun and tell the backend. So a call in
//! flight finishes first and its end ends what it admitted, and nothing the program sends can
//! transmit after its end began. Its spots and its mode asks pass the same gate, so none is kept for
//! a program whose end began: the end takes away what it had. The reader also stops running a
//! message the moment its program's end or close has begun.

use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use tungstenite::protocol::frame::coding::{CloseCode, Data, OpCode};
use tungstenite::protocol::frame::{CloseFrame, Frame, Utf8Bytes};
use tungstenite::{Message, WebSocket};

use crate::tci::frame::{BlockKind, RawHeader, HEADER_LEN};
use crate::tci::wire::{self, Fault, Inbound, Raw, ServerText, WireError};
use crate::tci::EXPERT_SDR3;

use super::backend::{AudioFormat, ClientId, Closed, Event, KeyKind};
use super::dispatch::{self, Cx, Sink, StopKind};
use super::state::{rx_sensor_lines, tx_sensor_lines, Out, Session, Snapshot};
use super::{Limits, Shared};

/// How often the reader wakes with nothing to read, to notice a close asked for elsewhere, a
/// silent program and a flood.
const READ_WAKE: Duration = Duration::from_millis(100);

/// How long one write to a program may take, all of it, before the program is taken for gone: one
/// that takes it a few bytes at a time is cut off here, not at a timeout on each write of the
/// socket ([`Stall`]).
pub(super) const WRITE_STALL: Duration = Duration::from_secs(5);

/// How long a closing connection is read (and discarded) for the program's own close, and how much
/// of it.
const CLOSE_LINGER: Duration = Duration::from_secs(2);
const DRAIN_CAP: usize = 1 << 20;

/// While a program is over the rate, one stop of each kind gets through this often.
pub(super) const STOP_SPACING: Duration = Duration::from_millis(100);

/// The most frames the writer gathers into one socket write.
const BATCH: usize = 64 * 1024;

/// Control frames and TX_CHRONOs held at most: a ping flood's pongs are dropped past this, never a
/// close.
const QUEUE_CAP: usize = 64;

/// The least a transmit frame is charged, in values: the smallest frame a program may stream
/// (`audio_stream_samples`, §4.3).
pub(super) const MIN_TX_VALUES: usize = *AudioFormat::SAMPLES.start() as usize;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// What a program has done, for the UI and the tests.
#[derive(Debug, Default)]
pub(super) struct Counters {
    commands: AtomicU64,
    unreadable: AtomicU64,
    rate_dropped: AtomicU64,
    bad_frames: AtomicU64,
    tx_frames: AtomicU64,
    tx_dropped: AtomicU64,
    audio_dropped: AtomicU64,
}

/// A program's counters at one moment.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    /// Commands run (accepted, mapped or refused by the table).
    pub commands: u64,
    /// Commands the codec could not read, or over a command's bounds.
    pub unreadable: u64,
    /// Commands dropped over the rate.
    pub rate_dropped: u64,
    /// Binary frames refused: short, not transmit audio, not receiver 0.
    pub bad_frames: u64,
    /// Transmit frames handed to the backend.
    pub tx_frames: u64,
    /// Transmit frames dropped: not the key holder's, or over twice real time.
    pub tx_dropped: u64,
    /// Receive-audio frames dropped because the program was not reading.
    pub audio_dropped: u64,
}

fn bump(c: &AtomicU64) {
    c.fetch_add(1, Ordering::Relaxed);
}

impl Counters {
    fn read(&self) -> Counts {
        let r = |c: &AtomicU64| c.load(Ordering::Relaxed);
        Counts {
            commands: r(&self.commands),
            unreadable: r(&self.unreadable),
            rate_dropped: r(&self.rate_dropped),
            bad_frames: r(&self.bad_frames),
            tx_frames: r(&self.tx_frames),
            tx_dropped: r(&self.tx_dropped),
            audio_dropped: r(&self.audio_dropped),
        }
    }
}

/// A close the server asked for: the code and reason sent, whether what was already queued for
/// the program goes first (a stop's last `trx:0,false;`), and why, for the log.
#[derive(Debug, Clone, Copy)]
pub(super) struct Closing {
    code: u16,
    reason: &'static str,
    graceful: bool,
    pub(super) why: Closed,
}

impl Closing {
    pub(super) fn new(why: Closed) -> Closing {
        let (code, reason, graceful) = match why {
            Closed::TooBig => (1009, "message too big", false),
            Closed::Flood => (1008, "over the command rate", false),
            Closed::Garbage => (1008, "not TCI", false),
            Closed::Backlog => (1008, "not reading", false),
            Closed::Protocol => (1002, "protocol error", false),
            Closed::Silent => (1001, "no answer to ping", false),
            Closed::Stopped => (1001, "Nexus stopped its TCI server", true),
            Closed::Disconnected => (1000, "disconnected in Nexus", true),
            Closed::Program | Closed::Lost => (1000, "", true),
        };
        Closing {
            code,
            reason,
            graceful,
            why,
        }
    }
}

/// What waits to be written to one program.
#[derive(Debug, Default)]
struct Outbox {
    control: VecDeque<Vec<u8>>,
    chrono: VecDeque<Vec<u8>>,
    text: VecDeque<String>,
    audio: VecDeque<Vec<u8>>,
    text_bytes: usize,
    audio_bytes: usize,
    close: Option<Closing>,
    close_written: bool,
    /// The reader has ended: write what is left, then stop.
    done: bool,
}

/// A frame's bytes as the server sends it: unmasked, final.
fn frame_bytes(frame: Frame, out: &mut Vec<u8>) {
    // Formatting into a Vec cannot fail.
    let _ = frame.format(out);
}

fn text_frame(line: String, out: &mut Vec<u8>) {
    frame_bytes(
        Frame::message(line.into_bytes(), OpCode::Data(Data::Text), true),
        out,
    );
}

fn binary_frame(bytes: Vec<u8>, out: &mut Vec<u8>) {
    frame_bytes(Frame::message(bytes, OpCode::Data(Data::Binary), true), out);
}

impl Outbox {
    fn bytes(&self) -> usize {
        self.text_bytes + self.audio_bytes
    }

    /// Room for `outbox` bytes: receive audio goes first. Returns how many audio frames went.
    fn make_room(&mut self, outbox: usize) -> u64 {
        let mut dropped = 0;
        while self.bytes() > outbox {
            let Some(frame) = self.audio.pop_front() else {
                break;
            };
            self.audio_bytes -= frame.len();
            dropped += 1;
        }
        dropped
    }

    /// Gather frames into one write: control first, then TX_CHRONO, then text, then receive audio.
    /// While closing, only control frames and (for a graceful close) the queued text.
    fn gather(&mut self, out: &mut Vec<u8>) {
        while out.len() < BATCH {
            if let Some(bytes) = self.control.pop_front() {
                out.extend_from_slice(&bytes);
                continue;
            }
            let closing = self.close.is_some();
            if !closing {
                if let Some(bytes) = self.chrono.pop_front() {
                    binary_frame(bytes, out);
                    continue;
                }
            }
            if !closing || self.close.is_some_and(|c| c.graceful) {
                if let Some(line) = self.text.pop_front() {
                    self.text_bytes -= line.len();
                    text_frame(line, out);
                    continue;
                }
            }
            if !closing {
                if let Some(frame) = self.audio.pop_front() {
                    self.audio_bytes -= frame.len();
                    binary_frame(frame, out);
                    continue;
                }
            }
            break;
        }
    }
}

/// The socket as tungstenite sees it. During the handshake it reads against the deadline and
/// writes to the socket (the HTTP answer); once open, it reads with [`READ_WAKE`] and hands every
/// write (tungstenite's pongs and close replies) to the program's outbox.
pub(super) struct Wire {
    tcp: TcpStream,
    deadline: Option<Instant>,
    conn: Option<Arc<Conn>>,
}

impl Wire {
    pub(super) fn handshake(tcp: TcpStream, deadline: Instant) -> Wire {
        Wire {
            tcp,
            deadline: Some(deadline),
            conn: None,
        }
    }

    fn open(&mut self, conn: Arc<Conn>) -> io::Result<()> {
        self.tcp.set_read_timeout(Some(READ_WAKE))?;
        self.deadline = None;
        self.conn = Some(conn);
        Ok(())
    }
}

fn is_timeout(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    )
}

impl Read for Wire {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let Some(deadline) = self.deadline else {
            return self.tcp.read(buf);
        };
        // The whole handshake inside the deadline, however the bytes trickle in.
        let timed_out = || io::Error::new(io::ErrorKind::TimedOut, "handshake deadline");
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(timed_out());
        }
        self.tcp.set_read_timeout(Some(left))?;
        match self.tcp.read(buf) {
            Err(e) if is_timeout(&e) => Err(timed_out()),
            other => other,
        }
    }
}

impl Write for Wire {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match &self.conn {
            None => self.tcp.write(buf),
            Some(conn) => {
                conn.push_control(buf.to_vec());
                Ok(buf.len())
            }
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match &self.conn {
            None => self.tcp.flush(),
            Some(_) => Ok(()),
        }
    }
}

/// One program.
pub(super) struct Conn {
    pub(super) id: ClientId,
    pub(super) peer: SocketAddr,
    /// A handle on the socket for `shutdown`, from any thread.
    tcp: TcpStream,
    out: Mutex<Outbox>,
    ready: Condvar,
    pub(super) session: Mutex<Session>,
    born: Instant,
    /// When the last frame arrived, in milliseconds since `born`.
    heard: AtomicU64,
    /// The program's transmit gate: true once its end has begun and the backend has been told. See
    /// the module header.
    gate: Mutex<bool>,
    counters: Counters,
    limits: Limits,
}

impl Sink for Conn {
    fn send(&self, lines: Vec<ServerText>) {
        self.push_lines(lines);
    }
}

impl Conn {
    fn new(id: ClientId, peer: SocketAddr, tcp: TcpStream, limits: Limits) -> Conn {
        Conn {
            id,
            peer,
            tcp,
            limits,
            out: Mutex::new(Outbox::default()),
            ready: Condvar::new(),
            session: Mutex::new(Session::default()),
            born: Instant::now(),
            heard: AtomicU64::new(0),
            gate: Mutex::new(false),
            counters: Counters::default(),
        }
    }

    pub(super) fn counts(&self) -> Counts {
        self.counters.read()
    }

    fn heard_now(&self) {
        let ms = u64::try_from(self.born.elapsed().as_millis()).unwrap_or(u64::MAX);
        self.heard.store(ms, Ordering::Relaxed);
    }

    fn quiet_for(&self) -> Duration {
        let heard = Duration::from_millis(self.heard.load(Ordering::Relaxed));
        self.born.elapsed().saturating_sub(heard)
    }

    fn holds_key(&self) -> bool {
        lock(&self.session).key.is_some()
    }

    /// Queue lines for this program. The caller holds the session lock when the lines carry state
    /// (the lock order: session, then outbox). A text backlog past the limit closes the program.
    pub(super) fn push_lines(&self, lines: Vec<ServerText>) {
        if lines.is_empty() {
            return;
        }
        let limits = self.limits;
        let backlog = {
            let mut out = lock(&self.out);
            if out.close.is_some() {
                return;
            }
            for line in lines {
                let line = line.into_string();
                out.text_bytes += line.len();
                out.text.push_back(line);
            }
            let dropped = out.make_room(limits.outbox);
            self.counters
                .audio_dropped
                .fetch_add(dropped, Ordering::Relaxed);
            out.text_bytes > limits.text_backlog
        };
        if backlog {
            self.close(Closing::new(Closed::Backlog));
        }
        self.ready.notify_all();
    }

    /// Queue one receive-audio frame; the oldest go first when the outbox is full.
    pub(super) fn push_audio(&self, frame: Vec<u8>) {
        let limits = self.limits;
        {
            let mut out = lock(&self.out);
            if out.close.is_some() {
                return;
            }
            out.audio_bytes += frame.len();
            out.audio.push_back(frame);
            let dropped = out.make_room(limits.outbox);
            self.counters
                .audio_dropped
                .fetch_add(dropped, Ordering::Relaxed);
        }
        self.ready.notify_all();
    }

    /// Queue one TX_CHRONO.
    pub(super) fn push_chrono(&self, frame: Vec<u8>) {
        {
            let mut out = lock(&self.out);
            if out.close.is_some() {
                return;
            }
            if out.chrono.len() >= QUEUE_CAP {
                out.chrono.pop_front();
            }
            out.chrono.push_back(frame);
        }
        self.ready.notify_all();
    }

    /// Queue control bytes tungstenite wrote (whole frames). Past [`QUEUE_CAP`] a pong is dropped;
    /// a close never is.
    fn push_control(&self, bytes: Vec<u8>) {
        {
            let mut out = lock(&self.out);
            let is_close = bytes.first().is_some_and(|b| b & 0x0f == 0x8);
            if out.control.len() >= QUEUE_CAP && !is_close {
                return;
            }
            out.control.push_back(bytes);
        }
        self.ready.notify_all();
    }

    /// Ask for the connection to close. The first reason wins.
    pub(super) fn close(&self, closing: Closing) {
        {
            let mut out = lock(&self.out);
            if out.close.is_none() {
                out.close = Some(closing);
            }
        }
        self.ready.notify_all();
    }

    fn closing(&self) -> Option<Closing> {
        lock(&self.out).close
    }

    /// The program's end begins: once its transmit calls in flight have returned, mark it begun
    /// and tell the backend, which ends whatever the program keyed (those calls' included). Nothing
    /// it sends reaches a transmit call after this. Once.
    pub(super) fn end_transmit(&self, shared: &Shared) {
        let mut ended = lock(&self.gate);
        if !*ended {
            *ended = true;
            shared.backend.disconnected(self.id);
        }
    }

    /// Whether the program's end has begun.
    fn ended(&self) -> bool {
        *lock(&self.gate)
    }

    /// Run a call that can transmit through the program's gate: only while its end has not begun,
    /// and holding that end off until the call returns. `None`: the end has begun, and nothing ran.
    pub(super) fn transmit<T>(&self, call: impl FnOnce() -> T) -> Option<T> {
        let ended = lock(&self.gate);
        (!*ended).then(call)
    }

    /// The connection is over: end its transmissions first, then free its slot, then let the
    /// writer finish.
    fn farewell(&self, shared: &Shared, why: Closed) {
        self.end_transmit(shared);
        {
            let mut session = lock(&self.session);
            session.key = None;
            session.audio = false;
        }
        shared.unregister(self.id);
        {
            let mut out = lock(&self.out);
            out.done = true;
        }
        self.ready.notify_all();
        if why != Closed::Program {
            let _ = self.tcp.shutdown(Shutdown::Both);
        }
        shared.event(Event::Closed {
            client: self.id,
            why,
        });
    }

    /// The init burst, under the snapshot and session locks so no snapshot slips in before it.
    fn greet(&self, shared: &Shared) {
        let model = lock(&shared.model);
        let mut session = lock(&self.session);
        let lines = session.greet(&model, self.id);
        self.push_lines(lines);
    }
}

// ── The connection's life ───────────────────────────────────────────────────────────────────

/// Serve one accepted connection: the handshake, then the program. Runs on the connection's own
/// thread, which becomes its reader.
pub(super) fn serve(shared: Arc<Shared>, tcp: TcpStream, peer: SocketAddr) {
    let ws = super::handshake::accept(&shared, tcp, peer);
    shared.pending.give();
    let Some((mut ws, writer_tcp, handle)) = ws else {
        return;
    };
    if shared.stopping() {
        shared.slots.give();
        let _ = handle.shutdown(Shutdown::Both);
        return;
    }
    let id = shared.next_id();
    let conn = Arc::new(Conn::new(id, peer, handle, shared.limits));
    if ws.get_mut().open(Arc::clone(&conn)).is_err() {
        shared.slots.give();
        return;
    }
    shared.register(Arc::clone(&conn));
    if shared.stopping() {
        // The stop listed the programs before this one joined: it closes itself.
        conn.close(Closing::new(Closed::Stopped));
    }
    shared.backend.connected(id, peer);
    shared.event(Event::Connected { client: id, peer });
    conn.greet(&shared);
    let writer = {
        let conn = Arc::clone(&conn);
        let shared = Arc::clone(&shared);
        std::thread::Builder::new()
            .name(format!("tci-out-{}", id.get()))
            .spawn(move || {
                let _guard = WriterGuard(&conn);
                write_loop(&conn, writer_tcp, &shared);
            })
    };
    let mut farewell = Farewell {
        conn: &conn,
        shared: &shared,
        why: Closed::Lost,
    };
    farewell.why = match writer {
        Ok(_) => read_loop(&conn, &mut ws, &shared),
        Err(_) => Closed::Lost,
    };
}

/// Ends a program however its reader ends: with the reason on a return, as lost on a panic. Its
/// transmissions end in both cases (`Conn::farewell`).
struct Farewell<'a> {
    conn: &'a Conn,
    shared: &'a Shared,
    why: Closed,
}

impl Drop for Farewell<'_> {
    fn drop(&mut self) {
        self.conn.farewell(self.shared, self.why);
    }
}

/// A writer that panics takes the connection with it, so the reader wakes and ends the program.
struct WriterGuard<'a>(&'a Conn);

impl Drop for WriterGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            self.0.close(Closing::new(Closed::Lost));
            let _ = self.0.tcp.shutdown(Shutdown::Both);
        }
    }
}

/// A bucket of tokens: `cap` at most, refilled at `per_sec`.
#[derive(Debug, Clone, Copy)]
pub(super) struct Bucket {
    level: f64,
    cap: f64,
    per_sec: f64,
    at: Instant,
}

impl Bucket {
    pub(super) fn new(cap: f64, per_sec: f64, now: Instant) -> Bucket {
        Bucket {
            level: cap,
            cap,
            per_sec,
            at: now,
        }
    }

    pub(super) fn take(&mut self, n: f64, now: Instant) -> bool {
        let dt = now.saturating_duration_since(self.at).as_secs_f64();
        self.level = (self.level + dt * self.per_sec).min(self.cap);
        self.at = now;
        if self.level >= n {
            self.level -= n;
            true
        } else {
            false
        }
    }
}

/// The command rate: a bucket, the run of seconds over it, and the stops that get
/// through anyway.
#[derive(Debug)]
pub(super) struct Limiter {
    bucket: Bucket,
    /// When this run over the rate began, and the last drop in it.
    over_since: Option<Instant>,
    last_drop: Option<Instant>,
    stops: [Option<Instant>; 4],
}

impl Limiter {
    pub(super) fn new(limits: &Limits, now: Instant) -> Limiter {
        Limiter {
            bucket: Bucket::new(f64::from(limits.burst), f64::from(limits.rate), now),
            over_since: None,
            last_drop: None,
            stops: [None; 4],
        }
    }

    /// Whether one command may run now. A stop over the rate still runs, one of each kind every
    /// [`STOP_SPACING`].
    pub(super) fn allow(&mut self, stop: Option<StopKind>, now: Instant) -> bool {
        if self.bucket.take(1.0, now) {
            return true;
        }
        self.dropped(now);
        let Some(kind) = stop else {
            return false;
        };
        let slot = &mut self.stops[kind as usize];
        if slot.is_none_or(|t| now.saturating_duration_since(t) >= STOP_SPACING) {
            *slot = Some(now);
            true
        } else {
            false
        }
    }

    fn dropped(&mut self, now: Instant) {
        // Drops less than a second apart are one run.
        let fresh = self
            .last_drop
            .is_none_or(|t| now.saturating_duration_since(t) > Duration::from_secs(1));
        if fresh {
            self.over_since = Some(now);
        }
        self.last_drop = Some(now);
    }

    /// The program has been over the rate, without a second's break, for at least `limit`.
    pub(super) fn flooding(&self, limit: Duration, now: Instant) -> bool {
        match (self.over_since, self.last_drop) {
            (Some(since), Some(last)) => {
                now.saturating_duration_since(last) <= Duration::from_secs(1)
                    && now.saturating_duration_since(since) >= limit
            }
            _ => false,
        }
    }
}

/// What the reader keeps between messages.
struct Reader {
    limiter: Limiter,
    /// Transmit audio allowed, in seconds of audio.
    tx_budget: Bucket,
    /// Messages in a row that held nothing the codec could split.
    garbage_run: u32,
}

/// Is this refusal the codec's "this is not TCI at all", rather than a command it does not know
/// or whose arguments are wrong?
fn structural(e: &WireError) -> bool {
    !matches!(
        e,
        WireError::Command {
            fault: Fault::Unknown | Fault::Direction | Fault::Arity(_) | Fault::BadArg(_),
            ..
        }
    )
}

fn command_bytes(raw: &Raw<'_>) -> usize {
    raw.name().len() + raw.args().iter().map(|a| a.len() + 1).sum::<usize>() + 1
}

/// A transmit frame cut to the `length` values it claims, read with the program's own format
/// (WSJT-X leaves the header's `channels` unset). `None` when the claim and the payload disagree:
/// it must claim at least one whole frame, carry whole samples, carry no more than twice its claim
/// (WSJT-X sends twice and fills the first half), and carry all of it.
pub(super) fn tx_payload<'a>(
    bytes: &'a [u8],
    length: u32,
    format: &AudioFormat,
) -> Option<&'a [u8]> {
    let size = format.sample_type.bytes();
    let length = usize::try_from(length).ok()?;
    let carried = bytes.len().checked_sub(HEADER_LEN)?;
    let whole_frames = length > 0 && length.is_multiple_of(format.channels.count());
    let whole_samples = carried.is_multiple_of(size);
    let at_most_twice = carried / size <= length.saturating_mul(2);
    if !(whole_frames && whole_samples && at_most_twice) {
        return None;
    }
    // Carrying less than it claims, the slice does not exist.
    bytes.get(..length.checked_mul(size)?.checked_add(HEADER_LEN)?)
}

impl Reader {
    fn new(limits: &Limits) -> Reader {
        let now = Instant::now();
        Reader {
            limiter: Limiter::new(limits, now),
            // Half a second of audio at once; refilled at twice real time.
            tx_budget: Bucket::new(0.5, f64::from(limits.tx_audio_speed), now),
            garbage_run: 0,
        }
    }

    /// One text message. `Some` closes the program.
    fn on_text(&mut self, conn: &Conn, shared: &Shared, text: &str) -> Option<Closed> {
        let limits = &shared.limits;
        let now = Instant::now();
        let (mut readable, mut garbage) = (false, false);
        for (i, item) in wire::split(text).enumerate() {
            // Its end or its close has begun (the operator's Disconnect, a stop, a backlog): the
            // rest of the message is not run.
            if conn.ended() || conn.closing().is_some() {
                break;
            }
            if i >= limits.commands {
                // The rest of the message is refused, counted once.
                bump(&conn.counters.unreadable);
                break;
            }
            let raw = match item {
                Ok(raw) => raw,
                Err(e) => {
                    bump(&conn.counters.unreadable);
                    garbage |= structural(&e);
                    continue;
                }
            };
            if raw.args().len() > limits.args || command_bytes(&raw) > limits.command_bytes {
                bump(&conn.counters.unreadable);
                garbage = true;
                continue;
            }
            readable = true;
            let inbound: Inbound = match Inbound::parse(&raw, &EXPERT_SDR3) {
                Ok(inbound) => inbound,
                Err(_) => {
                    bump(&conn.counters.unreadable);
                    continue;
                }
            };
            let action = dispatch::classify(inbound);
            if !self.limiter.allow(action.stop(), now) {
                bump(&conn.counters.rate_dropped);
                continue;
            }
            bump(&conn.counters.commands);
            run(conn, shared, action);
        }
        if readable {
            self.garbage_run = 0;
        } else if garbage {
            self.garbage_run += 1;
            if self.garbage_run >= limits.garbage {
                return Some(Closed::Garbage);
            }
        }
        None
    }

    /// One binary message: transmit audio from the key holder, or nothing.
    fn on_binary(&mut self, conn: &Conn, shared: &Shared, bytes: &[u8]) {
        let now = Instant::now();
        let audio = RawHeader::read(bytes)
            .filter(|h| h.kind == BlockKind::TxAudio.code() && h.receiver == 0);
        let Some(head) = audio else {
            // Short, not transmit audio, or not receiver 0: refused, counted, and paid for.
            bump(&conn.counters.bad_frames);
            let _ = self.limiter.allow(None, now);
            return;
        };
        let (key, format) = {
            let session = lock(&conn.session);
            (session.key, session.format)
        };
        if key != Some(KeyKind::Tci) {
            // Not the key holder's: dropped, never queued for a later key.
            bump(&conn.counters.tx_dropped);
            let _ = self.limiter.allow(None, now);
            return;
        }
        let Some(paid) = tx_payload(bytes, head.length, &format) else {
            // Its claim and its payload disagree: refused, counted, and paid for.
            bump(&conn.counters.bad_frames);
            let _ = self.limiter.allow(None, now);
            return;
        };
        // Charged for the values handed on, and never less than the smallest frame a program may
        // stream.
        let values = (head.length as usize).max(MIN_TX_VALUES);
        let seconds = (values / format.channels.count()) as f64 / f64::from(format.rate.hz());
        if !self.tx_budget.take(seconds, now) {
            bump(&conn.counters.tx_dropped);
            return;
        }
        match conn.transmit(|| shared.backend.tx_audio(conn.id, format, paid)) {
            Some(()) => bump(&conn.counters.tx_frames),
            // The program's end has begun.
            None => bump(&conn.counters.tx_dropped),
        }
    }
}

/// Run one command and deliver what it produced, with no lock held.
fn run(conn: &Conn, shared: &Shared, action: dispatch::Action) {
    let fx = dispatch::run(
        action,
        &Cx {
            client: conn.id,
            backend: shared.backend.as_ref(),
            model: &shared.model,
            session: &conn.session,
            sink: conn,
            gate: &conn.gate,
        },
    );
    shared.deliver(conn.id, &fx.all);
    for event in fx.events {
        shared.event(event);
    }
    // A key or a subscription may have changed the writer's timers.
    conn.ready.notify_all();
}

/// Read and discard what is left of a closing connection, until the program closes it or
/// [`CLOSE_LINGER`]: so the program reads the close frame before the socket goes, rather than a
/// reset. tungstenite is not used: after a capacity or protocol error its framing cannot be
/// trusted to make progress.
fn drain(ws: &mut WebSocket<Wire>) {
    let tcp = &mut ws.get_mut().tcp;
    let until = Instant::now() + CLOSE_LINGER;
    let mut buf = [0u8; 4096];
    let mut total = 0usize;
    while Instant::now() < until && total < DRAIN_CAP {
        match tcp.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => total += n,
            Err(e) if is_timeout(&e) => {}
            Err(_) => break,
        }
    }
}

fn read_loop(conn: &Conn, ws: &mut WebSocket<Wire>, shared: &Shared) -> Closed {
    let limits = shared.limits;
    let mut reader = Reader::new(&limits);
    conn.heard_now();
    loop {
        if let Some(closing) = conn.closing() {
            drain(ws);
            return closing.why;
        }
        let asked = match ws.read() {
            Ok(message) => {
                conn.heard_now();
                match message {
                    Message::Text(text) => {
                        if text.len() > limits.text_message {
                            Some(Closed::TooBig)
                        } else {
                            reader.on_text(conn, shared, text.as_str())
                        }
                    }
                    Message::Binary(bytes) => {
                        reader.on_binary(conn, shared, &bytes);
                        None
                    }
                    // tungstenite queued the pong, and the reply to a close: the next read
                    // writes it, and after a close returns `ConnectionClosed`.
                    Message::Ping(_) | Message::Pong(_) | Message::Close(_) | Message::Frame(_) => {
                        None
                    }
                }
            }
            Err(tungstenite::Error::Io(e)) if is_timeout(&e) => {
                let silence = if conn.holds_key() {
                    limits.keyed_silence
                } else {
                    limits.silence
                };
                (conn.quiet_for() > silence).then_some(Closed::Silent)
            }
            Err(tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed) => {
                return Closed::Program;
            }
            Err(tungstenite::Error::Capacity(_)) => Some(Closed::TooBig),
            Err(
                tungstenite::Error::Protocol(_)
                | tungstenite::Error::Utf8(_)
                | tungstenite::Error::AttackAttempt,
            ) => Some(Closed::Protocol),
            Err(_) => return Closed::Lost,
        };
        // Over the rate without a break for too long, whatever it sent: text or frames.
        let flooding = reader.limiter.flooding(limits.flood, Instant::now());
        if let Some(why) = asked.or(flooding.then_some(Closed::Flood)) {
            conn.close(Closing::new(why));
        }
    }
}

/// What the writer does next.
enum Job {
    Write(Vec<u8>),
    Close(Vec<u8>),
    Timer,
    Finish,
}

fn next_job(conn: &Conn, due: Instant) -> Job {
    let mut out = lock(&conn.out);
    let mut woken = false;
    loop {
        // After the close frame nothing more is written: not even a close reply tungstenite
        // queued for a close that crossed ours.
        if out.close_written {
            return Job::Finish;
        }
        let mut buf = Vec::new();
        out.gather(&mut buf);
        if !buf.is_empty() {
            return Job::Write(buf);
        }
        if let Some(closing) = out.close {
            out.close_written = true;
            let mut bytes = Vec::new();
            frame_bytes(
                Frame::close(Some(CloseFrame {
                    code: CloseCode::from(closing.code),
                    reason: Utf8Bytes::from_static(closing.reason),
                })),
                &mut bytes,
            );
            return Job::Close(bytes);
        }
        if out.done {
            return Job::Finish;
        }
        // Woken with nothing to write (a key, a subscription): the caller looks at its timers
        // again.
        let now = Instant::now();
        if woken || due <= now {
            return Job::Timer;
        }
        out = conn
            .ready
            .wait_timeout(out, due - now)
            .map(|(g, _)| g)
            .unwrap_or_else(|e| e.into_inner().0);
        woken = true;
    }
}

/// The writer's timers: the ping, and each sensor stream.
struct Timers {
    ping: Instant,
    rx: Option<Instant>,
    tx: Option<Instant>,
}

impl Timers {
    fn due(&self) -> Instant {
        [Some(self.ping), self.rx, self.tx]
            .into_iter()
            .flatten()
            .min()
            .unwrap_or(self.ping)
    }
}

/// The writer's socket for one of its writes (the frames it gathered, the close frame, a ping and
/// the sensor lines): each write of the socket gets only the time left before `deadline`, and none
/// starts once that has run out. A timeout on each alone would bound one write of the socket and no
/// more, so a program taking a few bytes inside each could hold one of the writer's writes as long
/// as it kept reading, and the farewell that ends its transmissions would wait as long.
struct Stall<'a> {
    tcp: &'a TcpStream,
    deadline: Instant,
}

impl Write for Stall<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let left = self.deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "write stall"));
        }
        self.tcp.set_write_timeout(Some(left))?;
        self.tcp.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.tcp.flush()
    }
}

/// Write all of `bytes` within [`WRITE_STALL`], or fail.
fn write_whole(tcp: &TcpStream, bytes: &[u8]) -> io::Result<()> {
    Stall {
        tcp,
        deadline: Instant::now() + WRITE_STALL,
    }
    .write_all(bytes)
}

fn write_loop(conn: &Conn, tcp: TcpStream, shared: &Shared) {
    let limits = shared.limits;
    let ping_every = |conn: &Conn| {
        if conn.holds_key() {
            limits.keyed_ping
        } else {
            limits.ping
        }
    };
    let mut timers = Timers {
        ping: Instant::now() + ping_every(conn),
        rx: None,
        tx: None,
    };
    loop {
        // A new subscription starts its clock; one ended stops it. A key held shortens the ping.
        {
            let session = lock(&conn.session);
            let now = Instant::now();
            timers.rx = session
                .rx_sensors
                .map(|every| timers.rx.unwrap_or(now + every));
            timers.tx = session
                .tx_sensors
                .map(|every| timers.tx.unwrap_or(now + every));
        }
        let keyed = ping_every(conn);
        timers.ping = timers.ping.min(Instant::now() + keyed);
        let job = next_job(conn, timers.due());
        let bytes = match job {
            Job::Write(bytes) => bytes,
            Job::Close(bytes) => {
                // The close frame, then our half of the connection. The read half is the
                // reader's: it reads on until the program closes too (`drain`), so the program
                // reads the close frame rather than a reset.
                let _ = write_whole(&tcp, &bytes);
                let _ = tcp.shutdown(Shutdown::Write);
                continue;
            }
            Job::Finish => break,
            Job::Timer => timer_bytes(conn, shared, &mut timers, keyed),
        };
        if !bytes.is_empty() && write_whole(&tcp, &bytes).is_err() {
            // The program is gone, or did not take this write in WRITE_STALL: the reader is woken
            // at once, and the program's transmissions end with it (`farewell`).
            conn.close(Closing::new(Closed::Lost));
            let _ = tcp.shutdown(Shutdown::Both);
            return;
        }
    }
    let _ = tcp.shutdown(Shutdown::Write);
}

/// What the timers have due: a ping, and the sensor lines from the latest snapshot.
fn timer_bytes(conn: &Conn, shared: &Shared, timers: &mut Timers, ping_every: Duration) -> Vec<u8> {
    let now = Instant::now();
    let mut bytes = Vec::new();
    if timers.ping <= now {
        frame_bytes(Frame::ping(Vec::new()), &mut bytes);
        timers.ping = now + ping_every;
    }
    let (rx_due, tx_due) = (
        timers.rx.is_some_and(|t| t <= now),
        timers.tx.is_some_and(|t| t <= now),
    );
    if rx_due || tx_due {
        let model = lock(&shared.model);
        let session = lock(&conn.session);
        if rx_due {
            for line in rx_sensor_lines(&model) {
                text_frame(line.into_string(), &mut bytes);
            }
            timers.rx = session.rx_sensors.map(|every| now + every);
        }
        if tx_due {
            for line in tx_sensor_lines(&model) {
                text_frame(line.into_string(), &mut bytes);
            }
            timers.tx = session.tx_sensors.map(|every| now + every);
        }
    }
    bytes
}

/// Publish one snapshot to one program: what changed, under its session lock.
pub(super) fn publish(conn: &Conn, s: &Snapshot) {
    let mut session = lock(&conn.session);
    let lines = session.publish(s, conn.id);
    conn.push_lines(lines);
}

/// Deliver state changes to one program, forced when it is the sender.
pub(super) fn deliver(conn: &Conn, sender: ClientId, outs: &[Out]) {
    let mut session = lock(&conn.session);
    if !session.inited {
        return;
    }
    let force = conn.id == sender;
    let lines: Vec<ServerText> = outs
        .iter()
        .flat_map(|o| session.echo(o.clone(), force))
        .collect();
    conn.push_lines(lines);
}

/// Send one line to one program regardless of what it was told (`trx:0,false;` as the server
/// stops).
pub(super) fn tell(conn: &Conn, o: Out) {
    let mut session = lock(&conn.session);
    let lines = session.echo(o, true);
    conn.push_lines(lines);
}
