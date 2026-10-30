//! A road's way across the internet: one UDP socket of the road's own, ICE to find a pair of
//! addresses on which the two ends reach each other (`is`, the agent str0m itself runs), and at this
//! computer a relayed address on a TURN server ([`crate::turn`]) for when no direct pair does.
//! Stop's second road and the audio road each have their own (one socket per internet road, as
//! ruled), and on the LAN neither uses this: there each road is the plain socket it always was, and
//! the shack hears only its own subnet.
//!
//! ## How a road finds its pair
//!
//! - **Credentials and candidates cross inside the command link** ([`Offer`]), the TLS session only
//!   the two pinned ends hold, never on a relay lane: so nobody between them can steer the path or
//!   answer a check. The credentials are 64 and 128 bits from the operating system's randomness.
//! - **What each end offers** ([`Gather`]): its host address (the socket's own, or the one the
//!   route to the internet leaves by); its server-reflexive address, as a STUN server sees it
//!   (`tempo_stream::stun`, the binding request the station's stream already makes); and at this
//!   computer, its relayed address on the TURN server, which names the server-reflexive address too.
//!   Each goes to the other end as it is found ([`IceRoad::trickle`]).
//! - **This computer controls** and the shack is controlled (RFC 8445): this computer nominates,
//!   the shack follows what it nominates. Through the relay, each peer is given a permission and a
//!   channel before anything is sent to it.
//!
//! ## Where a road's checks go
//!
//! ⛔ **Only to an address of the internet** (`Reach`): ICE sends its checks to whatever the other
//! end's candidates name, and through the relay asks the TURN server to reach them, so each one is
//! held to the rule where it enters the agent (`State::candidate`), at both ends:
//! - never this computer (loopback, or unspecified: 0.0.0.0/8 and `::`), a group (multicast, or
//!   IPv4's broadcast) or a link's own address (link-local, 169.254/16 and fe80::/10, where a cloud's
//!   metadata service sits), nor the reserved 240/4, and in IPv6 nothing outside its global unicast
//!   space (2000::/3: never the old site-local fec0::/10, for one);
//! - never a private (RFC 1918) or unique-local (fc00::/7) address either, whatever kind of
//!   candidate the other end says it is: across a LAN the LAN road is the way, under its subnet rule;
//!   nor any other block that is a network's own, never the internet's: a carrier-grade NAT's shared
//!   100.64/10 (Tailscale's: over Tailscale, too, the LAN road is the way), the IETF's 192.0.0/24 and
//!   the benchmarking 198.18/15;
//! - an IPv6 address that carries an IPv4 one (mapped, compatible, NAT64, 6to4 or Teredo) is read as
//!   that IPv4 address, as the hosted stream reads it (`tempo_stream::session::reachable`), and port 0
//!   is refused;
//! - a road takes at most [`CANDIDATES`] in all, so a peer cannot have it probe a range.
//!
//! A refused candidate is dropped, and noted once in this build's own words, never echoed; the link
//! goes on, since a computer behind a NAT names its private host address as any end does. A check
//! from such an address is never answered, so it never becomes a peer-reflexive candidate either.
//! Only a test's road, bound to loopback, reaches loopback (and nothing else), in a test build.
//!
//! ## What is heard
//!
//! ⛔ **A road's datagram is heard only at the pair ICE selected** ([`Route`]): from that pair's
//! remote address, and arriving where that pair's local end is: on the socket itself for a direct
//! pair, or through the TURN server from that peer for a relayed one. Anything else that is not
//! ICE's or the TURN server's own (a check, an answer) is dropped unread, before a road's keys
//! are tried on it. Until a pair is selected nothing is heard, and nothing goes.
//!
//! ## What never waits
//!
//! [`IceRoad::send`] is what a Stop, a press and the audio go out by, on whatever thread asks. It
//! reads the selected pair (a copy, under a lock nothing holds for longer) and writes the datagram,
//! wrapped for the relay if the pair is relayed: it never waits for ICE, for TURN or for the road's
//! thread. The road's thread does everything else inside [`IceRoad::recv`]: the agent's checks and
//! timers, the TURN client's requests and refreshes, the STUN exchange, and the pair's changes. A Stop
//! goes by the same pair, the same way, as the press it stops.
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use is::stun::{StunMessage, StunPacket};
use is::{Candidate, IceAgent, IceAgentEvent, IceCreds, Protocol, Sha1HmacProvider};
use ring::hmac;
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};

use crate::turn::{self, Turn, Wrap};

/// The most candidates one end names for a road (and the most a road takes from the other end, in
/// all), and the longest a candidate's line may be: what one offer may make the command link carry.
pub const CANDIDATES: usize = 8;
pub const CANDIDATE_BYTES: usize = 256;
/// The longest a username fragment or password may be (RFC 8445 §5.3 asks at most 256 characters
/// of each).
const CREDENTIAL_BYTES: usize = 256;
/// A STUN binding to the STUN server: sent again at 0.5, 1.5 and 3.5 s, then given up.
const STUN_SENDS: u32 = 4;
const STUN_RTO: Duration = Duration::from_millis(500);

/// ICE's HMAC-SHA1 (its checks' MESSAGE-INTEGRITY), on ring, as the rest of the roads' crypto is.
#[derive(Debug)]
struct RingSha1;

impl Sha1HmacProvider for RingSha1 {
    fn sha1_hmac(&self, key: &[u8], payloads: &[&[u8]]) -> [u8; 20] {
        let key = hmac::Key::new(hmac::HMAC_SHA1_FOR_LEGACY_USE_ONLY, key);
        let mut context = hmac::Context::with_key(&key);
        for payload in payloads {
            context.update(payload);
        }
        let mut out = [0u8; 20];
        out.copy_from_slice(context.sign().as_ref());
        out
    }
}

static SHA1: RingSha1 = RingSha1;

/// One end's ICE credentials and candidates for a road, as the other end reads them off the command
/// link. Its `Debug` leaves the password out.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Offer {
    pub ufrag: String,
    pub pwd: String,
    pub candidates: Vec<String>,
}

impl std::fmt::Debug for Offer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Offer")
            .field("ufrag", &self.ufrag)
            .field("candidates", &self.candidates)
            .finish_non_exhaustive()
    }
}

impl Offer {
    /// Whether the offer is one an end of this build makes: credentials of ICE's own characters
    /// (RFC 8445 §5.3's ice-char) and lengths, and at most [`CANDIDATES`] candidates, each a
    /// candidate line no longer than [`CANDIDATE_BYTES`].
    pub fn shaped(&self) -> bool {
        let ice_chars = |text: &str, least: usize| {
            (least..=CREDENTIAL_BYTES).contains(&text.len())
                && text
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'+' || b == b'/')
        };
        ice_chars(&self.ufrag, 4)
            && ice_chars(&self.pwd, 22)
            && self.candidates.len() <= CANDIDATES
            && self.candidates.iter().all(|line| line_shaped(line))
    }
}

/// Whether `line` is a candidate line this build reads: `candidate:`, at most [`CANDIDATE_BYTES`],
/// UDP, and parsed by the agent.
fn line_shaped(line: &str) -> bool {
    line.len() <= CANDIDATE_BYTES
        && Candidate::from_sdp_string(line).is_ok_and(|c| c.proto() == Protocol::Udp)
}

/// ⛔ Where a road's checks may go, and whose checks it answers: the addresses the other end's
/// candidates may name, and the sources a check is read from. Read off the road's own host address.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Reach {
    /// An internet road, as every road of the app is: an address of the internet alone.
    Internet,
    /// A test's road on this computer's loopback: loopback alone. No road of the app is bound
    /// there, and outside a test build there is no such reach.
    #[cfg(any(test, feature = "test-util"))]
    Loopback,
}

impl Reach {
    /// The reach of a road whose own host address is `host`.
    #[cfg_attr(not(any(test, feature = "test-util")), allow(unused_variables))]
    fn of(host: SocketAddr) -> Self {
        #[cfg(any(test, feature = "test-util"))]
        if host.ip().to_canonical().is_loopback() {
            return Reach::Loopback;
        }
        Reach::Internet
    }

