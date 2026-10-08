//! The Icom network protocol's packets: every UDP datagram Nexus and a network Icom exchange,
//! built and parsed as typed values. No sockets, no state, no clock.
//!
//! A session runs over three UDP sockets to the radio ([`Role`]): control (port 50001 by
//! default: the handshake, the login, keepalive and teardown), CI-V (complete `FE FE … FD`
//! frames) and audio. Every datagram opens with the same 16-byte [`Header`]: length
//! (little-endian 32), type (little-endian 16), sequence (little-endian 16), then the sender's
//! and the receiver's connection ids (big-endian 32 each). Several fields inside the packets are
//! big-endian too, so every field is read and written at its offset, never through an overlaid
//! layout.
//!
//! **Classified by the socket it arrived on.** Lengths alone are ambiguous: a CI-V packet can be
//! exactly as long as a token or a status packet. Pings, retransmit requests and bare control
//! opcodes are recognised on every socket, the fixed-size management packets only on the control
//! socket, CI-V and open/close only on the CI-V socket ([`decode`]).
//!
//! **Strict, where upstream is tolerant.** A datagram whose header length disagrees with what
//! arrived, whose type or opcode the protocol does not have, or whose fields do not fit (a CI-V
//! payload length that is not the rest of the packet, a ping flag that is neither request nor
//! reply, a capabilities count that disagrees with the length) is a [`WireError`], never a value
//! and never a zero. Building refuses what the wire cannot carry: a credential over
//! [`PASSCODE_MAX`] characters or with a character the cipher has no entry for, a radio name over
//! 32 bytes, an empty CI-V frame.
//!
//! **No transmit field is settable.** The connection-info request built here always carries
//! receive-enable 1 and transmit-enable 0 ([`connection_info`]): it reserves no transmit audio
//! path, and nothing a caller passes can change that.
//!
//! PORTED from Hamlib (https://github.com/Hamlib/Hamlib, pull request #2178, open when taken),
//! `rigs/icom/network_proto.c` and `rigs/icom/network_proto.h` at commit
//! `2e3e4a6add3bd806e828d035200777608d2bdbd5` (2026-10-07), translated from C to Rust.
//! Deliberate differences: typed packets and builders in place of byte buffers and offsets, with
//! one [`decode`] that classifies by socket and parses in the same step, and the header's two ids
//! named for the end that sent the packet and the end it is for; strict parsing (the header's
//! length must equal the datagram's, a data packet must carry type 0, a bare header a known
//! opcode, a ping a flag of 0 or 1, a CI-V packet a payload that is exactly the rest of it, a
//! status a disconnect flag of 0 or 1: a short, long or malformed packet is an error, never a
//! zero); the multi-sequence retransmit request is read as inclusive ranges, as kappanhang reads
//! it, where upstream reads only the first number of each pair (a request built here still names
//! each sequence twice, which both readings agree on); the credential cipher refuses more than 16
//! characters and any character outside printable ASCII, where upstream truncates and encodes
//! whatever it is given; the connection-info request carries receive-enable 1 and
//! transmit-enable 0 as constants, with no parameter for either; the login builder takes the
//! password as a `conf::Secret`, by value, and drops it once the field is encoded; the audio
//! packet and the codec geometry are not taken yet, and the capabilities reader and the rate
//! bitmaps are in `caps`.
//! The passcode substitution table is a protocol constant, the same value for value as
//! kappanhang's `passcode.go` (MIT), whose README credits W6EL with the algorithm.
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
//! Modified by KD9TAW, 2026-10-07: translated from C to Rust and changed as listed
//! above. These changes are licensed under the GNU General Public License, version 3 only, as Nexus
//! is (see COPYING), so this file as a whole is distributed under GPL version 3.

use std::fmt;

use super::caps::{self, Capabilities};
use super::conf::Secret;

/// The radio's default control port; the radio assigns the CI-V and audio ports at connect.
pub const PORT_CONTROL: u16 = 50001;
/// The radio's default CI-V port (its network menu's "Serial" port).
pub const PORT_CIV: u16 = 50002;
/// The radio's default audio port.
pub const PORT_AUDIO: u16 = 50003;

/// Every datagram starts with this many header bytes.
pub const HEADER_LEN: usize = 0x10;
/// A ping: the header, the request/reply flag, the sender's clock.
pub const PING_LEN: usize = 0x15;
/// The part of a CI-V packet before its frame.
pub const CIV_HEADER_LEN: usize = 0x15;
/// A stream open or close.
pub const OPENCLOSE_LEN: usize = 0x16;
/// The radio's watchdog packet (classified, never acted on).
pub const WATCHDOG_LEN: usize = 0x14;
/// A token request or reply.
pub const TOKEN_LEN: usize = 0x40;
/// A status: the connection-info reply, or the radio ending the session.
pub const STATUS_LEN: usize = 0x50;
/// The login reply.
pub const LOGIN_RESPONSE_LEN: usize = 0x60;
/// The login request.
pub const LOGIN_LEN: usize = 0x80;
/// The connection-info request.
pub const CONNECTION_INFO_LEN: usize = 0x90;

/// The longest user name or password the protocol's obfuscated 16-byte field can carry.
pub const PASSCODE_MAX: usize = 16;
/// The longest radio name a connection-info request can carry.
pub const RADIO_NAME_MAX: usize = 32;
/// The longest client name a login can carry.
pub const CLIENT_NAME_MAX: usize = 16;
/// The widest range one retransmit request may ask for: the replay buffer's size, so nothing
/// wider could be answered (`super::seq::SEQBUF_MAX`).
pub const RETRANSMIT_SPAN_MAX: u16 = 256;

/// The receive codec Nexus requests: 16-bit mono linear PCM, upstream's default.
pub const CODEC_LPCM16: u8 = 0x04;

// The header.
const OFF_LENGTH: usize = 0x00; // le32, the whole packet
const OFF_TYPE: usize = 0x04; // le16
const OFF_SEQUENCE: usize = 0x06; // le16
const OFF_SENDER: usize = 0x08; // be32 (upstream `local_id` on what it sends)
const OFF_RECEIVER: usize = 0x0c; // be32 (upstream `remote_id`)

// The type field.
pub(crate) const TYPE_DATA: u16 = 0x0000;
const TYPE_RETRANSMIT: u16 = 0x0001;
const TYPE_PING: u16 = 0x0007;

// CI-V packets.
const CIV_OFF_REPLY: usize = 0x10; // u8, 0xc0 or 0xc1
const CIV_OFF_PAYLOAD_LEN: usize = 0x11; // le16
const CIV_OFF_SEND_SEQ: usize = 0x13; // be16, unlike the header's sequence

// Pings.
const PING_OFF_REPLY: usize = 0x10; // u8, 0 request, 1 reply
const PING_OFF_TIME: usize = 0x11; // le32

// Open/close.
const OPENCLOSE_OFF_MARKER: usize = 0x10; // 0x01 0xc0
const OPENCLOSE_OFF_SEND_SEQ: usize = 0x13; // be16

// The request preamble shared by the login, token and connection-info requests.
const REQ_OFF_PAYLOAD_SIZE: usize = 0x10; // be32
const REQ_OFF_REQUEST_REPLY: usize = 0x14; // u8, 1 = request
const REQ_OFF_REQUEST_TYPE: usize = 0x15; // u8
const REQ_OFF_INNER_SEQ: usize = 0x16; // be16
const REQ_OFF_TOKEN_REQUEST: usize = 0x1a; // be16
const REQ_OFF_TOKEN: usize = 0x1c; // be32

// The login request.
const LOGIN_OFF_USER: usize = 0x40; // 16 bytes, obfuscated
const LOGIN_OFF_PASSWORD: usize = 0x50; // 16 bytes, obfuscated
const LOGIN_OFF_CLIENT: usize = 0x60; // 16 bytes, plain

// The token request.
const TOKEN_OFF_AUTHID: usize = 0x20; // 16 bytes

// The login reply.
const LOGIN_RESP_OFF_TOKEN_REQUEST: usize = 0x1a; // be16
const LOGIN_RESP_OFF_TOKEN: usize = 0x1c; // be32
const LOGIN_RESP_OFF_AUTH_START_ID: usize = 0x20; // be16
const LOGIN_RESP_OFF_ERROR: usize = 0x30; // le32, 0 = accepted
const LOGIN_RESP_OFF_CONNECTION: usize = 0x40; // 16 bytes, NUL-padded

// The status.
const STATUS_OFF_ERROR: usize = 0x30; // le32, 0 = success
const STATUS_OFF_DISCONNECT: usize = 0x40; // u8
const STATUS_OFF_CIV_PORT: usize = 0x42; // be16
const STATUS_OFF_AUDIO_PORT: usize = 0x46; // be16

// The connection-info request.
const CONN_OFF_IDENTITY: usize = 0x20; // 16 bytes, echoed from the capabilities
const CONN_OFF_NAME: usize = 0x40; // 32 bytes
const CONN_OFF_USER: usize = 0x60; // 16 bytes, obfuscated
const CONN_OFF_RX_ENABLE: usize = 0x70;
const CONN_OFF_TX_ENABLE: usize = 0x71;
const CONN_OFF_RX_CODEC: usize = 0x72;
const CONN_OFF_TX_CODEC: usize = 0x73;
const CONN_OFF_RX_RATE: usize = 0x74; // be32, Hz
const CONN_OFF_TX_RATE: usize = 0x78; // be32, Hz
const CONN_OFF_CIV_PORT: usize = 0x7c; // be32
const CONN_OFF_AUDIO_PORT: usize = 0x80; // be32
const CONN_OFF_TX_BUFFER: usize = 0x84; // be32, ms
const CONN_OFF_CONVERT: usize = 0x88; // u8

