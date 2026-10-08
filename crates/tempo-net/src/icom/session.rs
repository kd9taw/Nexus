//! One network session with the radio, from the first probe to the last disconnect: the
//! handshake, keepalive, retransmit requests, per-socket liveness, the CI-V stream and teardown,
//! as a state machine with no I/O of its own.
//!
//! **Who does what.** [`Session`] owns no socket and no clock. Its owner holds three UDP sockets
//! connected to the radio's control port, reports their local addresses ([`Locals`]), hands the
//! session every datagram with [`Session::on_datagram`], calls [`Session::poll`] at least every
//! 20 ms, reports send and receive failures with [`Session::on_socket_error`], and performs the
//! [`Action`]s it is given: send a datagram, point a socket at the port the radio assigned,
//! deliver a CI-V frame, report an [`Event`]. Time is milliseconds on a monotonic clock. One
//! owner drives one session; nothing here locks or blocks.
//!
//! **The handshake** runs in the radio's fixed order, each stage a request sent again every
//! 500 ms up to 20 times while idles keep the socket alive: probe and ready on the control
//! socket; the login; the token, answered with the radio's capabilities, from which the radio is
//! chosen; the connection-info request, answered with the ports the radio assigns; probe and
//! ready on the CI-V socket; the CI-V stream open. A reply the radio sent and lost on the way is
//! noticed from the gap in its sequence numbers and asked for again: resending the request would
//! not help, since the radio has the request and does not answer it twice. The radio numbers a
//! stream from 0, and tracking expects that, so even a lost first reply is recovered. Then the
//! radio's leftover CI-V frames from an earlier client are drained (until 50 ms pass with none,
//! at most 250 ms), and [`Event::Connected`] says CI-V can flow.
//!
//! **A connect that fails part-way gives back what the radio granted:** after the login the
//! radio keeps a token and a session slot for this client, and walking away from them makes it
//! refuse new attempts for tens of seconds. So a failed connect sends the token back, answers
//! the radio's retransmit requests for 300 ms, and disconnects. A closed session does the same
//! after closing the CI-V stream; [`Event::Closed`] says the sockets can go.
//!
//! **Liveness is per socket and fixed at [`LIVENESS_MS`].** The radio sends every socket steady
//! traffic, so silence on any one of them for 5000 ms loses the session, even while another is
//! busy: the control socket can go on with keepalives after the radio has stopped serving CI-V.
//! A socket error never ends the session by itself; it names the cause, and a socket that goes
//! silent after one is lost as [`LossReason::SocketError`] rather than
//! [`LossReason::LinkTimeout`]. The radio ending the session itself (another client took it) is
//! [`LossReason::PeerDisconnect`]. A lost session stays lost: reconnecting is the owner's
//! decision ([`super::reconnect`]), never this module's.
//!
//! **Nothing here transmits.** The session never originates a CI-V command: the only CI-V
//! packets it sends carry a frame its owner handed to [`Session::send_civ`], or are the very
//! packet sent again when the radio asks for that sequence. Everything else it sends is control,
//! ping, idle, retransmit, open/close and the login, token and connection-info requests. The
//! connection-info request always carries transmit-enable 0, so no transmit audio path is
//! reserved, and no audio stream is started: the audio socket's local port is named in the
//! request because the radio requires one, and that socket carries nothing but the teardown's
//! disconnect.
//!
//! **CI-V frames.** The radio echoes the controller's own commands back, addressed to the radio;
//! those are dropped, so only replies and unsolicited frames are delivered, each once (a repeated
//! reply would be taken as the answer to the next command). A frame missing its second `FE`, as
//! radios sometimes send, has it restored.
//!
//! PORTED from Hamlib (https://github.com/Hamlib/Hamlib, pull request #2178, open when taken),
//! `rigs/icom/network_session.c` and `rigs/icom/network_session.h` at commit
//! `2e3e4a6add3bd806e828d035200777608d2bdbd5` (2026-10-07), translated from C to Rust.
//! The facts the session's configuration and order rest on are read, with no code taken, from
//! `rigs/icom/ICOM.md` (the handshake order, losing the radio), `rigs/icom/network_conf.c` (the
//! 16-character credentials, the liveness default, the default control port) and
//! `rigs/icom/icom_network.c` (the open and close order around the session, the receive codec and
//! rate it requests, the client name field).
//! Deliberate differences: one owner with time passed in, a state machine whose calls return
//! actions, in place of a control thread, a CI-V thread and a reconnect thread around shared
//! state and locks (so the open upstream review point about two stream opens racing under a
//! shared read lock has no counterpart); no reconnect inside the session; liveness fixed at
//! 5000 ms per socket, with no "0 = never"; the connection-info request always carries
//! transmit-enable 0, and no audio stream is started; CI-V frames are delivered as actions rather
//! than queued for a blocking reader, never truncated (upstream copies at most 256 bytes), and an
//! empty payload delivers nothing; a CI-V frame handed over while the CI-V socket reports errors,
//! or one too long to keep for a retransmit, is refused before a sequence number is used, where
//! upstream sends it and reports a failure; a radio name that differs from the model's is a
//! warning when the radio advertises only one radio, where upstream refuses (with several
//! advertised, a name that matches none is still refused); a datagram on any socket is serviced
//! as it arrives, so a ping on the control socket during the CI-V handshake is answered then; the
//! control socket's replay buffer is purged at the same 10 s age as the CI-V one, so the login
//! packet, whose credential fields are only obfuscated, is not kept for the session's life; the
//! radio's connection id is learned only from a datagram whose header length is consistent; the
//! password is moved into the login builder and not kept; loss reasons and connect failures are
//! typed values.
//! The protocol knowledge descends from kappanhang (https://github.com/nonoo/kappanhang, MIT), as the
//! upstream author states. Recorded in the repo-root NOTICE (Hamlib entry), which reproduces
//! kappanhang's notice.
//!
//! The upstream notice, converted to the GNU General Public License under section 3 of the GNU
//! Lesser General Public License, version 2.1, and otherwise unchanged:
//!
//! Copyright (c) 2026 by Mikael Nousiainen OH3BHX
//!
//! This library is free software; you can redistribute it and/or modify it under the terms of the
//! GNU General Public License as published by the Free Software Foundation; either version 3 of the
//! License, or (at your option) any later version.
//!
//! This library is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY;
//! without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See
//! the GNU General Public License for more details.
//!
//! You should have received a copy of the GNU General Public License along with this library; if
//! not, write to the Free Software Foundation, Inc., 51 Franklin Street, Fifth Floor, Boston, MA
//! 02110-1301 USA
//!
//! SPDX-License-Identifier: GPL-3.0-or-later
//!
//! Modified by KD9TAW, 2026-10-08: translated from C to Rust and changed as listed
//! above. These changes are licensed under the GNU General Public License, version 3 only, as Nexus
//! is (see COPYING), so this file as a whole is distributed under GPL version 3.