    /// Whether the road may send a check to `to`, or answer one from it: never at port 0, and an
    /// IPv4-mapped address as the IPv4 one.
    fn reaches(self, to: SocketAddr) -> bool {
        let ip = to.ip().to_canonical();
        to.port() != 0
            && match self {
                Reach::Internet => on_the_internet(ip),
                #[cfg(any(test, feature = "test-util"))]
                Reach::Loopback => ip.is_loopback(),
            }
    }
}

/// ⛔ Whether an internet road may probe `ip`: an address of the internet alone (the module header).
/// The hosted stream's reading of an address (`tempo_stream::session::reachable`), with unique-local
/// IPv6 refused and IPv4 held to [`internet_v4`], in every IPv6 form that carries an IPv4 address too.
fn on_the_internet(ip: IpAddr) -> bool {
    tempo_stream::session::reachable(ip, internet_v4, false)
}

/// ⛔ The IPv4 addresses an internet road may probe: those a viewer of the stream could be at
/// (`tempo_stream::session::viewer_v4`: unicast, not loopback or link-local), less every block that is
/// a network's own and never the internet's: a private network (RFC 1918), a carrier-grade NAT's
/// shared space (100.64/10, where Tailscale numbers its machines: over Tailscale the LAN road is the
/// way), the IETF's protocol assignments (192.0.0/24) and the benchmarking block (198.18/15).
fn internet_v4(ip: Ipv4Addr) -> bool {
    let o = ip.octets();
    tempo_stream::session::viewer_v4(ip)
        && !ip.is_private()
        && !(o[0] == 100 && o[1] & 0xc0 == 64)
        && !(o[0] == 192 && o[1] == 0 && o[2] == 0)
        && !(o[0] == 198 && o[1] & 0xfe == 18)
}

/// A TURN server and the credential for it. Never logged, shown or sent but to that server, under
/// the long-term mechanism: no `Debug`.
#[derive(Clone)]
pub struct Relay {
    pub server: SocketAddr,
    pub username: String,
    pub password: String,
}

/// What a road gathers: its direct addresses, host and server-reflexive (left out to force the
/// relay), a STUN server to learn the server-reflexive one from, and a TURN server for a relayed
/// one.
pub struct Gather {
    pub direct: bool,
    pub stun: Option<SocketAddr>,
    pub relay: Option<Relay>,
}

/// Which end of a road this is: this computer controls ICE (it nominates), the shack is
/// controlled.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    Computer,
    Shack,
}

/// The pair ICE selected, as the road's senders read it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    /// Straight to the other end's address.
    Direct(SocketAddr),
    /// Through the TURN server at `server`, to `peer`, wrapped as `wrap` says.
    Relayed {
        server: SocketAddr,
        peer: SocketAddr,
        wrap: Wrap,
    },
}

/// A STUN binding outstanding at the STUN server.
struct Asking {
    server: SocketAddr,
    id: [u8; 12],
    sends: u32,
    next: Instant,
}

/// What the road's thread keeps: the agent, the TURN client, the STUN binding, what has been
/// gathered and told, and the pair the agent nominated.
struct State {
    agent: IceAgent,
    relay: Option<Turn>,
    /// The relayed candidate's address, once it is one of this end's candidates.
    relayed: Option<SocketAddr>,
    stun: Option<Asking>,
    /// Whether this end offers its direct addresses, and whether a server-reflexive one has been
    /// added.
    direct: bool,
    reflexive: bool,
    /// This end's own candidates, in the order found, and how many have been told.
    gathered: Vec<String>,
    told: usize,
    /// What the other end's candidates may name, how many of them were taken, and how many were
    /// refused (the first is noted).
    reach: Reach,
    taken: usize,
    refused: usize,
    /// The remote candidates' addresses: each is a peer the relay must permit.
    peers: Vec<SocketAddr>,
    /// The pair the agent last nominated: this end's address on it and the other end's.
    nominated: Option<(SocketAddr, SocketAddr)>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// ICE credentials from the operating system's randomness: a 64-bit username fragment and a
/// 128-bit password, as lowercase hex (ice-chars both). The agent's own would come from a
/// generator that is not cryptographic.
fn credentials() -> Option<IceCreds> {
    let random = SystemRandom::new();
    let mut ufrag = [0u8; 8];
    let mut pass = [0u8; 16];
    random.fill(&mut ufrag).ok()?;
    random.fill(&mut pass).ok()?;
    Some(IceCreds {
        ufrag: crate::hex(&ufrag),
        pass: crate::hex(&pass),
    })
}

/// The address the route to the internet leaves by: a UDP socket "connected" to a documentation
/// address (RFC 5737) learns its interface. Nothing is sent, and no name is looked up.
fn route_address() -> Option<IpAddr> {
    let probe = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    probe.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    Some(probe.local_addr().ok()?.ip())
}

/// Whether a road's datagrams under `tag` could read as a STUN message on an internet road's
/// socket: their bytes 4 to 8 are the tag's 2 to 6, and a STUN message has the magic cookie there
/// (RFC 7983's first byte, from 0 to 3, a datagram's version already meets). The shack never gives
/// out such a tag.
pub fn reads_as_stun(tag: &crate::datagram::Tag) -> bool {
    tag[2..6] == 0x2112_A442u32.to_be_bytes()
}

/// What an internet road's ends must agree on, for the build's contract: the labels its keys are
/// exported under, and how much of ICE one offer may carry.
pub fn contract_text() -> String {
    format!(
        "internet roads {} {} candidates {CANDIDATES} of {CANDIDATE_BYTES} credentials \
         {CREDENTIAL_BYTES} open.roads opened.roads candidate\n",
        String::from_utf8_lossy(crate::datagram::INTERNET_STOP_LABEL),
        String::from_utf8_lossy(crate::datagram::INTERNET_AUDIO_LABEL),
    )
}

/// How one road's datagrams cross: on the LAN, the road's own socket, connected to the other end's
/// road (so only that address is heard); on the internet, its ICE road. Both read with the road's
/// tick and send on whatever thread asks, so a road reads and writes either the same way.
pub enum Path {
    Lan(UdpSocket),
    Internet(std::sync::Arc<IceRoad>),
}

impl Path {
    /// One datagram to the other end, now: whether it went.
    pub fn send(&self, datagram: &[u8]) -> bool {
        match self {
            Path::Lan(socket) => socket.send(datagram).is_ok(),
            Path::Internet(ice) => ice.send(datagram),
        }
    }

    /// The next datagram from the other end, within the road's tick.
    pub fn recv(&self, buffer: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Path::Lan(socket) => socket.recv(buffer),
            Path::Internet(ice) => ice.recv(buffer),
        }
    }
}

/// One road's socket, its ICE agent and, at this computer, its TURN client.
pub struct IceRoad {
    socket: UdpSocket,
    /// This end's host address: the socket's own, or the route's interface when it is bound to
    /// every interface.
    host: SocketAddr,
    state: Mutex<State>,
    route: Mutex<Option<Route>>,
}

impl IceRoad {
    /// A road's own socket at `at`, read with a timeout of `tick`, with its ICE agent for `role` and
    /// what `gather` asks for: the host candidate at once and the server-reflexive one as the STUN
    /// server answers (unless the direct addresses are left out), the relayed one as the TURN server
    /// allocates it. `None` if no socket, address or randomness could
    /// be had.
    pub fn bind(at: SocketAddr, role: Role, gather: Gather, tick: Duration) -> Option<Self> {
        let socket = UdpSocket::bind(at).ok()?;
        socket.set_read_timeout(Some(tick)).ok()?;
        let mut host = socket.local_addr().ok()?;
        if host.ip().is_unspecified() {
            host.set_ip(route_address()?);
        }
        let mut agent = IceAgent::with_hmac(credentials()?, &SHA1);
        agent.set_controlling(role == Role::Computer);
        let mut gathered = Vec::new();
        if gather.direct {
            let candidate = Candidate::host(host, "udp").ok()?;
            gathered.push(agent.add_local_candidate(candidate)?.to_sdp_string());
        }
        let now = Instant::now();
        let relay = gather
            .relay
            .and_then(|relay| Turn::new(relay.server, &relay.username, &relay.password, now));
        let stun = match gather.stun {
            Some(server) => Some(Asking {
                server,
                id: turn::transaction_id()?,
                sends: 0,
                next: now,
            }),
            None => None,
        };
        Some(Self {
            socket,
            host,
            state: Mutex::new(State {
                agent,
                relay,
                relayed: None,
                stun,
                direct: gather.direct,
                reflexive: false,
                gathered,
                told: 0,
                reach: Reach::of(host),
                taken: 0,
                refused: 0,
                peers: Vec::new(),
                nominated: None,
            }),
            route: Mutex::new(None),
        })
    }

