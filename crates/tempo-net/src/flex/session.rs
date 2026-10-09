//! The SmartSDR session: one TCP connection to the radio, from prologue to teardown.
//!
//! [`Session`] is the protocol core, with no I/O of its own: bytes in ([`Session::on_bytes`]),
//! commands out through the writer each call is given, time passed in as milliseconds on a
//! monotonic clock. [`Connection`] runs it on a std thread over any `Read + Write` (a
//! `TcpStream` to the radio's port 4992 in production, the simulator in tests).
//!
//! **What the session does.**
//! - *Line assembly* ([`LineAssembler`]): bytes are held until a `\n` arrives; only whole lines
//!   are parsed, so a line split across reads is never a value (spec §10.5, "A partial reply is
//!   never a reading"). The terminator and one CR are stripped; blank lines are dropped; a run of
//!   [`MAX_LINE`] bytes with no terminator ends the session.
//! - *The prologue*: `V` then `H`. Only a TCP API version reviewed for transmit (1.4, any build;
//!   [`supports_protocol`]) on a first, well-formed prologue lets this session transmit. A late,
//!   repeated or malformed `V` or `H`, or handle zero, makes it receive-only for its lifetime,
//!   and never takes away an unkey.
//! - *The connect sequence* ([`super::handshake`]), then registration's verdict: a refusal ends the
//!   session for good ([`Event::RegistrationRejected`]). The one datagram the sequence needs, the
//!   UDP registration's, goes out through the sender its owner gives it ([`UdpRegistration`]).
//! - *Replies matched by sequence number*: every command gets a number and every reply is matched
//!   by it, never by arrival order.
//! - *Status*: decoded ([`super::status`]) and folded into the model ([`super::model`]).
//! - *Keepalive* ([`super::keepalive`]): one ping a second; five missed replies end the session; a
//!   missed reply while keyed unkeys at once.
//! - *Teardown*: a best-effort `xmit 0` if ours may be keyed, `stream remove` for our streams,
//!   each acknowledged (or the wait times out) before the close, then the byte `0x04`, then the
//!   close.
//!
//! **Transmit.** [`Session::start`] runs admission ([`super::admission`]); only an admitted start
//! is rendered, and the readback ([`super::ptt_evidence`]) is armed with its kind's profile and
//! "keyed" set before the write, so an attempt that fails partway still needs its stop.
//! [`Session::stop`] is never gated: it asks only "is it ours?" (an unconfirmed start of ours, or
//! an interlock naming this session's handle or one of our previous sessions').
//! [`Session::end_ours`] is rigctld's `T 0`: the open operation's own stop, or, with none open,
//! every stop the radio's status shows a session of ours may need. Keyed clears only when the
//! readback proves the end; a reply to a stop is not that proof. Every stop that ends the open
//! operation arms the escalation clock: if the proof does not come within the deadline after the
//! first one, the session sends the kind's stops again (`xmit 0`; for a tune `transmit tune 0` and
//! `xmit 0`), reports [`Event::UnkeyUnconfirmed`] and closes (spec §10.5). A radio-ended start (a
//! CWX word, an ATU cycle) whose window fails is escalated at once. A tune the radio latches is
//! ended by the session itself, at its hold plus [`ptt_evidence::TUNE_MARGIN_MS`], whether or not
//! the radio loop is running. Stop TX in the middle of a CW message is a `cwx clear`: its reply
//! empties the radio's buffer for the readback, which then waits only for the radio to hold idle,
//! and if the radio still shows our CWX transmitting the break-in delay + 300 ms after that reply,
//! the session sends `xmit 0`, once. The session itself never starts a transmission: none of its
//! reactions to lines, replies, time or teardown is a start.
//!
//! PORTED from AetherSDR (https://github.com/aethersdr/AetherSDR, GPL-3.0; the upstream file
//! carries no per-file header, the licence is the repository's), `src/core/backends/flex/RadioConnection.h`
//! and `src/core/backends/flex/RadioConnection.cpp`, with the write-and-observe composition of
//! `src/core/backends/flex/FlexPttWireSession.h` and `src/core/backends/flex/FlexPttWireSession.cpp`,
//! at commit `32fa50e4896a846a6970fa3f443bd49d667c139d` (2026-10-03), restructured from C++/Qt to
//! Rust. Kept: the 16 MiB line cap, the CR strip, one session per connection (upstream's
//! per-session generation), `stream remove` acknowledged before the close, the `0x04` disconnect
//! marker, the strict prologue rules and the composition that writes the key, stamps every write
//! and line at the transport and always attempts the unkey. Deliberate differences: a sans-I/O
//! core on a std thread instead of `QTcpSocket`/`QThread`; typed commands, so the composition's
//! text match on other writers' commands (`otherCommand`) is replaced by the kind the encoder
//! gives each command; the demo-radio branches, kernel RTT sampling and WAN paths are dropped;
//! evidence is consumed in the step that completes it rather than queued to a coordinator; a
//! failed write ends the session. Recorded in the repo-root NOTICE (AetherSDR entry).
//!
//! Added for Nexus, with no upstream code taken: the kinds other than `xmit` (each one's readback
//! profile, its own stops and escalation, the tune's deadline, CWX words joining an open
//! operation, the unkey that follows a CWX clear), [`Session::end_ours`], and the snapshot's
//! belief that something of ours may be on the air ([`Snapshot::ours_on_air`]).

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::{self, ErrorKind, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::num::NonZeroU64;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use super::admission::{self, Admitted, Facts, Refusal};
use super::encode::{
    self, ClientId, Command, Kind, Rendered, StartKind, Station, Target, TxAudio, TxStart, TxStop,
};
use super::handshake::{self, ConnectConfig, Registration, NOT_SUPPORTED};
use super::keepalive::{Keepalive, Tick};
use super::model::{ObjectRef, Owner, StatusModel};
use super::ptt_evidence::{
    self, Operation, Stamp, StopRequest, StopTracker, Window, TRANSITION_TIMEOUT_MS,
};
use super::reconnect::End;
use super::status::{decode, ClientAction, Decoded};
use super::wire::{self, parse_line, Line, Reply, Severity, Status, WireError, MAX_LINE};

/// Whether a prologue's version text names a TCP API reviewed for transmit: exactly four
/// dot-separated decimal numbers, no signs, spaces or leading zeros, each fitting an `i32`, with
/// major 1 and minor 4. The last two numbers are builds, not compatibility selectors, so any build
/// of 1.4 qualifies; firmware versions (`4.2.18.41174`) and other APIs do not.
pub fn supports_protocol(version: &str) -> bool {
    if version.is_empty() || version.len() > 32 {
        return false;
    }
    let parts: Vec<&str> = version.split('.').collect();
    let well_formed = parts.len() == 4
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes().all(|b| b.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
                && p.parse::<i32>().is_ok()
        });
    well_formed && parts[0] == "1" && parts[1] == "4"
}

/// Holds bytes until a whole line has arrived.
#[derive(Debug, Default)]
pub struct LineAssembler {
    buf: Vec<u8>,
}

/// More than [`MAX_LINE`] bytes arrived without a terminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TooLong;

impl LineAssembler {
    /// Append bytes; return every line they complete, terminator and one trailing CR removed,
    /// blank lines dropped. Bytes after the last terminator stay held, never returned.
    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, TooLong> {
        self.buf.extend_from_slice(bytes);
        let mut lines = Vec::new();
        let mut start = 0;
        while let Some(pos) = self.buf[start..].iter().position(|b| *b == b'\n') {
            let end = start + pos;
            let mut line = &self.buf[start..end];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            if !line.iter().all(u8::is_ascii_whitespace) {
                lines.push(line.to_vec());
            }
            start = end + 1;
        }
        self.buf.drain(..start);
        if self.buf.len() > MAX_LINE {
            self.buf.clear();
            return Err(TooLong);
        }
        Ok(lines)
    }

    /// Whether bytes of an unfinished line are held.
    pub fn has_partial(&self) -> bool {
        !self.buf.is_empty()
    }
}