use std::fmt;
use std::io;
use std::net::SocketAddrV4;

use super::caps::{self, Capabilities, Choice, Link, Model, Rates, Unselected};
use super::conf::{Config, Login, Secret, User};
use super::seq::{RxResult, RxTracker, TxBuffer, MISSING_FLUSH, SEQBUF_MAX, SEQBUF_PKTMAX};
use super::wire::{
    self, ConnectionRequest, ControlOp, Header, Ids, LoginFields, Packet, Role, SeqRange, StreamOp,
    TokenRequest, WireError, CIV_HEADER_LEN, CODEC_LPCM16,
};

/// Silence on any one socket for longer than this loses the session.
pub const LIVENESS_MS: u64 = 5_000;
/// The keepalive idle on the control and CI-V sockets, and on the socket a handshake stage uses.
pub const IDLE_MS: u64 = 100;
/// The keepalive ping on the control and CI-V sockets.
pub const PING_MS: u64 = 500;
/// The token is renewed this often.
pub const TOKEN_RENEW_MS: u64 = 60_000;
/// A missing sequence is asked for again at most this often.
pub const RETRANSMIT_MS: u64 = 100;
/// Sent packets are kept this long for a retransmit request.
pub const PURGE_MS: u64 = 10_000;
/// A handshake request is sent again after this long without its reply.
pub const HANDSHAKE_RESEND_MS: u64 = 500;
/// A handshake request is sent at most this many times.
pub const HANDSHAKE_TRIES: u32 = 20;
/// The CI-V drain ends when this long passes with no frame…
pub const DRAIN_QUIET_MS: u64 = 50;
/// …or this long after it began.
pub const DRAIN_TOTAL_MS: u64 = 250;
/// After the token goes back, the radio's retransmit requests are answered this long.
pub const DISCONNECT_GRACE_MS: u64 = 300;
/// The client name the login carries.
pub const CLIENT_NAME: &str = "Nexus";
/// The receive rate the connection request names (upstream's default; nothing is started).
pub const RX_RATE_HZ: u32 = 48_000;
/// The longest CI-V frame the session sends: one whose packet the replay buffer can keep.
pub const MAX_CIV_FRAME: usize = SEQBUF_PKTMAX - CIV_HEADER_LEN;

/// One datagram for a socket. Its `Debug` shows the socket and the length, never the bytes: a
/// login's credential fields are only obfuscated.
#[derive(Clone, PartialEq, Eq)]
pub struct Datagram {
    pub role: Role,
    pub bytes: Vec<u8>,
}

impl fmt::Debug for Datagram {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "Datagram {{ role: {:?}, len: {} }}",
            self.role,
            self.bytes.len()
        )
    }
}

/// What the owner must do, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Send this datagram on its socket.
    Send(Datagram),
    /// Point that socket at this port on the radio (the radio assigns the CI-V and audio ports).
    Redirect {
        role: Role,
        port: u16,
    },
    /// A CI-V frame from the radio, `FE FE` to `FD`.
    Deliver(Vec<u8>),
    Event(Event),
}

/// What the owner reports.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// The handshake is done and CI-V can flow.
    Connected(Radio),
    /// The connect failed; the session is giving back what the radio granted, then closes.
    Failed(ConnectError),
    /// A working session stopped working. It stays lost until its owner closes it.
    Lost(Loss),
    /// Everything the session had to send has gone; the sockets can be closed.
    Closed,
}

/// The radio a session connected to, as its capabilities entry described it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Radio {
    /// The name the radio reported.
    pub name: String,
    /// The name the model's server is expected to report.
    pub expected: &'static str,
    /// The radio's CI-V address.
    pub civ_addr: u8,
    pub link: Link,
    /// Whether the radio advertises transmit audio. Nothing is reserved either way.
    pub tx_audio_advertised: bool,
    /// The port the radio assigned for audio, if any.
    pub audio_port: Option<u16>,
}

impl Radio {
    /// Whether the radio reported the name its model's server is expected to.
    pub fn name_matches(&self) -> bool {
        self.name.eq_ignore_ascii_case(self.expected)
    }
}

/// The handshake stages, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    ControlProbe,
    ControlReady,
    Login,
    Capabilities,
    ConnectionInfo,
    CivProbe,
    CivReady,
}

