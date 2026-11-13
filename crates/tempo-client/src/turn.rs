//! A TURN client, the least an internet road needs: one allocation on a TURN server under the
//! long-term credential, with a permission and a channel for each peer the road's ICE agent pairs
//! the relayed address with, each kept alive until the road goes. Written from RFC 8656 (TURN) and
//! RFC 8489 (the STUN messages it is made of); it is not anyone's TURN code.
//!
//! ## What it does
//!
//! - **Allocate** (RFC 8656 §7): a relayed UDP address on the server. The first request carries no
//!   credential; the server's 401 names its realm and a nonce, and the request goes again with the
//!   username, the realm, the nonce and a MESSAGE-INTEGRITY under the long-term key: MD5 of
//!   `username:realm:password` (RFC 8489 §9.2.2). A 438 (a stale nonce) is asked again under the new
//!   nonce. The success names the relayed address and where the server saw this end
//!   (XOR-MAPPED-ADDRESS), which is this end's server-reflexive address as well.
//! - **A permission** (§9) for each peer's address, and **a channel** (§12) for each peer: the server
//!   relays nothing to or from an address it holds no permission for, and a channel carries each
//!   datagram behind a 4-byte header (ChannelData) where a Send indication takes 36 bytes or more.
//!   Until its channel is bound a peer is sent to by Send indication, and until its permission is in
//!   place what is meant for it waits here, a little while.
//! - **Refresh**, each a minute before it would lapse: the allocation (§8; its lifetime is the
//!   server's answer), each permission (300 s, §9) and each channel (600 s, §12). [`Turn::release`]
//!   gives the allocation back (a Refresh with a lifetime of 0) as the road goes.
//! - **Every request is sent again** while unanswered, at 0.5, 1.5, 3.5, 7.5, 15.5 and 31.5 s, and
//!   given up 8 s after the seventh (RFC 8489 §6.2.1). An allocation given up, or refused, ends this
//!   client ([`Turn::failed`]): the road then goes on without a relayed address.
//! - **What comes back**: the server's answers, read here, and a peer's datagram as ChannelData or a
//!   Data indication, unwrapped and handed on with the peer's address ([`Turn::handle`]).
//!
//! ## What it trusts
//!
//! An answer counts only for a request outstanding here, whose transaction id is 96 bits from the
//! operating system's randomness, and only from the server's address (the caller reads only that
//! address's packets into this). A success counts only once its MESSAGE-INTEGRITY verifies under the
//! long-term key, and its FINGERPRINT, when it has one, must match. The two errors read without one
//! (401 and 438, which a server cannot sign before the client has the nonce) can only make this end
//! ask again, never take an address. What the server relays is ciphertext: every datagram of a road
//! is sealed under that road's keys (`crate::datagram`), which the server never holds. What it can
//! still do is refuse, drop or delay.
//!
//! The password and the key made from it stay in memory, and nothing here logs, shows or sends them:
//! only the username and the MESSAGE-INTEGRITY the protocol asks for leave, and nothing here has
//! `Debug`.
use std::collections::{HashMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use md5::{Digest, Md5};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};

/// STUN's magic cookie (RFC 8489 §5).
const MAGIC: u32 = 0x2112_A442;
/// What a FINGERPRINT's CRC-32 is XOR'd with (RFC 8489 §14.7).
const FINGERPRINT_XOR: u32 = 0x5354_554E;
/// A STUN message's header.
const HEADER: usize = 20;

// The methods (RFC 8656 §17).
const ALLOCATE: u16 = 0x003;
const REFRESH: u16 = 0x004;
const SEND: u16 = 0x006;
const DATA: u16 = 0x007;
const CREATE_PERMISSION: u16 = 0x008;
const CHANNEL_BIND: u16 = 0x009;

// The attributes (RFC 8489 §18.3, RFC 8656 §18).
const USERNAME: u16 = 0x0006;
const MESSAGE_INTEGRITY: u16 = 0x0008;
const ERROR_CODE: u16 = 0x0009;
const CHANNEL_NUMBER: u16 = 0x000C;
const LIFETIME: u16 = 0x000D;
const XOR_PEER_ADDRESS: u16 = 0x0012;
const DATA_VALUE: u16 = 0x0013;
const REALM: u16 = 0x0014;
const NONCE: u16 = 0x0015;
const XOR_RELAYED_ADDRESS: u16 = 0x0016;
const REQUESTED_TRANSPORT: u16 = 0x0019;
const XOR_MAPPED_ADDRESS: u16 = 0x0020;
const FINGERPRINT: u16 = 0x8028;

/// UDP, as REQUESTED-TRANSPORT names it.
const UDP: u8 = 17;
/// The first and last channel numbers a client may bind (RFC 8656 §12).
const FIRST_CHANNEL: u16 = 0x4000;
const LAST_CHANNEL: u16 = 0x4FFF;
/// How long a permission lasts (RFC 8656 §9), and a channel (§12), at the server.
const PERMISSION_LIFETIME: Duration = Duration::from_secs(300);
const CHANNEL_LIFETIME: Duration = Duration::from_secs(600);
/// How long before it would lapse anything is refreshed.
const EARLY: Duration = Duration::from_secs(60);
/// The lifetime asked for at each Refresh: RFC 8656's default.
const ASKED_LIFETIME: u32 = 600;
/// A request's first retransmission timeout, and how many times it is sent (RFC 8489 §6.2.1).
const RTO: Duration = Duration::from_millis(500);
const SENDS: u32 = 7;
/// After the last send, a request is given up this many RTOs later (RFC 8489's Rm).
const LAST_WAIT: u32 = 16;
/// What waits for a peer's permission: so many datagrams, so long.
const WAITING: usize = 32;
const WAIT_FOR: Duration = Duration::from_secs(2);

/// A STUN message's class (RFC 8489 §5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Class {
    Request,
    Indication,
    Success,
    Error,
}

/// The message type for `method` and `class`: the class's two bits sit at bits 4 and 8, between
/// the method's (RFC 8489 §5, figure 3).
fn message_type(method: u16, class: Class) -> u16 {
    let c: u16 = match class {
        Class::Request => 0b00,
        Class::Indication => 0b01,
        Class::Success => 0b10,
        Class::Error => 0b11,
    };
    (method & 0x000F)
        | ((method & 0x0070) << 1)
        | ((method & 0x0F80) << 2)
        | ((c & 0b01) << 4)
        | ((c & 0b10) << 7)
}

/// The method and class a message type names.
fn split_type(kind: u16) -> (u16, Class) {
    let method = (kind & 0x000F) | ((kind >> 1) & 0x0070) | ((kind >> 2) & 0x0F80);
    let class = match ((kind >> 4) & 0b01) | ((kind >> 7) & 0b10) {
        0b00 => Class::Request,
        0b01 => Class::Indication,
        0b10 => Class::Success,
        _ => Class::Error,
    };
    (method, class)
}

/// Whether `packet` is shaped as a STUN message (RFC 7983's first byte, the magic cookie and a
/// length that is the packet's), as against ChannelData or a road's own datagram.
pub fn is_stun(packet: &[u8]) -> bool {
    packet.len() >= HEADER
        && packet[0] < 4
        && packet[4..8] == MAGIC.to_be_bytes()
        && usize::from(u16::from_be_bytes([packet[2], packet[3]])) + HEADER == packet.len()
}