/// How the session is set up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    pub connect: ConnectConfig,
    pub ping_interval_ms: u64,
    /// How long after the first stop the radio has to confirm the unkey, and how long each
    /// readback transition may take ([`TRANSITION_TIMEOUT_MS`] by default).
    pub unkey_deadline_ms: u64,
    /// How long the radio has to send its prologue, and then to answer `client gui`.
    pub registration_timeout_ms: u64,
    /// How long teardown waits for its `stream remove` replies.
    pub teardown_timeout_ms: u64,
    /// Our recent sessions' handles on this radio, newest first ([`super::reconnect::Ladder`]).
    pub previous_handles: Vec<u32>,
    /// The UDP registration's datagram, sent after registration, just before `client udpport`;
    /// `None` sends none.
    pub udp_registration: Option<UdpRegistration>,
    /// Admit every kind, also those whose readback no bench has confirmed: the tests' way to run
    /// the readback per kind. Compiled only in this crate's tests and with the test-support
    /// feature `flex-unbenched` (tempo-audio's tests), which a release build refuses
    /// ([`admission::BENCHED`]).
    #[cfg(any(test, feature = "flex-unbenched"))]
    pub unbenched: bool,
}

impl Config {
    /// The production settings for a station name.
    pub fn new(station: Station) -> Config {
        Config {
            connect: ConnectConfig {
                station,
                client_id: None,
                low_bandwidth: false,
                network_mtu: handshake::DEFAULT_NETWORK_MTU,
                udp_port: None,
            },
            ping_interval_ms: super::keepalive::PING_INTERVAL_MS,
            unkey_deadline_ms: TRANSITION_TIMEOUT_MS,
            registration_timeout_ms: 10_000,
            teardown_timeout_ms: 2_000,
            previous_handles: Vec::new(),
            udp_registration: None,
            #[cfg(any(test, feature = "flex-unbenched"))]
            unbenched: false,
        }
    }
}

/// What sends the UDP registration's one-byte datagram, from the socket `client udpport`
/// registers to the radio's port 4992. The socket's owner supplies it, because the session has no
/// I/O of its own, and the session calls it once, where [`super::handshake`] places the datagram:
/// after registration, just before `client udpport`.
#[derive(Clone)]
pub struct UdpRegistration(Arc<dyn Fn() + Send + Sync>);

impl UdpRegistration {
    pub fn new(send: impl Fn() + Send + Sync + 'static) -> UdpRegistration {
        UdpRegistration(Arc::new(send))
    }

    fn send(&self) {
        (self.0)()
    }
}

impl std::fmt::Debug for UdpRegistration {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("UdpRegistration")
    }
}

/// A registration is equal to itself and its clones: one sender.
impl PartialEq for UdpRegistration {
    fn eq(&self, other: &UdpRegistration) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl Eq for UdpRegistration {}

/// Where the session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    AwaitVersion,
    AwaitHandle,
    /// `client gui` sent, its reply awaited.
    Registering,
    /// Registered; the setup batch is out and `slice list` is awaited.
    Subscribing,
    /// Registered and subscribed.
    Ready,
    /// Teardown: waiting for `stream remove` replies.
    Closing,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    Unknown,
    Supported,
    Rejected,
}

/// What the session reports, in order.
#[derive(Debug, Clone, PartialEq)]
pub enum Event {
    /// The prologue's version, and whether this session may transmit on it.
    Prologue {
        version: String,
        transmit_protocol: bool,
    },
    /// This connection's client handle.
    Handle(u32),
    /// The prologue broke its rules or named an API not reviewed for transmit: receive only.
    ProtocolRejected,
    /// The radio accepted `client gui`; the id to store and present next time, if it gave one.
    Registered { client_id: Option<ClientId> },
    /// The radio refused `client gui`. The session closes; do not retry until the operator asks.
    RegistrationRejected { code: u32, detail: String },
    /// Registered and subscribed. The slices the radio already has for us (`slice list`), or
    /// `None` if that reply did not parse.
    Ready { slices: Option<Vec<u8>> },
    /// The reply to a command the caller sent, or to a start or stop.
    Reply {
        seq: u32,
        code: u32,
        message: String,
    },
    /// A setup command the radio refused.
    SetupRefused { command: String, code: u32 },
    /// `client udpport` was refused because the port is taken: rebind, then send it again.
    UdpPortInUse { port: u16 },
    Message {
        severity: Severity,
        number: u32,
        text: String,
    },
    /// A line that was not a value. It was dropped.
    LineRejected { error: WireError },
    /// Our key command was written to the transport in full.
    KeyWritten { seq: u32 },
    /// The radio proved our unkey: keyed is cleared.
    UnkeyConfirmed,
    /// A ping went unanswered while keyed, and the session sent `xmit 0` before anything else.
    UnkeyedOnMissedPing { misses: u32 },
    /// The radio did not confirm the unkey in time. The session sent `xmit 0` again and is
    /// closing. The operator must be told: the radio may still be transmitting.
    UnkeyUnconfirmed,
    /// The interlock names the handle of one of our previous sessions: that session's
    /// transmitter may still be keyed. Keying is refused while it holds the transmitter.
    PreviousSessionHoldsTransmitter { handle: u32 },
    /// The session ended. Nothing follows.
    Closed {
        end: End,
        was_keyed: bool,
        handle: Option<u32>,
    },
}

/// Why a command was not sent.
#[derive(Debug, Clone, PartialEq)]
pub enum SendError {
    /// Not registered and subscribed, or closing.
    NotReady,
    Encode(encode::EncodeError),
    /// The command is aimed at an object that is not this connection's.
    NotOurs {
        target: Target,
        owner: Owner,
    },
}

/// What ending everything of ours did ([`Session::end_ours`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EndOutcome {
    /// These stops went out, in order, with their sequence numbers. None: nothing of ours to stop.
    Sent(Vec<(TxStop, u32)>),
    /// The connection is closed; nothing can be written.
    NotConnected,
}

/// What a stop did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    /// The stop went out with this sequence number.
    Sent { seq: u32 },
    /// Nothing of ours to stop: no unconfirmed start, and the interlock names no session of ours.
    /// Another client's transmission is never stopped from here.
    NothingOfOurs,
    /// The connection is closed; nothing can be written.
    NotConnected,
}

/// A copy of the session's state, for whoever drives it.
#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub phase: Phase,
    /// This session may transmit on the radio's API version.
    pub transmit_protocol: bool,
    pub handle: Option<u32>,
    /// A start of ours is unconfirmed.
    pub keyed: bool,
    /// A transmission of ours may be on the air: a start of ours is unconfirmed, or the radio
    /// names this session or a previous one of ours as the transmitter while it keys or tunes.
    pub ours_on_air: bool,
    /// The readback has seen the radio idle and nothing of ours is keyed.
    pub transmit_ready: bool,
    /// Our DAX transmit stream, from the reply to our create, until the radio removes it.
    pub dax_tx_stream: Option<u32>,
    pub model: StatusModel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Pending {
    Ping,
    Gui,
    Setup(String),
    MicList,
    UdpPort(u16),
    SliceList,
    Caller,
    Key,
    Stop(TxStop),
    TxAudio(TxAudio),
    Teardown(u32),
}

/// An unconfirmed start of ours.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Keyed {
    operation: Operation,
    kind: StartKind,
    stop: Option<StopRequest>,
    stop_seq: Option<u32>,
    /// The readback accepted the stop binding: its write is reported to it.
    stop_tracked: bool,
    first_stop_ms: Option<u64>,
    escalated: bool,
    /// When the session ends it by itself: a tune's hold plus [`ptt_evidence::TUNE_MARGIN_MS`].
    deadline_ms: Option<u64>,
    /// When the first reply to a `cwx clear` for our CWX came back.
    clear_replied_ms: Option<u64>,
    /// The unkey that follows a clear the radio did not act on has gone out.
    unkey_followed: bool,
}

/// The break-in delay a CWX word's readback assumes when the radio has not reported one: the
/// longest the transmit status can carry (0-2000 ms), so the hold is never shorter than the radio's.
const BREAK_IN_DELAY_MAX_MS: u64 = 2_000;

/// The stops that end an operation of this kind, in the order they go out: Stop TX for the ATU
/// sends both the unkey and the tune-off (no command that ends a cycle is documented).
fn own_stops(kind: StartKind) -> &'static [TxStop] {
    match kind {
        StartKind::Key => &[TxStop::Unkey],
        StartKind::Tune => &[TxStop::TuneOff],
        StartKind::Cwx => &[TxStop::CwxClear],
        StartKind::Atu => &[TxStop::Unkey, TxStop::TuneOff],
    }
}