/// Why a connect failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    /// Nothing answered a stage's request in [`HANDSHAKE_TRIES`] tries: a wrong address, the
    /// radio's network control off, or a slot still held for an earlier session all look so.
    NoAnswer(Stage),
    /// The radio refused the user name or the password.
    LoginRefused,
    /// No advertised radio could be chosen.
    NoRadio(Unselected),
    /// The chosen radio does not offer the receive rate the request names.
    RateNotOffered(Rates),
    /// The radio answered the connection request with an error: it is still holding another
    /// session's slot.
    Busy(u32),
    /// The radio's answer to the connection request named no CI-V port.
    NoCivPort,
    /// A request could not be built.
    Request(WireError),
}

/// Why a working session was lost.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LossReason {
    /// A socket heard nothing for [`LIVENESS_MS`].
    LinkTimeout,
    /// A socket went silent after reporting an error.
    SocketError,
    /// The radio ended the session: another client has taken it.
    PeerDisconnect,
}

/// A loss and the socket it was seen on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Loss {
    pub reason: LossReason,
    pub socket: Role,
}

/// A failed send or receive, as the owner reports it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SocketError {
    /// A momentary shortage (a full buffer, an interrupted call) that passes by itself.
    Transient,
    /// Anything else: the socket is not working.
    Hard,
}

/// The error number for "no buffer space": momentary, like a full send buffer.
#[cfg(any(target_os = "linux", target_os = "android"))]
const ENOBUFS: i32 = 105;
/// `WSAENOBUFS`.
#[cfg(windows)]
const ENOBUFS: i32 = 10055;
/// macOS and the BSDs.
#[cfg(not(any(target_os = "linux", target_os = "android", windows)))]
const ENOBUFS: i32 = 55;

impl SocketError {
    /// Sorts an I/O error the way the session needs: a would-block, an interrupted call or a full
    /// buffer is momentary (and a receive timeout, which is how Windows reports one); anything
    /// else means the socket is not working.
    pub fn of(e: &io::Error) -> SocketError {
        let transient = match e.kind() {
            io::ErrorKind::WouldBlock | io::ErrorKind::Interrupted => true,
            io::ErrorKind::TimedOut => cfg!(windows),
            _ => e.raw_os_error() == Some(ENOBUFS),
        };
        if transient {
            SocketError::Transient
        } else {
            SocketError::Hard
        }
    }
}

/// Why a CI-V frame was not sent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CivError {
    /// The session is not connected (still in the handshake, draining, closing or closed).
    NotConnected,
    /// The CI-V socket has reported an error since it last heard from the radio.
    SocketFailing,
    /// Longer than [`MAX_CIV_FRAME`].
    TooLong {
        len: usize,
    },
    Frame(WireError),
}

/// The local addresses of the owner's three sockets, as each reports it once connected towards
/// the radio. The radio knows a socket by an id made from its address and port.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locals {
    pub control: SocketAddrV4,
    pub civ: SocketAddrV4,
    pub audio: SocketAddrV4,
}

/// Where a session is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Connecting(Stage),
    Draining,
    Connected,
    Closing,
    Closed,
}

/// The inner sequence the radio expects the login to carry first.
const INNER_SEQ_START: u16 = 0x30;
/// The client's request id, echoed in the radio's replies. One token exchange is ever in flight,
/// so a fixed marker does.
const TOKEN_REQUEST: u16 = 0x1234;
/// The transmit jitter buffer the connection request names (upstream's default; no transmit
/// path is reserved).
const TX_BUFFER_MS: u32 = 150;

fn at(role: Role) -> usize {
    match role {
        Role::Control => 0,
        Role::Civ => 1,
        Role::Audio => 2,
    }
}

impl Stage {
    /// The socket a stage's request goes out on.
    fn socket(self) -> Role {
        match self {
            Stage::CivProbe | Stage::CivReady => Role::Civ,
            _ => Role::Control,
        }
    }
}

/// One socket's side of the session.
struct Sock {
    ids: Ids,
    local_port: u16,
    /// The next tracked sequence; the radio gap-tracks these, and expects the login at 1.
    send_seq: u16,
    /// The pings' own counter, which must not advance the tracked one.
    ping_seq: u16,
    tx: TxBuffer,
    rx: RxTracker,
    last_heard: u64,
    /// The first hard error since the last datagram received.
    error_since: Option<u64>,
    last_idle: Option<u64>,
    last_ping: Option<u64>,
    resyncs: u32,
}

impl Sock {
    fn new(local: SocketAddrV4, now: u64) -> Sock {
        let mut rx = RxTracker::new();
        // The radio numbers a stream's tracked packets from 0.
        rx.expect(0);
        Sock {
            ids: Ids {
                sender: wire::make_id(local.ip().octets(), local.port()),
                receiver: 0,
            },
            local_port: local.port(),
            send_seq: 1,
            ping_seq: 0,
            tx: TxBuffer::new(),
            rx,
            last_heard: now,
            error_since: None,
            last_idle: None,
            last_ping: None,
            resyncs: 0,
        }
    }

    fn next_tracked(&mut self) -> u16 {
        let seq = self.send_seq;
        self.send_seq = seq.wrapping_add(1);
        seq
    }
}

/// The radio chosen from the capabilities reply.
struct Chosen {
    identity: [u8; 16],
    name: String,
    civ_addr: u8,
    link: Link,
    tx_audio: bool,
}

enum Phase {
    Handshake {
        stage: Stage,
        /// The request, kept to be sent again, byte for byte.
        request: Vec<u8>,
        /// Its sequence, when it is a tracked data packet.
        tracked_seq: Option<u16>,
        tries: u32,
        resend_at: u64,
    },
    Draining {
        quiet_until: u64,
        deadline: u64,
    },
    Connected,
    /// A failed or abandoned connect: the token went back, and the radio's retransmit requests
    /// are answered until `until`, when the control socket disconnects.
    Aborting {
        until: u64,
    },
    /// A teardown's grace, until the disconnects.
    Closing {
        until: u64,
    },
    Closed,
}