/// Which of the three sockets a datagram travels on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Role {
    /// Login, token, capabilities, status, keepalive.
    Control,
    /// CI-V frames.
    Civ,
    /// Audio (not started at this stage).
    Audio,
}

/// The two connection ids a header carries: the sending end's and the receiving end's.
///
/// Upstream calls them `local_id` and `remote_id` and names them from the client's side; here they
/// are named for the packet, so a packet the radio sent and one Nexus sent read the same way.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Ids {
    pub sender: u32,
    pub receiver: u32,
}

/// The common 16-byte header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// The whole packet's length, as the packet states it.
    pub length: u32,
    /// The type field: 0 for data packets, 1 retransmit, 7 ping, or a control opcode in a bare
    /// header.
    pub kind: u16,
    pub seq: u16,
    pub ids: Ids,
}

/// The control opcodes a bare 16-byte header carries.
///
/// Each socket opens the same way: the client sends [`Probe`](ControlOp::Probe) and the radio
/// answers [`Present`](ControlOp::Present); the client sends [`Ready`](ControlOp::Ready) and the
/// radio echoes it. [`Idle`](ControlOp::Idle) is the keepalive, [`Disconnect`](ControlOp::Disconnect)
/// the teardown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlOp {
    Idle,
    Probe,
    Present,
    Disconnect,
    Ready,
}

/// A ping, either end's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ping {
    pub header: Header,
    /// False for a request, true for the answer to one.
    pub is_reply: bool,
    /// The sender's clock, echoed in the reply.
    pub time: u32,
}

/// An inclusive run of sequence numbers a retransmit request asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeqRange {
    pub first: u16,
    pub last: u16,
}

/// A request to send stored packets again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Retransmit {
    pub header: Header,
    /// What is asked for. The single form (a bare header) asks for its own sequence number.
    pub wanted: Vec<SeqRange>,
}

/// A CI-V packet: one complete frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Civ {
    pub header: Header,
    /// `0xc1` on what a client sends; radios send `0xc0` and `0xc1`.
    pub reply: u8,
    /// The CI-V stream's own counter, big-endian, separate from the header's sequence.
    pub send_seq: u16,
    /// The frame, `FE FE` to `FD` (nothing is stripped on the wire).
    pub frame: Vec<u8>,
}

/// What an open/close packet does to the CI-V stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamOp {
    Open,
    Close,
}

/// A CI-V stream open or close.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OpenClose {
    pub header: Header,
    pub send_seq: u16,
    pub op: StreamOp,
}

/// The token request types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenRequest {
    /// Give the session token back; the radio frees the slot.
    Remove,
    /// Create the token; the radio answers with its capabilities.
    Create,
    StreamDisconnect,
    /// The once-a-minute renewal.
    Renew,
}

/// A token packet, either way. Received ones are classified and otherwise ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Token {
    pub header: Header,
    pub request_reply: u8,
    /// The raw request type; [`Token::request`] names the known ones.
    pub request_type: u8,
    pub inner_seq: u16,
    pub token_request: u16,
    pub token: u32,
    pub authid: [u8; 16],
}

impl Token {
    /// The request type, when it is one the protocol names.
    pub fn request(&self) -> Option<TokenRequest> {
        Some(match self.request_type {
            0x01 => TokenRequest::Remove,
            0x02 => TokenRequest::Create,
            0x04 => TokenRequest::StreamDisconnect,
            0x05 => TokenRequest::Renew,
            _ => return None,
        })
    }
}

/// The radio's answer to the login.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginResponse {
    pub header: Header,
    pub token_request: u16,
    pub token: u32,
    pub auth_start_id: u16,
    /// 0 when the radio accepted the user and password.
    pub error: u32,
    /// The connection type the radio names, as text.
    pub connection: String,
}

/// A status: the answer to the connection-info request, or the radio ending the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub header: Header,
    /// 0 for success. A radio still holding another session's slot answers with an error.
    pub error: u32,
    /// Set when the radio is ending the session (another client has taken it).
    pub disconnect: bool,
    pub civ_port: u16,
    pub audio_port: u16,
}

/// A login request as the radio receives it. The credential fields are not decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginRequest {
    pub header: Header,
    pub inner_seq: u16,
    pub token_request: u16,
    pub token: u32,
}

/// A connection-info request as the radio receives it. The obfuscated user name is not decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectionInfo {
    pub header: Header,
    pub inner_seq: u16,
    pub token_request: u16,
    pub token: u32,
    pub identity: [u8; 16],
    pub radio_name: String,
    pub rx_enable: u8,
    pub tx_enable: u8,
    pub rx_codec: u8,
    pub tx_codec: u8,
    pub rx_rate: u32,
    pub tx_rate: u32,
    pub civ_port: u32,
    pub audio_port: u32,
    pub tx_buffer_ms: u32,
    pub convert_audio: u8,
}

/// One decoded datagram.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Packet {
    /// A bare header carrying a control opcode.
    Control {
        header: Header,
        op: ControlOp,
    },
    Ping(Ping),
    Retransmit(Retransmit),
    Watchdog(Header),
    Token(Token),
    Status(Status),
    LoginResponse(LoginResponse),
    Login(LoginRequest),
    ConnectionInfo(ConnectionInfo),
    Capabilities {
        header: Header,
        caps: Capabilities,
    },
    Civ(Civ),
    OpenClose(OpenClose),
}

/// Everything that makes a datagram unreadable, or a packet unbuildable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WireError {
    /// Shorter than the 16-byte header.
    Short { len: usize },
    /// The header's length field disagrees with the datagram.
    Length { claimed: u32, actual: usize },
    /// A bare header whose opcode the protocol does not have.
    UnknownControl(u16),
    /// A data packet whose type field is not 0.
    Type { kind: u16, len: usize },
    /// A ping that is not 21 bytes, or whose flag is neither 0 nor 1.
    Ping,
    /// A retransmit request that is not 16 + 4n bytes, or a range that runs backwards or is wider
    /// than [`RETRANSMIT_SPAN_MAX`].
    Retransmit,
    /// No packet of this length travels on this socket.
    Unknown { role: Role, len: usize },
    /// A CI-V packet whose marker is wrong, or whose payload is not exactly the rest of it.
    Civ,
    /// An open/close whose marker or stream op is wrong.
    OpenClose,
    /// A status whose disconnect flag is neither 0 nor 1.
    Flag,
    /// A capabilities reply whose radio count disagrees with its length.
    Capabilities,
    /// Audio packets are not decoded at this stage.
    NotTaken,
    /// A value that does not fit its field, named; the value itself is never carried.
    Field(&'static str),
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            WireError::Short { len } => write!(f, "a {len}-byte datagram is shorter than a header"),
            WireError::Length { claimed, actual } => {
                write!(f, "the header says {claimed} bytes, {actual} arrived")
            }
            WireError::UnknownControl(op) => write!(f, "unknown control opcode {op:#06x}"),
            WireError::Type { kind, len } => {
                write!(
                    f,
                    "a {len}-byte packet of type {kind:#06x} is not a data packet"
                )
            }
            WireError::Ping => f.write_str("a malformed ping"),
            WireError::Retransmit => f.write_str("a malformed retransmit request"),
            WireError::Unknown { role, len } => {
                write!(f, "no {len}-byte packet travels on the {role:?} socket")
            }
            WireError::Civ => f.write_str("a malformed CI-V packet"),
            WireError::OpenClose => f.write_str("a malformed stream open or close"),
            WireError::Flag => f.write_str("a status flag that is neither 0 nor 1"),
            WireError::Capabilities => {
                f.write_str("a capabilities reply whose radio count disagrees with its length")
            }
            WireError::NotTaken => f.write_str("audio packets are not decoded here"),
            WireError::Field(field) => write!(f, "the {field} does not fit its field"),
        }
    }
}

impl std::error::Error for WireError {}

// ── byte helpers ────────────────────────────────────────────────────────────────────────────

pub(crate) fn le16(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes([b[at], b[at + 1]])
}

pub(crate) fn le32(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

pub(crate) fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}

pub(crate) fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

pub(crate) fn put_le16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_le32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

pub(crate) fn put_be16(b: &mut [u8], at: usize, v: u16) {
    b[at..at + 2].copy_from_slice(&v.to_be_bytes());
}

pub(crate) fn put_be32(b: &mut [u8], at: usize, v: u32) {
    b[at..at + 4].copy_from_slice(&v.to_be_bytes());
}

/// The connection id the radio knows a socket by, from the socket's own IPv4 address and port:
/// the address's last two bytes, then the port.
pub fn make_id(ipv4: [u8; 4], port: u16) -> u32 {
    (u32::from(ipv4[2]) << 24) | (u32::from(ipv4[3]) << 16) | u32::from(port)
}

// ── the header ──────────────────────────────────────────────────────────────────────────────

