//! A TURN server for tests, on loopback: the least of RFC 8656's server side that this crate's
//! client and a road's ICE agent need, written from the RFC with the client's own message reader and
//! writer. One user under the long-term credential; one allocation per client address, each with a
//! relayed socket of its own; permissions, channels, Send and Data indications and ChannelData; and
//! a count of everything it did ([`Seen`]), so a test can say by value which way a datagram went.
//! Like the internet between two computers, it can hold back a road's datagrams of one kind until
//! told, or lose the next one of a kind ([`StandIn::hold`], [`StandIn::lose_next`]): a datagram's
//! kind is in its clear header, so it reads no more of one than a network could.
//!
//! It shares the client's codec, so a mistake made the same way at both ends passes here: a TURN
//! server of someone else's, run once as the positive control, is what proves the client against
//! the protocol. Never shipped: it exists only in the crate's tests and behind `test-util`.
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::{
    long_term_key, read_xor_address, Class, Message, Writer, ALLOCATE, CHANNEL_BIND,
    CHANNEL_NUMBER, CREATE_PERMISSION, DATA, DATA_VALUE, ERROR_CODE, FIRST_CHANNEL, LAST_CHANNEL,
    LIFETIME, NONCE, PERMISSION_LIFETIME, REALM, REFRESH, REQUESTED_TRANSPORT, SEND, UDP, USERNAME,
    XOR_MAPPED_ADDRESS, XOR_PEER_ADDRESS, XOR_RELAYED_ADDRESS,
};

/// The realm the stand-in names.
pub const REALM_NAME: &str = "nexus.test";

/// What the stand-in has done so far.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Seen {
    /// Allocations made, and requests refused for their credential.
    pub allocations: u32,
    pub unauthorized: u32,
    /// Stale nonces said (438).
    pub stale: u32,
    /// Refreshes taken, and allocations released (a lifetime of 0).
    pub refreshes: u32,
    pub released: u32,
    /// The peer addresses permissions were installed for, and the channels bound, in order.
    pub permissions: Vec<IpAddr>,
    pub channels: Vec<(u16, SocketAddr)>,
    /// Datagrams relayed from a client to a peer: by Send indication, and by ChannelData.
    pub sent_by_indication: u32,
    pub sent_by_channel: u32,
    /// Datagrams relayed from a peer to a client: as a Data indication, and as ChannelData.
    pub delivered_by_indication: u32,
    pub delivered_by_channel: u32,
    /// Datagrams from a peer with no permission, dropped.
    pub unpermitted: u32,
    /// What was relayed to a client that is not STUN (a road's own datagrams), the newest 16.
    pub delivered: Vec<Vec<u8>>,
}

/// How the stand-in behaves, past the RFC's defaults.
#[derive(Clone, Copy, Debug)]
pub struct Behaviour {
    /// The allocation's lifetime, in seconds.
    pub lifetime: u32,
    /// Say the first authenticated request's nonce is stale, once.
    pub stale_once: bool,
    /// Relay nothing either way while set (the relay gone quiet).
    pub mute: bool,
}

impl Default for Behaviour {
    fn default() -> Self {
        Self {
            lifetime: 600,
            stale_once: false,
            mute: false,
        }
    }
}

/// What the stand-in does to a road's own datagrams (never a STUN message) as it relays them.
#[derive(Default)]
struct Rules {
    /// A client's datagrams of this kind are held back until released: each with its client and
    /// its peer.
    holding: Option<u8>,
    held: Vec<(SocketAddr, SocketAddr, Vec<u8>)>,
    releasing: bool,
    /// The next datagram of this kind to a client is lost.
    lose: Option<u8>,
    lost: usize,
}

/// A road datagram's kind, the byte after its version; `None` for a STUN message.
fn kind(payload: &[u8]) -> Option<u8> {
    (!super::is_stun(payload))
        .then(|| payload.get(1).copied())
        .flatten()
}

/// One allocation: its relayed socket, its permissions and channels, and when it lapses.
struct Allocation {
    relay: UdpSocket,
    permissions: HashMap<IpAddr, Instant>,
    channels: HashMap<u16, SocketAddr>,
    until: Instant,
}

impl Allocation {
    fn permits(&self, ip: IpAddr, now: Instant) -> bool {
        self.permissions.get(&ip).is_some_and(|until| now < *until)
    }
}