/// One session. See the module notes.
pub struct Session {
    model: Model,
    choice: Option<Choice>,
    user: User,
    /// Held only until the login is built, then moved into it.
    password: Option<Secret>,
    sockets: [Sock; 3],
    phase: Phase,
    inner_seq: u16,
    /// The session token; 0 until the login is accepted, and once it has been given back.
    token: u32,
    radio: Option<Chosen>,
    audio_port: Option<u16>,
    /// The CI-V stream's own counter, separate from the socket's packet sequence.
    civ_send_seq: u16,
    lost: Option<Loss>,
    last_token: u64,
    malformed: u64,
}

impl Session {
    /// Starts a session: the first probe goes out at once. The configuration is consumed; the
    /// password goes into the login and is not kept.
    pub fn start(config: Config, locals: Locals, now: u64) -> (Session, Vec<Action>) {
        let Config {
            model,
            choice,
            login: Login { user, password },
            ..
        } = config;
        let mut session = Session {
            model,
            choice,
            user,
            password: Some(password),
            sockets: [
                Sock::new(locals.control, now),
                Sock::new(locals.civ, now),
                Sock::new(locals.audio, now),
            ],
            phase: Phase::Closed,
            inner_seq: INNER_SEQ_START,
            token: 0,
            radio: None,
            audio_port: None,
            civ_send_seq: 0,
            lost: None,
            last_token: now,
            malformed: 0,
        };
        let mut out = Vec::new();
        session.begin(Stage::ControlProbe, now, &mut out);
        (session, out)
    }

    /// A datagram from the radio on the socket `role`.
    pub fn on_datagram(&mut self, role: Role, bytes: &[u8], now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        let serviced = match self.phase {
            Phase::Closed => false,
            // The audio socket is not read: no audio stream is started.
            _ if role == Role::Audio => false,
            // A failed connect answers only the control socket, for the token's retransmit.
            Phase::Aborting { .. } => role == Role::Control,
            _ => true,
        };
        if serviced {
            self.receive(role, bytes, now, &mut out);
        }
        self.tick(now, &mut out);
        out
    }

    /// The timers: resends, idles, pings, retransmit requests, the token renewal, purges,
    /// liveness, the drain's end, the teardown's grace.
    pub fn poll(&mut self, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        self.tick(now, &mut out);
        out
    }

    /// A failed send or receive on the socket `role`. A momentary one changes nothing; a hard one
    /// makes CI-V frames fail at once until the socket next hears from the radio, and names the
    /// cause if the socket then goes silent.
    pub fn on_socket_error(&mut self, role: Role, error: SocketError, now: u64) {
        let sock = self.sock(role);
        if error == SocketError::Hard && sock.error_since.is_none() {
            sock.error_since = Some(now);
        }
    }

    /// Sends one CI-V frame, `FE FE` to `FD`, as the owner hands it.
    pub fn send_civ(&mut self, frame: &[u8], now: u64) -> Result<Vec<Action>, CivError> {
        if !matches!(self.phase, Phase::Connected) {
            return Err(CivError::NotConnected);
        }
        if frame.len() > MAX_CIV_FRAME {
            return Err(CivError::TooLong { len: frame.len() });
        }
        let sock = &self.sockets[at(Role::Civ)];
        // A reply that cannot come is not waited for.
        if sock.error_since.is_some() {
            return Err(CivError::SocketFailing);
        }
        let packet = wire::civ(frame, 0xc1, self.civ_send_seq, sock.send_seq, sock.ids)
            .map_err(CivError::Frame)?;
        let seq = self.sockets[at(Role::Civ)].next_tracked();
        self.civ_send_seq = self.civ_send_seq.wrapping_add(1);
        let mut out = Vec::new();
        self.send_tracked(Role::Civ, seq, packet, now, &mut out);
        Ok(out)
    }

    /// Ends the session: during the handshake, gives the token back and disconnects; once
    /// connected, closes the CI-V stream first. Ends with [`Event::Closed`].
    pub fn close(&mut self, now: u64) -> Vec<Action> {
        let mut out = Vec::new();
        match self.phase {
            Phase::Handshake { .. } => self.abort(now, &mut out),
            Phase::Draining { .. } | Phase::Connected => self.teardown(now, &mut out),
            Phase::Aborting { .. } | Phase::Closing { .. } | Phase::Closed => {}
        }
        out
    }

    pub fn state(&self) -> State {
        match &self.phase {
            Phase::Handshake { stage, .. } => State::Connecting(*stage),
            Phase::Draining { .. } => State::Draining,
            Phase::Connected => State::Connected,
            Phase::Aborting { .. } | Phase::Closing { .. } => State::Closing,
            Phase::Closed => State::Closed,
        }
    }

    /// The loss, once the session is lost.
    pub fn loss(&self) -> Option<Loss> {
        self.lost
    }

    /// How many times a socket's sequence tracking had to start again: a loss wider than the
    /// replay window, or a sender numbering from scratch. A rising count points at the network.
    pub fn resyncs(&self, role: Role) -> u32 {
        self.sockets[at(role)].resyncs
    }

    /// How many datagrams arrived that do not decode.
    pub fn malformed(&self) -> u64 {
        self.malformed
    }

    /// Whether the password is still held: only until the login is built.
    #[cfg(test)]
    pub(crate) fn holds_password(&self) -> bool {
        self.password.is_some()
    }

    // ── sending ─────────────────────────────────────────────────────────────────────────────

    fn sock(&mut self, role: Role) -> &mut Sock {
        &mut self.sockets[at(role)]
    }

    fn send(&mut self, role: Role, bytes: Vec<u8>, out: &mut Vec<Action>) {
        out.push(Action::Send(Datagram { role, bytes }));
    }