/// The stops sent again when an operation of this kind is not proven ended: its own, then the
/// unkey after them.
fn escalation_stops(kind: StartKind) -> &'static [TxStop] {
    match kind {
        StartKind::Key => &[TxStop::Unkey],
        StartKind::Tune => &[TxStop::TuneOff, TxStop::Unkey],
        StartKind::Cwx => &[TxStop::CwxClear, TxStop::Unkey],
        StartKind::Atu => &[TxStop::Unkey, TxStop::TuneOff],
    }
}

/// Session identities are process-wide, nonzero and never reused.
static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

/// The protocol core of one connection.
pub struct Session {
    config: Config,
    generation: u64,
    phase: Phase,
    protocol: Protocol,
    handle: Option<u32>,
    assembler: LineAssembler,
    next_seq: u32,
    pending: BTreeMap<u32, Pending>,
    registration: Registration,
    keepalive: Keepalive,
    model: StatusModel,
    tracker: StopTracker,
    ordinal: u64,
    keyed: Option<Keyed>,
    next_operation: u64,
    cwx_block: u32,
    phase_deadline_ms: Option<u64>,
    closing: Option<(End, BTreeSet<u32>, u64)>,
    warned_previous: BTreeSet<u32>,
    dax_tx_stream: Option<u32>,
    events: VecDeque<Event>,
    /// Every command written, with its kind, for the tests' independent checks.
    #[cfg(test)]
    wrote: Vec<(u32, Kind, String)>,
}

impl Session {
    /// A session for a connection that has just opened.
    pub fn new(config: Config, now_ms: u64) -> Session {
        let deadline = now_ms + config.registration_timeout_ms;
        Session {
            keepalive: Keepalive::new(config.ping_interval_ms),
            tracker: StopTracker::new(config.unkey_deadline_ms),
            config,
            generation: NEXT_GENERATION.fetch_add(1, Ordering::Relaxed),
            phase: Phase::AwaitVersion,
            protocol: Protocol::Unknown,
            handle: None,
            assembler: LineAssembler::default(),
            next_seq: 1,
            pending: BTreeMap::new(),
            registration: Registration::default(),
            model: StatusModel::default(),
            ordinal: 0,
            keyed: None,
            next_operation: 1,
            cwx_block: 1,
            phase_deadline_ms: Some(deadline),
            closing: None,
            warned_previous: BTreeSet::new(),
            dax_tx_stream: None,
            events: VecDeque::new(),
            #[cfg(test)]
            wrote: Vec::new(),
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn is_closed(&self) -> bool {
        self.phase == Phase::Closed
    }

    pub fn handle(&self) -> Option<u32> {
        self.handle
    }

    pub fn model(&self) -> &StatusModel {
        &self.model
    }

    pub fn keyed(&self) -> bool {
        self.keyed.is_some()
    }

    /// The readback can track a key: the API is reviewed, the radio has been seen idle, and
    /// nothing of ours is keyed. Admission asks this and more.
    pub fn transmit_ready(&self) -> bool {
        self.protocol == Protocol::Supported
            && self.tracker.phase() == super::ptt_evidence::Phase::Idle
            && self.keyed.is_none()
    }

    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            phase: self.phase,
            transmit_protocol: self.protocol == Protocol::Supported,
            handle: self.handle,
            keyed: self.keyed.is_some(),
            ours_on_air: self.ours_on_air(),
            transmit_ready: self.transmit_ready(),
            dax_tx_stream: self.dax_tx_stream,
            model: self.model.clone(),
        }
    }

    /// Everything reported since the last call, in order.
    pub fn take_events(&mut self) -> Vec<Event> {
        self.events.drain(..).collect()
    }

    fn facts(&self) -> Facts {
        Facts {
            ready: self.phase == Phase::Ready,
            transmit_protocol: self.protocol == Protocol::Supported,
            handle: self.handle,
            keyed: self.keyed.is_some(),
            open: self.keyed.map(|k| k.kind),
            readback_idle: self.tracker.phase() == super::ptt_evidence::Phase::Idle,
            dax_tx_stream: self.dax_tx_stream,
        }
    }

    /// Whether admission takes starts of `kind` on this session: a kind a tester's bench has
    /// confirmed ([`admission::BENCHED`]), or, under test, any kind when the configuration's door
    /// is open. Its other checks still decide each start.
    pub fn switched_on(&self, kind: StartKind) -> bool {
        #[cfg(any(test, feature = "flex-unbenched"))]
        if self.config.unbenched {
            return true;
        }
        admission::BENCHED.contains(&kind)
    }

    /// Admission for `start`; under test, every kind when the configuration says so.
    fn admit(&self, start: TxStart) -> Result<Admitted, Refusal> {
        #[cfg(any(test, feature = "flex-unbenched"))]
        if self.config.unbenched {
            return admission::admit_unbenched(&self.model, &self.facts(), start);
        }
        admission::admit(&self.model, &self.facts(), start)
    }

    /// The break-in delay the radio reports, or the longest it can report when it has not.
    fn break_in_delay_ms(&self) -> u64 {
        self.model
            .transmit
            .cw_break_in_delay
            .and_then(|d| u64::try_from(d).ok())
            .unwrap_or(BREAK_IN_DELAY_MAX_MS)
    }

    /// Whether the radio still shows a session of ours transmitting: its last whole interlock
    /// sample is PTT_REQUESTED or TRANSMITTING naming one, or it has sent no whole sample since.
    fn still_transmitting(&self) -> bool {
        self.model.interlock.sample.as_ref().is_none_or(|s| {
            matches!(s.state.as_str(), "PTT_REQUESTED" | "TRANSMITTING")
                && self.is_ours(s.tx_client_handle)
        })
    }

    /// Whether the interlock's last owner is this session or one of our previous sessions.
    fn named(&self) -> bool {
        self.model
            .interlock
            .last_tx_client_handle
            .is_some_and(|h| self.is_ours(h))
    }

    /// A transmission of ours may be on the air: a start of ours is unconfirmed, or the radio names
    /// a session of ours as the transmitter while it keys, releases or tunes (or has not sent a
    /// whole sample since naming it).
    fn ours_on_air(&self) -> bool {
        self.keyed.is_some()
            || (self.named()
                && (self.model.transmit.tune == Some(true)
                    || self.model.interlock.sample.as_ref().is_none_or(|s| {
                        matches!(
                            s.state.as_str(),
                            "PTT_REQUESTED" | "TRANSMITTING" | "UNKEY_REQUESTED"
                        )
                    })))
    }

    fn stamp(&mut self) -> Stamp {
        self.ordinal += 1;
        Stamp {
            session: self.generation,
            ordinal: self.ordinal,
        }
    }

    fn take_seq(&mut self) -> u32 {
        let seq = self.next_seq;
        self.next_seq = self.next_seq.saturating_add(1);
        seq
    }

    /// Whether `handle` is this session's or one of our previous sessions'.
    fn is_ours(&self, handle: u32) -> bool {
        handle != 0
            && (Some(handle) == self.handle || self.config.previous_handles.contains(&handle))
    }

    // ── Writing ──────────────────────────────────────────────────────────────────────────────

    /// Write one rendered command and report it to the readback. A failed write ends the session.
    fn write(&mut self, out: &mut dyn Write, seq: u32, rendered: &Rendered, now: u64) -> bool {
        if self.phase == Phase::Closed {
            return false;
        }
        let bytes = wire::frame(seq, rendered.text());
        let ok = out.write_all(&bytes).and_then(|()| out.flush()).is_ok();
        #[cfg(test)]
        self.wrote
            .push((seq, rendered.kind(), rendered.text().to_string()));
        self.report_write(seq, rendered.kind(), ok, now);
        if !ok {
            self.transport_closed(now);
        }
        ok
    }