/// The stand-in, running on its own thread until dropped.
pub struct StandIn {
    /// Where clients reach it.
    pub address: SocketAddr,
    seen: Arc<Mutex<Seen>>,
    behaviour: Arc<Mutex<Behaviour>>,
    rules: Arc<Mutex<Rules>>,
    stopping: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl StandIn {
    /// A stand-in on loopback for `username` and `password`, with the RFC's defaults.
    pub fn start(username: &str, password: &str) -> Self {
        Self::start_with(username, password, Behaviour::default())
    }

    pub fn start_with(username: &str, password: &str, behaviour: Behaviour) -> Self {
        let socket = UdpSocket::bind("127.0.0.1:0").expect("a loopback socket");
        socket
            .set_read_timeout(Some(Duration::from_millis(2)))
            .expect("a read timeout");
        let address = socket.local_addr().expect("its address");
        let seen = Arc::new(Mutex::new(Seen::default()));
        let behaviour = Arc::new(Mutex::new(behaviour));
        let rules = Arc::new(Mutex::new(Rules::default()));
        let stopping = Arc::new(AtomicBool::new(false));
        let mut server = Server {
            socket,
            username: username.to_string(),
            key: long_term_key(username, REALM_NAME, password),
            nonce: 1,
            stale_said: false,
            allocations: HashMap::new(),
            seen: seen.clone(),
            behaviour: behaviour.clone(),
            rules: rules.clone(),
        };
        let theirs = stopping.clone();
        let thread = std::thread::Builder::new()
            .name("test-turn-stand-in".into())
            .spawn(move || server.run(&theirs))
            .expect("its thread");
        Self {
            address,
            seen,
            behaviour,
            rules,
            stopping,
            thread: Some(thread),
        }
    }

    /// What it has done so far.
    pub fn seen(&self) -> Seen {
        self.seen.lock().unwrap().clone()
    }

    /// Relay nothing, either way, while `mute` holds.
    pub fn mute(&self, mute: bool) {
        self.behaviour.lock().unwrap().mute = mute;
    }

    /// Hold back every road datagram of `kind` from a client, until [`StandIn::release`].
    pub fn hold(&self, kind: u8) {
        self.rules.lock().unwrap().holding = Some(kind);
    }

    /// The datagrams held back so far, in the order they came: for a test to send them some other
    /// way.
    pub fn held(&self) -> Vec<Vec<u8>> {
        let rules = self.rules.lock().unwrap();
        rules
            .held
            .iter()
            .map(|(_, _, payload)| payload.clone())
            .collect()
    }

    /// Let what was held back through, to the peers it was for, in the order it came, and hold
    /// nothing back from now.
    pub fn release(&self) {
        let mut rules = self.rules.lock().unwrap();
        rules.holding = None;
        rules.releasing = true;
    }

    /// Lose the next road datagram of `kind` to a client.
    pub fn lose_next(&self, kind: u8) {
        self.rules.lock().unwrap().lose = Some(kind);
    }

    /// How many road datagrams to a client were lost.
    pub fn lost(&self) -> usize {
        self.rules.lock().unwrap().lost
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Server {
    socket: UdpSocket,
    username: String,
    key: [u8; 16],
    nonce: u32,
    stale_said: bool,
    allocations: HashMap<SocketAddr, Allocation>,
    seen: Arc<Mutex<Seen>>,
    behaviour: Arc<Mutex<Behaviour>>,
    rules: Arc<Mutex<Rules>>,
}

impl Server {
    fn run(&mut self, stopping: &AtomicBool) {
        let mut buffer = [0u8; 2048];
        while !stopping.load(Ordering::SeqCst) {
            // An allocation past its lifetime is gone, its relayed address with it.
            let now = Instant::now();
            self.allocations
                .retain(|_, allocation| now < allocation.until);
            if let Ok((length, from)) = self.socket.recv_from(&mut buffer) {
                let packet = buffer[..length].to_vec();
                self.heard_from_client(&packet, from, Instant::now());
            }
            self.relay_from_peers(Instant::now());
            self.release_held();
        }
    }

    /// What was held back, once released: each to its peer, from its client's relayed socket.
    fn release_held(&mut self) {
        let held = {
            let mut rules = self.rules.lock().unwrap();
            if !rules.releasing {
                return;
            }
            rules.releasing = false;
            std::mem::take(&mut rules.held)
        };
        for (client, peer, payload) in held {
            if let Some(allocation) = self.allocations.get(&client) {
                let _ = allocation.relay.send_to(&payload, peer);
            }
        }
    }

    /// Whether `payload`, from `client` for `peer`, is held back, as the rules say: kept if so.
    fn held_back(&self, client: SocketAddr, peer: SocketAddr, payload: &[u8]) -> bool {
        let mut rules = self.rules.lock().unwrap();
        if rules.holding.is_none() || rules.holding != kind(payload) {
            return false;
        }
        rules.held.push((client, peer, payload.to_vec()));
        true
    }

    fn behaviour(&self) -> Behaviour {
        *self.behaviour.lock().unwrap()
    }

    fn nonce(&self) -> String {
        format!("nonce-{:08x}", self.nonce)
    }

    /// Each relayed socket's datagrams from its peers, to its client: dropped unless a permission
    /// for the peer's address is in place.
    fn relay_from_peers(&mut self, now: Instant) {
        let mute = self.behaviour().mute;
        let mut buffer = [0u8; 2048];
        let mut out = Vec::new();
        for (client, allocation) in &self.allocations {
            while let Ok((length, peer)) = allocation.relay.recv_from(&mut buffer) {
                let mut seen = self.seen.lock().unwrap();
                if !allocation.permits(peer.ip(), now) {
                    seen.unpermitted += 1;
                    continue;
                }
                if mute {
                    continue;
                }
                let payload = &buffer[..length];
                {
                    let mut rules = self.rules.lock().unwrap();
                    if rules.lose.is_some() && rules.lose == kind(payload) {
                        rules.lose = None;
                        rules.lost += 1;
                        continue;
                    }
                }
                if !super::is_stun(payload) {
                    if seen.delivered.len() >= 16 {
                        seen.delivered.remove(0);
                    }
                    seen.delivered.push(payload.to_vec());
                }
                let channel = allocation
                    .channels
                    .iter()
                    .find(|(_, bound)| **bound == peer)
                    .map(|(number, _)| *number);
                let datagram = match channel {
                    Some(number) => {
                        seen.delivered_by_channel += 1;
                        super::channel_data(number, payload)
                    }
                    None => {
                        seen.delivered_by_indication += 1;
                        super::transaction_id().map(|id| {
                            Writer::new(DATA, Class::Indication, &id)
                                .address(XOR_PEER_ADDRESS, peer)
                                .attribute(DATA_VALUE, payload)
                                .finish(None)
                        })
                    }
                };
                if let Some(datagram) = datagram {
                    out.push((*client, datagram));
                }
            }
        }
        for (client, datagram) in out {
            let _ = self.socket.send_to(&datagram, client);
        }
    }

    fn heard_from_client(&mut self, packet: &[u8], from: SocketAddr, now: Instant) {
        let mute = self.behaviour().mute;
        if super::is_channel_data(packet) {
            let number = u16::from_be_bytes([packet[0], packet[1]]);
            let length = usize::from(u16::from_be_bytes([packet[2], packet[3]]));
            let (Some(allocation), Some(payload)) =
                (self.allocations.get(&from), packet.get(4..4 + length))
            else {
                return;
            };
            if let Some(peer) = allocation.channels.get(&number) {
                if allocation.permits(peer.ip(), now)
                    && !mute
                    && !self.held_back(from, *peer, payload)
                {
                    let _ = allocation.relay.send_to(payload, *peer);
                    self.seen.lock().unwrap().sent_by_channel += 1;
                }
            }
            return;
        }
        let Some(message) = Message::read(packet) else {
            return;
        };
        if message.method == SEND && message.class == Class::Indication {
            let (Some(allocation), Some(peer), Some(payload)) = (
                self.allocations.get(&from),
                message.address(XOR_PEER_ADDRESS),
                message.get(DATA_VALUE),
            ) else {
                return;
            };
            if allocation.permits(peer.ip(), now) && !mute && !self.held_back(from, peer, payload) {
                let _ = allocation.relay.send_to(payload, peer);
                self.seen.lock().unwrap().sent_by_indication += 1;
            }
            return;
        }
        if message.class != Class::Request {
            return;
        }
        if let Some(answer) = self.authenticate(&message) {
            let _ = self.socket.send_to(&answer, from);
            return;
        }
        let answer = match message.method {
            ALLOCATE => self.allocate(&message, from, now),
            REFRESH => self.refresh(&message, from, now),
            CREATE_PERMISSION => self.permission(&message, from, now),
            CHANNEL_BIND => self.bind(&message, from, now),
            _ => None,
        };
        let answer = answer.unwrap_or_else(|| self.error(&message, 400, "Bad Request", false));
        let _ = self.socket.send_to(&answer, from);
    }

    /// The long-term credential's checks (RFC 8489 §9.2.4): an error to answer with, or `None` for
    /// a request that may go on.
    fn authenticate(&mut self, message: &Message) -> Option<Vec<u8>> {
        let signed = message.get(USERNAME).is_some() && message.integrity.is_some();
        if !signed {
            return Some(self.error(message, 401, "Unauthorized", true));
        }
        if message.text(USERNAME) != Some(self.username.as_str())
            || message.text(REALM) != Some(REALM_NAME)
            || !message.verifies(&self.key)
        {
            self.seen.lock().unwrap().unauthorized += 1;
            return Some(self.error(message, 401, "Unauthorized", true));
        }
        let stale = message.text(NONCE) != Some(self.nonce().as_str())
            || (self.behaviour().stale_once && !self.stale_said);
        if stale {
            if self.behaviour().stale_once && !self.stale_said {
                self.stale_said = true;
                self.nonce += 1;
            }
            self.seen.lock().unwrap().stale += 1;
            return Some(self.error(message, 438, "Stale Nonce", true));
        }
        None
    }

    /// An error answer, naming the realm and nonce when `challenge` asks for them. Unsigned, as
    /// the RFC has it for these.
    fn error(&self, message: &Message, code: u16, reason: &str, challenge: bool) -> Vec<u8> {
        let mut value = vec![0, 0, (code / 100) as u8, (code % 100) as u8];
        value.extend_from_slice(reason.as_bytes());
        let mut writer = Writer::new(message.method, Class::Error, &message.transaction)
            .attribute(ERROR_CODE, &value);
        if challenge {
            writer = writer
                .attribute(REALM, REALM_NAME.as_bytes())
                .attribute(NONCE, self.nonce().as_bytes());
        }
        writer.finish(None)
    }

    fn success(&self, message: &Message, writer: impl FnOnce(Writer) -> Writer) -> Vec<u8> {
        writer(Writer::new(
            message.method,
            Class::Success,
            &message.transaction,
        ))
        .finish(Some(&self.key))
    }

    fn allocate(&mut self, message: &Message, from: SocketAddr, now: Instant) -> Option<Vec<u8>> {
        if message.get(REQUESTED_TRANSPORT).and_then(|v| v.first()) != Some(&UDP) {
            return None;
        }
        let lifetime = self.behaviour().lifetime;
        if let std::collections::hash_map::Entry::Vacant(entry) = self.allocations.entry(from) {
            let relay = UdpSocket::bind("127.0.0.1:0").ok()?;
            relay.set_nonblocking(true).ok()?;
            entry.insert(Allocation {
                relay,
                permissions: HashMap::new(),
                channels: HashMap::new(),
                until: now + Duration::from_secs(u64::from(lifetime)),
            });
            self.seen.lock().unwrap().allocations += 1;
        }
        let relayed = self.allocations.get(&from)?.relay.local_addr().ok()?;
        Some(self.success(message, |w| {
            w.address(XOR_RELAYED_ADDRESS, relayed)
                .attribute(LIFETIME, &lifetime.to_be_bytes())
                .address(XOR_MAPPED_ADDRESS, from)
        }))
    }

    fn refresh(&mut self, message: &Message, from: SocketAddr, now: Instant) -> Option<Vec<u8>> {
        let asked = u32::from_be_bytes(message.get(LIFETIME)?.try_into().ok()?);
        if asked == 0 {
            self.allocations.remove(&from)?;
            self.seen.lock().unwrap().released += 1;
        } else {
            let allocation = self.allocations.get_mut(&from)?;
            allocation.until = now + Duration::from_secs(u64::from(asked));
            self.seen.lock().unwrap().refreshes += 1;
        }
        Some(self.success(message, |w| w.attribute(LIFETIME, &asked.to_be_bytes())))
    }

    fn permission(&mut self, message: &Message, from: SocketAddr, now: Instant) -> Option<Vec<u8>> {
        let peers: Vec<SocketAddr> = message
            .all(XOR_PEER_ADDRESS)
            .filter_map(|value| read_xor_address(value, &message.transaction))
            .collect();
        let allocation = self.allocations.get_mut(&from)?;
        if peers.is_empty() {
            return None;
        }
        for peer in &peers {
            allocation
                .permissions
                .insert(peer.ip(), now + PERMISSION_LIFETIME);
            self.seen.lock().unwrap().permissions.push(peer.ip());
        }
        Some(self.success(message, |w| w))
    }

    fn bind(&mut self, message: &Message, from: SocketAddr, now: Instant) -> Option<Vec<u8>> {
        let value = message.get(CHANNEL_NUMBER)?;
        let number = u16::from_be_bytes([*value.first()?, *value.get(1)?]);
        let peer = message.address(XOR_PEER_ADDRESS)?;
        if !(FIRST_CHANNEL..=LAST_CHANNEL).contains(&number) {
            return None;
        }
        let allocation = self.allocations.get_mut(&from)?;
        // As a real server (RFC 8656 §12.2): a number bound to another peer, or a peer bound
        // under another number, is refused. A binding here never lapses, so a client that asks
        // for a second number for a peer it already bound is always refused.
        if allocation
            .channels
            .iter()
            .any(|(bound, at)| (*bound == number) != (*at == peer))
        {
            return None;
        }
        allocation.channels.insert(number, peer);
        allocation
            .permissions
            .insert(peer.ip(), now + PERMISSION_LIFETIME);
        self.seen.lock().unwrap().channels.push((number, peer));
        Some(self.success(message, |w| w))
    }
}