impl Header {
    /// Reads the header of a datagram of at least 16 bytes. Whether its length field matches the
    /// datagram is [`Header::of`]'s question.
    pub fn parse(bytes: &[u8]) -> Result<Header, WireError> {
        if bytes.len() < HEADER_LEN {
            return Err(WireError::Short { len: bytes.len() });
        }
        Ok(Header {
            length: le32(bytes, OFF_LENGTH),
            kind: le16(bytes, OFF_TYPE),
            seq: le16(bytes, OFF_SEQUENCE),
            ids: Ids {
                sender: be32(bytes, OFF_SENDER),
                receiver: be32(bytes, OFF_RECEIVER),
            },
        })
    }

    /// Reads the header and checks that its length field is the datagram's length.
    pub fn of(bytes: &[u8]) -> Result<Header, WireError> {
        let header = Header::parse(bytes)?;
        if header.length as usize != bytes.len() {
            return Err(WireError::Length {
                claimed: header.length,
                actual: bytes.len(),
            });
        }
        Ok(header)
    }

    fn write(&self, out: &mut [u8]) {
        put_le32(out, OFF_LENGTH, self.length);
        put_le16(out, OFF_TYPE, self.kind);
        put_le16(out, OFF_SEQUENCE, self.seq);
        put_be32(out, OFF_SENDER, self.ids.sender);
        put_be32(out, OFF_RECEIVER, self.ids.receiver);
    }
}

/// A fresh packet of `len` bytes with its header written.
fn packet(len: usize, kind: u16, seq: u16, ids: Ids) -> Vec<u8> {
    let mut b = vec![0u8; len];
    Header {
        length: len as u32,
        kind,
        seq,
        ids,
    }
    .write(&mut b);
    b
}

/// Copies `text` into a NUL-padded field of `width`, refusing text that does not fit.
fn put_text(
    b: &mut [u8],
    at: usize,
    width: usize,
    text: &str,
    field: &'static str,
) -> Result<(), WireError> {
    let bytes = text.as_bytes();
    if bytes.len() > width {
        return Err(WireError::Field(field));
    }
    b[at..at + bytes.len()].copy_from_slice(bytes);
    Ok(())
}

/// Text from a NUL-padded field: up to the first NUL, trailing spaces removed.
pub(crate) fn text_of(field: &[u8]) -> String {
    let end = field.iter().position(|&c| c == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end])
        .trim_end_matches(' ')
        .to_string()
}

impl ControlOp {
    fn wire(self) -> u16 {
        match self {
            ControlOp::Idle => 0x0000,
            ControlOp::Probe => 0x0003,
            ControlOp::Present => 0x0004,
            ControlOp::Disconnect => 0x0005,
            ControlOp::Ready => 0x0006,
        }
    }

    fn from_wire(op: u16) -> Option<ControlOp> {
        Some(match op {
            0x0000 => ControlOp::Idle,
            0x0003 => ControlOp::Probe,
            0x0004 => ControlOp::Present,
            0x0005 => ControlOp::Disconnect,
            0x0006 => ControlOp::Ready,
            _ => return None,
        })
    }
}

impl TokenRequest {
    fn wire(self) -> u8 {
        match self {
            TokenRequest::Remove => 0x01,
            TokenRequest::Create => 0x02,
            TokenRequest::StreamDisconnect => 0x04,
            TokenRequest::Renew => 0x05,
        }
    }
}

impl Packet {
    /// The packet's header.
    pub fn header(&self) -> &Header {
        match self {
            Packet::Control { header, .. }
            | Packet::Watchdog(header)
            | Packet::Capabilities { header, .. } => header,
            Packet::Ping(p) => &p.header,
            Packet::Retransmit(r) => &r.header,
            Packet::Token(t) => &t.header,
            Packet::Status(s) => &s.header,
            Packet::LoginResponse(r) => &r.header,
            Packet::Login(l) => &l.header,
            Packet::ConnectionInfo(c) => &c.header,
            Packet::Civ(c) => &c.header,
            Packet::OpenClose(o) => &o.header,
        }
    }
}

// ── decoding ────────────────────────────────────────────────────────────────────────────────

/// Decodes a datagram that arrived on the socket `role`, strictly.
pub fn decode(role: Role, bytes: &[u8]) -> Result<Packet, WireError> {
    let header = Header::of(bytes)?;
    let len = bytes.len();
    // Pings and retransmit requests are the same on every socket.
    match header.kind {
        TYPE_PING => return decode_ping(header, bytes).map(Packet::Ping),
        TYPE_RETRANSMIT => return decode_retransmit(header, bytes).map(Packet::Retransmit),
        _ => {}
    }
    // So is a bare header: a control opcode.
    if len == HEADER_LEN {
        return ControlOp::from_wire(header.kind)
            .map(|op| Packet::Control { header, op })
            .ok_or(WireError::UnknownControl(header.kind));
    }
    if header.kind != TYPE_DATA {
        return Err(WireError::Type {
            kind: header.kind,
            len,
        });
    }
    match role {
        Role::Control => decode_management(header, bytes),
        Role::Civ if is_openclose(bytes) => decode_openclose(header, bytes),
        Role::Civ => decode_civ(header, bytes).map(Packet::Civ),
        Role::Audio if is_openclose(bytes) => decode_openclose(header, bytes),
        Role::Audio => Err(WireError::NotTaken),
    }
}

fn decode_ping(header: Header, b: &[u8]) -> Result<Ping, WireError> {
    if b.len() != PING_LEN {
        return Err(WireError::Ping);
    }
    let is_reply = match b[PING_OFF_REPLY] {
        0 => false,
        1 => true,
        _ => return Err(WireError::Ping),
    };
    Ok(Ping {
        header,
        is_reply,
        time: le32(b, PING_OFF_TIME),
    })
}

fn decode_retransmit(header: Header, b: &[u8]) -> Result<Retransmit, WireError> {
    if b.len() == HEADER_LEN {
        // The single form: the wanted sequence is the header's own.
        return Ok(Retransmit {
            header,
            wanted: vec![SeqRange {
                first: header.seq,
                last: header.seq,
            }],
        });
    }
    let list = &b[HEADER_LEN..];
    if !list.len().is_multiple_of(4) {
        return Err(WireError::Retransmit);
    }
    let mut wanted = Vec::with_capacity(list.len() / 4);
    for entry in list.chunks_exact(4) {
        let range = SeqRange {
            first: le16(entry, 0),
            last: le16(entry, 2),
        };
        if range.last.wrapping_sub(range.first) >= RETRANSMIT_SPAN_MAX {
            return Err(WireError::Retransmit);
        }
        wanted.push(range);
    }
    Ok(Retransmit { header, wanted })
}

/// The fixed-size management packets, and the capabilities reply: the control socket only.
fn decode_management(header: Header, b: &[u8]) -> Result<Packet, WireError> {
    let len = b.len();
    Ok(match len {
        WATCHDOG_LEN => Packet::Watchdog(header),
        TOKEN_LEN => {
            let mut authid = [0u8; 16];
            authid.copy_from_slice(&b[TOKEN_OFF_AUTHID..TOKEN_OFF_AUTHID + 16]);
            Packet::Token(Token {
                header,
                request_reply: b[REQ_OFF_REQUEST_REPLY],
                request_type: b[REQ_OFF_REQUEST_TYPE],
                inner_seq: be16(b, REQ_OFF_INNER_SEQ),
                token_request: be16(b, REQ_OFF_TOKEN_REQUEST),
                token: be32(b, REQ_OFF_TOKEN),
                authid,
            })
        }
        STATUS_LEN => Packet::Status(Status {
            header,
            error: le32(b, STATUS_OFF_ERROR),
            disconnect: match b[STATUS_OFF_DISCONNECT] {
                0 => false,
                1 => true,
                _ => return Err(WireError::Flag),
            },
            civ_port: be16(b, STATUS_OFF_CIV_PORT),
            audio_port: be16(b, STATUS_OFF_AUDIO_PORT),
        }),
        LOGIN_RESPONSE_LEN => Packet::LoginResponse(LoginResponse {
            header,
            token_request: be16(b, LOGIN_RESP_OFF_TOKEN_REQUEST),
            token: be32(b, LOGIN_RESP_OFF_TOKEN),
            auth_start_id: be16(b, LOGIN_RESP_OFF_AUTH_START_ID),
            error: le32(b, LOGIN_RESP_OFF_ERROR),
            connection: text_of(&b[LOGIN_RESP_OFF_CONNECTION..LOGIN_RESP_OFF_CONNECTION + 16]),
        }),
        LOGIN_LEN => Packet::Login(LoginRequest {
            header,
            inner_seq: be16(b, REQ_OFF_INNER_SEQ),
            token_request: be16(b, REQ_OFF_TOKEN_REQUEST),
            token: be32(b, REQ_OFF_TOKEN),
        }),
        CONNECTION_INFO_LEN => {
            let mut identity = [0u8; 16];
            identity.copy_from_slice(&b[CONN_OFF_IDENTITY..CONN_OFF_IDENTITY + 16]);
            Packet::ConnectionInfo(ConnectionInfo {
                header,
                inner_seq: be16(b, REQ_OFF_INNER_SEQ),
                token_request: be16(b, REQ_OFF_TOKEN_REQUEST),
                token: be32(b, REQ_OFF_TOKEN),
                identity,
                radio_name: text_of(&b[CONN_OFF_NAME..CONN_OFF_NAME + RADIO_NAME_MAX]),
                rx_enable: b[CONN_OFF_RX_ENABLE],
                tx_enable: b[CONN_OFF_TX_ENABLE],
                rx_codec: b[CONN_OFF_RX_CODEC],
                tx_codec: b[CONN_OFF_TX_CODEC],
                rx_rate: be32(b, CONN_OFF_RX_RATE),
                tx_rate: be32(b, CONN_OFF_TX_RATE),
                civ_port: be32(b, CONN_OFF_CIV_PORT),
                audio_port: be32(b, CONN_OFF_AUDIO_PORT),
                tx_buffer_ms: be32(b, CONN_OFF_TX_BUFFER),
                convert_audio: b[CONN_OFF_CONVERT],
            })
        }
        _ if len >= caps::FIRST_ENTRY + caps::ENTRY_LEN
            && (len - caps::FIRST_ENTRY).is_multiple_of(caps::ENTRY_LEN) =>
        {
            Packet::Capabilities {
                header,
                caps: caps::parse(b)?,
            }
        }
        _ => {
            return Err(WireError::Unknown {
                role: Role::Control,
                len,
            })
        }
    })
}