/// Whether `packet` is shaped as ChannelData (RFC 7983: a first byte from 64 to 79).
pub fn is_channel_data(packet: &[u8]) -> bool {
    packet.len() >= 4 && (64..=79).contains(&packet[0])
}

/// The long-term key for `username` in `realm` with `password` (RFC 8489 §9.2.2): MD5 of the three
/// joined by colons. The OpaqueString profile changes nothing in an ASCII credential, which is
/// what a TURN service mints.
pub(crate) fn long_term_key(username: &str, realm: &str, password: &str) -> [u8; 16] {
    Md5::digest(format!("{username}:{realm}:{password}").as_bytes()).into()
}

/// A message being written: its type and transaction id, and its attributes so far.
pub(crate) struct Writer {
    bytes: Vec<u8>,
}

impl Writer {
    pub(crate) fn new(method: u16, class: Class, transaction: &[u8; 12]) -> Self {
        let mut bytes = Vec::with_capacity(128);
        bytes.extend_from_slice(&message_type(method, class).to_be_bytes());
        bytes.extend_from_slice(&[0, 0]);
        bytes.extend_from_slice(&MAGIC.to_be_bytes());
        bytes.extend_from_slice(transaction);
        Self { bytes }
    }

    /// One attribute, padded to four bytes.
    pub(crate) fn attribute(mut self, kind: u16, value: &[u8]) -> Self {
        self.bytes.extend_from_slice(&kind.to_be_bytes());
        self.bytes
            .extend_from_slice(&(value.len() as u16).to_be_bytes());
        self.bytes.extend_from_slice(value);
        while !self.bytes.len().is_multiple_of(4) {
            self.bytes.push(0);
        }
        self
    }

    /// An address attribute, XOR'd with the magic cookie and this message's transaction id.
    pub(crate) fn address(self, kind: u16, address: SocketAddr) -> Self {
        let transaction: [u8; 12] = self.bytes[8..HEADER].try_into().unwrap_or_default();
        self.attribute(kind, &xor_address(address, &transaction))
    }

    /// The header's length field: the attributes so far, and `more` bytes to come.
    fn length(&mut self, more: usize) {
        let length = (self.bytes.len() - HEADER + more) as u16;
        self.bytes[2..4].copy_from_slice(&length.to_be_bytes());
    }

    /// The message, with a MESSAGE-INTEGRITY under `key` when there is one, and its FINGERPRINT
    /// last.
    pub(crate) fn finish(mut self, key: Option<&[u8]>) -> Vec<u8> {
        if let Some(key) = key {
            // The length covers the MESSAGE-INTEGRITY itself (RFC 8489 §14.5).
            self.length(24);
            let tag = hmac::sign(
                &hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key),
                &self.bytes,
            );
            self = self.attribute(MESSAGE_INTEGRITY, tag.as_ref());
        }
        // And then the FINGERPRINT (RFC 8489 §14.7).
        self.length(8);
        let crc = crc32fast::hash(&self.bytes) ^ FINGERPRINT_XOR;
        self.attribute(FINGERPRINT, &crc.to_be_bytes()).bytes
    }
}

/// `address` as an XOR-…-ADDRESS value (RFC 8489 §14.2).
fn xor_address(address: SocketAddr, transaction: &[u8; 12]) -> Vec<u8> {
    let magic = MAGIC.to_be_bytes();
    let port = address.port() ^ (MAGIC >> 16) as u16;
    let mut value = Vec::with_capacity(20);
    match address.ip() {
        IpAddr::V4(ip) => {
            value.extend_from_slice(&[0, 0x01]);
            value.extend_from_slice(&port.to_be_bytes());
            value.extend(ip.octets().iter().zip(magic).map(|(o, m)| o ^ m));
        }
        IpAddr::V6(ip) => {
            value.extend_from_slice(&[0, 0x02]);
            value.extend_from_slice(&port.to_be_bytes());
            let key = magic.iter().chain(transaction.iter());
            value.extend(ip.octets().iter().zip(key).map(|(o, k)| o ^ k));
        }
    }
    value
}

/// An XOR-…-ADDRESS value, read.
pub(crate) fn read_xor_address(value: &[u8], transaction: &[u8; 12]) -> Option<SocketAddr> {
    if value.len() < 4 {
        return None;
    }
    let magic = MAGIC.to_be_bytes();
    let port = u16::from_be_bytes([value[2], value[3]]) ^ (MAGIC >> 16) as u16;
    let ip = match (value[1], value.len()) {
        (0x01, 8) => {
            let octets: [u8; 4] = std::array::from_fn(|i| value[4 + i] ^ magic[i]);
            IpAddr::V4(Ipv4Addr::from(octets))
        }
        (0x02, 20) => {
            let key: Vec<u8> = magic.iter().chain(transaction.iter()).copied().collect();
            let octets: [u8; 16] = std::array::from_fn(|i| value[4 + i] ^ key[i]);
            IpAddr::V6(Ipv6Addr::from(octets))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

/// A STUN message, read: its method, class and transaction id, its attributes up to the
/// MESSAGE-INTEGRITY (any after it but the FINGERPRINT are not covered, so not read, RFC 8489
/// §14.5), and where the MESSAGE-INTEGRITY starts, if it has one.
pub(crate) struct Message<'a> {
    pub(crate) method: u16,
    pub(crate) class: Class,
    pub(crate) transaction: [u8; 12],
    attributes: Vec<(u16, &'a [u8])>,
    integrity: Option<usize>,
    bytes: &'a [u8],
}

impl<'a> Message<'a> {
    /// `packet` read as a STUN message, or `None` for anything else, a FINGERPRINT that does not
    /// match it, or one that is not its last attribute.
    pub(crate) fn read(packet: &'a [u8]) -> Option<Self> {
        if !is_stun(packet) {
            return None;
        }
        let (method, class) = split_type(u16::from_be_bytes([packet[0], packet[1]]));
        let transaction: [u8; 12] = packet[8..HEADER].try_into().ok()?;
        let mut attributes = Vec::new();
        let mut integrity = None;
        let mut at = HEADER;
        while at + 4 <= packet.len() {
            let kind = u16::from_be_bytes([packet[at], packet[at + 1]]);
            let size = usize::from(u16::from_be_bytes([packet[at + 2], packet[at + 3]]));
            let value = packet.get(at + 4..at + 4 + size)?;
            match kind {
                FINGERPRINT => {
                    let crc = crc32fast::hash(&packet[..at]) ^ FINGERPRINT_XOR;
                    if size != 4 || at + 8 != packet.len() || value != crc.to_be_bytes() {
                        return None;
                    }
                }
                MESSAGE_INTEGRITY if integrity.is_none() => {
                    if size != 20 {
                        return None;
                    }
                    integrity = Some(at);
                }
                _ if integrity.is_none() => attributes.push((kind, value)),
                _ => {}
            }
            at += 4 + size.div_ceil(4) * 4;
        }
        (at == packet.len()).then_some(Self {
            method,
            class,
            transaction,
            attributes,
            integrity,
            bytes: packet,
        })
    }

    /// The first attribute of `kind`.
    pub(crate) fn get(&self, kind: u16) -> Option<&'a [u8]> {
        self.attributes
            .iter()
            .find(|(k, _)| *k == kind)
            .map(|(_, v)| *v)
    }

    /// Every attribute of `kind`, in order.
    #[cfg(any(test, feature = "test-util"))]
    pub(crate) fn all(&self, kind: u16) -> impl Iterator<Item = &'a [u8]> + '_ {
        self.attributes
            .iter()
            .filter(move |(k, _)| *k == kind)
            .map(|(_, v)| *v)
    }

    /// An address attribute of `kind`, un-XOR'd.
    pub(crate) fn address(&self, kind: u16) -> Option<SocketAddr> {
        read_xor_address(self.get(kind)?, &self.transaction)
    }

    /// A text attribute of `kind`.
    pub(crate) fn text(&self, kind: u16) -> Option<&'a str> {
        std::str::from_utf8(self.get(kind)?).ok()
    }

    /// LIFETIME, in seconds.
    fn lifetime(&self) -> Option<u32> {
        Some(u32::from_be_bytes(self.get(LIFETIME)?.try_into().ok()?))
    }

    /// The ERROR-CODE's number (RFC 8489 §14.8): the class's hundreds and the number's units.
    pub(crate) fn error(&self) -> Option<u16> {
        let value = self.get(ERROR_CODE)?;
        (value.len() >= 4).then(|| u16::from(value[2] & 0x07) * 100 + u16::from(value[3]))
    }

    /// Whether the MESSAGE-INTEGRITY verifies under `key`: HMAC-SHA1 over the message up to it,
    /// with the length counted to its end (RFC 8489 §14.5). `false` for a message without one.
    pub(crate) fn verifies(&self, key: &[u8]) -> bool {
        let Some(at) = self.integrity else {
            return false;
        };
        let mut covered = self.bytes[..at].to_vec();
        let length = (at - HEADER + 24) as u16;
        covered[2..4].copy_from_slice(&length.to_be_bytes());
        hmac::verify(
            &hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key),
            &covered,
            &self.bytes[at + 4..at + 24],
        )
        .is_ok()
    }
}