    /// Tell the readback about a transmit-capable write, stamped where it happened. The open
    /// operation's own starts (the key, a CWX word) and its bound stop are its inputs; any other
    /// transmit-capable write during an attempt spoils the attempt's evidence, as another writer's
    /// would (upstream's `otherCommand`, typed here).
    fn report_write(&mut self, seq: u32, kind: Kind, ok: bool, now: u64) {
        let Some(keyed) = self.keyed else { return };
        match kind {
            // Admission refuses an audio-source change while anything is keyed, so it cannot
            // reach here; if it did, it is not a keying write.
            Kind::Ordinary | Kind::TxAudio(_) => {}
            // The readback checks the sequence number: a second start that is not its own fails.
            Kind::Start(k) if k == keyed.kind => {
                let stamp = self.stamp();
                self.tracker.command_written(stamp, seq, true, ok, now);
            }
            Kind::Stop(_) if keyed.stop_seq == Some(seq) => {
                if keyed.stop_tracked {
                    let stamp = self.stamp();
                    self.tracker.command_written(stamp, seq, false, ok, now);
                }
            }
            // A repeated unkey is not another writer: it is the same stop again, unreported, as
            // upstream sends it. Nor is the operation's own stop again, or a radio-ended
            // operation's stops, which its readback does not wait for.
            Kind::Stop(stop) if stop == TxStop::Unkey || own_stops(keyed.kind).contains(&stop) => {}
            Kind::Start(_) | Kind::Stop(_) => {
                let stamp = self.stamp();
                self.tracker.command_written(stamp, 0, true, false, now);
            }
        }
    }

    /// Send a command the session itself needs (setup, pings, teardown).
    fn send_internal(
        &mut self,
        out: &mut dyn Write,
        command: &Command,
        pending: Pending,
        now: u64,
    ) -> Option<u32> {
        let rendered = encode::render(command).ok()?;
        let seq = self.take_seq();
        self.pending.insert(seq, pending);
        self.write(out, seq, &rendered, now).then_some(seq)
    }

    /// Send a caller's command. Only once registered and subscribed, and only at our own objects.
    pub fn send(
        &mut self,
        out: &mut dyn Write,
        command: &Command,
        now: u64,
    ) -> Result<u32, SendError> {
        if self.phase != Phase::Ready {
            return Err(SendError::NotReady);
        }
        let rendered = encode::render(command).map_err(SendError::Encode)?;
        if let Some(target) = rendered.target() {
            let object = match target {
                Target::Slice(i) => ObjectRef::Slice(i),
                Target::Pan(id) => ObjectRef::Pan(id),
                Target::Waterfall(id) => ObjectRef::Waterfall(id),
                Target::Stream(id) => ObjectRef::Stream(id),
            };
            let owner = self.model.owner(object, self.handle);
            if owner != Owner::Ours {
                return Err(SendError::NotOurs { target, owner });
            }
        }
        let seq = self.take_seq();
        self.pending.insert(seq, Pending::Caller);
        self.write(out, seq, &rendered, now);
        Ok(seq)
    }

    // ── Transmit ─────────────────────────────────────────────────────────────────────────────

    /// Start a transmission, if admission allows it. The readback is armed and keyed is set before
    /// the write. [`Self::start_lasting`] with nothing known about how long it lasts: a tune is
    /// then held for at most [`ptt_evidence::TUNE_HOLD_MAX_MS`], and a CWX word is refused (its
    /// readback needs the word's keying time).
    pub fn start(&mut self, out: &mut dyn Write, start: TxStart, now: u64) -> Result<u32, Refusal> {
        self.start_lasting(out, start, None, now)
    }

    /// [`Self::start`] with what the caller knows of how long the transmission lasts: a tune's
    /// hold (the session ends it at that plus [`ptt_evidence::TUNE_MARGIN_MS`] by itself, and at
    /// most at [`ptt_evidence::TUNE_HOLD_MAX_MS`] plus the margin), or a CWX word's keying time at
    /// the radio's speed. A key and an ATU cycle take nothing from it. A CWX word sent while our
    /// own CWX operation is open joins it, until a stop has gone out for it.
    pub fn start_lasting(
        &mut self,
        out: &mut dyn Write,
        start: TxStart,
        lasting_ms: Option<u64>,
        now: u64,
    ) -> Result<u32, Refusal> {
        let kind = start.kind();
        // A window the radio has just closed is taken first, so a word is never added to it.
        self.tracker.poll(now);
        self.check_evidence(now);
        let admitted = self.admit(start)?;
        let window = match kind {
            StartKind::Cwx => {
                let word_ms = lasting_ms.ok_or(Refusal::NoReadback(kind))?;
                Some(Window::cwx(word_ms, self.break_in_delay_ms()))
            }
            StartKind::Atu => Some(Window::atu()),
            StartKind::Key | StartKind::Tune => None,
        };
        let append = self
            .keyed
            .filter(|k| kind == StartKind::Cwx && k.kind == StartKind::Cwx);
        if append.is_some_and(|k| k.first_stop_ms.is_some()) {
            // Stopping: the operation takes no more words.
            return Err(Refusal::AlreadyKeyed);
        }
        let seq = self.take_seq();
        match (append, window) {
            (Some(open), Some(window)) => {
                if !self.tracker.append(open.operation, seq, window, now) {
                    return Err(Refusal::ReadbackNotIdle);
                }
            }
            _ => {
                let operation = Operation(
                    NonZeroU64::new(self.next_operation).expect("operation numbers start at one"),
                );
                self.next_operation += 1;
                let end = StopRequest {
                    operation,
                    attempt: NonZeroU64::MIN,
                };
                let armed = match (kind, window) {
                    (StartKind::Tune, _) => {
                        self.tracker
                            .begin_as(operation, ptt_evidence::TUNE, seq, now)
                    }
                    (StartKind::Cwx, Some(w)) => {
                        self.tracker
                            .begin_window(operation, end, ptt_evidence::CWX, seq, w, now)
                    }
                    (StartKind::Atu, Some(w)) => {
                        self.tracker
                            .begin_window(operation, end, ptt_evidence::ATU, seq, w, now)
                    }
                    _ => self.tracker.begin(operation, seq, now),
                };
                if !armed {
                    return Err(Refusal::ReadbackNotIdle);
                }
                let deadline_ms = (kind == StartKind::Tune).then(|| {
                    let hold = lasting_ms
                        .unwrap_or(ptt_evidence::TUNE_HOLD_MAX_MS)
                        .min(ptt_evidence::TUNE_HOLD_MAX_MS);
                    now.saturating_add(hold)
                        .saturating_add(ptt_evidence::TUNE_MARGIN_MS)
                });
                self.keyed = Some(Keyed {
                    operation,
                    kind,
                    // A radio-ended operation's proof is its window's: no stop is bound to it.
                    stop: window.map(|_| end),
                    stop_seq: None,
                    stop_tracked: false,
                    first_stop_ms: None,
                    escalated: false,
                    deadline_ms,
                    clear_replied_ms: None,
                    unkey_followed: false,
                });
            }
        }
        let rendered = encode::render_start(admitted, self.cwx_block);
        if kind == StartKind::Cwx {
            self.cwx_block += 1;
        }
        self.pending.insert(seq, Pending::Key);
        if self.write(out, seq, &rendered, now) {
            self.events.push_back(Event::KeyWritten { seq });
        }
        Ok(seq)
    }

    /// Whether there is something of ours for this stop to end.
    fn ours_to_stop(&self, stop: TxStop) -> bool {
        let named = self.named();
        let started = self.keyed.map(|k| k.kind);
        match stop {
            TxStop::Unkey => started.is_some() || named,
            TxStop::TuneOff => {
                matches!(started, Some(StartKind::Tune | StartKind::Atu))
                    || (self.model.transmit.tune == Some(true) && named)
            }
            // Our CWX, or, with no operation of ours open, one the interlock says is ours. Not
            // beside another open operation: a clear written next to an over's unkey would spoil
            // that over's readback, and the radio loop sends `\stop_morse` after `T 0` when it
            // hands a radio over and when it shuts down.
            TxStop::CwxClear => started == Some(StartKind::Cwx) || (started.is_none() && named),
        }
    }

    /// End a transmission. Never gated: if anything of ours may be keyed, the stop is written,
    /// whatever else is true.
    pub fn stop(&mut self, out: &mut dyn Write, stop: TxStop, now: u64) -> StopOutcome {
        if self.phase == Phase::Closed {
            return StopOutcome::NotConnected;
        }
        if !self.ours_to_stop(stop) {
            return StopOutcome::NothingOfOurs;
        }
        let seq = self.take_seq();
        if let Some(keyed) = self.keyed.as_mut() {
            // The stop the readback proves: an over's unkey, a tune's tune-off.
            let bound = matches!(
                (keyed.kind, stop),
                (StartKind::Key, TxStop::Unkey) | (StartKind::Tune, TxStop::TuneOff)
            );
            if bound && keyed.stop.is_none() {
                let request = StopRequest {
                    operation: keyed.operation,
                    attempt: NonZeroU64::MIN,
                };
                keyed.stop = Some(request);
                keyed.stop_seq = Some(seq);
                keyed.stop_tracked = self
                    .tracker
                    .request_stop(keyed.operation, request, seq, now);
            }
            // Every stop that ends the operation arms the escalation clock, and so does an unkey,
            // whatever the operation.
            if stop == TxStop::Unkey || own_stops(keyed.kind).contains(&stop) {
                keyed.first_stop_ms.get_or_insert(now);
            }
        }
        self.pending.insert(seq, Pending::Stop(stop));
        self.write(out, seq, &encode::render_stop(stop), now);
        StopOutcome::Sent { seq }
    }