/// An open/close carries the constant `01 c0` marker at `0x10` and is exactly 22 bytes.
fn is_openclose(b: &[u8]) -> bool {
    b.len() == OPENCLOSE_LEN
        && b[OPENCLOSE_OFF_MARKER] == 0x01
        && b[OPENCLOSE_OFF_MARKER + 1] == 0xc0
}

fn decode_openclose(header: Header, b: &[u8]) -> Result<Packet, WireError> {
    let op = match b[REQ_OFF_REQUEST_TYPE] {
        0x04 => StreamOp::Open,
        0x00 => StreamOp::Close,
        _ => return Err(WireError::OpenClose),
    };
    Ok(Packet::OpenClose(OpenClose {
        header,
        send_seq: be16(b, OPENCLOSE_OFF_SEND_SEQ),
        op,
    }))
}

fn decode_civ(header: Header, b: &[u8]) -> Result<Civ, WireError> {
    if b.len() < CIV_HEADER_LEN {
        return Err(WireError::Civ);
    }
    let reply = b[CIV_OFF_REPLY];
    if reply != 0xc0 && reply != 0xc1 {
        return Err(WireError::Civ);
    }
    if usize::from(le16(b, CIV_OFF_PAYLOAD_LEN)) != b.len() - CIV_HEADER_LEN {
        return Err(WireError::Civ);
    }
    Ok(Civ {
        header,
        reply,
        send_seq: be16(b, CIV_OFF_SEND_SEQ),
        frame: b[CIV_HEADER_LEN..].to_vec(),
    })
}

// ── building ────────────────────────────────────────────────────────────────────────────────

/// A bare control packet. Untracked: the sequence is the caller's.
pub fn control(op: ControlOp, seq: u16, ids: Ids) -> Vec<u8> {
    packet(HEADER_LEN, op.wire(), seq, ids)
}

/// A ping request, or the reply to one.
pub fn ping(is_reply: bool, time: u32, seq: u16, ids: Ids) -> Vec<u8> {
    let mut b = packet(PING_LEN, TYPE_PING, seq, ids);
    b[PING_OFF_REPLY] = u8::from(is_reply);
    put_le32(&mut b, PING_OFF_TIME, time);
    b
}

/// Opens or closes the CI-V stream.
pub fn openclose(op: StreamOp, send_seq: u16, seq: u16, ids: Ids) -> Vec<u8> {
    let mut b = packet(OPENCLOSE_LEN, TYPE_DATA, seq, ids);
    b[OPENCLOSE_OFF_MARKER] = 0x01;
    b[OPENCLOSE_OFF_MARKER + 1] = 0xc0;
    put_be16(&mut b, OPENCLOSE_OFF_SEND_SEQ, send_seq);
    b[REQ_OFF_REQUEST_TYPE] = match op {
        StreamOp::Open => 0x04,
        StreamOp::Close => 0x00,
    };
    b
}

/// Asks the other end to send `wanted` again: one sequence in a bare header, several as a list
/// that names each sequence twice (a range of one, in the reading [`decode`] uses).
pub fn retransmit(wanted: &[u16], ids: Ids) -> Result<Vec<u8>, WireError> {
    match wanted {
        [] => Err(WireError::Field("retransmit list")),
        [one] => Ok(packet(HEADER_LEN, TYPE_RETRANSMIT, *one, ids)),
        _ if wanted.len() > usize::from(RETRANSMIT_SPAN_MAX) => {
            Err(WireError::Field("retransmit list"))
        }
        _ => {
            let mut b = packet(HEADER_LEN + wanted.len() * 4, TYPE_RETRANSMIT, 0, ids);
            for (i, &seq) in wanted.iter().enumerate() {
                put_le16(&mut b, HEADER_LEN + i * 4, seq);
                put_le16(&mut b, HEADER_LEN + i * 4 + 2, seq);
            }
            Ok(b)
        }
    }
}

/// A CI-V packet carrying one complete frame.
pub fn civ(
    frame: &[u8],
    reply: u8,
    send_seq: u16,
    seq: u16,
    ids: Ids,
) -> Result<Vec<u8>, WireError> {
    if frame.is_empty() || frame.len() > usize::from(u16::MAX) {
        return Err(WireError::Field("CI-V frame"));
    }
    if reply != 0xc0 && reply != 0xc1 {
        return Err(WireError::Field("CI-V reply marker"));
    }
    let mut b = packet(CIV_HEADER_LEN + frame.len(), TYPE_DATA, seq, ids);
    b[CIV_OFF_REPLY] = reply;
    put_le16(&mut b, CIV_OFF_PAYLOAD_LEN, frame.len() as u16);
    put_be16(&mut b, CIV_OFF_SEND_SEQ, send_seq);
    b[CIV_HEADER_LEN..].copy_from_slice(frame);
    Ok(b)
}

/// The substitution table for the credential cipher: an entry for each printable character
/// (32 to 126), zero elsewhere.
const PASSCODE_TABLE: [u8; 128] = {
    let printable: [u8; 95] = [
        0x47, 0x5d, 0x4c, 0x42, 0x66, 0x20, 0x23, 0x46, 0x4e, 0x57, 0x45, 0x3d, 0x67, 0x76, 0x60,
        0x41, 0x62, 0x39, 0x59, 0x2d, 0x68, 0x7e, 0x7c, 0x65, 0x7d, 0x49, 0x29, 0x72, 0x73, 0x78,
        0x21, 0x6e, 0x5a, 0x5e, 0x4a, 0x3e, 0x71, 0x2c, 0x2a, 0x54, 0x3c, 0x3a, 0x63, 0x4f, 0x43,
        0x75, 0x27, 0x79, 0x5b, 0x35, 0x70, 0x48, 0x6b, 0x56, 0x6f, 0x34, 0x32, 0x6c, 0x30, 0x61,
        0x6d, 0x7b, 0x2f, 0x4b, 0x64, 0x38, 0x2b, 0x2e, 0x50, 0x40, 0x3f, 0x55, 0x33, 0x37, 0x25,
        0x77, 0x24, 0x26, 0x74, 0x6a, 0x28, 0x53, 0x4d, 0x69, 0x22, 0x5c, 0x44, 0x31, 0x36, 0x58,
        0x3b, 0x7a, 0x51, 0x5f, 0x52,
    ];
    let mut table = [0u8; 128];
    let mut i = 0;
    while i < printable.len() {
        table[32 + i] = printable[i];
        i += 1;
    }
    table
};

/// The credential cipher: up to [`PASSCODE_MAX`] printable ASCII characters, each substituted
/// through the protocol's table at an offset that grows with its position. Obfuscation, not
/// encryption. Refuses longer text and any character outside printable ASCII, for which the
/// table has no entry.
pub fn passcode(text: &str) -> Result<[u8; 16], WireError> {
    let bytes = text.as_bytes();
    if bytes.len() > PASSCODE_MAX || !bytes.iter().all(|c| (b' '..=b'~').contains(c)) {
        return Err(WireError::Field("credential"));
    }
    let mut out = [0u8; 16];
    for (i, &c) in bytes.iter().enumerate() {
        let mut p = usize::from(c) + i;
        if p > 126 {
            p = 32 + p % 127;
        }
        out[i] = PASSCODE_TABLE[p];
    }
    Ok(out)
}

/// The fields a login carries besides the credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoginFields<'a> {
    pub client_name: &'a str,
    pub inner_seq: u16,
    pub token_request: u16,
    pub token: u32,
    pub seq: u16,
    pub ids: Ids,
}

/// The login request. The password is taken by value and dropped here, once its field is
/// encoded.
pub fn login(user: &str, password: Secret, fields: LoginFields<'_>) -> Result<Vec<u8>, WireError> {
    let user = passcode(user)?;
    let password = passcode(password.expose())?;
    let mut b = packet(LOGIN_LEN, TYPE_DATA, fields.seq, fields.ids);
    put_be32(&mut b, REQ_OFF_PAYLOAD_SIZE, 0x70);
    b[REQ_OFF_REQUEST_REPLY] = 0x01;
    b[REQ_OFF_REQUEST_TYPE] = 0x00;
    put_be16(&mut b, REQ_OFF_INNER_SEQ, fields.inner_seq);
    put_be16(&mut b, REQ_OFF_TOKEN_REQUEST, fields.token_request);
    put_be32(&mut b, REQ_OFF_TOKEN, fields.token);
    b[LOGIN_OFF_USER..LOGIN_OFF_USER + 16].copy_from_slice(&user);
    b[LOGIN_OFF_PASSWORD..LOGIN_OFF_PASSWORD + 16].copy_from_slice(&password);
    put_text(
        &mut b,
        LOGIN_OFF_CLIENT,
        CLIENT_NAME_MAX,
        fields.client_name,
        "client name",
    )?;
    Ok(b)
}