    /// A tracked data packet: kept for a retransmit request, then sent. One too large to keep
    /// loses only the answer to a later request for it.
    fn send_tracked(
        &mut self,
        role: Role,
        seq: u16,
        bytes: Vec<u8>,
        now: u64,
        out: &mut Vec<Action>,
    ) {
        let _ = self.sock(role).tx.add(seq, &bytes, now);
        self.send(role, bytes, out);
    }

    fn control(&mut self, role: Role, op: ControlOp, out: &mut Vec<Action>) {
        let ids = self.sock(role).ids;
        self.send(role, wire::control(op, 0, ids), out);
    }

    fn next_inner(&mut self) -> u16 {
        let inner = self.inner_seq;
        self.inner_seq = inner.wrapping_add(1);
        inner
    }

    fn send_request(
        &mut self,
        role: Role,
        request: Vec<u8>,
        seq: Option<u16>,
        now: u64,
        out: &mut Vec<Action>,
    ) {
        match seq {
            Some(seq) => self.send_tracked(role, seq, request, now, out),
            None => self.send(role, request, out),
        }
    }

    fn idle_if_due(&mut self, role: Role, now: u64, out: &mut Vec<Action>) {
        let sock = self.sock(role);
        if sock
            .last_idle
            .is_none_or(|t| now.saturating_sub(t) >= IDLE_MS)
        {
            sock.last_idle = Some(now);
            self.control(role, ControlOp::Idle, out);
        }
    }

    fn ping_if_due(&mut self, role: Role, now: u64, out: &mut Vec<Action>) {
        let sock = self.sock(role);
        if sock
            .last_ping
            .is_none_or(|t| now.saturating_sub(t) >= PING_MS)
        {
            sock.last_ping = Some(now);
            let seq = sock.ping_seq;
            sock.ping_seq = seq.wrapping_add(1);
            let ids = sock.ids;
            self.send(role, wire::ping(false, now as u32, seq, ids), out);
        }
    }

    /// Asks again for whatever the radio sent that never arrived.
    fn request_retransmits(&mut self, role: Role, now: u64, out: &mut Vec<Action>) {
        let sock = self.sock(role);
        let due = sock.rx.due(now, RETRANSMIT_MS, SEQBUF_MAX);
        if due.is_empty() {
            return;
        }
        if let Ok(packet) = wire::retransmit(&due, sock.ids) {
            self.send(role, packet, out);
        }
    }

    /// Sends again the stored packets the radio asked for.
    fn replay(&mut self, role: Role, wanted: &[SeqRange], out: &mut Vec<Action>) {
        for range in wanted {
            let mut seq = range.first;
            loop {
                if let Some(bytes) = self.sock(role).tx.get(seq) {
                    let bytes = bytes.to_vec();
                    self.send(role, bytes, out);
                }
                if seq == range.last {
                    break;
                }
                seq = seq.wrapping_add(1);
            }
        }
    }

    fn send_token_remove(&mut self, now: u64, out: &mut Vec<Action>) {
        if self.token == 0 {
            return;
        }
        let seq = self.sockets[at(Role::Control)].next_tracked();
        let inner = self.next_inner();
        let ids = self.sockets[at(Role::Control)].ids;
        let packet = wire::token(
            TokenRequest::Remove,
            inner,
            TOKEN_REQUEST,
            self.token,
            None,
            seq,
            ids,
        );
        self.send_tracked(Role::Control, seq, packet, now, out);
    }

    fn renew_token(&mut self, now: u64, out: &mut Vec<Action>) {
        let Some(identity) = self.radio.as_ref().map(|r| r.identity) else {
            return;
        };
        let seq = self.sockets[at(Role::Control)].next_tracked();
        let inner = self.next_inner();
        let ids = self.sockets[at(Role::Control)].ids;
        let packet = wire::token(
            TokenRequest::Renew,
            inner,
            TOKEN_REQUEST,
            self.token,
            Some(&identity),
            seq,
            ids,
        );
        self.send_tracked(Role::Control, seq, packet, now, out);
    }

    // ── the handshake ───────────────────────────────────────────────────────────────────────

    /// Builds a stage's request and sends it for the first time.
    fn begin(&mut self, stage: Stage, now: u64, out: &mut Vec<Action>) {
        let role = stage.socket();
        let ids = self.sock(role).ids;
        let built = match stage {
            Stage::ControlProbe | Stage::CivProbe => {
                Ok((wire::control(ControlOp::Probe, 0, ids), None))
            }
            Stage::ControlReady | Stage::CivReady => {
                Ok((wire::control(ControlOp::Ready, 0, ids), None))
            }
            Stage::Login => self.login_request(),
            Stage::Capabilities => {
                let seq = self.sock(role).next_tracked();
                let inner = self.next_inner();
                let packet = wire::token(
                    TokenRequest::Create,
                    inner,
                    TOKEN_REQUEST,
                    self.token,
                    None,
                    seq,
                    ids,
                );
                Ok((packet, Some(seq)))
            }
            Stage::ConnectionInfo => self.connection_request(),
        };
        match built {
            Err(e) => self.fail(e, now, out),
            Ok((request, tracked_seq)) => {
                self.phase = Phase::Handshake {
                    stage,
                    request: request.clone(),
                    tracked_seq,
                    tries: 1,
                    resend_at: now + HANDSHAKE_RESEND_MS,
                };
                self.send_request(role, request, tracked_seq, now, out);
                // Each request is followed at once by an idle, then one every 100 ms.
                self.sock(role).last_idle = None;
                self.handshake_keepalive(role, now, out);
            }
        }
    }