/// 96 bits of the operating system's randomness: a transaction id no one off the path can guess.
pub(crate) fn transaction_id() -> Option<[u8; 12]> {
    let mut id = [0u8; 12];
    SystemRandom::new().fill(&mut id).ok()?;
    Some(id)
}

/// A Send indication carrying `payload` to `peer` (RFC 8656 §11.1): what goes to a peer whose
/// channel is not bound yet. Indications carry no credential.
pub(crate) fn send_indication(peer: SocketAddr, payload: &[u8]) -> Option<Vec<u8>> {
    let id = transaction_id()?;
    Some(
        Writer::new(SEND, Class::Indication, &id)
            .address(XOR_PEER_ADDRESS, peer)
            .attribute(DATA_VALUE, payload)
            .finish(None),
    )
}

/// `payload` behind `channel`'s 4-byte header (RFC 8656 §12.4). Over UDP no padding is needed.
pub(crate) fn channel_data(channel: u16, payload: &[u8]) -> Option<Vec<u8>> {
    let length = u16::try_from(payload.len()).ok()?;
    let mut out = Vec::with_capacity(4 + payload.len());
    out.extend_from_slice(&channel.to_be_bytes());
    out.extend_from_slice(&length.to_be_bytes());
    out.extend_from_slice(payload);
    Some(out)
}

/// How a datagram for one peer goes through the relay, as the road's senders read it without
/// waiting for the client: by its channel, or by Send indication until that is bound.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Wrap {
    Channel(u16),
    Indication,
}

impl Wrap {
    /// `payload` for `peer`, wrapped.
    pub fn wrap(self, peer: SocketAddr, payload: &[u8]) -> Option<Vec<u8>> {
        match self {
            Wrap::Channel(channel) => channel_data(channel, payload),
            Wrap::Indication => send_indication(peer, payload),
        }
    }
}

/// What a request was for.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Allocate,
    Refresh,
    Permission(IpAddr),
    Bind(SocketAddr, u16),
    Release,
}

/// A request outstanding at the server.
struct Pending {
    id: [u8; 12],
    purpose: Purpose,
    bytes: Vec<u8>,
    sends: u32,
    next: Instant,
}

/// One peer address's permission, or one peer's channel: asked for, and in place until when.
#[derive(Default)]
struct Held {
    asked: bool,
    until: Option<Instant>,
}

impl Held {
    fn live(&self, now: Instant) -> bool {
        self.until.is_some_and(|until| now < until)
    }

    /// Due to be asked for: never asked, or in place and within a minute of lapsing, with no
    /// request outstanding.
    fn due(&self, now: Instant) -> bool {
        !self.asked
            && self
                .until
                .is_none_or(|until| until.saturating_duration_since(now) <= EARLY)
    }
}

/// The allocation, as this client knows it.
enum Allocation {
    /// Asked for; not answered yet.
    Asking,
    /// In place: the relayed address, where the server saw this end, and when it lapses.
    Live {
        relayed: SocketAddr,
        mapped: Option<SocketAddr>,
        until: Instant,
    },
    /// Refused, given up, lapsed, or released: why.
    Over(&'static str),
}

/// One allocation on a TURN server, as this end keeps it. Sans I/O: [`Turn::poll_transmit`] says
/// what to send the server, [`Turn::handle`] takes what it sent, and [`Turn::handle_timeout`]
/// keeps time.
pub struct Turn {
    server: SocketAddr,
    username: String,
    password: String,
    realm: Option<String>,
    nonce: Option<String>,
    /// The long-term key, once the realm is known.
    key: Option<[u8; 16]>,
    allocation: Allocation,
    pending: Vec<Pending>,
    permissions: HashMap<IpAddr, Held>,
    channels: HashMap<SocketAddr, (u16, Held)>,
    next_channel: u16,
    /// What waits for a peer's permission, with when it was given.
    waiting: VecDeque<(SocketAddr, Vec<u8>, Instant)>,
    out: VecDeque<Vec<u8>>,
    /// A refresh outstanding, so only one is.
    refreshing: bool,
}

impl Turn {
    /// A client for `server`, under `username` and `password`, with its first Allocate written
    /// (no credential yet: the server names its realm and nonce in answer). `None` with no
    /// randomness for a transaction id.
    pub fn new(server: SocketAddr, username: &str, password: &str, now: Instant) -> Option<Self> {
        let mut turn = Self {
            server,
            username: username.to_string(),
            password: password.to_string(),
            realm: None,
            nonce: None,
            key: None,
            allocation: Allocation::Asking,
            pending: Vec::new(),
            permissions: HashMap::new(),
            channels: HashMap::new(),
            next_channel: FIRST_CHANNEL,
            waiting: VecDeque::new(),
            out: VecDeque::new(),
            refreshing: false,
        };
        turn.request(Purpose::Allocate, now)?;
        Some(turn)
    }

    /// The server's address: the only one whose packets go to [`Turn::handle`].
    pub fn server(&self) -> SocketAddr {
        self.server
    }

    /// The relayed address, while the allocation is in place.
    pub fn relayed(&self) -> Option<SocketAddr> {
        match self.allocation {
            Allocation::Live { relayed, .. } => Some(relayed),
            _ => None,
        }
    }