/// A token request: create, renew or remove. `authid` is echoed from the radio when given, zeros
/// otherwise.
#[allow(clippy::too_many_arguments)]
pub fn token(
    request: TokenRequest,
    inner_seq: u16,
    token_request: u16,
    token: u32,
    authid: Option<&[u8; 16]>,
    seq: u16,
    ids: Ids,
) -> Vec<u8> {
    let mut b = packet(TOKEN_LEN, TYPE_DATA, seq, ids);
    put_be32(&mut b, REQ_OFF_PAYLOAD_SIZE, 0x30);
    b[REQ_OFF_REQUEST_REPLY] = 0x01;
    b[REQ_OFF_REQUEST_TYPE] = request.wire();
    put_be16(&mut b, REQ_OFF_INNER_SEQ, inner_seq);
    put_be16(&mut b, REQ_OFF_TOKEN_REQUEST, token_request);
    put_be32(&mut b, REQ_OFF_TOKEN, token);
    if let Some(authid) = authid {
        b[TOKEN_OFF_AUTHID..TOKEN_OFF_AUTHID + 16].copy_from_slice(authid);
    }
    b
}

/// What a connection-info request asks for. There is no transmit field: the request always
/// carries receive-enable 1 and transmit-enable 0, so it reserves no transmit audio path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConnectionRequest<'a> {
    pub inner_seq: u16,
    pub token_request: u16,
    pub token: u32,
    /// The radio's 16-byte identity, echoed from its capabilities entry.
    pub identity: &'a [u8; 16],
    /// The radio's name, as its capabilities entry reported it.
    pub radio_name: &'a str,
    pub user: &'a str,
    pub rx_codec: u8,
    pub tx_codec: u8,
    pub rx_rate: u32,
    pub tx_rate: u32,
    /// This end's CI-V and audio ports, where the radio is to send.
    pub civ_port: u16,
    pub audio_port: u16,
    pub tx_buffer_ms: u32,
}

/// The connection-info request (the radio select).
pub fn connection_info(
    req: &ConnectionRequest<'_>,
    seq: u16,
    ids: Ids,
) -> Result<Vec<u8>, WireError> {
    let user = passcode(req.user)?;
    let mut b = packet(CONNECTION_INFO_LEN, TYPE_DATA, seq, ids);
    put_text(
        &mut b,
        CONN_OFF_NAME,
        RADIO_NAME_MAX,
        req.radio_name,
        "radio name",
    )?;
    put_be32(&mut b, REQ_OFF_PAYLOAD_SIZE, 0x80);
    b[REQ_OFF_REQUEST_REPLY] = 0x01;
    b[REQ_OFF_REQUEST_TYPE] = 0x03;
    put_be16(&mut b, REQ_OFF_INNER_SEQ, req.inner_seq);
    put_be16(&mut b, REQ_OFF_TOKEN_REQUEST, req.token_request);
    put_be32(&mut b, REQ_OFF_TOKEN, req.token);
    b[CONN_OFF_IDENTITY..CONN_OFF_IDENTITY + 16].copy_from_slice(req.identity);
    b[CONN_OFF_USER..CONN_OFF_USER + 16].copy_from_slice(&user);
    // Receive is requested, as upstream does; no transmit audio path is reserved.
    b[CONN_OFF_RX_ENABLE] = 1;
    b[CONN_OFF_TX_ENABLE] = 0;
    b[CONN_OFF_RX_CODEC] = req.rx_codec;
    b[CONN_OFF_TX_CODEC] = req.tx_codec;
    put_be32(&mut b, CONN_OFF_RX_RATE, req.rx_rate);
    put_be32(&mut b, CONN_OFF_TX_RATE, req.tx_rate);
    put_be32(&mut b, CONN_OFF_CIV_PORT, u32::from(req.civ_port));
    put_be32(&mut b, CONN_OFF_AUDIO_PORT, u32::from(req.audio_port));
    put_be32(&mut b, CONN_OFF_TX_BUFFER, req.tx_buffer_ms);
    b[CONN_OFF_CONVERT] = 1;
    Ok(b)
}

#[cfg(test)]
pub(crate) mod tests {
    //! Translated from upstream's `test/test_icom_network_proto.c`: the packet cases, each named
    //! after the case it translates. The capabilities, radio-selection and rate cases are in
    //! `caps`; `audio_packet_roundtrip` is not translated, because audio is not decoded at this
    //! stage. The rest are Nexus's own: strictness, the retransmit ranges, and every parser on
    //! random input.
    use super::*;

    /// xorshift64*: a seeded, dependency-free generator, so every run is reproducible by seed.
    pub(crate) struct Rng(pub(crate) u64);

    impl Rng {
        pub(crate) fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }

        pub(crate) fn below(&mut self, n: usize) -> usize {
            (self.next() % n as u64) as usize
        }

        pub(crate) fn chance(&mut self, percent: usize) -> bool {
            self.below(100) < percent
        }