    /// The login: the password moves into the builder here, and is gone after.
    fn login_request(&mut self) -> Result<(Vec<u8>, Option<u16>), ConnectError> {
        let password = self
            .password
            .take()
            .ok_or(ConnectError::Request(WireError::Field("credential")))?;
        let seq = self.sock(Role::Control).next_tracked();
        let fields = LoginFields {
            client_name: CLIENT_NAME,
            inner_seq: self.next_inner(),
            token_request: TOKEN_REQUEST,
            token: 0,
            seq,
            ids: self.sockets[at(Role::Control)].ids,
        };
        let packet =
            wire::login(self.user.as_str(), password, fields).map_err(ConnectError::Request)?;
        Ok((packet, Some(seq)))
    }

    /// The connection request: receive enabled as upstream asks for it, transmit never.
    fn connection_request(&mut self) -> Result<(Vec<u8>, Option<u16>), ConnectError> {
        let seq = self.sock(Role::Control).next_tracked();
        let inner = self.next_inner();
        let Some(radio) = &self.radio else {
            return Err(ConnectError::NoRadio(Unselected::NoRadios));
        };
        let req = ConnectionRequest {
            inner_seq: inner,
            token_request: TOKEN_REQUEST,
            token: self.token,
            identity: &radio.identity,
            radio_name: &radio.name,
            user: self.user.as_str(),
            rx_codec: CODEC_LPCM16,
            tx_codec: CODEC_LPCM16,
            rx_rate: RX_RATE_HZ,
            tx_rate: RX_RATE_HZ,
            civ_port: self.sockets[at(Role::Civ)].local_port,
            audio_port: self.sockets[at(Role::Audio)].local_port,
            tx_buffer_ms: TX_BUFFER_MS,
        };
        wire::connection_info(&req, seq, self.sockets[at(Role::Control)].ids)
            .map(|packet| (packet, Some(seq)))
            .map_err(ConnectError::Request)
    }

    /// While a stage waits: idles on its socket, and requests for any reply lost on the way.
    fn handshake_keepalive(&mut self, role: Role, now: u64, out: &mut Vec<Action>) {
        self.idle_if_due(role, now, out);
        self.request_retransmits(role, now, out);
    }

    /// A packet on the socket a stage waits on. Anything but the reply it waits for (idles,
    /// unrelated noise) changes nothing.
    fn handshake_reply(&mut self, stage: Stage, packet: Packet, now: u64, out: &mut Vec<Action>) {
        match (stage, packet) {
            (
                Stage::ControlProbe,
                Packet::Control {
                    op: ControlOp::Present,
                    ..
                },
            ) => self.begin(Stage::ControlReady, now, out),
            (
                Stage::ControlReady,
                Packet::Control {
                    op: ControlOp::Ready,
                    ..
                },
            ) => self.begin(Stage::Login, now, out),
            (Stage::Login, Packet::LoginResponse(r)) => {
                if r.error != 0 {
                    self.fail(ConnectError::LoginRefused, now, out);
                } else {
                    self.token = r.token;
                    self.begin(Stage::Capabilities, now, out);
                }
            }
            (Stage::Capabilities, Packet::Capabilities { caps, .. }) => match self.choose(&caps) {
                Ok(()) => self.begin(Stage::ConnectionInfo, now, out),
                Err(e) => self.fail(e, now, out),
            },
            (Stage::ConnectionInfo, Packet::Status(status)) => {
                if status.error != 0 {
                    return self.fail(ConnectError::Busy(status.error), now, out);
                }
                if status.civ_port == 0 {
                    return self.fail(ConnectError::NoCivPort, now, out);
                }
                out.push(Action::Redirect {
                    role: Role::Civ,
                    port: status.civ_port,
                });
                self.restart_stream(Role::Civ);
                // Audio is optional: a radio that assigns no audio port still serves CI-V.
                if status.audio_port != 0 {
                    out.push(Action::Redirect {
                        role: Role::Audio,
                        port: status.audio_port,
                    });
                    self.restart_stream(Role::Audio);
                    self.audio_port = Some(status.audio_port);
                }
                self.begin(Stage::CivProbe, now, out);
            }
            (
                Stage::CivProbe,
                Packet::Control {
                    op: ControlOp::Present,
                    ..
                },
            ) => self.begin(Stage::CivReady, now, out),
            (
                Stage::CivReady,
                Packet::Control {
                    op: ControlOp::Ready,
                    ..
                },
            ) => self.open_civ(now, out),
            _ => {}
        }
    }

    /// A new port is a new stream, with its own numbering and its own id at the radio's end.
    fn restart_stream(&mut self, role: Role) {
        let sock = self.sock(role);
        sock.ids.receiver = 0;
        sock.rx.expect(0);
    }

    /// Chooses the radio from the capabilities: by the configured choice, else by the model's
    /// name, else the only radio advertised (its name reported, not refused).
    fn choose(&mut self, caps: &Capabilities) -> Result<(), ConnectError> {
        let index = match &self.choice {
            Some(choice) => caps::select(caps, choice).map_err(ConnectError::NoRadio)?,
            None => match caps::select(caps, &Choice::Name(self.model.radio_name().to_string())) {
                Ok(index) => index,
                Err(Unselected::NoSuchName) if caps.radios.len() == 1 => 0,
                Err(e) => return Err(ConnectError::NoRadio(e)),
            },
        };
        let radio = &caps.radios[index];
        if !radio.rx_rates.supports(RX_RATE_HZ) {
            return Err(ConnectError::RateNotOffered(radio.rx_rates));
        }
        self.radio = Some(Chosen {
            identity: radio.identity,
            name: radio.name.clone(),
            civ_addr: radio.civ_addr,
            link: radio.link,
            tx_audio: radio.tx_rates.0 != 0 && radio.tx_rates.supports(RX_RATE_HZ),
        });
        Ok(())
    }