    /// Where the server saw this end, while the allocation is in place.
    pub fn mapped(&self) -> Option<SocketAddr> {
        match self.allocation {
            Allocation::Live { mapped, .. } => mapped,
            _ => None,
        }
    }

    /// Why this client is over: the allocation refused, given up, lapsed or released.
    pub fn failed(&self) -> Option<&'static str> {
        match self.allocation {
            Allocation::Over(why) => Some(why),
            _ => None,
        }
    }

    /// The next datagram for the server.
    pub fn poll_transmit(&mut self) -> Option<Vec<u8>> {
        self.out.pop_front()
    }

    /// The peer `peer` may be reached through the relay: a permission for its address and a
    /// channel for it are asked for, if they are not in place or asked for already.
    pub fn permit(&mut self, peer: SocketAddr, now: Instant) {
        if self.relayed().is_none() {
            return;
        }
        self.permissions.entry(peer.ip()).or_default();
        if self.next_channel <= LAST_CHANNEL {
            if let std::collections::hash_map::Entry::Vacant(entry) = self.channels.entry(peer) {
                entry.insert((self.next_channel, Held::default()));
                self.next_channel += 1;
            }
        }
        self.keep(now);
    }

    /// How a datagram for `peer` goes now: by its channel once bound, by Send indication while its
    /// permission is in place, or not yet.
    pub fn wrap_for(&self, peer: SocketAddr, now: Instant) -> Option<Wrap> {
        if let Some((channel, held)) = self.channels.get(&peer) {
            if held.live(now) {
                return Some(Wrap::Channel(*channel));
            }
        }
        self.permissions
            .get(&peer.ip())
            .is_some_and(|held| held.live(now))
            .then_some(Wrap::Indication)
    }

    /// `payload` for `peer` through the relay: written now if `peer` may be reached, else kept a
    /// little while for when it may (its permission is asked for).
    pub fn send(&mut self, peer: SocketAddr, payload: &[u8], now: Instant) {
        if let Some(datagram) = self
            .wrap_for(peer, now)
            .and_then(|wrap| wrap.wrap(peer, payload))
        {
            self.out.push_back(datagram);
            return;
        }
        if self.relayed().is_none() {
            return;
        }
        self.permit(peer, now);
        if self.waiting.len() >= WAITING {
            self.waiting.pop_front();
        }
        self.waiting.push_back((peer, payload.to_vec(), now));
    }

    /// A packet from the server: its answer to a request, read here (`None`), or a peer's
    /// datagram, unwrapped: the peer's address and what it sent.
    pub fn handle(&mut self, packet: &[u8], now: Instant) -> Option<(SocketAddr, Vec<u8>)> {
        if is_channel_data(packet) {
            let channel = u16::from_be_bytes([packet[0], packet[1]]);
            let length = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
            let payload = packet.get(4..4 + length)?;
            let peer = self
                .channels
                .iter()
                .find(|(_, (number, held))| *number == channel && held.until.is_some())
                .map(|(peer, _)| *peer)?;
            return Some((peer, payload.to_vec()));
        }
        let message = Message::read(packet)?;
        if message.method == DATA && message.class == Class::Indication {
            // Unsigned, so taken only for a peer the server would relay from: one with a live
            // permission here.
            let peer = message.address(XOR_PEER_ADDRESS)?;
            if !self
                .permissions
                .get(&peer.ip())
                .is_some_and(|held| held.live(now))
            {
                return None;
            }
            let payload = message.get(DATA_VALUE)?;
            return Some((peer, payload.to_vec()));
        }
        self.answered(&message, now);
        None
    }

    /// An answer to one of this client's requests.
    fn answered(&mut self, message: &Message, now: Instant) {
        if !matches!(message.class, Class::Success | Class::Error) {
            return;
        }
        let Some(at) = self
            .pending
            .iter()
            .position(|p| p.id == message.transaction)
        else {
            return;
        };
        if message.method != method_of(self.pending[at].purpose) {
            return;
        }
        let purpose = self.pending[at].purpose;
        if message.class == Class::Error {
            match message.error() {
                // Unauthenticated, or a stale nonce: the request again, under the realm and
                // nonce the answer names, once they are not the ones it was sent under. One that
                // names the same again refuses the credential itself.
                Some(401 | 438) => {
                    let realm = message.text(REALM).map(str::to_string);
                    let nonce = message.text(NONCE).map(str::to_string);
                    self.pending.remove(at);
                    match (realm, nonce) {
                        (Some(realm), Some(nonce))
                            if self.key.is_none()
                                || self.realm.as_deref() != Some(realm.as_str())
                                || self.nonce.as_deref() != Some(nonce.as_str()) =>
                        {
                            self.key = Some(long_term_key(&self.username, &realm, &self.password));
                            self.realm = Some(realm);
                            self.nonce = Some(nonce);
                            let _ = self.request(purpose, now);
                        }
                        _ => self.refused(purpose, "unauthorized"),
                    }
                }
                _ => {
                    self.pending.remove(at);
                    self.refused(purpose, "refused");
                }
            }
            return;
        }
        // ⛔ A success is taken only under the long-term key.
        if !self.key.is_some_and(|key| message.verifies(&key)) {
            return;
        }
        self.pending.remove(at);
        match purpose {
            Purpose::Allocate => match message.address(XOR_RELAYED_ADDRESS) {
                Some(relayed) => {
                    let lifetime = message.lifetime().unwrap_or(ASKED_LIFETIME);
                    self.allocation = Allocation::Live {
                        relayed,
                        mapped: message.address(XOR_MAPPED_ADDRESS),
                        until: now + Duration::from_secs(u64::from(lifetime)),
                    };
                }
                None => self.allocation = Allocation::Over("refused"),
            },
            Purpose::Refresh => {
                self.refreshing = false;
                if let Allocation::Live { until, .. } = &mut self.allocation {
                    let lifetime = message.lifetime().unwrap_or(ASKED_LIFETIME);
                    *until = now + Duration::from_secs(u64::from(lifetime));
                }
            }
            Purpose::Permission(ip) => {
                if let Some(held) = self.permissions.get_mut(&ip) {
                    held.asked = false;
                    held.until = Some(now + PERMISSION_LIFETIME);
                }
            }
            Purpose::Bind(peer, _) => {
                if let Some((_, held)) = self.channels.get_mut(&peer) {
                    held.asked = false;
                    held.until = Some(now + CHANNEL_LIFETIME);
                }
                // A channel's binding is a permission for its peer's address too (§12).
                if let Some(held) = self.permissions.get_mut(&peer.ip()) {
                    held.until = Some(now + PERMISSION_LIFETIME);
                }
            }
            // Said once and never kept, so never answered here.
            Purpose::Release => {}
        }
        self.flush(now);
    }

    /// `purpose` refused, or given up.
    fn refused(&mut self, purpose: Purpose, why: &'static str) {
        match purpose {
            Purpose::Allocate => self.allocation = Allocation::Over(why),
            // An allocation the server no longer holds (437) is over; any other refusal of a
            // refresh leaves it to lapse in its own time.
            Purpose::Refresh => self.refreshing = false,
            Purpose::Permission(ip) => {
                self.permissions.remove(&ip);
            }
            Purpose::Bind(peer, _) => {
                self.channels.remove(&peer);
            }
            Purpose::Release => self.allocation = Allocation::Over("released"),
        }
    }