    /// This end's host address.
    pub fn host(&self) -> SocketAddr {
        self.host
    }

    /// A send the socket will not take within `wait` is dropped, not waited for: the audio road's
    /// rule, for which late audio is worth nothing. `false` if the socket would not take it.
    pub fn write_timeout(&self, wait: Option<Duration>) -> bool {
        self.socket.set_write_timeout(wait).is_ok()
    }

    /// This end's credentials and every candidate found so far: the road's offer, for the link.
    pub fn offer(&self) -> Offer {
        let mut state = lock(&self.state);
        state.told = state.gathered.len();
        let credentials = state.agent.local_credentials().clone();
        Offer {
            ufrag: credentials.ufrag,
            pwd: credentials.pass,
            candidates: state.gathered.clone(),
        }
    }

    /// The candidates found since the offer or the last trickle, for the link.
    pub fn trickle(&self) -> Vec<String> {
        let mut state = lock(&self.state);
        let new = state.gathered[state.told..].to_vec();
        state.told = state.gathered.len();
        new
    }

    /// The other end's offer: its credentials, and its candidates, each paired with this end's if the
    /// road may take it (`State::candidate`). `false`, and nothing taken, for an offer no end of
    /// this build makes.
    pub fn answer(&self, theirs: &Offer) -> bool {
        if !theirs.shaped() {
            return false;
        }
        let mut state = lock(&self.state);
        state.agent.set_remote_credentials(IceCreds {
            ufrag: theirs.ufrag.clone(),
            pass: theirs.pwd.clone(),
        });
        for line in &theirs.candidates {
            state.candidate(line, Instant::now());
        }
        true
    }

    /// One more of the other end's candidates, trickled. `false` for a line this build does not
    /// read; one it reads but will not probe is dropped, and `true` (`State::candidate`).
    pub fn add_candidate(&self, line: &str) -> bool {
        line_shaped(line) && lock(&self.state).candidate(line, Instant::now())
    }

    /// The pair ICE selected, if it has.
    pub fn route(&self) -> Option<Route> {
        *lock(&self.route)
    }

    /// ⛔ One road datagram, sent now on the selected pair, from the caller's own thread: never
    /// waits for ICE, TURN or the road's thread. `false` while no pair is selected, or if the socket
    /// would not take it.
    pub fn send(&self, datagram: &[u8]) -> bool {
        let route = *lock(&self.route);
        match route {
            Some(Route::Direct(to)) => self.socket.send_to(datagram, to).is_ok(),
            Some(Route::Relayed { server, peer, wrap }) => wrap
                .wrap(peer, datagram)
                .is_some_and(|wrapped| self.socket.send_to(&wrapped, server).is_ok()),
            None => false,
        }
    }

    /// The road's thread's read: waits up to the road's tick for one packet; reads what is ICE's,
    /// TURN's or the STUN server's; then keeps ICE's, TURN's and STUN's time, sends what they ask
    /// and follows the pair they select. The length of a road datagram heard at the selected pair,
    /// placed at the start of `buffer`; `WouldBlock` for anything else, or nothing; the socket's own
    /// error once it is one that ends the road (`tempo_stream::session::receive_ends_session`).
    pub fn recv(&self, buffer: &mut [u8]) -> std::io::Result<usize> {
        let received = self.socket.recv_from(buffer);
        let now = Instant::now();
        let mut state = lock(&self.state);
        let heard = match received {
            Ok((length, from)) => state.take(buffer, length, from, self.host, now),
            Err(error) if tempo_stream::session::receive_ends_session(&error) => {
                return Err(error);
            }
            Err(_) => None,
        };
        let route = state.drive(&self.socket, self.host, now);
        *lock(&self.route) = route;
        drop(state);
        heard.ok_or_else(|| std::io::ErrorKind::WouldBlock.into())
    }
}

impl Drop for IceRoad {
    /// The road is going: its relayed address is given back (one Refresh with a lifetime of 0, not
    /// waited for: the allocation lapses on its own anyway).
    fn drop(&mut self) {
        let mut state = lock(&self.state);
        if let Some(relay) = state.relay.as_mut() {
            relay.release(Instant::now());
            let server = relay.server();
            while let Some(datagram) = relay.poll_transmit() {
                let _ = self.socket.send_to(&datagram, server);
            }
        }
    }
}

impl State {
    /// ⛔ One of the other end's candidates, paired with this end's, and permitted at the relay, if
    /// the road may reach its address and has taken fewer than [`CANDIDATES`]; otherwise dropped
    /// ([`State::refuse`]). `false` for a line this build does not read.
    fn candidate(&mut self, line: &str, now: Instant) -> bool {
        if line.len() > CANDIDATE_BYTES {
            return false;
        }
        let Ok(candidate) = Candidate::from_sdp_string(line) else {
            return false;
        };
        if candidate.proto() != Protocol::Udp {
            return false;
        }
        let peer = candidate.addr();
        if !self.reach.reaches(peer) || self.taken >= CANDIDATES {
            self.refuse();
            return true;
        }
        self.taken += 1;
        self.agent.add_remote_candidate(candidate);
        if !self.peers.contains(&peer) {
            self.peers.push(peer);
        }
        if let Some(relay) = self.relay.as_mut() {
            relay.permit(peer, now);
        }
        true
    }

    /// A candidate the road will not take, dropped: noted once for the road, in this build's own
    /// words (nothing the other end sent is ever written).
    fn refuse(&mut self) {
        self.refused += 1;
        if self.refused == 1 {
            tempo_base::applog::warn(
                "remote",
                "an internet road refused a candidate: an address no internet road may reach, \
                 or one more than a road takes",
            );
        }
    }

    /// One packet off the socket, from `from`: the TURN server's, the STUN server's, a check or its
    /// answer for the agent, or a road datagram. A road datagram heard at the selected pair is left
    /// at the start of `buffer` and its length given; everything else is read here, or dropped.
    fn take(
        &mut self,
        buffer: &mut [u8],
        length: usize,
        from: SocketAddr,
        host: SocketAddr,
        now: Instant,
    ) -> Option<usize> {
        if let Some(relay) = self.relay.as_mut() {
            if from == relay.server() {
                let (peer, payload) = relay.handle(&buffer[..length], now)?;
                let relayed = self.relayed?;
                let n = payload.len().min(buffer.len());
                buffer[..n].copy_from_slice(&payload[..n]);
                return self.inbound(buffer, n, peer, relayed, now);
            }
        }
        if let Some(asking) = &self.stun {
            if from == asking.server && turn::is_stun(&buffer[..length]) {
                if let Some(mapped) = tempo_stream::stun::mapped(&buffer[..length], &asking.id) {
                    self.stun = None;
                    self.reflexive(mapped, host);
                }
                return None;
            }
        }
        self.inbound(buffer, length, from, host, now)
    }