    /// The last stage: the CI-V stream opens, the keepalive starts and the drain begins.
    fn open_civ(&mut self, now: u64, out: &mut Vec<Action>) {
        let seq = self.sock(Role::Civ).next_tracked();
        let ids = self.sockets[at(Role::Civ)].ids;
        let packet = wire::openclose(StreamOp::Open, self.civ_send_seq, seq, ids);
        self.send_tracked(Role::Civ, seq, packet, now, out);
        // Silence is judged from here: a slow handshake must not count against a socket that
        // was not being read.
        for sock in &mut self.sockets {
            sock.last_heard = now;
        }
        for role in [Role::Control, Role::Civ] {
            let sock = self.sock(role);
            sock.last_idle = None;
            sock.last_ping = None;
        }
        self.last_token = now;
        self.phase = Phase::Draining {
            quiet_until: now + DRAIN_QUIET_MS,
            deadline: now + DRAIN_TOTAL_MS,
        };
        self.keepalive(now, true, out);
    }

    fn fail(&mut self, error: ConnectError, now: u64, out: &mut Vec<Action>) {
        out.push(Action::Event(Event::Failed(error)));
        self.abort(now, out);
    }

    /// A connect that ends part-way gives back what the radio granted: the token, with a grace
    /// in which the radio's request for it again is answered, then the control disconnect.
    fn abort(&mut self, now: u64, out: &mut Vec<Action>) {
        if self.token != 0 {
            self.send_token_remove(now, out);
            self.phase = Phase::Aborting {
                until: now + DISCONNECT_GRACE_MS,
            };
        } else {
            self.control(Role::Control, ControlOp::Disconnect, out);
            self.finish(out);
        }
    }

    /// Closes the stream and gives the token back while the radio's retransmit requests for
    /// them can still be answered; the disconnects follow the grace.
    fn teardown(&mut self, now: u64, out: &mut Vec<Action>) {
        let seq = self.sock(Role::Civ).next_tracked();
        let ids = self.sockets[at(Role::Civ)].ids;
        let packet = wire::openclose(StreamOp::Close, 0, seq, ids);
        self.send_tracked(Role::Civ, seq, packet, now, out);
        self.send_token_remove(now, out);
        if self.lost.is_some() {
            // Still worth sending in case the loss was wrong, but answers will not come.
            self.disconnect_all(out);
            self.finish(out);
        } else {
            self.phase = Phase::Closing {
                until: now + DISCONNECT_GRACE_MS,
            };
        }
    }

    fn disconnect_all(&mut self, out: &mut Vec<Action>) {
        for role in [Role::Audio, Role::Civ, Role::Control] {
            self.control(role, ControlOp::Disconnect, out);
        }
    }

    fn finish(&mut self, out: &mut Vec<Action>) {
        self.token = 0;
        self.phase = Phase::Closed;
        out.push(Action::Event(Event::Closed));
    }

    // ── receiving ───────────────────────────────────────────────────────────────────────────

    fn receive(&mut self, role: Role, bytes: &[u8], now: u64, out: &mut Vec<Action>) {
        // Anything at all from the radio proves the socket alive, and clears its error.
        let header = Header::of(bytes).ok();
        let sock = self.sock(role);
        sock.last_heard = now;
        sock.error_since = None;
        if let Some(h) = header {
            if sock.ids.receiver == 0 {
                sock.ids.receiver = h.ids.sender;
            }
        }
        let packet = match wire::decode(role, bytes) {
            Ok(packet) => Some(packet),
            Err(_) => {
                self.malformed += 1;
                None
            }
        };
        // Pings and retransmit requests are answered whatever the stage.
        match &packet {
            Some(Packet::Ping(ping)) => {
                if !ping.is_reply {
                    let ids = self.sock(role).ids;
                    let reply = wire::ping(true, ping.time, ping.header.seq, ids);
                    self.send(role, reply, out);
                }
                return;
            }
            Some(Packet::Retransmit(r)) => {
                self.replay(role, &r.wanted, out);
                return;
            }
            _ => {}
        }
        // Every numbered packet is tracked, idles included, so a lost one shows as a gap; a
        // copy of one already received is dropped.
        let Some(header) = header else {
            return;
        };
        if header.kind == wire::TYPE_DATA
            && self.track(role, header.seq, now) == RxResult::Duplicate
        {
            return;
        }
        let Some(packet) = packet else {
            return;
        };
        match self.phase {
            Phase::Handshake { stage, .. } if role == stage.socket() => {
                self.handshake_reply(stage, packet, now, out)
            }
            Phase::Draining { deadline, .. } => {
                self.stream_packet(role, packet, Some(deadline), now, out)
            }
            Phase::Connected => self.stream_packet(role, packet, None, now, out),
            _ => {}
        }
    }

    /// A packet once the stream is open. While draining, a frame is a leftover from an earlier
    /// client: dropped, and the drain waits for quiet again.
    fn stream_packet(
        &mut self,
        role: Role,
        packet: Packet,
        draining: Option<u64>,
        now: u64,
        out: &mut Vec<Action>,
    ) {
        match (role, packet) {
            // The radio ending the session: another client has taken it.
            (Role::Control, Packet::Status(status)) if status.disconnect => self.mark_lost(
                Loss {
                    reason: LossReason::PeerDisconnect,
                    socket: Role::Control,
                },
                out,
            ),
            (Role::Civ, Packet::Civ(civ)) => {
                let Some(frame) = self.frame_for_owner(civ.frame) else {
                    return;
                };
                match draining {
                    Some(deadline) => {
                        self.phase = Phase::Draining {
                            quiet_until: now + DRAIN_QUIET_MS,
                            deadline,
                        }
                    }
                    None => out.push(Action::Deliver(frame)),
                }
            }
            _ => {}
        }
    }