    /// End everything of ours: rigctld's `T 0`. Never gated. With an operation of ours open, its
    /// own stops ([`own_stops`]). With none, every stop the radio's status shows a session of ours
    /// may need: `transmit tune 0` while `transmit tune=1` is reported and the interlock names a
    /// session of ours, then `cwx clear` and `xmit 0` while it names one. Another client's
    /// transmission is never stopped from here.
    pub fn end_ours(&mut self, out: &mut dyn Write, now: u64) -> EndOutcome {
        if self.phase == Phase::Closed {
            return EndOutcome::NotConnected;
        }
        let stops: Vec<TxStop> = match self.keyed {
            Some(keyed) => own_stops(keyed.kind).to_vec(),
            None => [TxStop::TuneOff, TxStop::CwxClear, TxStop::Unkey]
                .into_iter()
                .filter(|s| self.ours_to_stop(*s))
                .collect(),
        };
        let mut sent = Vec::new();
        for stop in stops {
            match self.stop(out, stop, now) {
                StopOutcome::Sent { seq } => sent.push((stop, seq)),
                StopOutcome::NothingOfOurs => {}
                StopOutcome::NotConnected => return EndOutcome::NotConnected,
            }
        }
        EndOutcome::Sent(sent)
    }

    /// Change where the transmitter's audio comes from, if its admission allows it (never while
    /// anything is keyed, never beside another program's DAX transmit stream). Answered once
    /// written; the radio's reply follows as an [`Event::Reply`].
    pub fn route(&mut self, out: &mut dyn Write, audio: TxAudio, now: u64) -> Result<u32, Refusal> {
        let admitted = admission::admit_tx_audio(&self.model, &self.facts(), audio)?;
        let seq = self.take_seq();
        self.pending.insert(seq, Pending::TxAudio(audio));
        self.write(out, seq, &encode::render_tx_audio(admitted), now);
        Ok(seq)
    }

    /// Take the readback's proof, if it has completed: keyed clears.
    fn check_evidence(&mut self, now: u64) {
        let Some(request) = self.keyed.and_then(|k| k.stop) else {
            return;
        };
        if self.tracker.evidence(now) == Some(request) && self.tracker.consume(request, now) {
            self.keyed = None;
            self.events.push_back(Event::UnkeyConfirmed);
        }
    }

    /// The end was not proven in time: the kind's stops again ([`escalation_stops`]), tell the
    /// operator, close.
    fn escalate(&mut self, out: &mut dyn Write, now: u64) {
        let kind = self.keyed.map_or(StartKind::Key, |k| k.kind);
        if let Some(keyed) = self.keyed.as_mut() {
            keyed.escalated = true;
            // Stops have gone out: teardown does not send them a third time.
            keyed.first_stop_ms.get_or_insert(now);
        }
        for stop in escalation_stops(kind) {
            let seq = self.take_seq();
            self.pending.insert(seq, Pending::Stop(*stop));
            self.write(out, seq, &encode::render_stop(*stop), now);
        }
        self.events.push_back(Event::UnkeyUnconfirmed);
        self.close_with(out, End::UnkeyUnconfirmed, now);
    }

    /// A radio-ended operation (a CWX word, an ATU cycle) whose window failed: nothing of ours
    /// will end it, so it is escalated at once.
    fn check_window(&mut self, out: &mut dyn Write, now: u64) {
        let failed = self.keyed.is_some_and(|k| {
            matches!(k.kind, StartKind::Cwx | StartKind::Atu)
                && !k.escalated
                && self.tracker.phase() == super::ptt_evidence::Phase::Failed
        });
        if failed {
            self.escalate(out, now);
        }
    }

    // ── Input ────────────────────────────────────────────────────────────────────────────────

    /// Bytes from the radio.
    pub fn on_bytes(&mut self, out: &mut dyn Write, bytes: &[u8], now: u64) {
        if self.phase == Phase::Closed {
            return;
        }
        match self.assembler.push(bytes) {
            Ok(lines) => {
                for raw in lines {
                    if self.phase == Phase::Closed {
                        break;
                    }
                    match String::from_utf8(raw) {
                        Ok(line) => self.on_line(out, &line, now),
                        Err(e) => {
                            // The readback still sees it, as upstream's lossy decode would show
                            // it; a garbled line during an attempt must spoil the attempt.
                            let lossy = String::from_utf8_lossy(e.as_bytes()).into_owned();
                            let stamp = self.stamp();
                            self.tracker.observe(stamp, &lossy, now);
                            self.events.push_back(Event::LineRejected {
                                error: WireError::NotUtf8,
                            });
                        }
                    }
                }
            }
            Err(TooLong) => self.close_with(out, End::ProtocolError, now),
        }
    }

    /// One whole line from the radio.
    pub fn on_line(&mut self, out: &mut dyn Write, line: &str, now: u64) {
        // The readback sees every line first, raw, in arrival order.
        let stamp = self.stamp();
        self.tracker.observe(stamp, line, now);
        match parse_line(line) {
            Ok(Line::Version(v)) => self.on_version(Some(v)),
            Err(WireError::BadVersion) => {
                self.events.push_back(Event::LineRejected {
                    error: WireError::BadVersion,
                });
                self.on_version(None);
            }
            Ok(Line::Handle(h)) => self.on_handle(out, Some(h), now),
            Err(WireError::BadHandle) => {
                self.events.push_back(Event::LineRejected {
                    error: WireError::BadHandle,
                });
                self.on_handle(out, None, now);
            }
            Ok(Line::Reply(r)) => self.on_reply(out, r, now),
            Ok(Line::Status(s)) => self.on_status(&s, now),
            Ok(Line::Message(m)) => {
                self.registration.note_radio_message(&m.text, m.severity);
                self.events.push_back(Event::Message {
                    severity: m.severity,
                    number: m.number,
                    text: m.text,
                });
            }
            Err(error) => self.events.push_back(Event::LineRejected { error }),
        }
        self.check_evidence(now);
        self.check_window(out, now);
    }

    /// This session may no longer transmit. The unkey path stays: keyed is kept, and stops are
    /// still written.
    fn reject_protocol(&mut self) {
        if self.protocol != Protocol::Rejected {
            self.protocol = Protocol::Rejected;
            self.events.push_back(Event::ProtocolRejected);
        }
        self.tracker.disconnect();
    }

    fn on_version(&mut self, version: Option<String>) {
        if self.phase != Phase::AwaitVersion {
            // A late or repeated prologue cannot renegotiate transmit.
            self.reject_protocol();
            return;
        }
        self.phase = Phase::AwaitHandle;
        let supported = version.as_deref().is_some_and(supports_protocol);
        if let Some(version) = version {
            self.events.push_back(Event::Prologue {
                version,
                transmit_protocol: supported,
            });
        }
        if supported {
            self.protocol = Protocol::Supported;
        } else {
            self.reject_protocol();
        }
    }

    fn on_handle(&mut self, out: &mut dyn Write, handle: Option<u32>, now: u64) {
        match self.phase {
            Phase::AwaitVersion => {
                // No version first: receive only, but the session goes on.
                self.reject_protocol();
            }
            Phase::AwaitHandle => {}
            _ => {
                self.reject_protocol();
                return;
            }
        }
        match handle.filter(|h| *h != 0) {
            Some(h) => {
                self.handle = Some(h);
                self.events.push_back(Event::Handle(h));
                if self.protocol == Protocol::Supported {
                    self.tracker.reset(self.generation, h);
                }
            }
            None => self.reject_protocol(),
        }
        self.phase = Phase::Registering;
        self.phase_deadline_ms = Some(now + self.config.registration_timeout_ms);
        for command in handshake::opening(&self.config.connect) {
            let pending = if matches!(command, Command::ClientGui(_)) {
                self.registration.begin();
                Pending::Gui
            } else {
                Pending::Setup(
                    encode::render(&command)
                        .map(|r| r.text().to_string())
                        .unwrap_or_default(),
                )
            };
            if self.send_internal(out, &command, pending, now).is_none() {
                return;
            }
        }
    }