        pub(crate) fn bytes(&mut self, len: usize) -> Vec<u8> {
            (0..len).map(|_| self.next() as u8).collect()
        }
    }

    const IDS: Ids = Ids {
        sender: 0,
        receiver: 0,
    };

    /// A zeroed packet of `length` with only its length and type set (upstream's `make_fixed`).
    fn make_fixed(length: usize, kind: u16) -> Vec<u8> {
        let mut b = vec![0u8; length.max(HEADER_LEN)];
        put_le32(&mut b, 0, length as u32);
        put_le16(&mut b, 4, kind);
        b
    }

    /// One well-formed capabilities reply with `count` zeroed entries.
    fn capabilities_of(count: usize) -> Vec<u8> {
        let len = caps::FIRST_ENTRY + count * caps::ENTRY_LEN;
        let mut b = make_fixed(len, 0);
        put_be16(&mut b, 0x40, count as u16);
        b
    }

    // upstream: test_byte_helpers
    #[test]
    fn byte_helpers() {
        let mut b = [0u8; 4];
        put_le16(&mut b, 0, 0x1234);
        assert_eq!(b[..2], [0x34, 0x12]);
        assert_eq!(le16(&b, 0), 0x1234);
        put_be16(&mut b, 0, 0x1234);
        assert_eq!(b[..2], [0x12, 0x34]);
        assert_eq!(be16(&b, 0), 0x1234);
        put_le32(&mut b, 0, 0xAABB_CCDD);
        assert_eq!(b, [0xDD, 0xCC, 0xBB, 0xAA]);
        assert_eq!(le32(&b, 0), 0xAABB_CCDD);
        put_be32(&mut b, 0, 0xAABB_CCDD);
        assert_eq!(b, [0xAA, 0xBB, 0xCC, 0xDD]);
        assert_eq!(be32(&b, 0), 0xAABB_CCDD);
    }

    // upstream: test_make_id (with a documentation address in place of upstream's private one)
    #[test]
    fn make_id_is_the_last_two_address_bytes_then_the_port() {
        // (2 << 24) | (100 << 16) | 50001
        assert_eq!(make_id([192, 0, 2, 100], 50001), 0x0264_C351);
        assert_eq!(make_id([192, 0, 2, 100], 50001), 40_158_033);
    }

    // upstream: test_header_roundtrip_layout
    #[test]
    fn header_roundtrip_layout() {
        let ids = Ids {
            sender: 0xAABB_CCDD,
            receiver: 0x1122_3344,
        };
        let b = packet(0x10, 0x0003, 0x1234, ids);
        assert_eq!(b.len(), 16);
        // length, type and sequence little-endian; the ids big-endian
        assert_eq!(b[0x00..0x02], [0x10, 0x00]);
        assert_eq!(b[0x04..0x06], [0x03, 0x00]);
        assert_eq!(b[0x06..0x08], [0x34, 0x12]);
        assert_eq!((b[0x08], b[0x0b]), (0xAA, 0xDD));
        assert_eq!((b[0x0c], b[0x0f]), (0x11, 0x44));
        let h = Header::parse(&b).unwrap();
        assert_eq!((h.length, h.kind, h.seq), (0x10, 0x0003, 0x1234));
        assert_eq!(h.ids, ids);
    }

    // upstream: test_header_short_buffer
    #[test]
    fn header_short_buffer() {
        assert_eq!(Header::parse(&[0u8; 8]), Err(WireError::Short { len: 8 }));
        assert_eq!(
            decode(Role::Control, &[0u8; 8]),
            Err(WireError::Short { len: 8 })
        );
    }

    // upstream: test_classify
    #[test]
    fn classify() {
        let control = Role::Control;
        assert!(matches!(
            decode(control, &make_fixed(0x10, 0x0003)),
            Ok(Packet::Control {
                op: ControlOp::Probe,
                ..
            })
        ));
        assert!(matches!(
            decode(control, &make_fixed(0x15, TYPE_PING)),
            Ok(Packet::Ping(_))
        ));
        assert!(matches!(
            decode(control, &make_fixed(0x10, TYPE_RETRANSMIT)),
            Ok(Packet::Retransmit(_))
        ));
        assert!(matches!(
            decode(control, &make_fixed(0x40, 0)),
            Ok(Packet::Token(_))
        ));
        assert!(matches!(
            decode(control, &make_fixed(0x50, 0)),
            Ok(Packet::Status(_))
        ));
        assert!(matches!(
            decode(control, &make_fixed(0x60, 0)),
            Ok(Packet::LoginResponse(_))
        ));
        assert!(matches!(
            decode(control, &make_fixed(0x80, 0)),
            Ok(Packet::Login(_))
        ));
        assert!(matches!(
            decode(control, &make_fixed(0x90, 0)),
            Ok(Packet::ConnectionInfo(_))
        ));
        // capabilities: 0x42 + 1 * 0x66 = 0xa8 (a count of one, which the strict parse requires)
        assert!(matches!(
            decode(control, &capabilities_of(1)),
            Ok(Packet::Capabilities { .. })
        ));
        // too short
        assert_eq!(decode(control, &[0u8; 4]), Err(WireError::Short { len: 4 }));
        // claims more than was received
        let b = make_fixed(0x50, 0);
        assert_eq!(
            decode(control, &b[..0x40]),
            Err(WireError::Length {
                claimed: 0x50,
                actual: 0x40
            })
        );
        // Nexus's own: claims less than was received, which upstream lets through
        let mut long = make_fixed(0x50, 0);
        long.push(0);
        assert_eq!(
            decode(control, &long),
            Err(WireError::Length {
                claimed: 0x50,
                actual: 0x51
            })
        );
    }

    // upstream: test_classify_data_packets (the audio half is not translated: audio is not
    // decoded at this stage)
    #[test]
    fn classify_data_packets() {
        let b = civ(&[0x98, 0xe0, 0x03], 0xc0, 7, 1, IDS).unwrap();
        assert_eq!(b.len(), 0x18);
        assert!(matches!(decode(Role::Civ, &b), Ok(Packet::Civ(_))));
        let b = openclose(StreamOp::Open, 1, 1, IDS);
        assert_eq!(b.len(), 0x16);
        assert!(matches!(decode(Role::Civ, &b), Ok(Packet::OpenClose(_))));
        // Pings, retransmit requests and bare control opcodes are the same on every socket.
        let idle = make_fixed(0x10, 0x0000);
        for role in [Role::Civ, Role::Audio] {
            assert!(matches!(
                decode(role, &idle),
                Ok(Packet::Control {
                    op: ControlOp::Idle,
                    ..
                })
            ));
        }
        assert!(matches!(
            decode(Role::Audio, &make_fixed(0x15, TYPE_PING)),
            Ok(Packet::Ping(_))
        ));
        // An audio packet is recognised, and left for a later stage.
        let mut audio = make_fixed(0x1c, 0);
        put_be16(&mut audio, 0x16, 4);
        assert_eq!(decode(Role::Audio, &audio), Err(WireError::NotTaken));
    }

    // upstream: test_classify_by_socket_role (the audio half is not translated)
    #[test]
    fn classify_by_socket_role() {
        // A CI-V packet whose total length happens to be a management packet's is still CI-V on
        // its own socket: every length that collides.
        let frame = [0x11u8; 512];
        for total in [0x40usize, 0x50, 0x60, 0x80, 0x90, 0xa8, 0x10e] {
            let b = civ(&frame[..total - CIV_HEADER_LEN], 0xc1, 3, 9, IDS).unwrap();
            assert_eq!(b.len(), total);
            assert!(
                matches!(decode(Role::Civ, &b), Ok(Packet::Civ(_))),
                "CI-V packet of {total:#x} bytes"
            );
            // The same bytes on the control socket are not CI-V.
            assert!(!matches!(decode(Role::Control, &b), Ok(Packet::Civ(_))));
        }
        // A management packet reaching the CI-V socket has no CI-V marker, so it is not taken for
        // a CI-V frame.
        assert_eq!(decode(Role::Civ, &make_fixed(0x50, 0)), Err(WireError::Civ));
        // A CI-V packet whose payload length runs past the packet is malformed.
        let mut b = civ(&frame[..8], 0xc1, 1, 1, IDS).unwrap();
        put_le16(&mut b, CIV_OFF_PAYLOAD_LEN, 9);
        assert_eq!(decode(Role::Civ, &b), Err(WireError::Civ));
        // Nexus's own: and so is one whose payload stops short of it.
        put_le16(&mut b, CIV_OFF_PAYLOAD_LEN, 7);
        assert_eq!(decode(Role::Civ, &b), Err(WireError::Civ));
    }

    // upstream: test_build_control
    #[test]
    fn build_control() {
        let b = control(
            ControlOp::Probe,
            0,
            Ids {
                sender: 0xDEAD_BEEF,
                receiver: 0,
            },
        );
        assert_eq!(b.len(), 0x10);
        assert_eq!(le32(&b, 0), 0x10);
        assert_eq!(le16(&b, 4), 0x0003);
        // the sender's id is big-endian
        assert_eq!(be32(&b, 8), 0xDEAD_BEEF);
    }

    // upstream: test_ping_roundtrip
    #[test]
    fn ping_roundtrip() {
        let b = ping(false, 0x1234_5678, 9, IDS);
        assert_eq!(b.len(), 0x15);
        assert_eq!(b[0x10], 0x00);
        // the time is little-endian
        assert_eq!((b[0x11], b[0x14]), (0x78, 0x12));
        let Ok(Packet::Ping(p)) = decode(Role::Control, &b) else {
            panic!("not a ping")
        };
        assert!(!p.is_reply);
        assert_eq!(p.time, 0x1234_5678);
        assert_eq!(p.header.seq, 9);
    }

    // upstream: test_openclose_layout
    #[test]
    fn openclose_layout() {
        let b = openclose(StreamOp::Open, 0x0102, 3, IDS);
        assert_eq!(b.len(), 0x16);
        assert_eq!(b[0x10..0x13], [0x01, 0xc0, 0x00]);
        // the send sequence is big-endian
        assert_eq!(b[0x13..0x15], [0x01, 0x02]);
        assert_eq!(b[0x15], 0x04);
        assert_eq!(openclose(StreamOp::Close, 0, 3, IDS)[0x15], 0x00);
    }

    // upstream: test_retransmit_single
    #[test]
    fn retransmit_single() {
        let b = retransmit(&[0x0009], IDS).unwrap();
        assert_eq!(b.len(), 0x10);
        assert_eq!(le16(&b, 4), TYPE_RETRANSMIT);
        assert_eq!(le16(&b, 6), 0x0009);
        let Ok(Packet::Retransmit(r)) = decode(Role::Civ, &b) else {
            panic!("not a retransmit request")
        };
        assert_eq!(r.wanted, [SeqRange { first: 9, last: 9 }]);
    }

    // upstream: test_retransmit_multi
    #[test]
    fn retransmit_multi() {
        let b = retransmit(&[5, 6, 7], IDS).unwrap();
        assert_eq!(b.len(), 0x10 + 3 * 4);
        assert_eq!(le32(&b, 0), 0x10 + 3 * 4);
        // each entry: the sequence, little-endian, twice
        assert_eq!((le16(&b, 0x10), le16(&b, 0x12)), (5, 5));
        assert_eq!((le16(&b, 0x14), le16(&b, 0x16)), (6, 6));
        assert_eq!((le16(&b, 0x18), le16(&b, 0x1a)), (7, 7));
        let Ok(Packet::Retransmit(r)) = decode(Role::Control, &b) else {
            panic!("not a retransmit request")
        };
        let firsts: Vec<u16> = r.wanted.iter().map(|w| w.first).collect();
        assert_eq!(firsts, [5, 6, 7]);
        assert!(r.wanted.iter().all(|w| w.first == w.last));
        assert_eq!(
            retransmit(&[], IDS),
            Err(WireError::Field("retransmit list"))
        );
    }

    #[test]
    fn a_retransmit_request_is_read_as_ranges() {
        // The reading kappanhang uses: each entry is an inclusive run, so (5, 8) asks for four.
        let mut b = make_fixed(0x18, TYPE_RETRANSMIT);
        put_le16(&mut b, 0x10, 5);
        put_le16(&mut b, 0x12, 8);
        put_le16(&mut b, 0x14, 0xfffe);
        put_le16(&mut b, 0x16, 0x0001);
        let Ok(Packet::Retransmit(r)) = decode(Role::Civ, &b) else {
            panic!("not a retransmit request")
        };
        assert_eq!(
            r.wanted,
            [
                SeqRange { first: 5, last: 8 },
                SeqRange {
                    first: 0xfffe,
                    last: 0x0001
                }
            ]
        );
        // A run that ends before it starts, or wider than the replay buffer, is malformed.
        put_le16(&mut b, 0x12, 4);
        assert_eq!(decode(Role::Civ, &b), Err(WireError::Retransmit));
        put_le16(&mut b, 0x12, 5 + RETRANSMIT_SPAN_MAX);
        assert_eq!(decode(Role::Civ, &b), Err(WireError::Retransmit));
        put_le16(&mut b, 0x12, 5 + RETRANSMIT_SPAN_MAX - 1);
        assert!(decode(Role::Civ, &b).is_ok());
        // A list that is not whole entries is malformed.
        let b = make_fixed(0x16, TYPE_RETRANSMIT);
        assert_eq!(decode(Role::Civ, &b), Err(WireError::Retransmit));
    }

    // upstream: test_civ_packet_roundtrip
    #[test]
    fn civ_packet_roundtrip() {
        // a whole frame: read the frequency, FE FE <radio> <controller> 03 FD
        let frame = [0xfe, 0xfe, 0x98, 0xe0, 0x03, 0xfd];
        let b = civ(&frame, 0xc1, 0x0007, 1, IDS).unwrap();
        assert_eq!(b.len(), 0x15 + 6);
        assert_eq!(b[0x10], 0xc1);
        // the payload length little-endian, the send sequence big-endian
        assert_eq!(b[0x11..0x13], [0x06, 0x00]);
        assert_eq!(b[0x13..0x15], [0x00, 0x07]);
        let Ok(Packet::Civ(p)) = decode(Role::Civ, &b) else {
            panic!("not a CI-V packet")
        };
        assert_eq!((p.reply, p.send_seq), (0xc1, 0x0007));
        assert_eq!(p.frame, frame);
        assert_eq!(
            civ(&[], 0xc1, 0, 1, IDS),
            Err(WireError::Field("CI-V frame"))
        );
    }

    // upstream: test_civ_packet_truncated
    #[test]
    fn civ_packet_truncated() {
        let b = civ(&[0x98, 0xe0, 0x03], 0xc0, 1, 1, IDS).unwrap();
        assert_eq!(b.len(), 0x18);
        // a datagram shorter than its payload length implies
        let mut short = b[..0x16].to_vec();
        put_le32(&mut short, 0, 0x16);
        assert_eq!(decode(Role::Civ, &short), Err(WireError::Civ));
    }

    // upstream: test_passcode_vectors
    #[test]
    fn passcode_vectors() {
        // By hand from the table: 't'(116)+0 -> 0x22; 'e'(101)+1=102 -> 0x3f;
        // 's'(115)+2=117 -> 0x5c; 't'(116)+3=119 -> 0x31.
        let out = passcode("test").unwrap();
        assert_eq!(out[..4], [0x22, 0x3f, 0x5c, 0x31]);
        assert_eq!(out[4..], [0u8; 12]); // zero padded
                                         // 'a'(97)+0 -> 0x38
        let out = passcode("a").unwrap();
        assert_eq!((out[0], out[1]), (0x38, 0x00));
        assert_eq!(passcode("").unwrap(), [0u8; 16]);
        // Past 126 the index wraps back into the printable range: '~'(126)+15=141 -> 32+14=46.
        let out = passcode("~~~~~~~~~~~~~~~~").unwrap();
        assert_eq!(out[15], 0x60);
    }

    #[test]
    fn the_cipher_refuses_what_it_cannot_carry() {
        // Upstream truncates at 16 and encodes any byte; a truncated or unencodable password
        // reaches the radio as a different password.
        assert!(passcode("0123456789abcdef").is_ok());
        assert_eq!(
            passcode("0123456789abcdefg"),
            Err(WireError::Field("credential"))
        );
        for bad in ["tab\there", "nul\0", "caf\u{e9}", "\u{7f}"] {
            assert_eq!(
                passcode(bad),
                Err(WireError::Field("credential")),
                "{bad:?}"
            );
        }
    }

    #[test]
    fn every_printable_character_encodes_to_a_table_entry() {
        // The table has an entry for every printable character at every position, so no
        // accepted credential ever encodes a zero (which the radio would read as the end).
        for position in 0..PASSCODE_MAX {
            for c in b' '..=b'~' {
                let text: String = std::iter::repeat_n('a', position)
                    .chain(std::iter::once(c as char))
                    .collect();
                assert_ne!(passcode(&text).unwrap()[position], 0, "{c} at {position}");
            }
        }
    }

    fn secret(text: &str) -> Secret {
        Secret::new(text.into()).expect("a test password")
    }

    fn login_fields() -> LoginFields<'static> {
        LoginFields {
            client_name: "Nexus",
            inner_seq: 0x0102,
            token_request: 0x1234,
            token: 0xAABB_CCDD,
            seq: 9,
            ids: IDS,
        }
    }

    // upstream: test_login_layout
    #[test]
    fn login_layout() {
        let b = login("test", secret("test"), login_fields()).unwrap();
        assert_eq!(b.len(), 0x80);
        // the payload size, big-endian 0x70, at 0x10
        assert_eq!(b[0x10..0x14], [0x00, 0x00, 0x00, 0x70]);
        assert_eq!(b[0x14..0x16], [0x01, 0x00]);
        // the inner sequence big-endian at 0x16
        assert_eq!(b[0x16..0x18], [0x01, 0x02]);
        // the token request big-endian at 0x1a, the token big-endian at 0x1c
        assert_eq!(b[0x1a..0x1c], [0x12, 0x34]);
        assert_eq!((b[0x1c], b[0x1f]), (0xAA, 0xDD));
        // "test", obfuscated, at 0x40
        assert_eq!(b[0x40..0x44], [0x22, 0x3f, 0x5c, 0x31]);
        assert_eq!(b[0x50..0x54], [0x22, 0x3f, 0x5c, 0x31]);
        // the client name, plain, at 0x60
        assert_eq!(&b[0x60..0x65], b"Nexus");
        assert_eq!(b[0x65], 0x00);
        let mut long = login_fields();
        long.client_name = "a client name over sixteen";
        assert_eq!(
            login("test", secret("test"), long),
            Err(WireError::Field("client name"))
        );
        assert_eq!(
            login("seventeen-chars-x", secret("test"), login_fields()),
            Err(WireError::Field("credential"))
        );
    }

    // upstream: test_login_response_parse
    #[test]
    fn login_response_parse() {
        let mut b = make_fixed(0x60, 0);
        put_be16(&mut b, 0x1a, 0x1234); // token request
        put_be32(&mut b, 0x1c, 0xAABB_CCDD); // token
        put_be16(&mut b, 0x20, 0x5678); // auth start id
        put_le32(&mut b, 0x30, 0); // accepted
        b[0x40..0x44].copy_from_slice(b"FTTH");
        let Ok(Packet::LoginResponse(r)) = decode(Role::Control, &b) else {
            panic!("not a login response")
        };
        assert_eq!((r.token_request, r.token), (0x1234, 0xAABB_CCDD));
        assert_eq!((r.auth_start_id, r.error), (0x5678, 0));
        assert_eq!(r.connection, "FTTH");
    }

    // upstream: test_token_build
    #[test]
    fn token_build() {
        let authid: [u8; 16] = std::array::from_fn(|i| 0x10 + i as u8);
        let b = token(
            TokenRequest::Create,
            0x0102,
            0x1234,
            0xAABB_CCDD,
            Some(&authid),
            1,
            IDS,
        );
        assert_eq!(b.len(), 0x40);
        assert_eq!(b[0x14..0x16], [0x01, 0x02]);
        assert_eq!(b[0x16..0x18], [0x01, 0x02]); // the inner sequence, big-endian
        assert_eq!((b[0x1c], b[0x1f]), (0xAA, 0xDD)); // the token, big-endian
        assert_eq!(b[0x20..0x30], authid);
        let Ok(Packet::Token(t)) = decode(Role::Control, &b) else {
            panic!("not a token")
        };
        assert_eq!(t.request(), Some(TokenRequest::Create));
        assert_eq!(
            token(TokenRequest::Remove, 0, 0, 0, None, 1, IDS)[0x15],
            0x01
        );
        assert_eq!(
            token(TokenRequest::Renew, 0, 0, 0, None, 1, IDS)[0x15],
            0x05
        );
    }

    // upstream: test_status_parse
    #[test]
    fn status_parse() {
        let mut b = make_fixed(0x50, 0);
        put_le32(&mut b, 0x30, 0); // success
        b[0x40] = 0x00; // no disconnect
        put_be16(&mut b, 0x42, 50002); // CI-V port
        put_be16(&mut b, 0x46, 50003); // audio port
        let Ok(Packet::Status(s)) = decode(Role::Control, &b) else {
            panic!("not a status")
        };
        assert_eq!((s.error, s.disconnect), (0, false));
        assert_eq!((s.civ_port, s.audio_port), (50002, 50003));
        b[0x40] = 0x01;
        assert!(matches!(
            decode(Role::Control, &b),
            Ok(Packet::Status(Status {
                disconnect: true,
                ..
            }))
        ));
        // Nexus's own: a flag that is neither 0 nor 1 is not guessed at.
        b[0x40] = 0x02;
        assert_eq!(decode(Role::Control, &b), Err(WireError::Flag));
    }

    fn request() -> ConnectionRequest<'static> {
        ConnectionRequest {
            inner_seq: 0x0102,
            token_request: 0x1234,
            token: 0xAABB_CCDD,
            identity: &[0x5a; 16],
            radio_name: "IC-7610",
            user: "test",
            rx_codec: CODEC_LPCM16,
            tx_codec: CODEC_LPCM16,
            rx_rate: 48000,
            tx_rate: 48000,
            civ_port: 50002,
            audio_port: 50003,
            tx_buffer_ms: 150,
        }
    }

    // upstream: test_connection_info_build
    #[test]
    fn connection_info_build() {
        let b = connection_info(&request(), 7, IDS).unwrap();
        assert_eq!(b.len(), 0x90);
        // the payload size big-endian 0x80, request type 0x03
        assert_eq!(b[0x10..0x14], [0x00, 0x00, 0x00, 0x80]);
        assert_eq!(b[0x14..0x16], [0x01, 0x03]);
        assert_eq!(b[0x16..0x18], [0x01, 0x02]); // the inner sequence, big-endian
        assert_eq!(b[0x20..0x30], [0x5a; 16]); // the identity, echoed
        assert_eq!(&b[0x40..0x47], b"IC-7610"); // the radio name
        assert_eq!(b[0x47], 0);
        assert_eq!(b[0x60], 0x22); // the user name, obfuscated
        assert_eq!((b[0x70], b[0x72]), (0x01, 0x04)); // receive enable, receive codec
                                                      // the sample rate and the ports, big-endian 32
        assert_eq!(be32(&b, 0x74), 48000);
        assert_eq!(be32(&b, 0x7c), 50002);
        assert_eq!(be32(&b, 0x80), 50003);
        assert_eq!(be32(&b, 0x84), 150);
        assert_eq!(b[0x88], 0x01);
        let long = ConnectionRequest {
            radio_name: "a radio name that runs past thirty-two bytes",
            ..request()
        };
        assert_eq!(
            connection_info(&long, 7, IDS),
            Err(WireError::Field("radio name"))
        );
    }

    #[test]
    fn the_connection_info_request_reserves_no_transmit_path() {
        // The transmit rule's wire half: whatever the request names, byte 0x71 (transmit
        // enable) is 0 and byte 0x70 (receive enable) is 1. Read from the bytes, not through
        // the parser.
        let mut rng = Rng(0x7EED);
        for _ in 0..2_000 {
            let name: String = (0..rng.below(RADIO_NAME_MAX + 1))
                .map(|_| (b' ' + rng.below(95) as u8) as char)
                .collect();
            let user: String = (0..rng.below(PASSCODE_MAX + 1))
                .map(|_| (b' ' + rng.below(95) as u8) as char)
                .collect();
            let identity: [u8; 16] = std::array::from_fn(|_| rng.next() as u8);
            let req = ConnectionRequest {
                inner_seq: rng.next() as u16,
                token_request: rng.next() as u16,
                token: rng.next() as u32,
                identity: &identity,
                radio_name: &name,
                user: &user,
                rx_codec: rng.next() as u8,
                tx_codec: rng.next() as u8,
                rx_rate: rng.next() as u32,
                tx_rate: rng.next() as u32,
                civ_port: rng.next() as u16,
                audio_port: rng.next() as u16,
                tx_buffer_ms: rng.next() as u32,
            };
            let b = connection_info(&req, rng.next() as u16, IDS).unwrap();
            assert_eq!(b[0x71], 0, "transmit enable");
            assert_eq!(b[0x70], 1, "receive enable");
            let Ok(Packet::ConnectionInfo(c)) = decode(Role::Control, &b) else {
                panic!("not a connection-info request")
            };
            assert_eq!((c.tx_enable, c.rx_enable), (0, 1));
        }
    }

    #[test]
    fn an_unknown_opcode_or_type_is_an_error() {
        assert_eq!(
            decode(Role::Control, &make_fixed(0x10, 0x0002)),
            Err(WireError::UnknownControl(0x0002))
        );
        assert_eq!(
            decode(Role::Control, &make_fixed(0x50, 0x0003)),
            Err(WireError::Type {
                kind: 0x0003,
                len: 0x50
            })
        );
        let mut p = ping(false, 1, 1, IDS);
        p[0x10] = 2;
        assert_eq!(decode(Role::Control, &p), Err(WireError::Ping));
        assert_eq!(
            decode(Role::Control, &make_fixed(0x16, TYPE_PING)),
            Err(WireError::Ping)
        );
        let mut oc = openclose(StreamOp::Open, 1, 1, IDS);
        oc[0x15] = 0x07;
        assert_eq!(decode(Role::Civ, &oc), Err(WireError::OpenClose));
        assert_eq!(
            decode(Role::Control, &make_fixed(0x44, 0)),
            Err(WireError::Unknown {
                role: Role::Control,
                len: 0x44
            })
        );
    }

    /// Valid packets of every kind the decoder knows, for the random test to mutate.
    pub(crate) fn valid_packets() -> Vec<(Role, Vec<u8>)> {
        let ids = Ids {
            sender: 0xA1A2_A3A4,
            receiver: 0x0102_0304,
        };
        let mut status = make_fixed(0x50, 0);
        put_be16(&mut status, 0x42, 50002);
        vec![
            (Role::Control, control(ControlOp::Present, 0, ids)),
            (Role::Civ, control(ControlOp::Ready, 0, ids)),
            (Role::Control, ping(false, 77, 3, ids)),
            (Role::Civ, ping(true, 77, 3, ids)),
            (Role::Control, retransmit(&[4], ids).unwrap()),
            (Role::Civ, retransmit(&[4, 9, 12], ids).unwrap()),
            (
                Role::Control,
                token(TokenRequest::Renew, 1, 2, 3, None, 4, ids),
            ),
            (Role::Control, status),
            (Role::Control, make_fixed(0x60, 0)),
            (Role::Control, make_fixed(0x14, 0)),
            (
                Role::Control,
                login("user", secret("pass"), login_fields()).unwrap(),
            ),
            (Role::Control, connection_info(&request(), 2, ids).unwrap()),
            (Role::Control, capabilities_of(2)),
            (
                Role::Civ,
                civ(&[0xfe, 0xfe, 0xe0, 0x98, 0x03, 0xfd], 0xc0, 1, 2, ids).unwrap(),
            ),
            (Role::Civ, openclose(StreamOp::Close, 0, 5, ids)),
        ]
    }

    #[test]
    fn every_parser_on_random_input_returns_an_error_or_a_value() {
        // Random lengths and bytes, and valid packets truncated, extended and bit-flipped, on
        // every socket. Nothing may panic; anything that decodes must decode again the same.
        let mut rng = Rng(0x1C0A_2178);
        let seeds = valid_packets();
        let mut decoded = 0usize;
        for round in 0..60_000 {
            let role = [Role::Control, Role::Civ, Role::Audio][rng.below(3)];
            let bytes = match round % 4 {
                0 => {
                    let len = rng.below(0x200);
                    let mut b = rng.bytes(len);
                    if len >= 4 && rng.chance(50) {
                        // a believable length field, so the deeper parsers are reached
                        put_le32(&mut b, 0, len as u32);
                    }
                    if len >= 6 && rng.chance(50) {
                        put_le16(&mut b, 4, [0u16, 1, 7][rng.below(3)]);
                    }
                    b
                }
                1 => {
                    let (_, mut b) = seeds[rng.below(seeds.len())].clone();
                    let at = rng.below(b.len());
                    b[at] ^= 1 << rng.below(8);
                    b
                }
                2 => {
                    let (_, b) = seeds[rng.below(seeds.len())].clone();
                    let cut = rng.below(b.len() + 1);
                    b[..cut].to_vec()
                }
                _ => {
                    let (_, mut b) = seeds[rng.below(seeds.len())].clone();
                    let n = 1 + rng.below(8);
                    let extra = rng.bytes(n);
                    b.extend_from_slice(&extra);
                    if rng.chance(50) {
                        let len = b.len() as u32;
                        put_le32(&mut b, 0, len);
                    }
                    b
                }
            };
            let _ = Header::parse(&bytes);
            let _ = caps::parse(&bytes);
            if let Ok(p) = decode(role, &bytes) {
                decoded += 1;
                assert_eq!(decode(role, &bytes), Ok(p));
            }
        }
        // the generator reaches the decoders, not only the length check
        assert!(decoded > 5_000, "{decoded}");
        for (role, b) in seeds {
            assert!(decode(role, &b).is_ok(), "{role:?} {:02x?}", &b[..16]);
        }
    }

    #[test]
    fn every_builder_on_random_input_returns_an_error_or_a_value() {
        let mut rng = Rng(0xB01D);
        for _ in 0..5_000 {
            let text: String = (0..rng.below(24))
                .map(|_| char::from(rng.next() as u8))
                .collect();
            let other: String = (0..rng.below(40))
                .map(|_| char::from(rng.next() as u8))
                .collect();
            let n = rng.below(64);
            let frame = rng.bytes(n);
            let wanted: Vec<u16> = (0..rng.below(8)).map(|_| rng.next() as u16).collect();
            let _ = passcode(&text);
            if let Ok(password) = Secret::new(other.clone()) {
                let _ = login(&text, password, login_fields());
            }
            let _ = civ(&frame, 0xc1, 1, 1, IDS);
            let _ = retransmit(&wanted, IDS);
            let identity = [0u8; 16];
            let req = ConnectionRequest {
                radio_name: &other,
                user: &text,
                identity: &identity,
                ..request()
            };
            if let Ok(b) = connection_info(&req, 1, IDS) {
                assert_eq!(b[0x71], 0);
            }
        }
    }

    #[test]
    fn a_wire_error_names_the_field_never_the_value() {
        let e = login(
            "secret-user-name-too-long",
            secret("secret"),
            login_fields(),
        )
        .unwrap_err();
        let text = format!("{e} {e:?}");
        assert!(!text.contains("secret"), "{text}");
    }
}