    /// A packet from `from` that arrived at this end's `local` address (the socket's own, or the
    /// relayed one when it came through the TURN server): a check or answer to the agent, or a road
    /// datagram heard only at the selected pair.
    fn inbound(
        &mut self,
        buffer: &mut [u8],
        length: usize,
        from: SocketAddr,
        local: SocketAddr,
        now: Instant,
    ) -> Option<usize> {
        // ⛔ Nothing from an address the road may not reach is read: a check from one is never
        // answered, so it never becomes a peer-reflexive candidate the agent would probe.
        if !self.reach.reaches(from) {
            return None;
        }
        if turn::is_stun(&buffer[..length]) {
            // The agent takes only a check under this road's credentials, or the answer to one of
            // its own (`IceAgent::accepts_message`, asked inside).
            if let Ok(message) = StunMessage::parse(&buffer[..length]) {
                self.agent.handle_packet(
                    now,
                    StunPacket {
                        proto: Protocol::Udp,
                        source: from,
                        destination: local,
                        message,
                    },
                );
            }
            return None;
        }
        // ⛔ A road datagram: heard only from the selected pair's other end, arriving where this
        // end's address on that pair is (through the relay for a relayed pair, on the socket itself
        // for a direct one). Anything else is dropped here, unread.
        let (ours, theirs) = self.nominated?;
        (theirs == from && ours == local).then_some(length)
    }

    /// The server-reflexive address `mapped`, as one of this end's candidates, based on `host`.
    fn reflexive(&mut self, mapped: SocketAddr, host: SocketAddr) {
        if !self.direct || self.reflexive || mapped == host || mapped.is_ipv4() != host.is_ipv4() {
            return;
        }
        if let Ok(candidate) = Candidate::server_reflexive(mapped, host, "udp") {
            if let Some(added) = self.agent.add_local_candidate(candidate) {
                self.gathered.push(added.to_sdp_string());
                self.reflexive = true;
            }
        }
    }

    /// Time, for the agent, the TURN client and the STUN binding; what they ask to send, sent; and
    /// the pair the agent selects, as the road's senders will read it.
    fn drive(&mut self, socket: &UdpSocket, host: SocketAddr, now: Instant) -> Option<Route> {
        // The STUN binding, sent again while unanswered, then given up.
        if let Some(asking) = self.stun.as_mut() {
            if now >= asking.next {
                if asking.sends >= STUN_SENDS {
                    self.stun = None;
                } else {
                    let _ = socket.send_to(&tempo_stream::stun::request(&asking.id), asking.server);
                    asking.next = now + STUN_RTO * (1 << asking.sends);
                    asking.sends += 1;
                }
            }
        }
        // The relay: its time, its relayed address as a candidate once allocated (and where it saw
        // this end, as a server-reflexive one), and a permission for every peer.
        if let Some(relay) = self.relay.as_mut() {
            relay.handle_timeout(now);
        }
        let allocated = self
            .relay
            .as_ref()
            .and_then(|relay| Some((relay.relayed()?, relay.mapped())));
        if let (None, Some((relayed, mapped))) = (self.relayed, allocated) {
            if let Ok(candidate) = Candidate::relayed(relayed, host, "udp") {
                if let Some(added) = self.agent.add_local_candidate(candidate) {
                    self.gathered.push(added.to_sdp_string());
                    self.relayed = Some(relayed);
                }
            }
            if let Some(mapped) = mapped {
                self.reflexive(mapped, host);
            }
            if let Some(relay) = self.relay.as_mut() {
                for peer in &self.peers {
                    relay.permit(*peer, now);
                }
            }
        }
        // The agent's own time. It asks to be woken again (its `poll_timeout`), and is woken at
        // least every tick of the road's thread, which reads with that timeout.
        if self.agent.poll_timeout().is_none_or(|at| at <= now) {
            self.agent.handle_timeout(now);
        }
        while let Some(transmit) = self.agent.poll_transmit() {
            if Some(transmit.source) == self.relayed {
                if let Some(relay) = self.relay.as_mut() {
                    relay.send(transmit.destination, &transmit.contents, now);
                }
            } else {
                let _ = socket.send_to(&transmit.contents, transmit.destination);
            }
        }
        if let Some(relay) = self.relay.as_mut() {
            let server = relay.server();
            while let Some(datagram) = relay.poll_transmit() {
                let _ = socket.send_to(&datagram, server);
            }
        }
        while let Some(event) = self.agent.poll_event() {
            if let IceAgentEvent::NominatedSend {
                source,
                destination,
                ..
            } = event
            {
                self.nominated = Some((source, destination));
            }
        }
        let (ours, theirs) = self.nominated?;
        if self.relayed == Some(ours) {
            let relay = self.relay.as_ref()?;
            Some(Route::Relayed {
                server: relay.server(),
                peer: theirs,
                wrap: relay.wrap_for(theirs, now)?,
            })
        } else {
            Some(Route::Direct(theirs))
        }
    }
}

/// A socket that counts what reaches it, named as one of the other end's host candidates: for the
/// tests that show where a road's checks go, at this computer and at the shack.
#[cfg(any(test, feature = "test-util"))]
pub struct Counter {
    socket: UdpSocket,
    heard: std::sync::atomic::AtomicUsize,
}

#[cfg(any(test, feature = "test-util"))]
impl Counter {
    /// A counter at a port of its own on `ip`.
    pub fn on(ip: IpAddr) -> Self {
        let socket = UdpSocket::bind(SocketAddr::new(ip, 0)).expect("a counter's socket");
        socket
            .set_nonblocking(true)
            .expect("a counter's socket never waits");
        Self {
            socket,
            heard: Default::default(),
        }
    }

    /// Where it listens.
    pub fn at(&self) -> SocketAddr {
        self.socket.local_addr().expect("a counter's address")
    }

    /// The host candidate line that names it.
    pub fn line(&self) -> String {
        let at = self.at();
        format!(
            "candidate:1 1 udp 2130706431 {} {} typ host",
            at.ip(),
            at.port()
        )
    }