    /// The frame as the owner gets it, or nothing: an empty payload is no frame, and an echo of
    /// our own command (addressed to the radio) is dropped. A missing second `FE` is restored so
    /// the address reads right.
    fn frame_for_owner(&self, mut frame: Vec<u8>) -> Option<Vec<u8>> {
        if frame.is_empty() {
            return None;
        }
        if frame.len() >= 2 && frame[0] == 0xfe && frame[1] != 0xfe {
            frame.insert(0, 0xfe);
        }
        let radio = self.radio.as_ref().map(|r| r.civ_addr);
        if frame.len() >= 3 && Some(frame[2]) == radio {
            return None;
        }
        Some(frame)
    }

    /// Observes a numbered packet's sequence.
    fn track(&mut self, role: Role, seq: u16, now: u64) -> RxResult {
        let sock = self.sock(role);
        // Sequence 0 is a stream's first tracked packet, but untracked packets carry it too:
        // track it only where the numbering can be at 0 (the start of a stream, or across the
        // wrap), and never drop one as a copy.
        if seq == 0
            && sock
                .rx
                .last()
                .is_some_and(|last| (MISSING_FLUSH..=u16::MAX - MISSING_FLUSH).contains(&last))
        {
            return RxResult::New;
        }
        let result = sock.rx.observe(seq, now);
        if seq == 0 && result == RxResult::Duplicate {
            return RxResult::New;
        }
        if result == RxResult::Resync {
            // The missing set cannot be recovered by retransmits: start again from here.
            sock.rx.reset();
            sock.rx.observe(seq, now);
            sock.resyncs += 1;
        }
        result
    }

    // ── the timers ──────────────────────────────────────────────────────────────────────────

    fn tick(&mut self, now: u64, out: &mut Vec<Action>) {
        match self.phase {
            Phase::Closed => {}
            Phase::Handshake {
                stage,
                tries,
                resend_at,
                ..
            } => {
                let role = stage.socket();
                if now >= resend_at {
                    if tries >= HANDSHAKE_TRIES {
                        return self.fail(ConnectError::NoAnswer(stage), now, out);
                    }
                    let mut resend = None;
                    if let Phase::Handshake {
                        request,
                        tracked_seq,
                        tries,
                        resend_at,
                        ..
                    } = &mut self.phase
                    {
                        *tries += 1;
                        *resend_at = now + HANDSHAKE_RESEND_MS;
                        resend = Some((request.clone(), *tracked_seq));
                    }
                    if let Some((request, seq)) = resend {
                        self.send_request(role, request, seq, now, out);
                        self.sock(role).last_idle = None;
                    }
                }
                self.handshake_keepalive(role, now, out);
            }
            Phase::Draining {
                quiet_until,
                deadline,
            } => {
                self.keepalive(now, true, out);
                if now >= quiet_until || now >= deadline {
                    self.phase = Phase::Connected;
                    out.push(Action::Event(Event::Connected(self.report())));
                }
            }
            Phase::Connected => self.keepalive(now, true, out),
            Phase::Closing { until } => {
                self.keepalive(now, false, out);
                if now >= until {
                    self.disconnect_all(out);
                    self.finish(out);
                }
            }
            Phase::Aborting { until } => {
                if now >= until {
                    self.control(Role::Control, ControlOp::Disconnect, out);
                    self.finish(out);
                }
            }
        }
    }

    /// What the control and CI-V threads do between packets upstream: retransmit requests,
    /// liveness, idles, pings, the token renewal and the purges.
    fn keepalive(&mut self, now: u64, liveness: bool, out: &mut Vec<Action>) {
        for role in [Role::Control, Role::Civ] {
            self.request_retransmits(role, now, out);
        }
        if liveness {
            self.check_health(now, out);
        }
        for role in [Role::Control, Role::Civ] {
            self.idle_if_due(role, now, out);
            self.ping_if_due(role, now, out);
        }
        if now.saturating_sub(self.last_token) >= TOKEN_RENEW_MS {
            self.last_token = now;
            self.renew_token(now, out);
        }
        for role in [Role::Control, Role::Civ] {
            self.sock(role).tx.purge(now, PURGE_MS);
        }
    }

    /// Loses the session when a socket has heard nothing for the window. Several can pass it in
    /// one sweep; then the one reporting errors names the cause, whatever the order.
    fn check_health(&mut self, now: u64, out: &mut Vec<Action>) {
        if self.lost.is_some() {
            return;
        }
        let mut lost: Option<(Role, bool)> = None;
        for role in [Role::Control, Role::Civ] {
            let sock = &self.sockets[at(role)];
            if now.saturating_sub(sock.last_heard) <= LIVENESS_MS {
                continue;
            }
            let failing = sock.error_since.is_some();
            if lost.is_none() || failing {
                lost = Some((role, failing));
            }
            if failing {
                break;
            }
        }
        if let Some((socket, failing)) = lost {
            let reason = if failing {
                LossReason::SocketError
            } else {
                LossReason::LinkTimeout
            };
            self.mark_lost(Loss { reason, socket }, out);
        }
    }

    /// The first reason wins: it is the one that broke the session.
    fn mark_lost(&mut self, loss: Loss, out: &mut Vec<Action>) {
        if self.lost.is_none() {
            self.lost = Some(loss);
            out.push(Action::Event(Event::Lost(loss)));
        }
    }

    fn report(&self) -> Radio {
        let expected = self.model.radio_name();
        match &self.radio {
            Some(r) => Radio {
                name: r.name.clone(),
                expected,
                civ_addr: r.civ_addr,
                link: r.link,
                tx_audio_advertised: r.tx_audio,
                audio_port: self.audio_port,
            },
            None => Radio {
                name: String::new(),
                expected,
                civ_addr: 0,
                link: Link::Other(0),
                tx_audio_advertised: false,
                audio_port: self.audio_port,
            },
        }
    }
}

#[cfg(test)]
mod tests;