    fn on_reply(&mut self, out: &mut dyn Write, reply: Reply, now: u64) {
        let Some(pending) = self.pending.remove(&reply.seq) else {
            // Not a number this session issued (or already answered): never someone else's
            // answer.
            return;
        };
        let ok = reply.is_success();
        match pending {
            Pending::Ping => {
                self.keepalive.reply(reply.seq, now);
            }
            Pending::Gui => self.on_registration(out, &reply, now),
            Pending::Setup(command) => {
                if !ok {
                    self.events.push_back(Event::SetupRefused {
                        command,
                        code: reply.code,
                    });
                }
            }
            Pending::MicList => {
                if ok {
                    self.model.mic_inputs = Some(super::kv::split_list(&reply.message));
                } else {
                    self.events.push_back(Event::SetupRefused {
                        command: "mic list".into(),
                        code: reply.code,
                    });
                }
            }
            Pending::UdpPort(port) => {
                if handshake::is_udp_port_in_use(reply.code, &reply.message) {
                    self.events.push_back(Event::UdpPortInUse { port });
                } else if !ok && reply.code != NOT_SUPPORTED {
                    self.events.push_back(Event::SetupRefused {
                        command: format!("client udpport {port}"),
                        code: reply.code,
                    });
                }
            }
            Pending::SliceList => {
                let slices = if ok {
                    reply
                        .message
                        .split(' ')
                        .filter(|w| !w.is_empty())
                        .map(|w| wire::parse_dec_u32(w).and_then(|n| u8::try_from(n).ok()))
                        .collect::<Option<Vec<u8>>>()
                } else {
                    None
                };
                if self.phase == Phase::Subscribing {
                    self.phase = Phase::Ready;
                }
                self.events.push_back(Event::Ready { slices });
            }
            Pending::TxAudio(audio) => {
                // Our transmit stream's id is the create's answer: the one way to know a stream
                // whose status may not name its owner is ours.
                if ok && audio == TxAudio::CreateDaxTx {
                    if let Some(id) =
                        super::ownership::parse_create_response_stream_id(&reply.message)
                    {
                        self.dax_tx_stream = Some(id);
                    }
                }
                self.events.push_back(Event::Reply {
                    seq: reply.seq,
                    code: reply.code,
                    message: reply.message,
                });
            }
            Pending::Stop(stop) => {
                // Stop TX in the middle of a CW message: the clear's reply starts the wait for
                // the follow-up unkey ([`Self::poll`]), and a clear the radio took has emptied its
                // buffer, so our CWX's window no longer waits out the last word's expected end.
                if stop == TxStop::CwxClear {
                    if let Some(keyed) = self.keyed.as_mut().filter(|k| k.kind == StartKind::Cwx) {
                        keyed.clear_replied_ms.get_or_insert(now);
                        if ok {
                            self.tracker.cleared(now);
                        }
                    }
                }
                self.events.push_back(Event::Reply {
                    seq: reply.seq,
                    code: reply.code,
                    message: reply.message,
                });
            }
            Pending::Caller | Pending::Key => {
                self.events.push_back(Event::Reply {
                    seq: reply.seq,
                    code: reply.code,
                    message: reply.message,
                });
            }
            Pending::Teardown(stream) => {
                let done = match self.closing.as_mut() {
                    Some((_, waiting, _)) => {
                        waiting.remove(&stream);
                        waiting.is_empty()
                    }
                    None => false,
                };
                if done {
                    self.finish_close(out);
                }
            }
        }
    }

    fn on_registration(&mut self, out: &mut dyn Write, reply: &Reply, now: u64) {
        let result = self.registration.complete(reply.code, &reply.message);
        if !result.accepted() {
            self.events.push_back(Event::RegistrationRejected {
                code: result.code,
                detail: result.detail.clone(),
            });
            self.close_with(
                out,
                End::RegistrationRejected {
                    code: result.code,
                    detail: result.detail,
                },
                now,
            );
            return;
        }
        self.events.push_back(Event::Registered {
            client_id: ClientId::parse(reply.message.trim()),
        });
        self.phase = Phase::Subscribing;
        self.phase_deadline_ms = None;
        for command in handshake::after_registration(&self.config.connect) {
            let pending = match &command {
                Command::MicList => Pending::MicList,
                Command::SliceList => Pending::SliceList,
                Command::ClientUdpPort(port) => Pending::UdpPort(*port),
                other => Pending::Setup(
                    encode::render(other)
                        .map(|r| r.text().to_string())
                        .unwrap_or_default(),
                ),
            };
            // The UDP registration's datagram goes here: after registration, just before
            // `client udpport` (see `handshake`). Once: the sender goes with it.
            if matches!(command, Command::ClientUdpPort(_)) {
                if let Some(registration) = self.config.udp_registration.take() {
                    registration.send();
                }
            }
            if self.send_internal(out, &command, pending, now).is_none() {
                return;
            }
            if command == Command::KeepaliveEnable {
                self.keepalive.start(now);
            }
        }
    }

    fn on_status(&mut self, status: &Status, now: u64) {
        let decoded = decode(status);
        if let Decoded::Client {
            handle,
            action: ClientAction::Connected,
            ..
        } = &decoded
        {
            if Some(*handle) != self.handle {
                self.keepalive.note_foreign_client_connected(now);
            }
        }
        self.model.apply(&decoded);
        if let Decoded::Stream {
            id, removed: true, ..
        } = &decoded
        {
            if self.dax_tx_stream == Some(*id) {
                self.dax_tx_stream = None;
            }
        }
        if let Decoded::Interlock(d) = &decoded {
            if let Some(h) = d.tx_client_handle {
                let previous =
                    h != 0 && Some(h) != self.handle && self.config.previous_handles.contains(&h);
                if previous && self.warned_previous.insert(h) {
                    self.events
                        .push_back(Event::PreviousSessionHoldsTransmitter { handle: h });
                }
            }
        }
    }

    // ── Time ─────────────────────────────────────────────────────────────────────────────────

    /// Let time pass: pings, deadlines, the readback's clock.
    pub fn poll(&mut self, out: &mut dyn Write, now: u64) {
        if self.phase == Phase::Closed {
            return;
        }
        if let Some(deadline) = self.phase_deadline_ms {
            if now >= deadline
                && matches!(
                    self.phase,
                    Phase::AwaitVersion | Phase::AwaitHandle | Phase::Registering
                )
            {
                self.close_with(out, End::ProtocolError, now);
                return;
            }
        }
        match self.keepalive.tick(now) {
            Tick::Idle => {}
            Tick::Ping => self.ping(out, now),
            Tick::Missed { misses } => {
                // A missed ping while keyed ends what is ours before anything else.
                if self.keyed.is_some() {
                    self.end_ours(out, now);
                    self.events.push_back(Event::UnkeyedOnMissedPing { misses });
                }
                self.ping(out, now);
            }
            Tick::Lost { .. } => {
                self.close_with(out, End::KeepaliveLost, now);
                return;
            }
        }
        self.tracker.poll(now);
        self.check_evidence(now);
        self.check_window(out, now);
        if let Some(keyed) = self.keyed {
            let due = keyed
                .first_stop_ms
                .is_some_and(|first| now >= first.saturating_add(self.config.unkey_deadline_ms));
            if due && !keyed.escalated {
                self.escalate(out, now);
                return;
            }
            // Stop TX in the middle of a CW message: the clear empties the radio's buffer, and
            // only the word going out may finish. A radio still showing our CWX transmitting
            // break-in delay + 300 ms after the clear's reply is told to unkey, once; the
            // escalation clock the clear armed stands.
            let follow_up = keyed.clear_replied_ms.is_some_and(|at| {
                now >= at
                    .saturating_add(self.break_in_delay_ms())
                    .saturating_add(ptt_evidence::CWX_IDLE_MS)
            });
            if follow_up && !keyed.unkey_followed && !keyed.escalated && self.still_transmitting() {
                if let Some(k) = self.keyed.as_mut() {
                    k.unkey_followed = true;
                }
                self.stop(out, TxStop::Unkey, now);
            }
            // A latched tune is ended here at its deadline, whether or not the radio loop asks.
            let latched_out = keyed.deadline_ms.is_some_and(|d| now >= d);
            if latched_out && keyed.first_stop_ms.is_none() && !keyed.escalated {
                self.end_ours(out, now);
            }
        }
        if let Some((_, _, deadline)) = &self.closing {
            if now >= *deadline {
                self.finish_close(out);
            }
        }
    }