    /// How many datagrams have reached it so far.
    pub fn heard(&self) -> usize {
        use std::sync::atomic::Ordering::Relaxed;
        let mut buffer = [0u8; 2048];
        while self.socket.recv_from(&mut buffer).is_ok() {
            self.heard.fetch_add(1, Relaxed);
        }
        self.heard.load(Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::turn::stand_in::StandIn;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{mpsc, Arc};

    const TICK: Duration = Duration::from_millis(5);
    /// A road datagram's shape: the datagram layer's version first, and no magic cookie.
    const DATAGRAM: &[u8] = &[
        1, 7, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xa8, 0, 0, 0, 1,
    ];

    fn loopback() -> SocketAddr {
        "127.0.0.1:0".parse().unwrap()
    }

    fn host_only() -> Gather {
        Gather {
            direct: true,
            stun: None,
            relay: None,
        }
    }

    /// A road read by a thread of its own, as a road's thread reads it, with what it heard.
    struct Driven {
        road: Arc<IceRoad>,
        heard: mpsc::Receiver<Vec<u8>>,
        stopping: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Driven {
        fn new(road: IceRoad) -> Self {
            let road = Arc::new(road);
            let (tell, heard) = mpsc::channel();
            let stopping = Arc::new(AtomicBool::new(false));
            let (theirs, stop) = (road.clone(), stopping.clone());
            let thread = std::thread::spawn(move || {
                let mut buffer = [0u8; 2048];
                while !stop.load(Ordering::SeqCst) {
                    if let Ok(length) = theirs.recv(&mut buffer) {
                        let _ = tell.send(buffer[..length].to_vec());
                    }
                }
            });
            Self {
                road,
                heard,
                stopping,
                thread: Some(thread),
            }
        }

        fn heard_within(&self, wait: Duration) -> Option<Vec<u8>> {
            self.heard.recv_timeout(wait).ok()
        }
    }

    impl Drop for Driven {
        fn drop(&mut self) {
            self.stopping.store(true, Ordering::SeqCst);
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// Waits up to five seconds for `done`, passing trickled candidates both ways meanwhile, as
    /// the link would.
    fn until(computer: &Driven, shack: &Driven, done: impl Fn() -> bool) -> bool {
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(5) {
            for line in computer.road.trickle() {
                shack.road.add_candidate(&line);
            }
            for line in shack.road.trickle() {
                computer.road.add_candidate(&line);
            }
            if done() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        false
    }

    /// Both ends, each given the other's offer as the link carries it.
    fn paired(computer: Gather, shack: Gather) -> (Driven, Driven) {
        let computer = IceRoad::bind(loopback(), Role::Computer, computer, TICK).unwrap();
        let shack = IceRoad::bind(loopback(), Role::Shack, shack, TICK).unwrap();
        let (to_shack, to_computer) = (computer.offer(), shack.offer());
        assert!(shack.answer(&to_shack) && computer.answer(&to_computer));
        (Driven::new(computer), Driven::new(shack))
    }

    /// ★ Two agents over loopback find each other: this computer nominates, the shack follows, and
    /// each end's pair is the other's host address, by value. A road datagram then goes each way and
    /// is heard as it was sent.
    #[test]
    fn two_agents_over_loopback_select_a_pair_and_carry_a_datagram() {
        let (computer, shack) = paired(host_only(), host_only());
        let (at_computer, at_shack) = (computer.road.host(), shack.road.host());
        assert!(until(&computer, &shack, || computer.road.route().is_some()
            && shack.road.route().is_some()));
        assert_eq!(computer.road.route(), Some(Route::Direct(at_shack)));
        assert_eq!(shack.road.route(), Some(Route::Direct(at_computer)));

        assert!(computer.road.send(DATAGRAM));
        assert_eq!(
            shack.heard_within(Duration::from_secs(2)).as_deref(),
            Some(DATAGRAM)
        );
        assert!(shack.road.send(b"\x01back"));
        assert_eq!(
            computer.heard_within(Duration::from_secs(2)).as_deref(),
            Some(&b"\x01back"[..])
        );
    }

    /// ★ A relayed-only path, through the TURN stand-in: this computer offers only its relayed
    /// address, so every check and every datagram goes through the relay. This computer's pair is
    /// the relay to the shack's host address, by its channel; the shack's is this computer's relayed
    /// address. Datagrams go both ways, and the stand-in counts them through its channel; the
    /// shack's own address sending straight to this computer, past the relay, is not heard; and the
    /// road gives its relayed address back as it goes. CONTROL: with the relay quiet, nothing
    /// arrives.
    #[test]
    fn a_relayed_only_path_goes_through_the_turn_stand_in() {
        let turn = StandIn::start("road", "throwaway-password");
        let (computer, shack) = paired(
            Gather {
                direct: false,
                stun: None,
                relay: Some(Relay {
                    server: turn.address,
                    username: "road".into(),
                    password: "throwaway-password".into(),
                }),
            },
            host_only(),
        );
        let at_shack = shack.road.host();
        assert!(until(&computer, &shack, || matches!(
            computer.road.route(),
            Some(Route::Relayed {
                wrap: Wrap::Channel(_),
                ..
            })
        ) && shack
            .road
            .route()
            .is_some()));
        let Some(Route::Relayed { server, peer, wrap }) = computer.road.route() else {
            unreachable!()
        };
        assert_eq!(
            (server, peer, wrap),
            (turn.address, at_shack, Wrap::Channel(0x4000))
        );
        let Some(Route::Direct(relayed)) = shack.road.route() else {
            panic!("the shack's pair: {:?}", shack.road.route());
        };
        assert_ne!(
            relayed,
            computer.road.host(),
            "the shack's pair is the relay"
        );
        assert_eq!(turn.seen().allocations, 1);

        let before = turn.seen();
        assert!(computer.road.send(DATAGRAM));
        assert_eq!(
            shack.heard_within(Duration::from_secs(2)).as_deref(),
            Some(DATAGRAM)
        );
        assert!(shack.road.send(b"\x01back"));
        assert_eq!(
            computer.heard_within(Duration::from_secs(2)).as_deref(),
            Some(&b"\x01back"[..])
        );
        let after = turn.seen();
        assert!(after.sent_by_channel > before.sent_by_channel);
        assert!(after.delivered_by_channel > before.delivered_by_channel);

        // The shack's own address, straight to this computer's socket, past the relay: not the
        // selected pair (that one is through the relay), so it is dropped unread.
        shack
            .road
            .socket
            .send_to(DATAGRAM, computer.road.host())
            .unwrap();
        assert_eq!(computer.heard_within(Duration::from_millis(300)), None);

        // CONTROL: the relay quiet, and nothing gets through.
        turn.mute(true);
        assert!(computer.road.send(DATAGRAM));
        assert_eq!(shack.heard_within(Duration::from_millis(300)), None);
        turn.mute(false);
        assert!(computer.road.send(DATAGRAM));
        assert_eq!(
            shack.heard_within(Duration::from_secs(2)).as_deref(),
            Some(DATAGRAM)
        );

        // And a road that goes gives its relayed address back.
        assert_eq!(turn.seen().released, 0);
        drop(computer);
        let started = Instant::now();
        while turn.seen().released == 0 && started.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(turn.seen().released, 1);
    }

    /// ★ With a direct pair selected, a datagram that comes through the relay, though from the
    /// other end's own address, is not the selected pair and is dropped unread: this computer offers
    /// its host and relayed addresses, ICE picks the direct pair, and the shack's socket sending to
    /// the relayed address is not heard, though the relay delivered it. CONTROL: the same bytes on
    /// the selected pair are.
    #[test]
    fn a_datagram_through_the_relay_is_not_heard_while_the_pair_is_direct() {
        let turn = StandIn::start("road", "throwaway-password");
        let (computer, shack) = paired(
            Gather {
                direct: true,
                stun: None,
                relay: Some(Relay {
                    server: turn.address,
                    username: "road".into(),
                    password: "throwaway-password".into(),
                }),
            },
            host_only(),
        );
        let relayed = || lock(&computer.road.state).relayed;
        assert!(until(&computer, &shack, || relayed().is_some()
            && !turn.seen().channels.is_empty()
            && computer.road.route().is_some()
            && shack.road.route().is_some()));
        assert_eq!(
            computer.road.route(),
            Some(Route::Direct(shack.road.host()))
        );
        assert!(turn.seen().delivered.is_empty());
        shack
            .road
            .socket
            .send_to(DATAGRAM, relayed().unwrap())
            .unwrap();
        assert_eq!(computer.heard_within(Duration::from_millis(300)), None);
        assert_eq!(
            turn.seen().delivered,
            vec![DATAGRAM.to_vec()],
            "the relay delivered it"
        );
        // CONTROL.
        assert!(shack.road.send(DATAGRAM));
        assert_eq!(
            computer.heard_within(Duration::from_secs(2)).as_deref(),
            Some(DATAGRAM)
        );
    }

    /// ★ A datagram from an address ICE did not select is dropped unread: the same bytes from a
    /// stranger's socket are never heard, from the selected pair they are. And before any pair is
    /// selected, nothing is heard and nothing goes.
    #[test]
    fn a_datagram_from_an_address_ice_did_not_select_is_dropped_unread() {
        let early = IceRoad::bind(loopback(), Role::Shack, host_only(), TICK).unwrap();
        assert!(!early.send(DATAGRAM), "sent before a pair was selected");
        let stranger = UdpSocket::bind(loopback()).unwrap();
        stranger.send_to(DATAGRAM, early.host()).unwrap();
        let mut buffer = [0u8; 64];
        for _ in 0..20 {
            assert!(
                early.recv(&mut buffer).is_err(),
                "heard before a pair was selected"
            );
        }

        let (computer, shack) = paired(host_only(), host_only());
        assert!(until(&computer, &shack, || computer.road.route().is_some()
            && shack.road.route().is_some()));
        stranger.send_to(DATAGRAM, shack.road.host()).unwrap();
        assert_eq!(
            shack.heard_within(Duration::from_millis(300)),
            None,
            "a stranger was heard"
        );
        // CONTROL: the same bytes from the selected pair.
        assert!(computer.road.send(DATAGRAM));
        assert_eq!(
            shack.heard_within(Duration::from_secs(2)).as_deref(),
            Some(DATAGRAM)
        );
    }

    /// ★ A send never waits for the road's thread: with the agent's state held, as while the road's
    /// thread is in the middle of ICE's or TURN's work, a datagram still goes, and is heard.
    #[test]
    fn a_send_never_waits_for_the_roads_thread() {
        let (computer, shack) = paired(host_only(), host_only());
        assert!(until(&computer, &shack, || computer.road.route().is_some()
            && shack.road.route().is_some()));
        let held = lock(&computer.road.state);
        let (road, (done, sent)) = (computer.road.clone(), mpsc::channel());
        std::thread::spawn(move || {
            let _ = done.send(road.send(DATAGRAM));
        });
        assert_eq!(
            sent.recv_timeout(Duration::from_secs(1)),
            Ok(true),
            "the send waited for the road's thread"
        );
        drop(held);
        assert_eq!(
            shack.heard_within(Duration::from_secs(2)).as_deref(),
            Some(DATAGRAM)
        );
    }

    /// ★ This end's server-reflexive address, as a STUN server sees it (`tempo_stream::stun`'s
    /// binding request, answered by a stand-in that says 203.0.113.7:61000), is one of its
    /// candidates, its base never named.
    #[test]
    fn the_server_reflexive_address_comes_from_the_stun_server() {
        let stun = UdpSocket::bind(loopback()).unwrap();
        let stun_at = stun.local_addr().unwrap();
        let answered = std::thread::spawn(move || {
            let mut buffer = [0u8; 512];
            let (length, from) = stun.recv_from(&mut buffer).unwrap();
            let id: [u8; 12] = buffer[8..20].try_into().unwrap();
            assert_eq!(&buffer[..2], &[0, 1], "a binding request");
            assert_eq!(length, 20);
            let answer = turn::Writer::new(0x001, turn::Class::Success, &id)
                .address(0x0020, "203.0.113.7:61000".parse().unwrap())
                .finish(None);
            stun.send_to(&answer, from).unwrap();
        });
        let road = Driven::new(
            IceRoad::bind(
                loopback(),
                Role::Shack,
                Gather {
                    direct: true,
                    stun: Some(stun_at),
                    relay: None,
                },
                TICK,
            )
            .unwrap(),
        );
        let host = road.road.offer().candidates;
        assert_eq!(host.len(), 1);
        let started = Instant::now();
        let mut found = Vec::new();
        while found.is_empty() && started.elapsed() < Duration::from_secs(3) {
            found = road.road.trickle();
            std::thread::sleep(Duration::from_millis(5));
        }
        answered.join().unwrap();
        assert_eq!(found.len(), 1, "{found:?}");
        assert!(
            found[0].contains(" 203.0.113.7 61000 typ srflx raddr 0.0.0.0 rport 0"),
            "{}",
            found[0]
        );
    }

    /// ★ An offer crosses the link as JSON and back; one shaped as no end of this build makes it is
    /// refused whole: too many candidates, one not UDP, a short password, a credential outside
    /// ICE's characters, an unknown field. The credentials are 64 and 128 bits of hex, and two
    /// roads' differ.
    #[test]
    fn an_offer_crosses_the_link_and_a_strange_one_is_refused() {
        let road = IceRoad::bind(loopback(), Role::Shack, host_only(), TICK).unwrap();
        let other = IceRoad::bind(loopback(), Role::Shack, host_only(), TICK).unwrap();
        let offer = road.offer();
        assert_eq!((offer.ufrag.len(), offer.pwd.len()), (16, 32));
        assert!(offer
            .ufrag
            .bytes()
            .chain(offer.pwd.bytes())
            .all(|b| b.is_ascii_hexdigit()));
        assert_ne!(offer, other.offer());
        assert!(
            !format!("{offer:?}").contains(&offer.pwd),
            "the password in Debug"
        );
        let json = serde_json::to_string(&offer).unwrap();
        assert_eq!(serde_json::from_str::<Offer>(&json).unwrap(), offer);
        assert!(offer.shaped());

        let mut many = offer.clone();
        many.candidates = vec![offer.candidates[0].clone(); CANDIDATES + 1];
        let mut tcp = offer.clone();
        tcp.candidates = vec![offer.candidates[0].replace(" udp ", " tcp ")];
        let mut short = offer.clone();
        short.pwd.truncate(21);
        let mut strange = offer.clone();
        strange.ufrag = "ab:cd".into();
        for refused in [many, tcp, short, strange] {
            assert!(!refused.shaped(), "{refused:?}");
            assert!(!other.answer(&refused));
        }
        let unknown = json.replacen('{', "{\"port\":42077,", 1);
        assert!(serde_json::from_str::<Offer>(&unknown).is_err());
    }

    // ----- Where a road's checks go -----

    /// The other end's offer as the link carries it: credentials of this build's shape, and these
    /// candidates.
    fn offer_naming(candidates: Vec<String>) -> Offer {
        Offer {
            ufrag: "0011223344556677".into(),
            pwd: "00112233445566778899aabbccddeeff".into(),
            candidates,
        }
    }

    /// A road on every interface, as an internet road is bound: its host address is the one the
    /// route to the internet leaves by, on the computers these tests run on a private one.
    fn on_every_interface(role: Role, gather: Gather) -> IceRoad {
        let road = IceRoad::bind("0.0.0.0:0".parse().unwrap(), role, gather, TICK).unwrap();
        assert!(
            matches!(road.host().ip(), IpAddr::V4(own) if own.is_private()),
            "premise: this computer is on a private network ({})",
            road.host()
        );
        road
    }

    fn on_loopback() -> Counter {
        Counter::on(Ipv4Addr::LOCALHOST.into())
    }

    /// Whether `counter` heard anything within three seconds; and then half a second more, for any
    /// other road's checks begun at the same time.
    fn until_heard(counter: &Counter) -> bool {
        let started = Instant::now();
        while counter.heard() == 0 && started.elapsed() < Duration::from_secs(3) {
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(500));
        counter.heard() > 0
    }

    /// ⛔ An internet road never probes an address the other end names that no internet road may
    /// reach, at either end: one on this computer's loopback (in the offer) and one on the network
    /// the road itself is on (trickled) are read, and nothing ever reaches either. CONTROL: a test's
    /// road on loopback probes a loopback address it is given, within the same wait.
    #[test]
    fn an_internet_road_never_probes_loopback_or_the_network_it_is_on() {
        let heard: Vec<_> = [Role::Shack, Role::Computer]
            .into_iter()
            .map(|role| {
                let road = on_every_interface(role, host_only());
                let (loopback_one, its_network) = (on_loopback(), Counter::on(road.host().ip()));
                assert!(road.answer(&offer_naming(vec![loopback_one.line()])));
                assert!(road.add_candidate(&its_network.line()), "read");
                let probed = on_loopback();
                let control = IceRoad::bind(loopback(), role, host_only(), TICK).unwrap();
                assert!(control.answer(&offer_naming(vec![probed.line()])));
                let _driven = (Driven::new(road), Driven::new(control));
                assert!(
                    until_heard(&probed),
                    "CONTROL: {role:?}'s road on loopback probed nothing"
                );
                (role, loopback_one.heard(), its_network.heard())
            })
            .collect();
        assert_eq!(
            heard,
            [(Role::Shack, 0, 0), (Role::Computer, 0, 0)],
            "(the end, what reached this computer's loopback, what reached the road's own network)"
        );
    }

    /// ⛔ A road takes at most [`CANDIDATES`] of the other end's candidates in all, however they
    /// are named: eight lines naming one address, each a candidate of its own to the agent (their
    /// related addresses differ), fill it, and a ninth, naming another address, is read and never
    /// probed. CONTROL: the first address is probed.
    #[test]
    fn a_road_takes_at_most_eight_candidates_however_they_are_named() {
        let (first, ninth) = (on_loopback(), on_loopback());
        let at = first.at();
        let eight = (1..=CANDIDATES)
            .map(|n| {
                format!(
                    "candidate:{n} 1 udp 1694498815 {} {} typ srflx raddr 192.0.2.{n} rport {n}",
                    at.ip(),
                    at.port()
                )
            })
            .collect();
        let road = IceRoad::bind(loopback(), Role::Shack, host_only(), TICK).unwrap();
        assert!(road.answer(&offer_naming(eight)));
        assert!(road.add_candidate(&ninth.line()), "read");
        let _road = Driven::new(road);
        assert!(
            until_heard(&first),
            "CONTROL: the first address was never probed"
        );
        assert_eq!(ninth.heard(), 0, "the ninth candidate was probed");
    }

    /// ⛔ A check from an address an internet road may not reach is never answered, so the road
    /// never takes it as a peer-reflexive candidate, and never probes it: a test's road on loopback,
    /// given the internet road's credentials and its port on loopback, checks it from there, and
    /// neither end selects a pair. CONTROL: the same checks at a road on loopback are answered, and
    /// both ends select the pair.
    #[test]
    fn a_check_from_an_address_the_road_may_not_reach_is_never_answered() {
        for internet in [false, true] {
            let road = match internet {
                true => on_every_interface(Role::Shack, host_only()),
                false => IceRoad::bind(loopback(), Role::Shack, host_only(), TICK).unwrap(),
            };
            let checking = IceRoad::bind(loopback(), Role::Computer, host_only(), TICK).unwrap();
            let (ours, theirs) = (road.offer(), checking.offer());
            let at = SocketAddr::new(Ipv4Addr::LOCALHOST.into(), road.host().port());
            assert!(road.answer(&Offer {
                candidates: Vec::new(),
                ..theirs
            }));
            assert!(checking.answer(&Offer {
                candidates: vec![format!(
                    "candidate:1 1 udp 2130706431 {} {} typ host",
                    at.ip(),
                    at.port()
                )],
                ..ours
            }));
            let (road, checking) = (Driven::new(road), Driven::new(checking));
            let started = Instant::now();
            while started.elapsed() < Duration::from_millis(1500)
                && !(road.road.route().is_some() && checking.road.route().is_some())
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            let routes = (road.road.route(), checking.road.route());
            if internet {
                assert_eq!(
                    routes,
                    (None, None),
                    "(the internet road's pair, the checking road's)"
                );
            } else {
                assert_eq!(
                    routes,
                    (
                        Some(Route::Direct(checking.road.host())),
                        Some(Route::Direct(at))
                    ),
                    "CONTROL: a road on loopback answers the checks"
                );
            }
        }
    }

    /// ⛔ The relay is never asked to reach an address the road may not: an internet road at this
    /// computer, relayed only, given the other end's candidates on this computer's loopback (in the
    /// offer, before its relayed address is allocated) and on the network the road is on (trickled
    /// after), asks the TURN server for no permission and sends nothing through it, and nothing
    /// reaches either address. CONTROL: a test's road on loopback, given a loopback one trickled
    /// after its allocation, is permitted it at the relay, and its checks reach it through the relay.
    #[test]
    fn the_relay_is_never_asked_to_reach_an_address_the_road_may_not() {
        let relayed = |turn: &StandIn| Gather {
            direct: false,
            stun: None,
            relay: Some(Relay {
                server: turn.address,
                username: "road".into(),
                password: "throwaway-password".into(),
            }),
        };
        let allocated = |turn: &StandIn| {
            let started = Instant::now();
            while turn.seen().allocations == 0 && started.elapsed() < Duration::from_secs(3) {
                std::thread::sleep(Duration::from_millis(5));
            }
            turn.seen().allocations == 1
        };
        let turn = StandIn::start("road", "throwaway-password");
        let road = on_every_interface(Role::Computer, relayed(&turn));
        let (loopback_one, its_network) = (on_loopback(), Counter::on(road.host().ip()));
        assert!(road.answer(&offer_naming(vec![loopback_one.line()])));
        let road = Driven::new(road);
        assert!(
            allocated(&turn),
            "premise: the internet road has its relayed address"
        );
        assert!(road.road.add_candidate(&its_network.line()), "read");
        let control_turn = StandIn::start("road", "throwaway-password");
        let probed = on_loopback();
        let control =
            IceRoad::bind(loopback(), Role::Computer, relayed(&control_turn), TICK).unwrap();
        assert!(control.answer(&offer_naming(Vec::new())));
        let control = Driven::new(control);
        assert!(
            allocated(&control_turn),
            "premise: the control has its relayed address"
        );
        assert!(control.road.add_candidate(&probed.line()));
        assert!(
            until_heard(&probed),
            "CONTROL: nothing reached the loopback address through the relay"
        );
        assert!(
            control_turn
                .seen()
                .permissions
                .contains(&Ipv4Addr::LOCALHOST.into()),
            "CONTROL: {:?}",
            control_turn.seen().permissions
        );
        let seen = turn.seen();
        assert_eq!(
            (
                seen.permissions,
                seen.sent_by_indication,
                seen.sent_by_channel,
                loopback_one.heard(),
                its_network.heard()
            ),
            (Vec::new(), 0, 0, 0, 0),
            "(the permissions asked, what went through the relay, what reached each address)"
        );
    }

    /// `(probed though refused, refused though taken)` for an internet road, by address.
    fn misread(refused: &[&str], taken: &[&str]) -> (Vec<String>, Vec<String>) {
        let reaches = |at: &&str| Reach::Internet.reaches(at.parse().unwrap());
        (
            refused
                .iter()
                .filter(|at| reaches(at))
                .map(|at| at.to_string())
                .collect(),
            taken
                .iter()
                .filter(|at| !reaches(at))
                .map(|at| at.to_string())
                .collect(),
        )
    }
    const NONE: (Vec<String>, Vec<String>) = (Vec::new(), Vec::new());

    /// ⛔ The carrier-grade NAT's shared space, 100.64/10, is no address of the internet: a carrier's
    /// own customers, and the machines of a Tailscale network, are numbered there. Over Tailscale
    /// the LAN road is the way.
    #[test]
    fn an_internet_road_never_probes_a_carrier_nats_shared_space() {
        let refused = [
            "100.64.0.0:9",
            "100.64.0.1:41641",
            "100.100.100.100:53",
            "100.85.152.128:41641",
            "100.127.255.255:9",
            "[::ffff:100.64.1.2]:9",
            "[64:ff9b::6440:102]:9",
            "[2002:6440:102::1]:9",
        ];
        let taken = [
            "100.63.255.255:9",
            "100.128.0.0:9",
            "99.255.255.255:9",
            "101.0.0.0:9",
        ];
        assert_eq!(
            misread(&refused, &taken),
            NONE,
            "(probed though refused, refused though taken)"
        );
    }

    /// ⛔ The benchmarking block, 198.18/15 (RFC 2544), is a test network's, never the internet's.
    #[test]
    fn an_internet_road_never_probes_the_benchmarking_block() {
        let refused = [
            "198.18.0.0:9",
            "198.18.0.1:3478",
            "198.19.255.255:9",
            "[::ffff:198.19.1.1]:9",
            "[2002:c612:1::1]:9",
        ];
        let taken = ["198.17.255.255:9", "198.20.0.0:9", "198.51.100.9:50000"];
        assert_eq!(
            misread(&refused, &taken),
            NONE,
            "(probed though refused, refused though taken)"
        );
    }

    /// ⛔ The IETF's protocol assignments, 192.0.0/24 (RFC 6890): special addresses such as DS-Lite's
    /// and the dummy 192.0.0.8, none an end of an internet road.
    #[test]
    fn an_internet_road_never_probes_the_ietfs_protocol_block() {
        let refused = [
            "192.0.0.0:9",
            "192.0.0.1:9",
            "192.0.0.8:9",
            "192.0.0.255:9",
            "[::ffff:192.0.0.170]:9",
        ];
        let taken = ["191.255.255.255:9", "192.0.1.0:9", "192.0.2.1:3478"];
        assert_eq!(
            misread(&refused, &taken),
            NONE,
            "(probed though refused, refused though taken)"
        );
    }

    /// ⛔ The reserved 240/4, broadcast at its top, is nobody's unicast address.
    #[test]
    fn an_internet_road_never_probes_the_reserved_block() {
        let refused = [
            "240.0.0.0:9",
            "240.0.0.1:9",
            "250.1.2.3:9",
            "255.255.255.254:9",
            "255.255.255.255:9",
            "[::ffff:240.0.0.1]:9",
            "[64:ff9b::f000:1]:9",
        ];
        let taken = ["223.255.255.255:9", "8.8.8.8:3478"];
        assert_eq!(
            misread(&refused, &taken),
            NONE,
            "(probed though refused, refused though taken)"
        );
    }

    /// ⛔ IPv6 is probed only in its global unicast space, 2000::/3: never the old site-local
    /// fec0::/10, the discard prefix, or any block outside it (unique-local and the rest are in
    /// `an_internet_road_reaches_the_internet_alone`).
    #[test]
    fn an_internet_road_probes_ipv6_only_in_its_global_unicast_space() {
        let refused = [
            "[fec0::1]:9",
            "[fed0::1]:9",
            "[feff:ffff::1]:9",
            "[100::1]:9",
            "[1fff:ffff::1]:9",
            "[4000::1]:9",
            "[e000::1]:9",
            "[fbff::1]:9",
            "[fe7f::1]:9",
        ];
        let taken = [
            "[2000::1]:9",
            "[2001:db8::1]:40000",
            "[2606:4700:4700::1111]:3478",
            "[3fff:ffff::1]:9",
        ];
        assert_eq!(
            misread(&refused, &taken),
            NONE,
            "(probed though refused, refused though taken)"
        );
    }

    /// ⛔ An IPv6 address that carries an IPv4 one is that IPv4 address, held to the internet road's
    /// IPv4 rule: IPv4-mapped and IPv4-compatible, NAT64's well-known and local-use prefixes, 6to4,
    /// and Teredo (its client inverted). Each carrying a private, a loopback or a shared address is
    /// refused; each carrying an address of the internet is taken.
    #[test]
    fn an_internet_road_reads_every_form_that_carries_an_ipv4_address_as_that_address() {
        let refused = [
            // 10.0.0.1, 127.0.0.1 and 100.64.1.2, in each form.
            "[::ffff:10.0.0.1]:9",
            "[::10.0.0.1]:9",
            "[::127.0.0.1]:9",
            "[::100.64.1.2]:9",
            "[64:ff9b::a00:1]:9",
            "[64:ff9b::7f00:1]:9",
            // NAT64's local-use prefix, private by its /48, /56, /64 and /96 readings in turn.
            "[64:ff9b:1:a01:71:708:90a:b0c]:9",
            "[64:ff9b:1:cb0a:71:708:90a:b0c]:9",
            "[64:ff9b:1:cb01:a:708:90a:b0c]:9",
            "[64:ff9b:1:cb01:71:708:a0a:b0c]:9",
            "[2002:a00:1::1]:9",
            "[2002:7f00:1::1]:9",
            "[2001:0:4136:e378:8000:63bf:f5ff:fffe]:9",
            "[2001:0:4136:e378:8000:63bf:9bbf:fefd]:9",
        ];
        let taken = [
            "[::ffff:8.8.8.8]:9",
            "[::203.0.113.7]:9",
            "[64:ff9b::cb00:7107]:9",
            "[64:ff9b:1:cb01:71:708:90a:b0c]:9",
            "[2002:cb00:7107::1]:9",
            "[2001:0:4136:e378:8000:63bf:34ff:8ef8]:9",
        ];
        assert_eq!(
            misread(&refused, &taken),
            NONE,
            "(probed though refused, refused though taken)"
        );
    }

    /// ⛔ The rule, by value. An internet road refuses each class, and each in its IPv4-mapped form,
    /// and port 0; it takes an address of the internet, the edges of each private block among them.
    /// Through the road, the kind of candidate the other end names changes nothing: a private
    /// address is refused as a server-reflexive or relayed one too, and a server-reflexive, relayed
    /// or IPv6 host address of the internet is taken; each refusal is counted, and what is taken is
    /// the road's peers. A test's road on loopback reaches loopback alone.
    #[test]
    fn an_internet_road_reaches_the_internet_alone() {
        let refused = [
            // This computer: loopback, and unspecified ("this network").
            "127.0.0.1:4532",
            "127.255.255.254:9",
            "[::1]:9",
            "[::ffff:127.0.0.1]:9",
            "0.0.0.0:9",
            "0.1.2.3:9",
            "[::]:9",
            "[::ffff:0.0.0.0]:9",
            // A group: multicast, and IPv4's broadcast.
            "224.0.0.251:5353",
            "239.255.255.250:1900",
            "[ff02::1]:9",
            "[ff05::1:3]:547",
            "[::ffff:239.255.255.250]:1900",
            "255.255.255.255:9",
            "[::ffff:255.255.255.255]:9",
            // A link's own address, a cloud's metadata service among them.
            "169.254.169.254:80",
            "169.254.0.1:9",
            "[fe80::1]:9",
            "[febf::1]:9",
            "[::ffff:169.254.169.254]:80",
            // A private network, IPv4's and IPv6's unique-local.
            "10.0.0.1:9",
            "172.16.0.1:9",
            "172.31.255.254:9",
            "192.168.1.1:9",
            "[::ffff:10.1.2.3]:9",
            "[::ffff:192.168.1.1]:9",
            "[fc00::1]:9",
            "[fd12:3456::1]:9",
            // Port 0, at an address otherwise taken.
            "198.51.100.7:0",
            "[2001:db8::1]:0",
        ];
        let taken = [
            "8.8.8.8:3478",
            "203.0.113.7:61000",
            "1.1.1.1:65535",
            "1.0.0.0:9",
            "9.255.255.255:9",
            "11.0.0.0:9",
            "126.255.255.255:9",
            "128.0.0.0:9",
            "169.253.255.255:9",
            "169.255.0.0:9",
            "172.15.255.255:9",
            "172.32.0.0:9",
            "192.167.255.255:9",
            "192.169.0.0:9",
            "223.255.255.255:9",
            "[2606:4700:4700::1111]:3478",
            "[2001:db8::1]:9",
            "[::ffff:8.8.8.8]:9",
        ];
        let reaches = |reach: Reach, at: &str| reach.reaches(at.parse().unwrap());
        let wrong: Vec<_> = refused
            .iter()
            .filter(|at| reaches(Reach::Internet, at))
            .chain(taken.iter().filter(|at| !reaches(Reach::Internet, at)))
            .collect();
        assert!(wrong.is_empty(), "read the wrong way: {wrong:?}");
        let on_loopback: Vec<_> = [
            "127.0.0.1:9",
            "[::1]:9",
            "[::ffff:127.0.0.1]:9",
            "127.0.0.1:0",
            "10.0.0.1:9",
            "8.8.8.8:9",
        ]
        .iter()
        .map(|at| reaches(Reach::Loopback, at))
        .collect();
        assert_eq!(on_loopback, [true, true, true, false, false, false]);

        let road = on_every_interface(Role::Shack, host_only());
        assert_eq!(lock(&road.state).reach, Reach::Internet);
        assert!(road.answer(&offer_naming(Vec::new())));
        for line in [
            "candidate:1 1 udp 1694498815 192.168.1.1 9 typ srflx raddr 0.0.0.0 rport 0",
            "candidate:2 1 udp 16777215 10.0.0.1 9 typ relay raddr 0.0.0.0 rport 0",
            "candidate:3 1 udp 2130706431 127.0.0.1 9 typ host",
            "candidate:4 1 udp 1694498815 203.0.113.7 61000 typ srflx raddr 0.0.0.0 rport 0",
            "candidate:5 1 udp 16777215 198.51.100.9 50000 typ relay raddr 0.0.0.0 rport 0",
            "candidate:6 1 udp 2130706431 2001:db8::1 40000 typ host",
        ] {
            assert!(road.add_candidate(line), "read: {line}");
        }
        let state = lock(&road.state);
        assert_eq!(
            (state.peers.clone(), state.taken, state.refused),
            (
                vec![
                    "203.0.113.7:61000".parse().unwrap(),
                    "198.51.100.9:50000".parse().unwrap(),
                    "[2001:db8::1]:40000".parse().unwrap()
                ],
                3,
                3
            ),
            "(the road's peers, how many it took, how many it refused)"
        );
    }
}