    /// Writes what waited for a permission now in place, and drops what waited too long.
    fn flush(&mut self, now: Instant) {
        let waiting = std::mem::take(&mut self.waiting);
        for (peer, payload, since) in waiting {
            if now.saturating_duration_since(since) >= WAIT_FOR {
                continue;
            }
            match self
                .wrap_for(peer, now)
                .and_then(|wrap| wrap.wrap(peer, &payload))
            {
                Some(datagram) => self.out.push_back(datagram),
                None => self.waiting.push_back((peer, payload, since)),
            }
        }
    }

    /// Time: requests sent again or given up, and the allocation, permissions and channels
    /// refreshed when due.
    pub fn handle_timeout(&mut self, now: Instant) {
        let mut given_up = Vec::new();
        for pending in &mut self.pending {
            if now < pending.next {
                continue;
            }
            if pending.sends >= SENDS {
                given_up.push(pending.id);
                continue;
            }
            self.out.push_back(pending.bytes.clone());
            pending.sends += 1;
            // 0.5 s, then doubling; after the last send, sixteen RTOs.
            pending.next = now
                + if pending.sends >= SENDS {
                    RTO * LAST_WAIT
                } else {
                    RTO * (1 << (pending.sends - 1))
                };
        }
        for id in given_up {
            if let Some(at) = self.pending.iter().position(|p| p.id == id) {
                let purpose = self.pending.remove(at).purpose;
                self.refused(purpose, "noAnswer");
            }
        }
        if let Allocation::Live { until, .. } = self.allocation {
            if now >= until {
                self.allocation = Allocation::Over("lapsed");
            } else if !self.refreshing && until.saturating_duration_since(now) <= EARLY {
                self.refreshing = self.request(Purpose::Refresh, now).is_some();
            }
        }
        self.keep(now);
        self.waiting
            .retain(|(_, _, since)| now.saturating_duration_since(*since) < WAIT_FOR);
    }

    /// Each permission and channel asked for that is due.
    fn keep(&mut self, now: Instant) {
        if self.relayed().is_none() {
            return;
        }
        let permissions: Vec<IpAddr> = self
            .permissions
            .iter()
            .filter(|(_, held)| held.due(now))
            .map(|(ip, _)| *ip)
            .collect();
        for ip in permissions {
            if self.request(Purpose::Permission(ip), now).is_some() {
                if let Some(held) = self.permissions.get_mut(&ip) {
                    held.asked = true;
                }
            }
        }
        let channels: Vec<(SocketAddr, u16)> = self
            .channels
            .iter()
            .filter(|(_, (_, held))| held.due(now))
            .map(|(peer, (number, _))| (*peer, *number))
            .collect();
        for (peer, number) in channels {
            if self.request(Purpose::Bind(peer, number), now).is_some() {
                if let Some((_, held)) = self.channels.get_mut(&peer) {
                    held.asked = true;
                }
            }
        }
    }

    /// Gives the allocation back: a Refresh with a lifetime of 0, sent once. The road is going.
    pub fn release(&mut self, now: Instant) {
        if self.relayed().is_some() {
            let _ = self.request(Purpose::Release, now);
            self.allocation = Allocation::Over("released");
        }
    }

    /// Writes a request for `purpose`, under the credential once the realm is known, and keeps it
    /// for its retransmissions. `None` with no randomness for its id.
    fn request(&mut self, purpose: Purpose, now: Instant) -> Option<()> {
        let id = transaction_id()?;
        let mut writer = Writer::new(method_of(purpose), Class::Request, &id);
        writer = match purpose {
            Purpose::Allocate => writer.attribute(REQUESTED_TRANSPORT, &[UDP, 0, 0, 0]),
            Purpose::Refresh => writer.attribute(LIFETIME, &ASKED_LIFETIME.to_be_bytes()),
            Purpose::Release => writer.attribute(LIFETIME, &0u32.to_be_bytes()),
            Purpose::Permission(ip) => writer.address(XOR_PEER_ADDRESS, SocketAddr::new(ip, 0)),
            Purpose::Bind(peer, number) => {
                let mut value = [0u8; 4];
                value[..2].copy_from_slice(&number.to_be_bytes());
                writer
                    .attribute(CHANNEL_NUMBER, &value)
                    .address(XOR_PEER_ADDRESS, peer)
            }
        };
        if let (Some(realm), Some(nonce)) = (&self.realm, &self.nonce) {
            writer = writer
                .attribute(USERNAME, self.username.as_bytes())
                .attribute(REALM, realm.as_bytes())
                .attribute(NONCE, nonce.as_bytes());
        }
        let bytes = writer.finish(self.key.as_ref().map(|key| key.as_slice()));
        // A release is said once: the road is going, and the allocation lapses anyway.
        if purpose != Purpose::Release {
            self.pending.push(Pending {
                id,
                purpose,
                bytes: bytes.clone(),
                sends: 1,
                next: now + RTO,
            });
        }
        self.out.push_back(bytes);
        Some(())
    }
}

/// The method a request for `purpose` is.
fn method_of(purpose: Purpose) -> u16 {
    match purpose {
        Purpose::Allocate => ALLOCATE,
        Purpose::Refresh | Purpose::Release => REFRESH,
        Purpose::Permission(_) => CREATE_PERMISSION,
        Purpose::Bind(..) => CHANNEL_BIND,
    }
}

/// A TURN server for tests ([`stand_in::StandIn`]).
#[cfg(any(test, feature = "test-util"))]
pub mod stand_in;

#[cfg(test)]
mod tests {
    use super::stand_in::{Behaviour, StandIn};
    use super::*;
    use std::net::UdpSocket;