    fn ping(&mut self, out: &mut dyn Write, now: u64) {
        if let Some(seq) = self.send_internal(out, &Command::Ping, Pending::Ping, now) {
            self.keepalive.sent(seq, now);
        }
    }

    // ── Ending ───────────────────────────────────────────────────────────────────────────────

    /// Close on purpose: teardown, then [`Event::Closed`] with [`End::Closed`].
    pub fn close(&mut self, out: &mut dyn Write, now: u64) {
        self.close_with(out, End::Closed, now);
    }

    /// Teardown: a best-effort end of everything ours ([`Self::end_ours`]) if ours may be keyed
    /// and no stop has gone out, `stream remove` for our streams, then the marker and the close
    /// once they are answered or the wait runs out.
    fn close_with(&mut self, out: &mut dyn Write, end: End, now: u64) {
        if matches!(self.phase, Phase::Closing | Phase::Closed) {
            return;
        }
        let unstopped = self.keyed.is_some_and(|k| k.first_stop_ms.is_none());
        if unstopped || (self.named() && self.keyed.is_none()) {
            self.end_ours(out, now);
            if self.phase == Phase::Closed {
                return;
            }
        }
        self.phase = Phase::Closing;
        self.phase_deadline_ms = None;
        let mut waiting = BTreeSet::new();
        for stream in self.model.our_streams(self.handle) {
            if self
                .send_internal(
                    out,
                    &Command::StreamRemove { stream },
                    Pending::Teardown(stream),
                    now,
                )
                .is_some()
            {
                waiting.insert(stream);
            }
            if self.phase == Phase::Closed {
                return;
            }
        }
        let empty = waiting.is_empty();
        self.closing = Some((end, waiting, now + self.config.teardown_timeout_ms));
        if empty {
            self.finish_close(out);
        }
    }

    /// The disconnect marker, then closed.
    fn finish_close(&mut self, out: &mut dyn Write) {
        if self.phase == Phase::Closed {
            return;
        }
        let _ = out.write_all(&[0x04]).and_then(|()| out.flush());
        let end = self
            .closing
            .take()
            .map(|(end, _, _)| end)
            .unwrap_or(End::Closed);
        self.finished(end);
    }

    /// The connection is gone (end of stream, a read or write error). Nothing more can be written.
    /// A close already under way keeps the reason it was started for: a refused registration stays
    /// terminal even when the radio drops the connection first.
    pub fn transport_closed(&mut self, _now: u64) {
        if self.phase == Phase::Closed {
            return;
        }
        let end = self
            .closing
            .take()
            .map(|(end, _, _)| end)
            .unwrap_or(End::Lost);
        self.finished(end);
    }

    fn finished(&mut self, end: End) {
        self.phase = Phase::Closed;
        self.registration.reset();
        self.tracker.disconnect();
        self.events.push_back(Event::Closed {
            end,
            was_keyed: self.keyed.is_some(),
            handle: self.handle,
        });
    }
}

// ── The thread driver ────────────────────────────────────────────────────────────────────────

/// How long the driver waits for bytes before it looks at requests and time again. A stop waits
/// at most this long behind a read.
pub const READ_POLL: Duration = Duration::from_millis(20);

/// How long [`Connection::connect`] may take to open TCP. Bounded so that nothing waiting on it
/// (ultimately the radio loop, which unkeys) can be held by an unreachable address.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// How long one write may block before the session counts the connection as gone.
pub const WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// Why a request through [`Connection`] got no reply.
#[derive(Debug, Clone, PartialEq)]
pub enum ConnError {
    /// The session refused to send it.
    Refused(SendError),
    /// No reply within the wait.
    Timeout,
    /// The session is closed.
    Closed,
}

enum Request {
    Send(Command, Sender<Result<Reply, ConnError>>),
    Start(TxStart, Option<u64>, Sender<Result<u32, Refusal>>),
    Stop(TxStop, Sender<StopOutcome>),
    EndOurs(Sender<EndOutcome>),
    Route(TxAudio, Sender<Result<u32, Refusal>>),
    Close,
}

struct Shared {
    snapshot: Mutex<Snapshot>,
    changed: Condvar,
    /// An over of ours (`xmit 1`) unconfirmed, and the session open, readable without copying the
    /// model: the DAX transmit pacer asks before every packet.
    over: AtomicBool,
    open: AtomicBool,
    /// The snapshot's `ours_on_air`, the same way: the radio loop asks every tick.
    on_air: AtomicBool,
    /// Test only: where a test holds the driver (see [`Connection::hold_after`]).
    #[cfg(test)]
    hold: Mutex<Option<(Announcement, Receiver<()>)>>,
}

/// Something the driver tells a reader, which a test can hold the driver right after
/// ([`Connection::hold_after`]).
#[cfg(test)]
#[derive(PartialEq)]
pub(super) enum Announcement {
    /// The answer to a start, a stop or a refused command.
    Answer,
    Event(Event),
}

#[cfg(test)]
impl Shared {
    /// If a test is holding the driver after `announcement`, wait here until it lets go.
    fn pause_after(&self, announcement: &Announcement) {
        let held = lock(&self.hold).take_if(|(after, _)| *after == *announcement);
        if let Some((_, release)) = held {
            let _ = release.recv();
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// A [`Session`] running on its own thread over one connection. `Sync`, so one connection can
/// serve several callers at once: each request waits on its own answer, and a stop never queues
/// behind another caller's request.
pub struct Connection {
    requests: Sender<Request>,
    /// Behind a lock only so the connection is `Sync`; one reader drains it.
    events: Mutex<Receiver<Event>>,
    shared: Arc<Shared>,
    thread: Option<JoinHandle<()>>,
    /// The kinds of start its admission takes ([`Session::switched_on`]), fixed for its life.
    switched_on: Vec<StartKind>,
}

impl Connection {
    /// Open TCP to the radio and start the session.
    pub fn connect(addr: SocketAddr, config: Config) -> io::Result<Connection> {
        let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT)?;
        stream.set_nodelay(true)?;
        stream.set_read_timeout(Some(READ_POLL))?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
        Connection::spawn(stream, config)
    }

    /// Start the session over an open transport. Its reads must time out (the driver interleaves
    /// them with requests and time).
    pub fn spawn<S: Read + Write + Send + 'static>(
        io: S,
        config: Config,
    ) -> io::Result<Connection> {
        let (requests, request_rx) = mpsc::channel();
        let (event_tx, events) = mpsc::channel();
        let epoch = Instant::now();
        let session = Session::new(config, 0);
        let switched_on = [
            StartKind::Key,
            StartKind::Tune,
            StartKind::Atu,
            StartKind::Cwx,
        ]
        .into_iter()
        .filter(|k| session.switched_on(*k))
        .collect();
        let shared = Arc::new(Shared {
            snapshot: Mutex::new(session.snapshot()),
            changed: Condvar::new(),
            over: AtomicBool::new(false),
            open: AtomicBool::new(true),
            on_air: AtomicBool::new(false),
            #[cfg(test)]
            hold: Mutex::new(None),
        });
        let thread = {
            let shared = Arc::clone(&shared);
            std::thread::Builder::new()
                .name("flex-session".into())
                .spawn(move || drive(io, session, epoch, &request_rx, &event_tx, &shared))?
        };
        Ok(Connection {
            requests,
            events: Mutex::new(events),
            shared,
            thread: Some(thread),
            switched_on,
        })
    }

    /// Whether this connection's admission takes starts of `kind` ([`Session::switched_on`]): a
    /// kind it refuses before any other check is one its owner does not offer.
    pub fn switched_on(&self, kind: StartKind) -> bool {
        self.switched_on.contains(&kind)
    }

    /// Send a command and wait for its reply.
    pub fn request(&self, command: Command, timeout: Duration) -> Result<Reply, ConnError> {
        let (tx, rx) = mpsc::channel();
        self.requests
            .send(Request::Send(command, tx))
            .map_err(|_| ConnError::Closed)?;
        match rx.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => Err(ConnError::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(ConnError::Closed),
        }
    }

    /// Start a transmission (admission decides). `Err(None)` when the session is gone.
    pub fn start(&self, start: TxStart) -> Result<u32, Option<Refusal>> {
        self.start_lasting(start, None)
    }

    /// [`Session::start_lasting`]: a start with how long it lasts, as its caller knows it.
    pub fn start_lasting(
        &self,
        start: TxStart,
        lasting_ms: Option<u64>,
    ) -> Result<u32, Option<Refusal>> {
        let (tx, rx) = mpsc::channel();
        self.requests
            .send(Request::Start(start, lasting_ms, tx))
            .map_err(|_| None)?;
        rx.recv().map_err(|_| None)?.map_err(Some)
    }

    /// Change the transmitter's audio source (its admission decides). `Err(None)` when the
    /// session is gone.
    pub fn route(&self, audio: TxAudio) -> Result<u32, Option<Refusal>> {
        let (tx, rx) = mpsc::channel();
        self.requests
            .send(Request::Route(audio, tx))
            .map_err(|_| None)?;
        rx.recv().map_err(|_| None)?.map_err(Some)
    }

    /// Whether an over of ours (`xmit 1`) is unconfirmed, on a session that has not closed: what
    /// the last published state says, without copying it. The DAX transmit pacer sends only then:
    /// a CWX word, a tune or an ATU cycle is keyed unconfirmed too, but the radio makes it itself,
    /// and no DAX audio belongs on it.
    pub fn over_keyed(&self) -> bool {
        self.shared.open.load(Ordering::Acquire) && self.shared.over.load(Ordering::Acquire)
    }

    /// End a transmission. Never gated.
    pub fn stop(&self, stop: TxStop) -> StopOutcome {
        let (tx, rx) = mpsc::channel();
        if self.requests.send(Request::Stop(stop, tx)).is_err() {
            return StopOutcome::NotConnected;
        }
        rx.recv().unwrap_or(StopOutcome::NotConnected)
    }

    /// End everything of ours ([`Session::end_ours`]): rigctld's `T 0`. Never gated.
    pub fn end_ours(&self) -> EndOutcome {
        let (tx, rx) = mpsc::channel();
        if self.requests.send(Request::EndOurs(tx)).is_err() {
            return EndOutcome::NotConnected;
        }
        rx.recv().unwrap_or(EndOutcome::NotConnected)
    }

    /// Whether a transmission of ours may be on the air ([`Snapshot::ours_on_air`]), as the last
    /// published snapshot says, without copying it. Kept once the session has closed: a session
    /// that ended with something of ours unconfirmed still says so.
    pub fn ours_on_air(&self) -> bool {
        self.shared.on_air.load(Ordering::Acquire)
    }

    /// The next event, waiting up to `timeout`.
    pub fn next_event(&self, timeout: Duration) -> Option<Event> {
        lock(&self.events).recv_timeout(timeout).ok()
    }

    /// The session's state as of its last step. Each step's state is published before its answers
    /// and events go out, so whoever acts on an answer or an event reads the state it reports, or
    /// a later one.
    pub fn snapshot(&self) -> Snapshot {
        lock(&self.shared.snapshot).clone()
    }

    /// Read the same state in place, without the copy [`Self::snapshot`] makes: for a caller that
    /// asks every tick for a field or two.
    pub fn read<T>(&self, f: impl FnOnce(&Snapshot) -> T) -> T {
        f(&lock(&self.shared.snapshot))
    }

    /// Wait until `done` holds for the state, or `timeout` passes. Returns whether it held.
    pub fn wait_until(&self, timeout: Duration, done: impl Fn(&Snapshot) -> bool) -> bool {
        let deadline = Instant::now() + timeout;
        let mut snapshot = lock(&self.shared.snapshot);
        loop {
            if done(&snapshot) {
                return true;
            }
            let now = Instant::now();
            if now >= deadline {
                return false;
            }
            snapshot = self
                .shared
                .changed
                .wait_timeout(snapshot, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Test only: the driver stops right after its next `announcement` and stays stopped until
    /// the returned sender is dropped, so a test reads the state at the first moment a reader
    /// woken by that announcement can.
    #[cfg(test)]
    pub(super) fn hold_after(&self, announcement: Announcement) -> Sender<()> {
        let (release, held) = mpsc::channel();
        *lock(&self.shared.hold) = Some((announcement, held));
        release
    }

    /// Close on purpose, waiting for teardown to finish.
    pub fn close(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        let _ = self.requests.send(Request::Close);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn retryable(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted
    )
}

fn drive<S: Read + Write>(
    mut io: S,
    mut session: Session,
    epoch: Instant,
    requests: &Receiver<Request>,
    events: &Sender<Event>,
    shared: &Shared,
) {
    let now = || u64::try_from(epoch.elapsed().as_millis()).unwrap_or(u64::MAX);
    let mut waiters: BTreeMap<u32, Sender<Result<Reply, ConnError>>> = BTreeMap::new();
    let mut buf = vec![0u8; 16 * 1024];
    loop {
        // Requests first, so a stop never waits behind more than one read.
        loop {
            match requests.try_recv() {
                Ok(Request::Send(command, reply)) => match session.send(&mut io, &command, now()) {
                    Ok(seq) => {
                        waiters.insert(seq, reply);
                    }
                    Err(e) => answer(&session, shared, reply, Err(ConnError::Refused(e))),
                },
                Ok(Request::Start(start, lasting_ms, reply)) => {
                    let started = session.start_lasting(&mut io, start, lasting_ms, now());
                    answer(&session, shared, reply, started);
                }
                Ok(Request::Stop(stop, reply)) => {
                    let stopped = session.stop(&mut io, stop, now());
                    answer(&session, shared, reply, stopped);
                }
                Ok(Request::EndOurs(reply)) => {
                    let ended = session.end_ours(&mut io, now());
                    answer(&session, shared, reply, ended);
                }
                Ok(Request::Route(audio, reply)) => {
                    let routed = session.route(&mut io, audio, now());
                    answer(&session, shared, reply, routed);
                }
                Ok(Request::Close) | Err(TryRecvError::Disconnected) => {
                    session.close(&mut io, now());
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if !session.is_closed() {
            match io.read(&mut buf) {
                Ok(0) => session.transport_closed(now()),
                Ok(n) => session.on_bytes(&mut io, &buf[..n], now()),
                Err(e) if retryable(&e) => {}
                Err(_) => session.transport_closed(now()),
            }
            session.poll(&mut io, now());
        }
        publish(&session, shared);
        for event in session.take_events() {
            if let Event::Reply { seq, code, message } = &event {
                if let Some(waiter) = waiters.remove(seq) {
                    let _ = waiter.send(Ok(Reply {
                        seq: *seq,
                        code: *code,
                        message: message.clone(),
                    }));
                }
            }
            #[cfg(test)]
            let announced = Announcement::Event(event.clone());
            let _ = events.send(event);
            #[cfg(test)]
            shared.pause_after(&announced);
        }
        if session.is_closed() {
            break;
        }
    }
    // Waiters still pending see their channel close.
    drop(waiters);
    let _ = io.flush();
}

/// Store the session's state for [`Connection::snapshot`] and wake [`Connection::wait_until`].
/// The driver does this before it announces anything a step did (an answer, a reply, an event),
/// so a reader woken by an announcement reads the state it reports, or a later one, never the
/// state from before it.
fn publish(session: &Session, shared: &Shared) {
    let snapshot = session.snapshot();
    shared.on_air.store(snapshot.ours_on_air, Ordering::Release);
    *lock(&shared.snapshot) = snapshot;
    let over = session.keyed.is_some_and(|k| k.kind == StartKind::Key);
    shared.over.store(over, Ordering::Release);
    shared.open.store(!session.is_closed(), Ordering::Release);
    shared.changed.notify_all();
}

/// Answer a request, once the state it left is published.
fn answer<T>(session: &Session, shared: &Shared, reply: Sender<T>, value: T) {
    publish(session, shared);
    let _ = reply.send(value);
    #[cfg(test)]
    shared.pause_after(&Announcement::Answer);
}

#[cfg(test)]
mod kind_tests;
#[cfg(test)]
mod tests;