    fn bytes(hex: &str) -> Vec<u8> {
        let digits: String = hex.split_whitespace().collect();
        (0..digits.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).unwrap())
            .collect()
    }

    /// RFC 5769 §2.1, the sample request: short-term credential, FINGERPRINT last.
    const SAMPLE_REQUEST: &str = "00 01 00 58 21 12 a4 42 b7 e7 a7 01 bc 34 d6 86 fa 87 df ae
        80 22 00 10 53 54 55 4e 20 74 65 73 74 20 63 6c 69 65 6e 74 00 24 00 04 6e 00 01 ff
        80 29 00 08 93 2f f9 b1 51 26 3b 36 00 06 00 09 65 76 74 6a 3a 68 36 76 59 20 20 20
        00 08 00 14 9a ea a7 0c bf d8 cb 56 78 1e f2 b5 b2 d3 f2 49 c1 b5 71 a2
        80 28 00 04 e5 7a 3b cf";
    /// §2.2, the sample IPv4 response: 192.0.2.1 port 32853.
    const SAMPLE_V4: &str = "01 01 00 3c 21 12 a4 42 b7 e7 a7 01 bc 34 d6 86 fa 87 df ae
        80 22 00 0b 74 65 73 74 20 76 65 63 74 6f 72 20 00 20 00 08 00 01 a1 47 e1 12 a6 43
        00 08 00 14 2b 91 f5 99 fd 9e 90 c3 8c 74 89 f9 2a f9 ba 53 f0 6b e7 d7
        80 28 00 04 c0 7d 4c 96";
    /// §2.3, the sample IPv6 response: 2001:db8:1234:5678:11:2233:4455:6677 port 32853.
    const SAMPLE_V6: &str = "01 01 00 48 21 12 a4 42 b7 e7 a7 01 bc 34 d6 86 fa 87 df ae
        80 22 00 0b 74 65 73 74 20 76 65 63 74 6f 72 20 00 20 00 14 00 02 a1 47
        01 13 a9 fa a5 d3 f1 79 bc 25 f4 b5 be d2 b9 d9
        00 08 00 14 a3 82 95 4e 4b e6 7b f1 17 84 c9 7c 82 92 c2 75 bf e3 ed 41
        80 28 00 04 c8 fb 0b 4c";
    /// §2.4, the sample request with long-term authentication.
    const SAMPLE_LONG_TERM: &str = "00 01 00 60 21 12 a4 42 78 ad 34 33 c6 ad 72 c0 29 da 41 2e
        00 06 00 12 e3 83 9e e3 83 88 e3 83 aa e3 83 83 e3 82 af e3 82 b9 00 00
        00 15 00 1c 66 2f 2f 34 39 39 6b 39 35 34 64 36 4f 4c 33 34 6f 4c 39 46 53 54 76 79
        36 34 73 41 00 14 00 0b 65 78 61 6d 70 6c 65 2e 6f 72 67 00
        00 08 00 14 f6 70 24 65 6d d6 4a 3e 02 b8 e0 71 2e 85 c9 a2 8c a8 96 66";
    const SHORT_TERM: &[u8] = b"VOkJxbRl1RmTxUk/WvJxBt";
    /// §2.4's username, and its password after SASLprep.
    const LONG_TERM_USER: &str = "\u{30DE}\u{30C8}\u{30EA}\u{30C3}\u{30AF}\u{30B9}";
    const LONG_TERM_PASSWORD: &str = "TheMatrIX";

    /// ★ The message types are RFC 8489's and RFC 8656's, by value, and every method and class
    /// reads back as written.
    #[test]
    fn the_message_types_are_the_rfcs() {
        let written = [
            (0x001, Class::Request, 0x0001),
            (0x001, Class::Success, 0x0101),
            (ALLOCATE, Class::Request, 0x0003),
            (ALLOCATE, Class::Success, 0x0103),
            (ALLOCATE, Class::Error, 0x0113),
            (REFRESH, Class::Request, 0x0004),
            (SEND, Class::Indication, 0x0016),
            (DATA, Class::Indication, 0x0017),
            (CREATE_PERMISSION, Class::Request, 0x0008),
            (CHANNEL_BIND, Class::Request, 0x0009),
            (CHANNEL_BIND, Class::Success, 0x0109),
        ];
        for (method, class, kind) in written {
            assert_eq!(message_type(method, class), kind, "{method:#x} {class:?}");
            assert_eq!(split_type(kind), (method, class));
        }
        for method in 0..0x1000u16 {
            for class in [
                Class::Request,
                Class::Indication,
                Class::Success,
                Class::Error,
            ] {
                assert_eq!(split_type(message_type(method, class)), (method, class));
            }
        }
    }

    /// ★ RFC 5769's sample messages read, and their FINGERPRINT and MESSAGE-INTEGRITY verify: the
    /// short-term key on §2.1–§2.3, the long-term key (MD5 of user, realm and password) on §2.4.
    /// The mapped addresses are the RFC's. CONTROL: one byte changed in a body is refused at its
    /// FINGERPRINT; another key, or one changed MESSAGE-INTEGRITY byte, does not verify.
    #[test]
    fn rfc_5769s_samples_read_and_verify() {
        let request = bytes(SAMPLE_REQUEST);
        let read = Message::read(&request).expect("the sample request");
        assert_eq!((read.method, read.class), (0x001, Class::Request));
        assert_eq!(read.text(USERNAME), Some("evtj:h6vY"));
        assert!(read.verifies(SHORT_TERM));
        assert!(!read.verifies(b"VOkJxbRl1RmTxUk/WvJxBu"));

        let v4 = bytes(SAMPLE_V4);
        let read = Message::read(&v4).expect("the IPv4 response");
        assert_eq!(read.class, Class::Success);
        assert_eq!(
            read.address(XOR_MAPPED_ADDRESS),
            Some("192.0.2.1:32853".parse().unwrap())
        );
        assert!(read.verifies(SHORT_TERM));

        let v6 = bytes(SAMPLE_V6);
        let read = Message::read(&v6).expect("the IPv6 response");
        assert_eq!(
            read.address(XOR_MAPPED_ADDRESS),
            Some(
                "[2001:db8:1234:5678:11:2233:4455:6677]:32853"
                    .parse()
                    .unwrap()
            )
        );
        assert!(read.verifies(SHORT_TERM));

        let long_term = bytes(SAMPLE_LONG_TERM);
        let read = Message::read(&long_term).expect("the long-term request");
        assert_eq!(read.text(USERNAME), Some(LONG_TERM_USER));
        assert_eq!(read.text(REALM), Some("example.org"));
        assert_eq!(read.text(NONCE), Some("f//499k954d6OL34oL9FSTvy64sA"));
        let key = long_term_key(LONG_TERM_USER, "example.org", LONG_TERM_PASSWORD);
        assert!(read.verifies(&key));
        assert!(!read.verifies(&long_term_key(LONG_TERM_USER, "example.org", "TheMatrix")));

        // CONTROLS.
        let mut changed = request.clone();
        changed[30] ^= 1;
        assert!(Message::read(&changed).is_none(), "FINGERPRINT not checked");
        let mut changed = long_term.clone();
        let last = changed.len() - 1;
        changed[last] ^= 1;
        assert!(!Message::read(&changed).unwrap().verifies(&key));
        assert!(Message::read(&request[..request.len() - 8]).is_none());
    }

    /// ★ What this end writes is what RFC 5769 §2.4 holds: the same attributes under the long-term
    /// key give its MESSAGE-INTEGRITY byte for byte, and the FINGERPRINT after it reads back.
    #[test]
    fn the_writer_signs_as_rfc_5769_does() {
        let sample = bytes(SAMPLE_LONG_TERM);
        let transaction: [u8; 12] = sample[8..20].try_into().unwrap();
        let key = long_term_key(LONG_TERM_USER, "example.org", LONG_TERM_PASSWORD);
        let written = Writer::new(0x001, Class::Request, &transaction)
            .attribute(USERNAME, LONG_TERM_USER.as_bytes())
            .attribute(NONCE, b"f//499k954d6OL34oL9FSTvy64sA")
            .attribute(REALM, b"example.org")
            .finish(Some(&key));
        assert_eq!(written.len(), sample.len() + 8, "and a FINGERPRINT");
        assert_eq!(written[..2], sample[..2]);
        assert_eq!(
            written[4..sample.len()],
            sample[4..],
            "the attributes and their seal"
        );
        let read = Message::read(&written).expect("the FINGERPRINT reads back");
        assert!(read.verifies(&key));
    }

    /// Addresses XOR'd and back, both families, and a value of neither family refused.
    #[test]
    fn an_address_crosses_xored() {
        let id = [7u8; 12];
        for address in ["203.0.113.7:61000", "[2001:db8::7]:3478", "127.0.0.1:1"] {
            let address: SocketAddr = address.parse().unwrap();
            assert_eq!(
                read_xor_address(&xor_address(address, &id), &id),
                Some(address)
            );
        }
        assert_eq!(read_xor_address(&[0, 3, 0, 0, 1, 2, 3, 4], &id), None);
    }

    /// A TURN client on a socket of its own, pumped: what it writes goes to the server, what the
    /// server sends comes back, and a peer's datagrams are collected.
    struct Pumped {
        turn: Turn,
        socket: UdpSocket,
        delivered: Vec<(SocketAddr, Vec<u8>)>,
    }

    impl Pumped {
        fn new(server: SocketAddr, username: &str, password: &str) -> Self {
            let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
            socket
                .set_read_timeout(Some(Duration::from_millis(5)))
                .unwrap();
            Self {
                turn: Turn::new(server, username, password, Instant::now()).unwrap(),
                socket,
                delivered: Vec::new(),
            }
        }

        /// One round at `now`.
        fn round(&mut self, now: Instant) {
            self.turn.handle_timeout(now);
            while let Some(datagram) = self.turn.poll_transmit() {
                self.socket.send_to(&datagram, self.turn.server()).unwrap();
            }
            let mut buffer = [0u8; 2048];
            while let Ok((length, from)) = self.socket.recv_from(&mut buffer) {
                if from == self.turn.server() {
                    if let Some(peer) = self.turn.handle(&buffer[..length], now) {
                        self.delivered.push(peer);
                    }
                }
            }
            while let Some(datagram) = self.turn.poll_transmit() {
                self.socket.send_to(&datagram, self.turn.server()).unwrap();
            }
        }

        /// Rounds until `done`, for up to two seconds of real time.
        fn until(&mut self, done: impl Fn(&Self) -> bool) -> bool {
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(2) {
                self.round(Instant::now());
                if done(self) {
                    return true;
                }
            }
            false
        }
    }

    /// ★ An allocation under the long-term credential: the first Allocate unauthenticated, the
    /// server's 401 naming its realm and nonce, the Allocate again under them, and the relayed and
    /// mapped addresses the server names, by value. A stale nonce is asked again. CONTROL: a wrong
    /// password is refused, "unauthorized", and allocates nothing.
    #[test]
    fn an_allocation_is_made_under_the_long_term_credential() {
        let server = StandIn::start("road", "throwaway-password");
        let mut client = Pumped::new(server.address, "road", "throwaway-password");
        assert!(client.until(|c| c.turn.relayed().is_some()));
        let relayed = client.turn.relayed().unwrap();
        assert!(relayed.ip().is_loopback() && relayed != server.address);
        assert_eq!(
            client.turn.mapped(),
            Some(client.socket.local_addr().unwrap())
        );
        assert_eq!(server.seen().allocations, 1);
        assert_eq!(server.seen().unauthorized, 0);

        let stale = StandIn::start_with(
            "road",
            "throwaway-password",
            Behaviour {
                stale_once: true,
                ..Behaviour::default()
            },
        );
        let mut client = Pumped::new(stale.address, "road", "throwaway-password");
        assert!(client.until(|c| c.turn.relayed().is_some()));
        assert_eq!((stale.seen().stale, stale.seen().allocations), (1, 1));

        // CONTROL.
        let mut wrong = Pumped::new(server.address, "road", "not-the-password");
        assert!(wrong.until(|c| c.turn.failed().is_some()));
        assert_eq!(wrong.turn.failed(), Some("unauthorized"));
        assert_eq!(wrong.turn.relayed(), None);
        assert_eq!(server.seen().allocations, 1);
        assert_eq!(server.seen().unauthorized, 1);
    }

    /// ★ A peer through the relay: a permission for its address and a channel for it are asked
    /// for; what is sent before the permission is in place waits, and goes by Send indication once
    /// it is; then each datagram goes by the channel and the peer's come back by it, unwrapped, by
    /// value. CONTROL: a peer with no permission is not relayed to this end at all.
    #[test]
    fn a_peer_is_reached_by_permission_and_channel() {
        let server = StandIn::start("road", "throwaway-password");
        let mut client = Pumped::new(server.address, "road", "throwaway-password");
        assert!(client.until(|c| c.turn.relayed().is_some()));
        let relayed = client.turn.relayed().unwrap();
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        peer.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        let peer_at = peer.local_addr().unwrap();

        // Sent before the permission: it waits, and goes once the channel is bound.
        let now = Instant::now();
        assert_eq!(client.turn.wrap_for(peer_at, now), None);
        client.turn.send(peer_at, b"first", now);
        assert!(client.until(
            |c| c.turn.wrap_for(peer_at, Instant::now()) == Some(Wrap::Channel(FIRST_CHANNEL))
        ));
        let mut buffer = [0u8; 64];
        let (n, from) = peer.recv_from(&mut buffer).unwrap();
        assert_eq!((&buffer[..n], from), (&b"first"[..], relayed));
        assert_eq!(server.seen().permissions, vec![peer_at.ip()]);
        assert_eq!(server.seen().channels, vec![(FIRST_CHANNEL, peer_at)]);

        // By the channel, both ways.
        client.turn.send(peer_at, b"second", Instant::now());
        client.round(Instant::now());
        let (n, _) = peer.recv_from(&mut buffer).unwrap();
        assert_eq!(&buffer[..n], b"second");
        peer.send_to(b"back", relayed).unwrap();
        assert!(client.until(|c| !c.delivered.is_empty()));
        assert_eq!(client.delivered, vec![(peer_at, b"back".to_vec())]);
        // The first went by Send indication as soon as the permission was in place, ahead of
        // its channel; the second by the channel.
        let seen = server.seen();
        assert_eq!(
            (
                seen.sent_by_indication,
                seen.sent_by_channel,
                seen.delivered_by_channel
            ),
            (1, 1, 1)
        );

        // CONTROL: a stranger the relay holds no permission for.
        let stranger = UdpSocket::bind("127.0.0.2:0")
            .or_else(|_| UdpSocket::bind("127.0.0.1:0"))
            .unwrap();
        if stranger.local_addr().unwrap().ip() != peer_at.ip() {
            stranger.send_to(b"stranger", relayed).unwrap();
            std::thread::sleep(Duration::from_millis(50));
            client.round(Instant::now());
            assert_eq!(client.delivered.len(), 1, "a stranger was relayed");
            assert_eq!(server.seen().unpermitted, 1);
        }
    }

    /// ⛔ A Data indication is taken only from a peer this end holds a live permission for: one
    /// naming any other peer, which needs no key to send, is dropped. CONTROL: the same from the
    /// permitted peer is delivered.
    #[test]
    fn a_data_indication_from_a_peer_with_no_permission_is_dropped() {
        let server = StandIn::start("road", "throwaway-password");
        let mut client = Pumped::new(server.address, "road", "throwaway-password");
        assert!(client.until(|c| c.turn.relayed().is_some()));
        let permitted: SocketAddr = "127.0.0.1:40000".parse().unwrap();
        client.turn.permit(permitted, Instant::now());
        assert!(client.until(|c| c.turn.wrap_for(permitted, Instant::now()).is_some()));
        let indication = |peer| {
            Writer::new(DATA, Class::Indication, &[9; 12])
                .address(XOR_PEER_ADDRESS, peer)
                .attribute(DATA_VALUE, b"data")
                .finish(None)
        };
        let now = Instant::now();
        let stranger: SocketAddr = "198.51.100.9:50000".parse().unwrap();
        assert_eq!(client.turn.handle(&indication(stranger), now), None);
        // CONTROL.
        assert_eq!(
            client.turn.handle(&indication(permitted), now),
            Some((permitted, b"data".to_vec()))
        );
    }

    /// ★ Refreshes, each a minute before it would lapse: the permission at 240 s, the allocation
    /// and the channel at 540 s (the server's 600 s lifetime), each answered. And the release at
    /// the end, a lifetime of 0.
    #[test]
    fn the_allocation_permission_and_channel_are_refreshed_before_they_lapse() {
        let server = StandIn::start("road", "throwaway-password");
        let mut client = Pumped::new(server.address, "road", "throwaway-password");
        assert!(client.until(|c| c.turn.relayed().is_some()));
        let peer: SocketAddr = "127.0.0.1:9".parse().unwrap();
        let start = Instant::now();
        client.turn.permit(peer, start);
        assert!(
            client.until(
                |c| c.turn.wrap_for(peer, Instant::now()) == Some(Wrap::Channel(FIRST_CHANNEL))
            )
        );
        let (permissions, channels) = (
            server.seen().permissions.len(),
            server.seen().channels.len(),
        );
        assert_eq!((permissions, channels), (1, 1));

        // Nothing is due a minute in.
        client.round(start + Duration::from_secs(60));
        assert_eq!(server.seen().refreshes, 0);
        assert_eq!(server.seen().permissions.len(), 1);

        // The permission, at 240 s.
        let at = start + Duration::from_secs(241);
        client.turn.handle_timeout(at);
        let mut rounds = 0;
        while server.seen().permissions.len() < 2 && rounds < 100 {
            client.round(at);
            rounds += 1;
        }
        assert_eq!(server.seen().permissions.len(), 2);
        assert_eq!(server.seen().refreshes, 0);

        // The allocation and the channel, at 540 s.
        let at = start + Duration::from_secs(541);
        let mut rounds = 0;
        while (server.seen().refreshes < 1 || server.seen().channels.len() < 2) && rounds < 100 {
            client.round(at);
            rounds += 1;
        }
        assert_eq!(server.seen().refreshes, 1);
        assert_eq!(server.seen().channels.len(), 2);
        assert!(client.turn.relayed().is_some(), "still allocated");

        client.turn.release(Instant::now());
        client.round(Instant::now());
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!(server.seen().released, 1);
        assert_eq!(client.turn.failed(), Some("released"));
    }

    /// ★ A request nobody answers is sent at 0, 0.5, 1.5, 3.5, 7.5, 15.5 and 31.5 s, and given up
    /// 8 s after the last: the allocation then fails, "noAnswer". By value, each send's time.
    #[test]
    fn an_unanswered_request_is_sent_again_and_then_given_up() {
        let silent = UdpSocket::bind("127.0.0.1:0").unwrap();
        let start = Instant::now();
        let mut turn = Turn::new(silent.local_addr().unwrap(), "road", "pw", start).unwrap();
        let mut sent = Vec::new();
        for ms in (0..=40_000u64).step_by(50) {
            let now = start + Duration::from_millis(ms);
            turn.handle_timeout(now);
            while turn.poll_transmit().is_some() {
                sent.push(ms);
            }
            if turn.failed().is_some() {
                assert_eq!(ms, 39_500);
                break;
            }
        }
        assert_eq!(sent, [0, 500, 1500, 3500, 7500, 15500, 31500]);
        assert_eq!(turn.failed(), Some("noAnswer"));
    }

    /// ★ A success is taken only under the long-term key: answered by a server that says 401, then
    /// a success naming a relayed address but signed under another key, and then one with no
    /// MESSAGE-INTEGRITY at all, this end takes neither and stays unallocated. CONTROL: the same
    /// success signed under the right key is taken, its address by value.
    #[test]
    fn a_success_that_does_not_verify_is_never_taken() {
        let server = UdpSocket::bind("127.0.0.1:0").unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let mut client = Pumped::new(server.local_addr().unwrap(), "road", "pw");
        let mut buffer = [0u8; 512];
        let mut next_request = |client: &mut Pumped| loop {
            client.round(Instant::now());
            if let Ok((n, from)) = server.recv_from(&mut buffer) {
                let read = Message::read(&buffer[..n]).unwrap();
                return (read.transaction, read.integrity.is_some(), from);
            }
        };
        let (id, signed, from) = next_request(&mut client);
        assert!(!signed, "the first Allocate carries no credential");
        let challenge = Writer::new(ALLOCATE, Class::Error, &id)
            .attribute(ERROR_CODE, &[0, 0, 4, 1])
            .attribute(REALM, b"nexus.test")
            .attribute(NONCE, b"nonce-1")
            .finish(None);
        server.send_to(&challenge, from).unwrap();
        let (id, signed, from) = next_request(&mut client);
        assert!(signed, "the Allocate again, under the credential");
        let relayed: SocketAddr = "198.51.100.9:49152".parse().unwrap();
        let success = |key: Option<&[u8]>| {
            Writer::new(ALLOCATE, Class::Success, &id)
                .address(XOR_RELAYED_ADDRESS, relayed)
                .attribute(LIFETIME, &600u32.to_be_bytes())
                .finish(key)
        };
        let wrong = long_term_key("road", "nexus.test", "not-pw");
        for forged in [success(Some(&wrong)), success(None)] {
            server.send_to(&forged, from).unwrap();
            std::thread::sleep(Duration::from_millis(20));
            client.round(Instant::now());
            assert_eq!(client.turn.relayed(), None, "a forged success was taken");
            assert_eq!(client.turn.failed(), None);
        }
        // CONTROL.
        let right = long_term_key("road", "nexus.test", "pw");
        server.send_to(&success(Some(&right)), from).unwrap();
        assert!(client.until(|c| c.turn.relayed().is_some()));
        assert_eq!(client.turn.relayed(), Some(relayed));
    }

    /// The relay's framing: ChannelData's four bytes, and a Send indication carrying the peer and
    /// the datagram, read back.
    #[test]
    fn a_datagram_is_wrapped_for_the_relay() {
        let peer: SocketAddr = "198.51.100.4:50000".parse().unwrap();
        assert_eq!(
            Wrap::Channel(0x4001).wrap(peer, b"abc").unwrap(),
            [0x40, 0x01, 0, 3, b'a', b'b', b'c']
        );
        let indication = Wrap::Indication.wrap(peer, b"abc").unwrap();
        let read = Message::read(&indication).unwrap();
        assert_eq!((read.method, read.class), (SEND, Class::Indication));
        assert_eq!(read.address(XOR_PEER_ADDRESS), Some(peer));
        assert_eq!(read.get(DATA_VALUE), Some(&b"abc"[..]));
        assert!(is_channel_data(
            &Wrap::Channel(0x4001).wrap(peer, b"").unwrap()
        ));
        assert!(!is_stun(&Wrap::Channel(0x4001).wrap(peer, b"").unwrap()));
    }
}
