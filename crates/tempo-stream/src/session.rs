//! One streamed session, the station's side: a WebRTC peer (str0m) that answers the page's offer,
//! sends the Nexus window as VP8, receives the page's microphone as Opus, and carries the three
//! data channels of the contract.
//!
//! Sans-I/O like str0m itself. The caller owns the UDP socket and the clock: it feeds packets and
//! timeouts in, sends what [`Session::take_transmits`] hands back, reacts to
//! [`Session::take_events`], and waits no longer than [`Session::next_timeout`]. Every method that
//! changes the session drains str0m before it returns, so str0m's one hard rule (drain to a timeout
//! after every mutation) holds by construction instead of by every caller remembering it.
//!
//! What the transport decides on the station's behalf, all of it here:
//! - **Admission comes first, and is not this file's.** [`Session::accept`] is called only for an
//!   offer the station's authority has admitted (security test A3), and it runs the offer check
//!   (A4) before any WebRTC state exists.
//! - **Windows only, today.** The approved crypto backend is Windows CNG. Elsewhere
//!   [`Session::accept`] refuses [`Refusal::Unavailable`] before it reaches str0m's builder, which
//!   would panic for want of a provider.
//! - **VP8 out, Opus in, and nothing else.** The codec list is cleared to VP8 (the station's
//!   picture) and Opus (the page's microphone, plan S6: an audio line the page sends and the
//!   station receives). The microphone's packets are handed on as they arrived
//!   ([`SessionEvent::Mic`]), undecoded and unjudged: what may reach a transmitter, and when, is
//!   `tempo_app::mic`'s to decide, and the session keys nothing.
//! - **The shack's own LAN address, and no other (A8, as the operator ruled on 2026-10-03).** The
//!   station signals two candidates: the server-reflexive address a STUN server reported
//!   ([`Session::add_reflexive`]), whose `raddr` str0m writes as `0.0.0.0 0`, and, when its socket's
//!   own address is the shack's LAN address ([`crate::lan::host`]), a host candidate for exactly
//!   that address ([`Session::host_candidate`]), so a browser on the same network connects directly.
//!   A public address on the socket (no NAT in front of the shack) is offered the same way: the
//!   reflexive candidate would be that same address, which str0m drops as the host one's duplicate.
//!   Before that ruling no LAN address was signalled, and a browser on the shack's own network found
//!   no path to it. str0m needs the socket's own address as a local candidate either way: it answers
//!   connectivity checks only on a host or relay candidate, and discards every other one as an
//!   unknown interface (measured: srflx alone never connects). It is added AFTER the answer is
//!   written, so the answer carries no candidate; nothing re-negotiates, so no later SDP can carry
//!   one, and ICE's own messages carry no local address. Every SDP and candidate line is read by
//!   [`crate::lan::leaks`] before it is handed out, and a leak (any other LAN address, or this one
//!   anywhere but its own candidate) is never handed out.
//! - **Only an address a viewer could really be at is tried (2026-10-10).** str0m runs full ICE: it
//!   sends checks to every candidate it holds, answers a check where it came from, and then checks
//!   that source too. So every candidate the page names, in its offer and its trickle alike, goes in
//!   through [`Session::add_remote_candidate`] and is held to [`may_try`] there, and a datagram
//!   from an address [`may_try`] refuses never reaches str0m ([`Session::receive`]). Refused:
//!   loopback, "this network" (0/8, `::`), multicast, the reserved 240/4 and broadcast, link-local
//!   (169.254/16, where a cloud's metadata service answers, and fe80::/10), IPv6's old site-local
//!   fec0::/10 and anything else outside its global and unique-local space, and port 0. An IPv6
//!   address that carries an IPv4 one is read as that IPv4 address. Kept: the private networks,
//!   unique-local fc00::/7 and carrier-grade NAT's 100.64/10, so a viewer on the shack's own
//!   network, or on a Tailscale network, still connects directly. A host name (a browser's mDNS
//!   `.local` one) is never looked up. A session tries at most [`CANDIDATES`] of the page's
//!   candidates, and takes at most [`CANDIDATES_IN_ALL`] in all, counting the ones it could never
//!   try, which str0m keeps as well. A refused candidate is dropped and counted
//!   ([`Session::refused`]), and the session goes on.
//! - **The page's certificate must match its offer.** str0m verifies the peer's DTLS certificate
//!   against the offer's fingerprint by default; [`Session::accept`] asserts that default rather than
//!   assuming it. The device-key binding (security test A5) signs that same fingerprint, read with
//!   [`crate::protocol::offer_fingerprint`]: the station checks the signature at admission, before
//!   `accept`, and once DTLS is up holds [`Session::remote_fingerprint`], the certificate the page
//!   actually presented, against the one it signed.
//! - **Finding a path is not failing.** ICE meets refusals on the way: a check its destination
//!   refuses, or whose time to live runs out, which Windows reports on the socket's next receive
//!   ([`receive_ends_session`]), and a first trickled candidate the station cannot pair, which
//!   str0m reports as Disconnected. Neither ends a session that has not connected yet;
//!   [`CONNECT_TIMEOUT`] does.
//! - **Control must be reliable and ordered.** A `control` channel opened any other way is not used:
//!   a Stop that the channel is allowed to drop is not a Stop.
//! - **Where the picture goes** ([`Session::path`]): the selected pair's remote address, from
//!   str0m's own statistics every [`STATS_EVERY`], read by [`crate::lan::path`]. The picture's size
//!   and bit rate follow it (`video::picture::Bound`); nothing else does.
//! - **What the link carries** ([`Session::estimate`], 2026-10-03): str0m's own bandwidth
//!   estimation (Google congestion control on the page's transport-wide feedback), read from the
//!   same statistics, so the picture's bit rate follows the link (`video::rate`) instead of a fixed
//!   budget it may not carry. [`Session::want`] tells it how far to probe: the link the whole
//!   picture would use. It is on only for an offer that carries transport-wide feedback
//!   (`transport_feedback`); every current browser's does, and one that does not keeps the
//!   fixed budget. With it on, str0m paces the picture's packets at about the estimate, and the
//!   data channels (receive audio, `control`) go out ahead of them unpaced.
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::time::{Duration, Instant};

use str0m::bwe::Bitrate;
use str0m::change::SdpOffer;
use str0m::channel::{ChannelId, Reliability};
use str0m::format::Codec;
use str0m::media::{MediaKind, MediaTime, Mid, Pt};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};

use crate::frame_clock::VideoClock;
use crate::offer::{self, OfferRefusal};
use crate::protocol::{StreamReason, AUDIO_CHANNEL, CONTROL_CHANNEL, PTT_CHANNEL};
use crate::video::picture::Path;

/// How long ICE and DTLS may take before the session is given up on.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);
/// How often the session reads str0m's statistics: where the picture goes ([`Session::path`]) and
/// what the link carries ([`Session::estimate`]). Five times a second, so a link that narrows is
/// followed within a fifth of a second of str0m seeing it.
pub const STATS_EVERY: Duration = Duration::from_millis(200);
/// Where str0m's estimate of the link starts, in kbit/s, before the page's first feedback. Its first
/// probes go to three and six times this straight away, so a wide link is found within about a
/// second, while the first keyframe is not sized for a link nobody has measured.
pub const START_KBPS: u32 = 1000;
/// The transport-wide congestion control header extension (draft-holmer-rmcat-transport-wide-cc).
const TRANSPORT_WIDE_CC: &str = "transport-wide-cc-extensions-01";
/// The most timeouts one drain serves: str0m's pacer sends up to 40 ms of the link's rate at once
/// (about 40 packets at 10 Mbit/s), so this is room to spare.
const DUE_ROUNDS: usize = 256;
/// How far ahead of the caller's clock a timeout may be and still be served in the same drain: the
/// pacer's microsecond steps, a few hundred of them at most, with room to spare.
const DUE_SLACK: Duration = Duration::from_millis(1);

/// Does the page's offer let str0m estimate the link? Its bandwidth estimation runs on the page's
/// transport-wide feedback, so the video line must carry the transport-wide sequence number and
/// `transport-cc` feedback. Without them no feedback would ever move the estimate, and str0m's
/// pacer would hold the picture to where it started.
fn transport_feedback(sdp: &str) -> bool {
    let (mut video, mut sequence, mut feedback) = (false, false, false);
    for line in sdp.lines().map(str::trim) {
        if let Some(media) = line.strip_prefix("m=") {
            if video && sequence && feedback {
                return true;
            }
            (video, sequence, feedback) = (media.starts_with("video "), false, false);
        } else if video {
            sequence |= line.starts_with("a=extmap:") && line.contains(TRANSPORT_WIDE_CC);
            feedback |= line.starts_with("a=rtcp-fb:") && line.ends_with(" transport-cc");
        }
    }
    video && sequence && feedback
}

/// Windows' WSAEMSGSIZE on a receive: the datagram was longer than the buffer it was read into.
const WSAEMSGSIZE: i32 = 10040;
/// Windows' WSAENETRESET on a datagram socket's receive: "the time to live has expired"
/// (Microsoft's `recvfrom` reference), an ICMP time-exceeded for an earlier datagram.
const WSAENETRESET: i32 = 10052;

/// Does this error, from a receive on the session's socket, end the session? A receive that waited
/// out its timeout does not, and neither does a report about one datagram, which leaves the socket
/// unharmed. On an unconnected UDP socket Windows makes three such reports:
/// - `ConnectionReset` (WSAECONNRESET) passes on an ICMP "port unreachable" for an EARLIER
///   datagram: ICE sends checks to every candidate the page offered, and a destination that refuses
///   one (a NAT with no mapping for the station yet, a router that will not loop a packet back to
///   its own public address) answers that way. Which candidates work is ICE's to decide. Measured
///   against Chrome, the first such report came within 50 ms of the answer, before DTLS, and ending
///   the session on it ended every stream.
/// - WSAENETRESET is the same for an ICMP "time exceeded": a check that met a routing loop or too
///   short a path.
/// - WSAEMSGSIZE is a datagram longer than the caller's buffer, which anyone who can reach the
///   socket can send. Windows hands back the part that fit with the error, and that part is not a
///   datagram: the caller drops it, and the next datagram is received whole.
///
/// Any other error is the socket's own, and ends the session.
pub fn receive_ends_session(error: &std::io::Error) -> bool {
    use std::io::ErrorKind::{ConnectionReset, TimedOut, WouldBlock};
    let one_datagram = error.kind() == ConnectionReset
        || (cfg!(windows) && matches!(error.raw_os_error(), Some(WSAEMSGSIZE | WSAENETRESET)));
    !matches!(error.kind(), WouldBlock | TimedOut) && !one_datagram
}

/// The most of the page's candidates one session tries, from its offer and its trickle together.
/// Each is an address the station sends checks to, so this bounds where one page can make it send.
/// For each network it is on, a browser names a reflexive address, a relayed one for each of the
/// relay's routes, and its host address, which it names by an mDNS name (never tried) unless the
/// page holds its microphone's permission; its relayed ones come last. One this session's socket
/// could never try (a TCP candidate, or one of the other IP family) draws no check and does not
/// count here, only toward [`CANDIDATES_IN_ALL`], so a browser that names many of those still has
/// the rest tried.
pub const CANDIDATES: usize = 16;

/// The most of the page's candidates one session takes in all, of every kind: the ones it tries
/// ([`CANDIDATES`]) and the ones its socket never could. str0m keeps a candidate it can never pair
/// just as it keeps one it can, so without this a page that named candidates without end would
/// grow what the station holds without end. A browser names a few for each network it is on.
pub const CANDIDATES_IN_ALL: usize = 64;

/// May the station try `peer`, an address the page named, or answer a datagram from it? Only if a
/// viewer could really be at it: a unicast address of the internet, of a private network or of a
/// carrier-grade NAT, or IPv6's unique-local one, and never port 0. It is an allowlist: an address
/// in none of those is refused, whatever it is (the module header names what that refuses).
pub fn may_try(peer: SocketAddr) -> bool {
    peer.port() != 0
        && match peer.ip() {
            IpAddr::V4(v4) => viewer_v4(v4),
            IpAddr::V6(v6) => viewer_v6(v6),
        }
}

/// IPv4's unicast space, 1.0.0.0 to 223.255.255.255, less loopback (127/8: the station's own
/// services) and link-local (169.254/16: the link's own devices, and a cloud's metadata service at
/// 169.254.169.254). Below it is "this network" (0/8, the unspecified address among it), above it
/// multicast (224/4), the reserved 240/4 and broadcast. The private networks (RFC 1918) and the
/// carrier-grade NAT's 100.64/10, where Tailscale numbers its machines, are in it.
fn viewer_v4(ip: Ipv4Addr) -> bool {
    matches!(ip.octets()[0], 1..=223) && !ip.is_loopback() && !ip.is_link_local()
}

/// IPv6's global unicast space (2000::/3) and unique-local fc00::/7, and nothing else: not loopback
/// or the unspecified address, multicast, link-local fe80::/10 or the old site-local fec0::/10. An
/// address that carries an IPv4 one is that IPv4 address, held to IPv4's rule.
fn viewer_v6(ip: Ipv6Addr) -> bool {
    let o = ip.octets();
    let v4 =
        |a: usize, b: usize, c: usize, d: usize| viewer_v4(Ipv4Addr::new(o[a], o[b], o[c], o[d]));
    match ip.segments() {
        // IPv4-mapped (::ffff:0:0/96) and IPv4-compatible (::/96, which holds `::` and `::1`).
        [0, 0, 0, 0, 0, 0 | 0xffff, ..] => v4(12, 13, 14, 15),
        // NAT64's well-known prefix, 64:ff9b::/96.
        [0x64, 0xff9b, 0, 0, 0, 0, ..] => v4(12, 13, 14, 15),
        // NAT64's local-use prefix, 64:ff9b:1::/48 (RFC 8215). A network carves its own from it at
        // /48, /56, /64 or /96, and each puts the IPv4 address somewhere else (RFC 6052), so every
        // one of those readings must pass.
        [0x64, 0xff9b, 1, ..] => {
            v4(6, 7, 9, 10) && v4(7, 9, 10, 11) && v4(9, 10, 11, 12) && v4(12, 13, 14, 15)
        }
        // 6to4, 2002::/16.
        [0x2002, ..] => v4(2, 3, 4, 5),
        // Teredo, 2001::/32: its client's own address, which it carries inverted.
        [0x2001, 0, ..] => viewer_v4(!Ipv4Addr::new(o[12], o[13], o[14], o[15])),
        [first, ..] => first & 0xe000 == 0x2000 || first & 0xfe00 == 0xfc00,
    }
}

/// Why an offer was not answered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    Offer(OfferRefusal),
    /// This platform has no WebRTC session to offer, or it could not be started.
    Unavailable,
}

impl Refusal {
    /// The page-facing reason.
    pub fn reason(self) -> StreamReason {
        match self {
            Self::Offer(_) => StreamReason::InvalidOffer,
            Self::Unavailable => StreamReason::StreamUnavailable,
        }
    }
}

/// The data channels the contract names.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lane {
    Control,
    Ptt,
    Audio,
}

/// One packet of the page's microphone, as it arrived.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MicPacket {
    /// The RTP sequence number, extended by the transport so it never wraps.
    pub seq: u64,
    /// The RTP timestamp, extended, on the track's clock.
    pub rtp: u64,
    /// The track's clock rate: 48 kHz, as WebRTC signals every Opus track.
    pub clock_hz: u32,
    /// When the packet reached the station's socket.
    pub arrived: Instant,
    /// One Opus packet.
    pub payload: Vec<u8>,
}

/// What happened in the session since the caller last asked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// ICE and DTLS are up, and the page's certificate matched its offer.
    Connected,
    /// A message from the page on `control` or `ptt`, as it arrived.
    Message(Lane, Vec<u8>),
    /// The page's `audio` channel opened: receive audio may flow.
    AudioOpen,
    /// The page closed its `audio` channel.
    AudioClosed,
    /// The page asked for a keyframe.
    KeyframeRequest,
    /// A packet of the page's microphone.
    Mic(MicPacket),
    /// The session is over. Nothing more will come out of it.
    Closed(StreamReason),
}

/// One datagram to send from the session's socket.
#[derive(Clone, Debug)]
pub struct Transmit {
    pub destination: SocketAddr,
    pub contents: Vec<u8>,
}

pub struct Session {
    rtc: Rtc,
    /// The local socket address: the base of the reflexive candidate, and the station's host
    /// candidate when it is the shack's LAN address.
    base: SocketAddr,
    /// That host candidate's line, to trickle to the page: `None` when the station offers none.
    host: Option<String>,
    /// The first bundled media id, which trickled candidates name.
    mid: String,
    video: Option<(Mid, Pt)>,
    /// The page's microphone: the audio line it sends and the station receives.
    mic: Option<Mid>,
    control: Option<ChannelId>,
    ptt: Option<ChannelId>,
    audio: Option<ChannelId>,
    clock: VideoClock,
    /// Where the picture goes, once ICE has selected a pair.
    path: Option<Path>,
    /// What the link carries, in kbit/s, as str0m last estimated it; `None` for a session with no
    /// estimation (an offer without transport-wide feedback).
    estimate: Option<u32>,
    /// The link last asked of the estimation ([`Session::want`]).
    wanted: Option<u32>,
    /// The page's candidates taken that the session could try: at most [`CANDIDATES`].
    tried: usize,
    /// The page's candidates taken, of every kind: at most [`CANDIDATES_IN_ALL`].
    taken: usize,
    /// The page's candidates refused ([`Session::refused`]).
    refused: usize,
    started: Instant,
    connected: bool,
    closed: bool,
    timeout: Instant,
    transmits: Vec<Transmit>,
    events: Vec<SessionEvent>,
}

impl Session {
    /// Answer an admitted page's offer, sending from the socket at `base`. Returns the session and
    /// the answer to signal back.
    pub fn accept(sdp: &str, base: SocketAddr, now: Instant) -> Result<(Session, String), Refusal> {
        Self::precheck(sdp)?;
        Self::build(sdp, base, None, now)
    }

    /// Everything [`Session::accept`] would refuse without touching the network or building
    /// anything: the offer check (A4) and the platform. The caller runs it before it opens a
    /// socket, so a refused offer costs the station nothing.
    pub fn precheck(sdp: &str) -> Result<(), Refusal> {
        offer::check(sdp).map_err(Refusal::Offer)?;
        if !cfg!(windows) {
            return Err(Refusal::Unavailable);
        }
        Ok(())
    }

    /// The session behind [`Session::accept`], past the checks that must come before it.
    /// `host` exists for one test: A8's positive control, which shows what a host candidate for an
    /// address other than the socket's own WOULD put in the answer.
    fn build(
        sdp: &str,
        base: SocketAddr,
        host: Option<SocketAddr>,
        now: Instant,
    ) -> Result<(Session, String), Refusal> {
        // str0m would take the offer's own candidates as they are: they go in as the trickle's
        // do, through `add_remote_candidate`, once the session exists.
        let candidates: Vec<&str> = sdp
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("a=candidate:"))
            .collect();
        let bare: String = sdp
            .split_inclusive('\n')
            .filter(|line| !line.trim().starts_with("a=candidate:"))
            .collect();
        let parsed = SdpOffer::from_sdp_string(&bare)
            .map_err(|_| Refusal::Offer(OfferRefusal::Unparseable))?;
        let estimated = transport_feedback(sdp);
        let config = Rtc::builder()
            .clear_codecs()
            .enable_vp8(true)
            // The page's microphone (S6). No RED: the page sends plain Opus.
            .enable_opus(true, false)
            .set_ice_lite(false)
            // The selected pair and the link's estimate arrive with the statistics.
            .set_stats_interval(Some(STATS_EVERY))
            .enable_bwe(estimated.then(|| Bitrate::kbps(u64::from(START_KBPS))));
        // The peer's certificate is checked against its offer's fingerprint. str0m's default, and
        // the lock A4 and A5 stand on, so it is asserted rather than assumed.
        if !config.fingerprint_verification() {
            return Err(Refusal::Unavailable);
        }
        let mut rtc = config.build(now);
        if let Some(host) = host {
            let candidate = Candidate::host(host, "udp").map_err(|_| Refusal::Unavailable)?;
            rtc.add_local_candidate(candidate);
        }
        let answer = rtc
            .sdp_api()
            .accept_offer(parsed)
            .map_err(|_| Refusal::Offer(OfferRefusal::Unparseable))?
            .to_sdp_string();
        if host.is_none() && crate::lan::leaks(&answer, None) {
            return Err(Refusal::Unavailable);
        }
        // Only now, with the answer written: the socket's own address, which ICE needs to use the
        // socket at all, and which is signalled, as its own candidate, only when it is the shack's
        // LAN address or a public one (see the module header).
        let own = Candidate::host(base, "udp").map_err(|_| Refusal::Unavailable)?;
        let line = rtc.add_local_candidate(own).map(Candidate::to_sdp_string);
        let lan = crate::lan::host(base);
        let offered = lan.and(line).filter(|line| !crate::lan::leaks(line, lan));
        let mid = sdp
            .lines()
            .find_map(|l| l.trim().strip_prefix("a=mid:"))
            .unwrap_or("0")
            .to_string();
        let mut session = Session {
            rtc,
            base,
            host: offered,
            mid,
            video: None,
            mic: None,
            control: None,
            ptt: None,
            audio: None,
            clock: VideoClock::new(now),
            path: None,
            estimate: estimated.then_some(START_KBPS),
            wanted: None,
            tried: 0,
            taken: 0,
            refused: 0,
            started: now,
            connected: false,
            closed: false,
            timeout: now,
            transmits: Vec::new(),
            events: Vec::new(),
        };
        for line in candidates {
            session.add_remote_candidate(line, now);
        }
        session.pump(now);
        Ok((session, answer))
    }

    /// Add the server-reflexive candidate a STUN server reported for this session's socket, and
    /// return the candidate line to trickle to the page. `None`, and nothing added, for an address
    /// that belongs to a LAN: that would be exactly the leak this session refuses to make.
    pub fn add_reflexive(&mut self, public: SocketAddr, now: Instant) -> Option<String> {
        if self.closed || crate::lan::private(public.ip()) {
            return None;
        }
        let candidate = Candidate::server_reflexive(public, self.base, "udp").ok()?;
        let line = self.rtc.add_local_candidate(candidate)?.to_sdp_string();
        self.pump(now);
        (!crate::lan::leaks(&line, None)).then_some(line)
    }

    /// The station's own address as a host candidate line, to trickle to the page with the answer:
    /// what lets a browser on the shack's network connect directly, and any browser reach a shack
    /// with a public address and no NAT. `None` when the socket's address is not one the station
    /// offers ([`crate::lan::host`]).
    pub fn host_candidate(&self) -> Option<&str> {
        self.host.as_deref()
    }

    /// The media id a trickled candidate names.
    pub fn mid(&self) -> &str {
        &self.mid
    }

    /// A candidate the page named, trickled or in its offer. Returns whether the session took it.
    /// One it may not try ([`may_try`], or one past [`CANDIDATES`]), or any past
    /// [`CANDIDATES_IN_ALL`], is refused and counted; one it cannot read (a host name, which is
    /// never looked up, or a malformed line) is ignored. Either way ICE works with whatever
    /// candidates remain.
    pub fn add_remote_candidate(&mut self, line: &str, now: Instant) -> bool {
        if self.closed {
            return false;
        }
        let line = line.trim();
        let line = line.strip_prefix("a=").unwrap_or(line);
        let Ok(candidate) = Candidate::from_sdp_string(line) else {
            return false;
        };
        // str0m pairs a candidate only with a local one of its protocol and IP family, and the
        // station's are UDP on its socket's: only such a candidate draws checks. It keeps the
        // others too, so every one taken counts toward the total.
        let tried =
            candidate.proto() == Protocol::Udp && candidate.addr().is_ipv4() == self.base.is_ipv4();
        if !may_try(candidate.addr())
            || self.taken >= CANDIDATES_IN_ALL
            || (tried && self.tried >= CANDIDATES)
        {
            self.refused += 1;
            return false;
        }
        self.taken += 1;
        self.tried += usize::from(tried);
        self.rtc.add_remote_candidate(candidate);
        self.pump(now);
        true
    }

    /// How many of the page's candidates the session has refused (see [`may_try`], [`CANDIDATES`]
    /// and [`CANDIDATES_IN_ALL`]). The station notes the first, in its own words, never the
    /// address.
    pub fn refused(&self) -> usize {
        self.refused
    }

    /// A datagram that arrived on the session's socket. Returns false when it was not the
    /// session's (a STUN answer to the station's own binding request, say). One from an address
    /// the station may not try ([`may_try`]) is never read: str0m would answer a check there, and
    /// then try that address itself.
    pub fn receive(&mut self, now: Instant, source: SocketAddr, data: &[u8]) -> bool {
        if self.closed || !may_try(source) {
            return false;
        }
        let Ok(receive) = Receive::new(Protocol::Udp, source, self.base, data) else {
            return false;
        };
        let input = Input::Receive(now, receive);
        if !self.rtc.accepts(&input) {
            return false;
        }
        if self.rtc.handle_input(input).is_err() {
            self.fail(StreamReason::ConnectionFailed);
        }
        self.pump(now);
        true
    }

    /// Time moved on. Also gives up on a session that has not connected within
    /// [`CONNECT_TIMEOUT`].
    pub fn timeout(&mut self, now: Instant) {
        if self.closed {
            return;
        }
        if !self.connected && now.saturating_duration_since(self.started) >= CONNECT_TIMEOUT {
            self.fail(StreamReason::ConnectionFailed);
            return;
        }
        if self.rtc.handle_input(Input::Timeout(now)).is_err() {
            self.fail(StreamReason::ConnectionFailed);
        }
        self.pump(now);
    }

    /// Send one encoded VP8 frame, captured at `captured_at`. The frame is stamped on the session's
    /// video clock, which is what a page's `decodedFrameAt` echoes back. Returns false when there
    /// is nowhere to send it yet, or the transport refused it.
    pub fn send_video(&mut self, now: Instant, captured_at: Instant, frame: &[u8]) -> bool {
        if self.closed || !self.connected {
            return false;
        }
        let Some((mid, pt)) = self.video else {
            return false;
        };
        let ticks = self.clock.ticks(captured_at);
        let sent = self.rtc.writer(mid).is_some_and(|writer| {
            writer
                .write(
                    pt,
                    captured_at,
                    MediaTime::from_90khz(ticks),
                    frame.to_vec(),
                )
                .is_ok()
        });
        if sent {
            // Only a frame the transport took can be echoed back as shown.
            self.clock.sent(captured_at);
        }
        self.pump(now);
        sent
    }

    /// Send one message to the page on `control` or `audio`. Returns false when that channel is
    /// not open or its buffer is full; audio is then simply lost, as the contract allows.
    pub fn send(&mut self, lane: Lane, text: &str, now: Instant) -> bool {
        if self.closed {
            return false;
        }
        let id = match lane {
            Lane::Control => self.control,
            Lane::Audio => self.audio,
            Lane::Ptt => None,
        };
        let Some(id) = id else {
            return false;
        };
        let sent = self
            .rtc
            .channel(id)
            .is_some_and(|mut channel| channel.write(false, text.as_bytes()).unwrap_or(false));
        self.pump(now);
        sent
    }

    /// Where the picture goes: the page's network as seen from the selected ICE pair, `None` until
    /// there is one. Read every [`STATS_EVERY`], so it follows a pair ICE selects later.
    pub fn path(&self) -> Option<Path> {
        self.path
    }

    /// What the link to the page carries, in kbit/s, as str0m estimates it: [`START_KBPS`] until
    /// the page's feedback moves it, read every [`STATS_EVERY`]. `None` for a session with no
    /// estimation, whose picture keeps the path's fixed budget.
    pub fn estimate(&self) -> Option<u32> {
        self.estimate
    }

    /// Ask str0m's estimation to look for a link of `kbps`: it probes up to that (and up to twice
    /// it, as WebRTC does, to find room), and no further. A session with no estimation ignores it.
    pub fn want(&mut self, kbps: u32, now: Instant) {
        if self.closed || self.estimate.is_none() || self.wanted == Some(kbps) {
            return;
        }
        self.wanted = Some(kbps);
        self.rtc
            .bwe()
            .set_desired_bitrate(Bitrate::kbps(u64::from(kbps)));
        self.pump(now);
    }

    /// S9: is the picture the page says it last showed fresh enough to renew transmit presence?
    pub fn fresh(&self, decoded: Option<u32>, now: Instant) -> bool {
        self.clock.fresh(decoded, now)
    }

    /// End the session now.
    pub fn close(&mut self, reason: StreamReason, now: Instant) {
        if self.closed {
            return;
        }
        self.rtc.disconnect();
        self.pump(now);
        self.fail(reason);
    }

    pub fn is_connected(&self) -> bool {
        self.connected && !self.closed
    }

    /// The SHA-256 fingerprint of the certificate the page's DTLS actually presented, once the
    /// handshake has one: what A5 holds against the fingerprint the page signed. `None` before
    /// that, and for any other hash.
    pub fn remote_fingerprint(&mut self) -> Option<[u8; 32]> {
        let api = self.rtc.direct_api();
        let fingerprint = api.remote_dtls_fingerprint()?;
        if !fingerprint.hash_func.eq_ignore_ascii_case("sha-256") {
            return None;
        }
        fingerprint.bytes.as_slice().try_into().ok()
    }

    pub fn is_closed(&self) -> bool {
        self.closed
    }

    /// When the session next needs [`Session::timeout`].
    pub fn next_timeout(&self) -> Instant {
        self.timeout
    }

    pub fn take_transmits(&mut self) -> Vec<Transmit> {
        std::mem::take(&mut self.transmits)
    }

    pub fn take_events(&mut self) -> Vec<SessionEvent> {
        std::mem::take(&mut self.events)
    }

    fn fail(&mut self, reason: StreamReason) {
        if !self.closed {
            self.closed = true;
            self.events.push(SessionEvent::Closed(reason));
        }
    }

    /// Drain str0m to its next timeout: the one rule its API has. A timeout due now, or within
    /// [`DUE_SLACK`] of now, is served here too, at its own instant, round after round, rather than
    /// left to the caller's next turn. With the link's estimate on, str0m's pacer releases one
    /// packet per timeout and asks for the next a microsecond on (each packet leaves at its own
    /// instant, which the page's feedback reports back), while a caller comes back once a turn:
    /// on Windows a socket wait is a scheduler tick, about 15.6 ms. Measured 2026-10-03, a packet
    /// a turn held an 8 Mbit/s link to about 1.2 Mbit/s and the frames queued behind it for
    /// seconds. [`DUE_ROUNDS`] bounds a str0m that keeps answering "now".
    fn pump(&mut self, now: Instant) {
        for _ in 0..DUE_ROUNDS {
            loop {
                match self.rtc.poll_output() {
                    Ok(Output::Timeout(at)) => {
                        self.timeout = at;
                        break;
                    }
                    Ok(Output::Transmit(t)) => self.transmits.push(Transmit {
                        destination: t.destination,
                        contents: t.contents.to_vec(),
                    }),
                    Ok(Output::Event(event)) => self.event(event),
                    Err(_) => {
                        self.fail(StreamReason::ConnectionFailed);
                        self.timeout = now + Duration::from_millis(100);
                        return;
                    }
                }
            }
            if self.closed || self.timeout > now + DUE_SLACK {
                return;
            }
            if self
                .rtc
                .handle_input(Input::Timeout(self.timeout.max(now)))
                .is_err()
            {
                self.fail(StreamReason::ConnectionFailed);
                return;
            }
        }
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Connected => {
                self.connected = true;
                self.events.push(SessionEvent::Connected);
            }
            // Before the session first connects, str0m's Disconnected means only that no pair can
            // work YET. The page trickles its candidates one at a time, and one the station cannot
            // pair (an IPv6 address, beside its IPv4 socket) arriving first leaves none; ICE goes
            // on as the rest arrive, and CONNECT_TIMEOUT is when the station gives up. Once
            // connected, losing every pair is the end of the connection.
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) if self.connected => {
                self.fail(StreamReason::ConnectionFailed)
            }
            Event::MediaAdded(media)
                if media.kind == MediaKind::Video && media.direction.is_sending() =>
            {
                let pt = self.rtc.writer(media.mid).and_then(|writer| {
                    writer
                        .payload_params()
                        .find(|p| p.spec().codec == Codec::Vp8)
                        .map(|p| p.pt())
                });
                if let Some(pt) = pt {
                    self.video = Some((media.mid, pt));
                }
            }
            Event::MediaAdded(media)
                if media.kind == MediaKind::Audio && media.direction.is_receiving() =>
            {
                self.mic = Some(media.mid);
            }
            Event::MediaData(data) if Some(data.mid) == self.mic => {
                self.events.push(SessionEvent::Mic(MicPacket {
                    seq: **data.seq_range.end(),
                    rtp: data.time.numer(),
                    clock_hz: data.time.denom(),
                    arrived: data.network_time,
                    payload: data.data.to_vec(),
                }));
            }
            Event::ChannelOpen(id, label) => self.open(id, &label),
            Event::ChannelData(data) => {
                let lane = if Some(data.id) == self.control {
                    Lane::Control
                } else if Some(data.id) == self.ptt {
                    Lane::Ptt
                } else {
                    return;
                };
                self.events.push(SessionEvent::Message(lane, data.data));
            }
            Event::ChannelClose(id) => {
                if Some(id) == self.control {
                    self.fail(StreamReason::StreamClosed);
                } else if Some(id) == self.audio {
                    self.audio = None;
                    self.events.push(SessionEvent::AudioClosed);
                } else if Some(id) == self.ptt {
                    self.ptt = None;
                }
            }
            Event::KeyframeRequest(_) => self.events.push(SessionEvent::KeyframeRequest),
            Event::PeerStats(stats) => {
                if let Some(pair) = stats.selected_candidate_pair {
                    self.path = Some(crate::lan::path(pair.remote.addr));
                }
                // The estimate as str0m holds it now. Its `EgressBitrateEstimate` event is a
                // three-second average, which would follow a link that narrows seconds late.
                if let (Some(estimate), Some(link)) = (self.estimate.as_mut(), stats.bwe_tx) {
                    *estimate = u32::try_from(link.as_u64() / 1000).unwrap_or(u32::MAX);
                }
            }
            _ => {}
        }
    }

    fn open(&mut self, id: ChannelId, label: &str) {
        match label {
            CONTROL_CHANNEL => {
                let reliable = self.rtc.channel(id).is_some_and(|channel| {
                    channel
                        .config()
                        .is_some_and(|c| c.ordered && c.reliability == Reliability::Reliable)
                });
                if reliable {
                    self.control = Some(id);
                }
            }
            PTT_CHANNEL => self.ptt = Some(id),
            AUDIO_CHANNEL => {
                self.audio = Some(id);
                self.events.push(SessionEvent::AudioOpen);
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_offer() -> String {
        let file: serde_json::Value = serde_json::from_str(include_str!(
            "../../../remote/test/fixtures/stream/signal.json"
        ))
        .unwrap();
        file["roomToStation"][0]["message"]["payload"]["sdp"]
            .as_str()
            .unwrap()
            .to_string()
    }

    /// Off Windows there is no session to offer, and the refusal comes before str0m's builder,
    /// which would panic for want of a crypto provider. On Windows the same offer is answered
    /// (`the_admitted_offer_is_answered_and_connects`, below).
    #[cfg(not(windows))]
    #[test]
    fn off_windows_a_valid_offer_is_refused_unavailable_without_panicking() {
        let now = Instant::now();
        let base = "10.0.0.5:61000".parse().unwrap();
        assert_eq!(
            Session::accept(&fixture_offer(), base, now).err(),
            Some(Refusal::Unavailable)
        );
    }

    /// ICE sends checks to every candidate the page offered, and a destination that refuses one
    /// answers with ICMP "port unreachable", which Windows reports on the socket's next receive as
    /// ConnectionReset. That is the destination's word about one datagram, not a failed socket, so
    /// it does not end the session; nor does a receive that only waited. CONTROL: a socket whose
    /// network is gone does.
    #[test]
    fn a_refused_check_is_not_the_end_of_the_session() {
        use std::io::{Error, ErrorKind};
        assert!(!receive_ends_session(&Error::from(
            ErrorKind::ConnectionReset
        )));
        assert!(!receive_ends_session(&Error::from(ErrorKind::WouldBlock)));
        assert!(!receive_ends_session(&Error::from(ErrorKind::TimedOut)));
        assert!(receive_ends_session(&Error::from(ErrorKind::NetworkDown)));
    }

    /// The same on a real socket: a datagram sent where nobody listens is refused, and the socket
    /// still receives what arrives next. On Windows the refusal is that ConnectionReset
    /// (WSAECONNRESET, os error 10054). Measured against Chrome, it arrived tens of milliseconds
    /// after the answer, before DTLS, and ending the session there ended every stream. Elsewhere an
    /// unconnected socket is told nothing, and the receive only waits.
    #[test]
    fn a_datagram_refused_on_a_real_socket_leaves_it_receiving() {
        use std::net::UdpSocket;
        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        // Bound and dropped at once: nobody listens there.
        let nobody = UdpSocket::bind("127.0.0.1:0")
            .unwrap()
            .local_addr()
            .unwrap();
        socket.send_to(b"check", nobody).unwrap();
        socket
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let mut buf = [0u8; 16];
        let refused = socket
            .recv_from(&mut buf)
            .expect_err("nothing was sent to this socket");
        if cfg!(windows) {
            assert_eq!(refused.raw_os_error(), Some(10054), "{refused}");
        }
        assert!(!receive_ends_session(&refused), "{refused}");
        let peer = UdpSocket::bind("127.0.0.1:0").unwrap();
        peer.send_to(b"next", socket.local_addr().unwrap()).unwrap();
        let (n, _) = socket.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..n], b"next");
    }

    /// The offer check runs first, on every platform: a plain-RTP offer is refused as an invalid
    /// offer, never as "unavailable", so the page is told what was wrong with it.
    #[test]
    fn a_plain_rtp_offer_is_refused_before_any_session_exists() {
        let now = Instant::now();
        let base = "10.0.0.5:61000".parse().unwrap();
        let plain = fixture_offer().replace("UDP/TLS/RTP/SAVPF", "RTP/AVP");
        let refusal = Session::accept(&plain, base, now).err();
        assert_eq!(refusal, Some(Refusal::Offer(OfferRefusal::NotDtls)));
        assert_eq!(refusal.unwrap().reason(), StreamReason::InvalidOffer);
    }

    /// ★ The rule, by value (2026-10-10): every class it refuses and every one it keeps, at each
    /// edge, and every IPv6 form that carries an IPv4 address, carrying one refused and one kept.
    /// The sessions' tests (Windows) show it at work, where the page's candidates go in.
    #[test]
    fn the_station_tries_only_an_address_a_viewer_could_be_at() {
        let refused = [
            // Loopback, "this network" with the unspecified address, and link-local, where a
            // cloud's metadata service answers.
            "127.0.0.1:50000",
            "127.255.255.255:50000",
            "0.0.0.0:50000",
            "0.1.2.3:50000",
            "0.255.255.255:50000",
            "169.254.0.0:50000",
            "169.254.169.254:80",
            "169.254.255.255:50000",
            // Multicast, the reserved 240/4 and broadcast.
            "224.0.0.0:50000",
            "224.0.0.251:5353",
            "239.255.255.250:1900",
            "240.0.0.1:50000",
            "255.255.255.254:50000",
            "255.255.255.255:50000",
            // Port 0, on addresses otherwise kept.
            "203.0.113.7:0",
            "192.168.1.5:0",
            "[2001:db8::1]:0",
            // IPv6: loopback, unspecified, multicast, link-local, site-local, and the rest of what
            // is outside its global and unique-local space.
            "[::1]:50000",
            "[::]:50000",
            "[ff02::1]:50000",
            "[ff0e::1]:50000",
            "[fe80::1]:50000",
            "[febf:ffff::1]:50000",
            "[fec0::1]:50000",
            "[feff::1]:50000",
            "[fe00::1]:50000",
            "[100::1]:50000",
            "[1fff::1]:50000",
            "[4000::1]:50000",
            // IPv4-mapped and IPv4-compatible, carrying a refused IPv4 address.
            "[::ffff:127.0.0.1]:50000",
            "[::ffff:169.254.169.254]:80",
            "[::ffff:0.0.0.0]:50000",
            "[::ffff:224.0.0.1]:50000",
            "[::ffff:255.255.255.255]:50000",
            "[::127.0.0.1]:50000",
            "[::169.254.169.254]:80",
            // NAT64's well-known prefix.
            "[64:ff9b::7f00:1]:50000",
            "[64:ff9b::a9fe:a9fe]:80",
            // NAT64's local-use prefix, refused by its /48, /56, /64 and /96 readings in turn.
            "[64:ff9b:1:7f01:71:708:90a:b0c]:50000",
            "[64:ff9b:1:cb7f:71:708:90a:b0c]:50000",
            "[64:ff9b:1:cb01:a9:fe08:90a:b0c]:50000",
            "[64:ff9b:1:cb01:71:708:e00a:b0c]:50000",
            // 6to4.
            "[2002:7f00:1::1]:50000",
            "[2002:a9fe:a9fe::1]:80",
            // Teredo, whose client is 127.0.0.1 or 169.254.169.254, inverted.
            "[2001:0:4136:e378:8000:63bf:80ff:fffe]:50000",
            "[2001:0:4136:e378:8000:63bf:5601:5601]:80",
        ];
        let kept = [
            // The internet, at each edge of every block refused, and a reflexive and a relayed
            // address.
            "1.0.0.0:50000",
            "126.255.255.255:50000",
            "128.0.0.0:50000",
            "169.253.255.255:50000",
            "169.255.0.0:50000",
            "223.255.255.255:50000",
            "203.0.113.7:61000",
            "198.51.100.77:3478",
            // The private networks, and carrier-grade NAT's 100.64/10 (Tailscale's).
            "10.0.0.9:50000",
            "172.16.0.1:50000",
            "172.31.255.255:50000",
            "192.168.1.5:50000",
            "100.64.0.0:50000",
            "100.127.255.255:50000",
            "100.85.152.128:41641",
            // IPv6's global unicast space and unique-local fc00::/7, at their edges.
            "[2000::1]:50000",
            "[2001:db8::7]:61234",
            "[3fff:ffff::1]:50000",
            "[fc00::1]:50000",
            "[fdff:ffff::1]:50000",
            // Each form that carries an IPv4 address, carrying one kept.
            "[::ffff:203.0.113.7]:50000",
            "[::ffff:192.168.1.5]:50000",
            "[::203.0.113.7]:50000",
            "[64:ff9b::cb00:7107]:50000",
            "[64:ff9b:1:cb01:71:708:90a:b0c]:50000",
            "[2002:cb00:7107::1]:50000",
            "[2001:0:4136:e378:8000:63bf:34ff:8ef8]:50000",
        ];
        let wrong = |list: &[&str], keep: bool| -> Vec<String> {
            list.iter()
                .filter(|peer| may_try(peer.parse().unwrap()) != keep)
                .map(|peer| peer.to_string())
                .collect()
        };
        assert_eq!(
            (wrong(&refused, false), wrong(&kept, true)),
            (Vec::<String>::new(), Vec::<String>::new()),
            "(tried though refused, refused though kept)"
        );
    }

    /// The acceptance tests that need a real WebRTC session: Windows only, because that is where
    /// the approved crypto backend exists. A browser-shaped str0m peer plays the page, the network
    /// is an in-memory NAT, and time is driven by the test.
    #[cfg(windows)]
    mod windows {
        use super::*;
        use std::net::{IpAddr, Ipv4Addr, UdpSocket};
        use str0m::change::{SdpAnswer, SdpPendingOffer};
        use str0m::channel::ChannelConfig;
        use str0m::media::Direction;

        /// The station's LAN address (its host candidate) and what its NAT maps it to.
        const BASE: &str = "10.0.0.5:61000";
        const PUBLIC: &str = "203.0.113.7:61000";
        /// The page's own (public) candidate.
        const PAGE: &str = "198.51.100.23:51234";
        /// A page on the shack's own network, behind the same router.
        const NEIGHBOUR: &str = "10.0.0.9:51234";

        /// A page the way the contract describes it: VP8 video it receives, and the three
        /// channels with their reliability.
        fn page(now: Instant) -> (Page, String, SdpPendingOffer) {
            let (page, offer, pending, _) = page_with(now, false);
            (page, offer, pending)
        }

        /// The same page, and with `mic` its microphone too: an Opus line it sends (S6).
        fn page_with(now: Instant, mic: bool) -> (Page, String, SdpPendingOffer, Option<Mid>) {
            page_on(now, mic, PAGE)
        }

        /// The same page, at the address `at`.
        fn page_on(
            now: Instant,
            mic: bool,
            at: &str,
        ) -> (Page, String, SdpPendingOffer, Option<Mid>) {
            let mut rtc = Rtc::builder()
                .clear_codecs()
                .enable_vp8(true)
                .enable_opus(mic, false)
                .build(now);
            rtc.add_local_candidate(Candidate::host(at.parse().unwrap(), "udp").unwrap());
            let mut change = rtc.sdp_api();
            change.add_media(MediaKind::Video, Direction::RecvOnly, None, None, None);
            let mic = mic
                .then(|| change.add_media(MediaKind::Audio, Direction::SendOnly, None, None, None));
            change.add_channel_with_config(ChannelConfig {
                label: CONTROL_CHANNEL.into(),
                ..ChannelConfig::default()
            });
            for label in [PTT_CHANNEL, AUDIO_CHANNEL] {
                change.add_channel_with_config(ChannelConfig {
                    label: label.into(),
                    ordered: false,
                    reliability: Reliability::MaxRetransmits { retransmits: 0 },
                    ..ChannelConfig::default()
                });
            }
            let (offer, pending) = change.apply().unwrap();
            let mut page = Page {
                rtc,
                events: Vec::new(),
                outbox: Vec::new(),
                lost: Vec::new(),
            };
            page.drain();
            (page, offer.to_sdp_string(), pending, mic)
        }

        struct Page {
            rtc: Rtc,
            events: Vec<Event>,
            /// Datagrams the page has sent and the network has not yet delivered.
            outbox: Vec<(SocketAddr, Vec<u8>)>,
            /// Where the page sent datagrams that the network did not deliver.
            lost: Vec<SocketAddr>,
        }

        impl Page {
            /// str0m's rule, on the test's own peer too: every mutation is followed by this.
            fn drain(&mut self) {
                loop {
                    match self.rtc.poll_output().unwrap() {
                        Output::Timeout(_) => return,
                        Output::Transmit(t) => {
                            self.outbox.push((t.destination, t.contents.to_vec()))
                        }
                        Output::Event(e) => self.events.push(e),
                    }
                }
            }

            fn input(&mut self, input: Input) {
                self.rtc.handle_input(input).unwrap();
                self.drain();
            }
        }

        /// Run both peers for `span` of simulated time through a NAT that maps the station's
        /// BASE to PUBLIC, the way a home router would, for a page elsewhere (at PAGE): it can
        /// reach the station only at PUBLIC.
        fn run(page: &mut Page, station: &mut Session, now: &mut Instant, span: Duration) {
            travel(page, station, now, span, PUBLIC, PAGE);
        }

        /// Run both peers on a network where the page, at `page_at`, reaches the station only at
        /// `station_at`, and the station's datagrams reach the page from there. Everything else
        /// the page sends is lost.
        fn travel(
            page: &mut Page,
            station: &mut Session,
            now: &mut Instant,
            span: Duration,
            station_at: &str,
            page_at: &str,
        ) {
            let public: SocketAddr = station_at.parse().unwrap();
            let page_addr: SocketAddr = page_at.parse().unwrap();
            let end = *now + span;
            while *now < end {
                for (destination, packet) in std::mem::take(&mut page.outbox) {
                    if destination == public {
                        station.receive(*now, page_addr, &packet);
                    } else {
                        page.lost.push(destination);
                    }
                }
                for t in station.take_transmits() {
                    if t.destination == page_addr {
                        let receive =
                            Receive::new(Protocol::Udp, public, page_addr, &t.contents).unwrap();
                        page.input(Input::Receive(*now, receive));
                    }
                }
                *now += Duration::from_millis(5);
                page.input(Input::Timeout(*now));
                station.timeout(*now);
            }
        }

        /// Accept, answer, trickle both ways, and run until connected.
        fn connect(now: &mut Instant) -> (Page, Session, String, String) {
            let (page, station, answer, line, _) = connect_with(now, false);
            (page, station, answer, line)
        }

        /// The same, for a page with its microphone on or off.
        fn connect_with(
            now: &mut Instant,
            mic: bool,
        ) -> (Page, Session, String, String, Option<Mid>) {
            let (mut page, offer, pending, mid) = page_with(*now, mic);
            let (mut station, answer) = Session::accept(&offer, BASE.parse().unwrap(), *now)
                .expect("an admitted, valid offer is answered");
            page.rtc
                .sdp_api()
                .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
                .unwrap();
            page.drain();
            // The page is handed every candidate the station signals, in its order: the LAN
            // address with the answer, the reflexive one once STUN answers.
            if let Some(host) = station.host_candidate() {
                page.rtc
                    .add_remote_candidate(Candidate::from_sdp_string(host).unwrap());
                page.drain();
            }
            let line = station
                .add_reflexive(PUBLIC.parse().unwrap(), *now)
                .expect("a public reflexive address is advertised");
            page.rtc
                .add_remote_candidate(Candidate::from_sdp_string(&line).unwrap());
            page.drain();
            let page_line = Candidate::host(PAGE.parse().unwrap(), "udp")
                .unwrap()
                .to_sdp_string();
            assert!(station.add_remote_candidate(&page_line, *now));
            for _ in 0..40 {
                run(&mut page, &mut station, now, Duration::from_millis(250));
                if station.is_connected() {
                    break;
                }
            }
            if !station.is_connected() {
                // The evidence, if it never connects: what each side saw.
                eprintln!("station events: {:?}", station.take_events());
                eprintln!("page events: {:?}", page.events);
            }
            (page, station, answer, line, mid)
        }

        /// The real answer the compiled stream scenario hands the page
        /// (`remote/test/fixtures/stream/station-answer.json`) is what the station writes today for
        /// the page's offer beside it: the same lines in the same order, apart from what every
        /// session makes new (its ICE credentials, certificate fingerprint, stream ids and SSRCs).
        /// Red means the station's answer changed: record the pair again (the fixtures' README).
        #[test]
        fn the_recorded_answer_is_what_the_station_writes_for_the_page_offer() {
            let file: serde_json::Value = serde_json::from_str(include_str!(
                "../../../remote/test/fixtures/stream/station-answer.json"
            ))
            .unwrap();
            let offer = file["offer"].as_str().unwrap();
            let (_, answer) =
                Session::accept(offer, BASE.parse().unwrap(), Instant::now()).unwrap();
            let fresh = |sdp: &str| -> Vec<String> {
                const NEW_EACH_SESSION: [&str; 8] = [
                    "o=str0m-0.24.0 ",
                    "a=msid-semantic:",
                    "a=ice-ufrag:",
                    "a=ice-pwd:",
                    "a=fingerprint:sha-256 ",
                    "a=msid:",
                    "a=ssrc:",
                    "a=ssrc-group:FID ",
                ];
                sdp.split("\r\n")
                    .map(
                        |line| match NEW_EACH_SESSION.iter().find(|p| line.starts_with(**p)) {
                            Some(prefix) => prefix.to_string(),
                            None => line.to_string(),
                        },
                    )
                    .collect()
            };
            assert_eq!(fresh(&answer), fresh(file["answer"].as_str().unwrap()));
        }

        /// The page trickles its candidates one at a time, and the first the station can read may
        /// be one it cannot pair with its IPv4 socket: an IPv6 address, as a browser on a
        /// dual-stack network reports. For that moment str0m has no pair and says Disconnected.
        /// The session waits for the next candidate and connects. CONTROL: once connected, a page
        /// that goes silent still ends the session.
        #[test]
        fn an_unpairable_first_candidate_does_not_end_the_session() {
            let mut now = Instant::now();
            let (mut page, offer, pending) = page(now);
            // A browser's offer carries no candidates: it trickles every one afterwards. This
            // page's str0m writes its own into the offer, which would pair from the start.
            let offer: String = offer
                .split_inclusive("\r\n")
                .filter(|line| !line.starts_with("a=candidate:"))
                .collect();
            assert!(!offer.contains("a=candidate:"));
            let (mut station, answer) =
                Session::accept(&offer, BASE.parse().unwrap(), now).unwrap();
            page.rtc
                .sdp_api()
                .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
                .unwrap();
            page.drain();
            let ipv6 = "candidate:1 1 udp 1686052607 2001:db8::7 61234 typ srflx raddr :: rport 0";
            assert!(station.add_remote_candidate(ipv6, now));
            for _ in 0..50 {
                now += Duration::from_millis(10);
                station.timeout(now);
            }
            assert!(!station.is_closed(), "{:?}", station.take_events());
            // The candidates it can use arrive, and the session connects.
            let line = station.add_reflexive(PUBLIC.parse().unwrap(), now).unwrap();
            page.rtc
                .add_remote_candidate(Candidate::from_sdp_string(&line).unwrap());
            page.drain();
            let page_line = Candidate::host(PAGE.parse().unwrap(), "udp")
                .unwrap()
                .to_sdp_string();
            assert!(station.add_remote_candidate(&page_line, now));
            for _ in 0..40 {
                run(
                    &mut page,
                    &mut station,
                    &mut now,
                    Duration::from_millis(250),
                );
                if station.is_connected() {
                    break;
                }
            }
            assert!(station.is_connected(), "{:?}", station.take_events());
            assert!(!station.is_closed());
            // CONTROL: connected, and then nothing more arrives from the page.
            for _ in 0..600 {
                now += Duration::from_millis(100);
                station.timeout(now);
                let _ = station.take_transmits();
                if station.is_closed() {
                    break;
                }
            }
            assert!(
                station
                    .take_events()
                    .contains(&SessionEvent::Closed(StreamReason::ConnectionFailed)),
                "a silent page ends the connected session"
            );
        }

        /// A5, after DTLS: the station reads the certificate the page actually presented, which is
        /// the page's own (the one its offer carries and it signed). CONTROL: before DTLS there is
        /// no certificate to read.
        #[test]
        fn the_page_certificate_is_read_once_dtls_is_up() {
            let now = Instant::now();
            let (_page, offer, _pending) = page(now);
            let (mut unconnected, _) = Session::accept(&offer, BASE.parse().unwrap(), now).unwrap();
            assert_eq!(
                unconnected.remote_fingerprint(),
                None,
                "no certificate before DTLS"
            );
            let mut now = now;
            let (mut page, mut station, _, _) = connect(&mut now);
            assert!(station.is_connected(), "the session never connected");
            let presented = page.rtc.direct_api().local_dtls_fingerprint().bytes.clone();
            assert_eq!(
                station.remote_fingerprint().map(|f| f.to_vec()),
                Some(presented)
            );
        }

        /// ★ A3's positive control: the same offer that admission refuses without a lease is
        /// answered under one, and the session connects end to end.
        #[test]
        fn the_admitted_offer_is_answered_and_connects() {
            let mut now = Instant::now();
            let (page, mut station, _, _) = connect(&mut now);
            assert!(station.is_connected(), "the session never connected");
            assert!(station.take_events().contains(&SessionEvent::Connected));
            assert!(page.rtc.is_connected());
        }

        /// ★ A4: the answer is DTLS-SRTP (a SHA-256 fingerprint, only DTLS profiles), and str0m
        /// will not even answer an offer that carries no fingerprint.
        #[test]
        fn the_answer_is_dtls_srtp_and_an_offer_without_a_fingerprint_is_refused() {
            let now = Instant::now();
            let (_page, offer, _pending) = page(now);
            let (_station, answer) = Session::accept(&offer, BASE.parse().unwrap(), now).unwrap();
            assert!(answer.contains("a=fingerprint:sha-256 "), "{answer}");
            for m in answer.lines().filter(|l| l.starts_with("m=")) {
                assert!(
                    m.contains(" UDP/TLS/RTP/SAVPF ") || m.contains(" UDP/DTLS/SCTP "),
                    "a media line off DTLS: {m}"
                );
            }
            assert!(!answer.contains("RTP/AVP"));
            // CONTROL: past the station's own check, str0m itself refuses a fingerprint-less offer.
            let bare: String = offer
                .lines()
                .filter(|l| !l.starts_with("a=fingerprint:"))
                .map(|l| format!("{l}\r\n"))
                .collect();
            assert!(Session::build(&bare, BASE.parse().unwrap(), None, now).is_err());
        }

        /// ★ A8, as the operator ruled on 2026-10-03: the one LAN address the station signals is
        /// its socket's own, as its own host candidate, and nothing else it signals names one: not
        /// the answer, not the reflexive candidate's `raddr`. An overlay VPN's address is not
        /// offered; a public one on the socket is, as its own host candidate (2026-10-03).
        /// CONTROL: a host candidate for another address puts that address in the answer, and the
        /// leak check catches it.
        #[test]
        fn the_shack_offers_its_own_lan_address_and_no_other() {
            let mut now = Instant::now();
            let (_page, station, answer, line) = connect(&mut now);
            assert!(station.is_connected(), "premise: the session connected");
            let base: SocketAddr = BASE.parse().unwrap();
            let host = station
                .host_candidate()
                .expect("the station offers its LAN address");
            assert!(host.contains(" 10.0.0.5 61000 typ host"), "{host}");
            assert!(!crate::lan::leaks(host, Some(base)), "{host}");
            for text in [&answer, &line] {
                assert!(!text.contains("10.0.0.5"), "the LAN base leaked: {text}");
                assert!(!crate::lan::leaks(text, None), "{text}");
            }
            assert!(!answer.contains("a=candidate:"), "{answer}");
            assert!(
                line.contains(" typ srflx ") && line.contains("raddr 0.0.0.0 rport 0"),
                "{line}"
            );
            let (_page, offer, _pending) = page(now);
            let (overlay, _) =
                Session::accept(&offer, "100.101.102.103:61000".parse().unwrap(), now).unwrap();
            assert_eq!(overlay.host_candidate(), None);
            let public: SocketAddr = PUBLIC.parse().unwrap();
            let (_page, offer, _pending) = page(now);
            let (open, open_answer) = Session::accept(&offer, public, now).unwrap();
            let line = open
                .host_candidate()
                .expect("a public socket address is offered");
            assert!(line.contains(" 203.0.113.7 61000 typ host"), "{line}");
            assert!(!crate::lan::leaks(line, Some(public)), "{line}");
            assert!(!open_answer.contains("a=candidate:"), "{open_answer}");
            // CONTROL: with a host candidate for another address on, the answer carries a 192.168
            // address, and the leak check catches it.
            let (_page, offer, _pending) = page(now);
            let (_s, leaky) = Session::build(
                &offer,
                base,
                Some("192.168.1.20:61000".parse().unwrap()),
                now,
            )
            .unwrap();
            assert!(
                leaky.contains("192.168.1.20"),
                "control: no host candidate in {leaky}"
            );
            assert!(crate::lan::leaks(&leaky, Some(base)));
        }

        /// A browser on the shack's own network, behind the same router, which does not loop a
        /// packet sent to its own public address back in: the page reaches the station only at its
        /// LAN address, and, as Chrome hides its own behind an mDNS name, the station learns the
        /// page's address only from its checks. With the station's LAN address signalled, that is
        /// the path, and the session connects over it. Before 2026-10-03 this page never did.
        #[test]
        fn a_browser_on_the_shacks_network_connects_to_its_lan_address() {
            let mut now = Instant::now();
            let (mut page, offer, pending, _) = page_on(now, false, NEIGHBOUR);
            // As Chrome: no candidate in the offer, and none the station can use trickled after
            // it but its reflexive one, at the router's public address.
            let offer: String = offer
                .split_inclusive("\r\n")
                .filter(|line| !line.starts_with("a=candidate:"))
                .collect();
            let (mut station, answer) =
                Session::accept(&offer, BASE.parse().unwrap(), now).unwrap();
            page.rtc
                .sdp_api()
                .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
                .unwrap();
            page.drain();
            if let Some(host) = station.host_candidate() {
                page.rtc
                    .add_remote_candidate(Candidate::from_sdp_string(host).unwrap());
            }
            let line = station.add_reflexive(PUBLIC.parse().unwrap(), now).unwrap();
            page.rtc
                .add_remote_candidate(Candidate::from_sdp_string(&line).unwrap());
            page.drain();
            let reflexive =
                "candidate:1 1 udp 1686052607 203.0.113.7 51234 typ srflx raddr 0.0.0.0 rport 0";
            assert!(station.add_remote_candidate(reflexive, now));
            for _ in 0..40 {
                travel(
                    &mut page,
                    &mut station,
                    &mut now,
                    Duration::from_millis(250),
                    BASE,
                    NEIGHBOUR,
                );
                if station.is_connected() {
                    break;
                }
            }
            assert!(
                station.is_connected(),
                "a browser on the shack's network found no path: {:?}",
                station.take_events()
            );
            assert!(page.rtc.is_connected());
            // The picture goes to the page's LAN address: the shack network's budget, once the
            // statistics have read the selected pair.
            travel(
                &mut page,
                &mut station,
                &mut now,
                STATS_EVERY * 2,
                BASE,
                NEIGHBOUR,
            );
            assert_eq!(station.path(), Some(Path::Lan));
        }

        /// A browser elsewhere is handed the shack's LAN address too, and cannot reach it: its
        /// checks there are lost. It connects over the reflexive candidate, as every browser did
        /// before the station offered its LAN address.
        #[test]
        fn a_browser_that_cannot_reach_the_lan_address_connects_over_the_reflexive_one() {
            let mut now = Instant::now();
            let (mut page, mut station, _, _) = connect(&mut now);
            let base: SocketAddr = BASE.parse().unwrap();
            assert!(
                page.lost.contains(&base),
                "premise: the page tried the LAN address, {:?}",
                page.lost
            );
            assert!(station.is_connected(), "the session never connected");
            assert!(page.rtc.is_connected());
            // The picture goes to the page's public address: the internet's budget.
            run(&mut page, &mut station, &mut now, STATS_EVERY * 2);
            assert_eq!(station.path(), Some(Path::Internet));
        }

        /// A shack with a public address on its own interface, no NAT in front of it: a STUN
        /// server reports that same address, so the reflexive candidate is the host one's
        /// duplicate and str0m drops it. The station's host candidate is then the only way to it,
        /// and a browser elsewhere connects there. The browser sits behind a home router, which
        /// lets a datagram in only from an address the browser has sent to, so the station's own
        /// checks reach it only once the browser has tried the station. Before 2026-10-03 such a
        /// station signalled no candidate at all, the browser never tried it, and no browser
        /// behind a router ever connected.
        #[test]
        fn a_shack_with_a_public_address_on_its_interface_is_reached_there() {
            let mut now = Instant::now();
            let (mut page, offer, pending, _) = page_on(now, false, PAGE);
            // As Chrome: no candidate in the offer; its own arrives trickled, as a reflexive one.
            let offer: String = offer
                .split_inclusive("\r\n")
                .filter(|line| !line.starts_with("a=candidate:"))
                .collect();
            let (mut station, answer) =
                Session::accept(&offer, PUBLIC.parse().unwrap(), now).unwrap();
            page.rtc
                .sdp_api()
                .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
                .unwrap();
            page.drain();
            assert_eq!(
                station.add_reflexive(PUBLIC.parse().unwrap(), now),
                None,
                "premise: the reflexive candidate duplicates the host one"
            );
            if let Some(host) = station.host_candidate() {
                page.rtc
                    .add_remote_candidate(Candidate::from_sdp_string(host).unwrap());
                page.drain();
            }
            let reflexive =
                "candidate:1 1 udp 1686052607 198.51.100.23 51234 typ srflx raddr 0.0.0.0 rport 0";
            assert!(station.add_remote_candidate(reflexive, now));
            let (station_at, page_at): (SocketAddr, SocketAddr) =
                (PUBLIC.parse().unwrap(), PAGE.parse().unwrap());
            let mut opened = false;
            let end = now + Duration::from_secs(10);
            while now < end && !station.is_connected() {
                for (destination, packet) in std::mem::take(&mut page.outbox) {
                    if destination == station_at {
                        opened = true;
                        station.receive(now, page_at, &packet);
                    } else {
                        page.lost.push(destination);
                    }
                }
                for t in station.take_transmits() {
                    if t.destination == page_at && opened {
                        let receive =
                            Receive::new(Protocol::Udp, station_at, page_at, &t.contents).unwrap();
                        page.input(Input::Receive(now, receive));
                    }
                }
                now += Duration::from_millis(5);
                page.input(Input::Timeout(now));
                station.timeout(now);
            }
            assert!(
                station.is_connected(),
                "a shack with a public address found no path: {:?}",
                station.take_events()
            );
            assert!(page.rtc.is_connected());
        }

        /// ★ S6: the page's microphone is answered as an Opus line the station RECEIVES, and its
        /// packets come out of the session as they went in: every one, in order, on the 48 kHz
        /// clock, byte for byte. CONTROL: a page with its microphone off gets no audio line and
        /// yields no packet.
        #[test]
        fn the_pages_microphone_arrives_as_its_opus_packets() {
            use str0m::media::Frequency;
            for mic in [true, false] {
                let mut now = Instant::now();
                let (mut page, mut station, answer, _, mid) = connect_with(&mut now, mic);
                assert!(station.is_connected(), "mic {mic}: never connected");
                let audio = answer.lines().find(|l| l.starts_with("m=audio"));
                assert_eq!(audio.is_some(), mic, "mic {mic}: {answer}");
                if mic {
                    assert!(
                        answer.contains("a=recvonly"),
                        "the station would send audio: {answer}"
                    );
                    assert!(
                        answer.to_ascii_lowercase().contains("opus/48000/2"),
                        "{answer}"
                    );
                }
                station.take_events();
                let mut sent = Vec::new();
                if let Some(mid) = mid {
                    let pt = page
                        .rtc
                        .writer(mid)
                        .expect("the page's microphone line")
                        .payload_params()
                        .find(|p| p.spec().codec == Codec::Opus)
                        .expect("Opus was negotiated")
                        .pt();
                    for k in 0..5u64 {
                        // A browser's 20 ms silence frame, marked so each packet is its own.
                        let payload = vec![0xF8, 0xFF, 0xFE, k as u8];
                        page.rtc
                            .writer(mid)
                            .unwrap()
                            .write(
                                pt,
                                now,
                                MediaTime::new(960 * k, Frequency::FORTY_EIGHT_KHZ),
                                payload.clone(),
                            )
                            .unwrap();
                        page.drain();
                        sent.push(payload);
                        run(&mut page, &mut station, &mut now, Duration::from_millis(20));
                    }
                }
                run(
                    &mut page,
                    &mut station,
                    &mut now,
                    Duration::from_millis(200),
                );
                let got: Vec<MicPacket> = station
                    .take_events()
                    .into_iter()
                    .filter_map(|e| match e {
                        SessionEvent::Mic(p) => Some(p),
                        _ => None,
                    })
                    .collect();
                assert_eq!(
                    got.len(),
                    sent.len(),
                    "mic {mic}: {} packets for {} sent",
                    got.len(),
                    sent.len()
                );
                for (i, (p, payload)) in got.iter().zip(&sent).enumerate() {
                    assert_eq!(&p.payload, payload, "packet {i} changed on the way");
                    assert_eq!(p.clock_hz, 48_000);
                    if i > 0 {
                        assert_eq!(p.seq, got[i - 1].seq + 1, "packet {i} out of sequence");
                        assert_eq!(
                            p.rtp - got[i - 1].rtp,
                            960,
                            "packet {i} off the 20 ms clock"
                        );
                    }
                }
            }
        }

        /// Send the page a frame every 33 ms for `span`, as the picture does, running both peers.
        fn stream_for(page: &mut Page, station: &mut Session, now: &mut Instant, span: Duration) {
            let end = *now + span;
            while *now < end {
                assert!(station.send_video(*now, *now, &[0x10; 1100]));
                run(page, station, now, Duration::from_millis(33));
            }
        }

        /// ★ The link's estimate (2026-10-03): a page whose offer carries transport-wide feedback
        /// gets str0m's bandwidth estimation, which starts from [`START_KBPS`] and moves only with
        /// the page's own feedback on the frames it receives. Which way it moves is the bench's to
        /// show: this network hands packets over in 5 ms steps, which models no link's capacity.
        /// CONTROL: connected but sent nothing, so with no feedback yet, it is still at its start.
        #[test]
        fn the_estimate_follows_the_pages_feedback() {
            let mut now = Instant::now();
            let (mut page, mut station, _, _) = connect(&mut now);
            assert!(station.is_connected(), "the session never connected");
            run(&mut page, &mut station, &mut now, STATS_EVERY * 2);
            assert_eq!(station.estimate(), Some(START_KBPS));
            station.want(8000, now);
            stream_for(&mut page, &mut station, &mut now, Duration::from_secs(4));
            let moved = station.estimate().expect("the estimate went away");
            assert_ne!(moved, START_KBPS, "no feedback reached the estimate");
        }

        /// ★ What the pacer may send now goes now (2026-10-03). With the estimate on, str0m paces
        /// the picture and releases one packet per timeout, and a caller comes back once a turn:
        /// on Windows a socket wait is a scheduler tick, about 15.6 ms. One packet a turn held an
        /// 8 Mbit/s link to about 1.2 Mbit/s and queued the frames for seconds. A 60 kB frame on a
        /// link estimated at 8 Mbit/s now leaves in about the time the link takes to carry it.
        #[test]
        fn a_paced_frame_leaves_at_the_links_rate_not_a_packet_a_turn() {
            let mut now = Instant::now();
            let (mut page, mut station, _, _) = connect(&mut now);
            run(&mut page, &mut station, &mut now, STATS_EVERY * 2);
            station.rtc.bwe().reset(Bitrate::kbps(8000));
            station.want(9000, now);
            assert!(station.send_video(now, now, &vec![0x10; 60_000]));
            let (public, page_at): (SocketAddr, SocketAddr) =
                (PUBLIC.parse().unwrap(), PAGE.parse().unwrap());
            let (start, mut bytes) = (now, 0usize);
            while bytes < 60_000 && now < start + Duration::from_secs(2) {
                for (destination, packet) in std::mem::take(&mut page.outbox) {
                    if destination == public {
                        station.receive(now, page_at, &packet);
                    }
                }
                for t in station.take_transmits() {
                    bytes += t.contents.len();
                    let receive =
                        Receive::new(Protocol::Udp, public, page_at, &t.contents).unwrap();
                    page.input(Input::Receive(now, receive));
                }
                now += Duration::from_micros(15_600);
                page.input(Input::Timeout(now));
                station.timeout(now);
            }
            assert!(
                now - start <= Duration::from_millis(150),
                "{bytes} bytes took {:?}",
                now - start
            );
        }

        /// A page whose offer carries no transport-wide feedback gives str0m nothing to estimate
        /// with: the session has no estimate, and the picture keeps its fixed budget. CONTROL: the
        /// same page's offer as written has the feedback (the test above).
        #[test]
        fn an_offer_without_transport_feedback_has_no_estimate() {
            let now = Instant::now();
            let (_page, offer, _pending) = page(now);
            assert!(offer.contains("transport-cc") && offer.contains("transport-wide-cc"));
            let bare: String = offer
                .split_inclusive("\r\n")
                .filter(|l| !l.contains("transport-cc") && !l.contains("transport-wide-cc"))
                .collect();
            let (station, _) = Session::accept(&bare, BASE.parse().unwrap(), now).unwrap();
            assert_eq!(station.estimate(), None);
        }

        /// The data channels carry what the contract says: the page's control message arrives as
        /// a control message, and the station's reply reaches the page.
        #[test]
        fn a_control_message_crosses_both_ways() {
            let mut now = Instant::now();
            let (mut page, mut station, _, _) = connect(&mut now);
            run(
                &mut page,
                &mut station,
                &mut now,
                Duration::from_millis(500),
            );
            let control = page
                .events
                .iter()
                .find_map(|e| match e {
                    Event::ChannelOpen(id, label) if label == CONTROL_CHANNEL => Some(*id),
                    _ => None,
                })
                .expect("the control channel opened at the page");
            let sent = page
                .rtc
                .channel(control)
                .unwrap()
                .write(
                    false,
                    br#"{"type":"state","requestId":"10000000-0000-4000-8000-000000000001"}"#,
                )
                .unwrap();
            assert!(sent);
            run(
                &mut page,
                &mut station,
                &mut now,
                Duration::from_millis(500),
            );
            let events = station.take_events();
            assert!(
                events
                    .iter()
                    .any(|e| matches!(e, SessionEvent::Message(Lane::Control, _))),
                "{events:?}"
            );
            assert!(station.send(Lane::Control, r#"{"type":"pttState"}"#, now));
            run(
                &mut page,
                &mut station,
                &mut now,
                Duration::from_millis(500),
            );
            assert!(page
                .events
                .iter()
                .any(|e| matches!(e, Event::ChannelData(d) if d.id == control)));
        }

        /// A UDP socket that counts the STUN messages reaching it: where a check the station sends,
        /// or its answer to one, would land.
        struct Counter {
            socket: UdpSocket,
            at: SocketAddr,
        }

        impl Counter {
            fn on(ip: IpAddr) -> Self {
                let socket = UdpSocket::bind(SocketAddr::new(ip, 0)).unwrap();
                socket.set_nonblocking(true).unwrap();
                let at = socket.local_addr().unwrap();
                Counter { socket, at }
            }

            /// The STUN messages that have reached it since it was last asked.
            fn stun(&self) -> usize {
                std::thread::sleep(Duration::from_millis(100));
                let mut buf = [0u8; 2048];
                let mut count = 0;
                while let Ok((n, _)) = self.socket.recv_from(&mut buf) {
                    count += usize::from(stun(&buf[..n]));
                }
                count
            }
        }

        /// Is this datagram a STUN message: RFC 8489's magic cookie, in its place?
        fn stun(datagram: &[u8]) -> bool {
            datagram.len() >= 20 && datagram[4..8] == [0x21, 0x12, 0xa4, 0x42]
        }

        /// This computer's own address on its network, where the route off it leaves, found the
        /// way the station finds its own (`stream.rs`, `open_socket`). A viewer on this computer's
        /// network names an address like it.
        fn own_address() -> IpAddr {
            let probe = UdpSocket::bind("0.0.0.0:0").unwrap();
            probe
                .connect("192.0.2.1:9")
                .expect("a route off this computer");
            let ip = probe.local_addr().unwrap().ip();
            assert!(!ip.is_loopback() && !ip.is_unspecified(), "{ip}");
            ip
        }

        /// A candidate line as a page trickles it: `at`, of the kind `typ`.
        fn named(foundation: usize, at: SocketAddr, typ: &str) -> String {
            let any = if at.is_ipv4() { "0.0.0.0" } else { "::" };
            let (priority, related) = match typ {
                "host" => (2_122_260_223u32, String::new()),
                "srflx" => (1_686_052_607, format!(" raddr {any} rport 0")),
                _ => (41_885_439, format!(" raddr {any} rport 0")),
            };
            format!(
                "candidate:{foundation} 1 udp {priority} {} {} typ {typ}{related}",
                at.ip(),
                at.port()
            )
        }

        /// A page's offer with its own candidate lines taken out (a browser's carries none: it
        /// trickles every one afterwards) and `carried` put in their place.
        fn offer_carrying(offer: &str, carried: &[String]) -> String {
            let mut offer: String = offer
                .split_inclusive("\r\n")
                .filter(|line| !line.starts_with("a=candidate:"))
                .collect();
            for line in carried {
                offer.push_str(&format!("a={line}\r\n"));
            }
            offer
        }

        /// The station's session for a page whose offer carries no candidate, with the page.
        fn answered(now: Instant) -> (Page, Session, SdpPendingOffer, String) {
            let (page, offer, pending) = page(now);
            let (station, answer) =
                Session::accept(&offer_carrying(&offer, &[]), BASE.parse().unwrap(), now).unwrap();
            (page, station, pending, answer)
        }

        /// Run the station alone for `span`, and hand each datagram it sends to one of `counters`
        /// to that counter through a real socket (nothing goes anywhere else). Returns where each
        /// datagram it sent was for.
        fn drive(
            station: &mut Session,
            now: &mut Instant,
            span: Duration,
            counters: &[&Counter],
        ) -> Vec<SocketAddr> {
            let socket = UdpSocket::bind("0.0.0.0:0").unwrap();
            let mut sent = Vec::new();
            let end = *now + span;
            loop {
                for t in station.take_transmits() {
                    if counters.iter().any(|c| c.at == t.destination) {
                        socket.send_to(&t.contents, t.destination).unwrap();
                    }
                    sent.push(t.destination);
                }
                if *now >= end {
                    return sent;
                }
                *now += Duration::from_millis(10);
                station.timeout(*now);
            }
        }

        /// ★ The station tries no address a page names that no viewer could be at (2026-10-10).
        /// Before, it sent checks to whatever a page trickled: its own loopback services, the
        /// link's devices, the metadata service a cloud answers at 169.254.169.254. Counted at two
        /// real sockets: one on loopback, which the page names, and one on this computer's own
        /// address, which a viewer on its network names. The session goes on. CONTROL: that
        /// viewer's address draws checks there, and so do a reflexive, a relayed and a carrier-NAT
        /// address.
        #[test]
        fn a_page_naming_loopback_or_link_local_draws_no_check_there() {
            let mut now = Instant::now();
            let (_page, mut station, _, _) = answered(now);
            let (loopback, own) = (
                Counter::on(Ipv4Addr::LOCALHOST.into()),
                Counter::on(own_address()),
            );
            let metadata: SocketAddr = "169.254.169.254:80".parse().unwrap();
            let viewer: [(SocketAddr, &str); 4] = [
                (own.at, "host"),
                ("203.0.113.9:50001".parse().unwrap(), "srflx"),
                ("198.51.100.77:3478".parse().unwrap(), "relay"),
                ("100.64.1.2:50002".parse().unwrap(), "host"),
            ];
            let took: Vec<bool> = [(loopback.at, "host"), (metadata, "host")]
                .iter()
                .chain(&viewer)
                .enumerate()
                .map(|(n, &(at, typ))| station.add_remote_candidate(&named(n, at, typ), now))
                .collect();
            let sent = drive(
                &mut station,
                &mut now,
                Duration::from_secs(2),
                &[&loopback, &own],
            );
            let checks = |at: SocketAddr| sent.iter().filter(|&&to| to == at).count();
            let (at_loopback, at_own) = (loopback.stun(), own.stun());
            // CONTROL: every address a viewer could be at was taken, and drew checks.
            assert_eq!(took[2..], [true; 4]);
            assert!(
                at_own > 0,
                "no check reached this computer's address: {sent:?}"
            );
            for (at, typ) in viewer {
                assert!(
                    checks(at) > 0,
                    "no check to the {typ} candidate {at}: {sent:?}"
                );
            }
            assert!(!station.is_closed(), "{:?}", station.take_events());
            assert_eq!(
                (
                    &took[..2],
                    station.refused(),
                    at_loopback,
                    checks(loopback.at),
                    checks(metadata)
                ),
                (&[false, false][..], 2, 0, 0, 0),
                "(taken, refused, checks counted at loopback, sent to loopback, sent to the \
                 metadata service)"
            );
        }

        /// ★ The candidates an offer carries go through the same rule as the trickle's
        /// (2026-10-10): str0m takes an offer's own candidates as they are, and a page's offer can
        /// carry them, though a browser's carries none. CONTROL: this computer's own address, in the
        /// same offer, draws checks there.
        #[test]
        fn the_candidates_an_offer_carries_are_held_to_the_same_rule() {
            let mut now = Instant::now();
            let (_page, offer, _pending) = page(now);
            let (loopback, own) = (
                Counter::on(Ipv4Addr::LOCALHOST.into()),
                Counter::on(own_address()),
            );
            let metadata: SocketAddr = "169.254.169.254:80".parse().unwrap();
            let carried: Vec<String> = [loopback.at, metadata, own.at]
                .iter()
                .enumerate()
                .map(|(n, &at)| named(n, at, "host"))
                .collect();
            let (mut station, _) = Session::accept(
                &offer_carrying(&offer, &carried),
                BASE.parse().unwrap(),
                now,
            )
            .expect("the offer is answered");
            let sent = drive(
                &mut station,
                &mut now,
                Duration::from_secs(2),
                &[&loopback, &own],
            );
            let checks = |at: SocketAddr| sent.iter().filter(|&&to| to == at).count();
            let (at_loopback, at_own) = (loopback.stun(), own.stun());
            assert!(
                at_own > 0,
                "control: no check reached this computer's address: {sent:?}"
            );
            assert_eq!(
                (
                    station.refused(),
                    at_loopback,
                    checks(loopback.at),
                    checks(metadata)
                ),
                (2, 0, 0, 0),
                "(refused, checks counted at loopback, sent to loopback, sent to the metadata \
                 service)"
            );
        }

        /// ★ A check's source is held to the same rule (2026-10-10): str0m answers a check where it
        /// came from, and takes that address as a peer-reflexive candidate that it then checks. The
        /// page's own check, as if from loopback, is never read, and nothing goes back there.
        /// CONTROL: the same check from this computer's own address is answered there.
        #[test]
        fn a_check_from_an_address_no_viewer_could_be_at_is_never_answered() {
            let mut now = Instant::now();
            let (mut page, mut station, pending, answer) = answered(now);
            page.rtc
                .sdp_api()
                .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
                .unwrap();
            page.drain();
            let host = station.host_candidate().expect("premise: a LAN address");
            page.rtc
                .add_remote_candidate(Candidate::from_sdp_string(host).unwrap());
            page.drain();
            let base: SocketAddr = BASE.parse().unwrap();
            let mut check = None;
            for _ in 0..200 {
                check = page
                    .outbox
                    .iter()
                    .find(|(to, packet)| *to == base && stun(packet))
                    .map(|(_, packet)| packet.clone());
                if check.is_some() {
                    break;
                }
                now += Duration::from_millis(10);
                page.input(Input::Timeout(now));
            }
            let check = check.expect("premise: the page checks the station's address");
            let (loopback, own) = (
                Counter::on(Ipv4Addr::LOCALHOST.into()),
                Counter::on(own_address()),
            );
            let read_from_loopback = station.receive(now, loopback.at, &check);
            let read_from_own = station.receive(now, own.at, &check);
            let sent = drive(
                &mut station,
                &mut now,
                Duration::from_secs(1),
                &[&loopback, &own],
            );
            let (at_loopback, at_own) = (loopback.stun(), own.stun());
            assert!(
                read_from_own && at_own > 0,
                "control: the check from this computer's address was not answered: {sent:?}"
            );
            assert_eq!(
                (read_from_loopback, at_loopback, sent.contains(&loopback.at)),
                (false, 0, false),
                "(read, counted at loopback, anything sent there)"
            );
        }

        /// ★ A session tries at most sixteen of the page's candidates, its offer's and its
        /// trickle's together, and refuses any more (2026-10-10). One this IPv4 socket could never
        /// try (a TCP one, an IPv6 one) does not count, so a browser that names many of those still
        /// has the rest taken. CONTROL: the address the seventeenth names, as a session's first,
        /// draws checks.
        #[test]
        fn a_session_tries_at_most_sixteen_of_the_pages_candidates() {
            const CAP: usize = CANDIDATES;
            assert_eq!(CAP, 16);
            let own = Counter::on(own_address());
            let mut now = Instant::now();
            let (_page, offer, _pending) = page(now);
            let viewer = |n: usize| SocketAddr::from(([198, 51, 100, n as u8], 50_000));
            let carried: Vec<String> = (1..=CAP / 2)
                .map(|n| named(n, viewer(n), "srflx"))
                .collect();
            let (mut station, _) = Session::accept(
                &offer_carrying(&offer, &carried),
                BASE.parse().unwrap(),
                now,
            )
            .unwrap();
            let never = [
                "candidate:90 1 tcp 1518280447 203.0.113.5 9 typ host tcptype active".to_string(),
                named(91, "[2001:db8::5]:50000".parse().unwrap(), "srflx"),
            ];
            let unpairable: Vec<bool> = never
                .iter()
                .map(|line| station.add_remote_candidate(line, now))
                .collect();
            let trickled: Vec<bool> = (CAP / 2 + 1..=CAP)
                .map(|n| station.add_remote_candidate(&named(n, viewer(n), "srflx"), now))
                .collect();
            let past = station.add_remote_candidate(&named(CAP + 1, own.at, "host"), now);
            let sent = drive(&mut station, &mut now, Duration::from_secs(3), &[&own]);
            let checks = |at: SocketAddr| sent.iter().filter(|&&to| to == at).count();
            let untried: Vec<usize> = (1..=CAP).filter(|&n| checks(viewer(n)) == 0).collect();
            let at_own = own.stun();
            // CONTROL: a session whose first candidate names that address tries it.
            let (_page, mut first, _, _) = answered(now);
            assert!(first.add_remote_candidate(&named(0, own.at, "host"), now));
            drive(&mut first, &mut now, Duration::from_secs(1), &[&own]);
            assert!(
                own.stun() > 0,
                "control: a session's first candidate drew no check"
            );
            assert_eq!(
                (unpairable, trickled, untried),
                (vec![true, true], vec![true; CAP / 2], vec![]),
                "(never tried, the trickled half, the sixteen without a check)"
            );
            assert_eq!(
                (past, station.refused(), at_own, checks(own.at)),
                (false, 1, 0, 0),
                "the seventeenth: (taken, refused, checks counted, sent)"
            );
            assert!(!station.is_closed(), "{:?}", station.take_events());
        }

        /// A TCP candidate at a port of its own, in as few bytes as str0m reads one (so that an
        /// offer can carry many within the stream's limit): one this session's socket could never
        /// try. It draws no check, but str0m keeps it.
        fn tcp(n: usize) -> String {
            format!("candidate:{n} 1 tcp 1 203.0.113.5 {} typ host", 9_000 + n)
        }

        /// ★ A session takes at most sixty-four of the page's candidates in all (2026-10-10). A
        /// TCP one draws no check from this socket, but str0m keeps every candidate it is given,
        /// so before, a page that named them without end had every one taken. Past the total, one
        /// the session could try is refused too, though it has tried none, and the session goes
        /// on. CONTROL: that candidate, as a session's first, draws checks.
        #[test]
        fn a_session_takes_at_most_sixty_four_of_the_pages_candidates_in_all() {
            const ALL: usize = 64;
            let own = Counter::on(own_address());
            let mut now = Instant::now();
            let (_page, mut station, _, _) = answered(now);
            let took: Vec<bool> = (1..=ALL + 1)
                .map(|n| station.add_remote_candidate(&tcp(n), now))
                .collect();
            let past = station.add_remote_candidate(&named(ALL + 2, own.at, "host"), now);
            let sent = drive(&mut station, &mut now, Duration::from_secs(2), &[&own]);
            let at_own = own.stun();
            // CONTROL: a session whose first candidate names that address tries it.
            let (_page, mut first, _, _) = answered(now);
            assert!(first.add_remote_candidate(&named(0, own.at, "host"), now));
            drive(&mut first, &mut now, Duration::from_secs(1), &[&own]);
            assert!(
                own.stun() > 0,
                "control: a session's first candidate drew no check"
            );
            assert_eq!(
                (
                    took.iter().filter(|&&taken| taken).count(),
                    took[ALL],
                    past,
                    station.refused(),
                    at_own,
                    sent.contains(&own.at)
                ),
                (ALL, false, false, 2, 0, false),
                "(TCP candidates taken of the 65, the 65th, the next, which could be tried, \
                 refused, checks counted for that one, anything sent there)"
            );
            assert!(!station.is_closed(), "{:?}", station.take_events());
        }

        /// ★ The total counts what a session tries and what it never could, its offer's and its
        /// trickle's together (2026-10-10): sixteen to try and forty-eight never tried (TCP and
        /// IPv6 ones) are all taken, and one more is refused. A candidate refused on the way, by
        /// the rule or as a seventeenth to try, takes no place in it. CONTROL: the sixteen draw
        /// checks.
        #[test]
        fn sixteen_to_try_and_forty_eight_never_tried_are_taken_and_one_more_is_refused() {
            let mut now = Instant::now();
            let (_page, offer, _pending) = page(now);
            let viewer = |n: usize| SocketAddr::from(([198, 51, 100, n as u8], 50_000));
            let never = |n: usize| match n % 2 {
                0 => tcp(100 + n),
                _ => named(
                    100 + n,
                    format!("[2001:db8::{n:x}]:50000").parse().unwrap(),
                    "srflx",
                ),
            };
            // The offer carries half of each kind, and the trickle names the rest.
            let carried: Vec<String> = (1..=8)
                .map(|n| named(n, viewer(n), "srflx"))
                .chain((1..=24).map(never))
                .collect();
            let (mut station, _) = Session::accept(
                &offer_carrying(&offer, &carried),
                BASE.parse().unwrap(),
                now,
            )
            .unwrap();
            // Before the sixteen are reached, so that the rule alone refuses it.
            let loopback = station
                .add_remote_candidate(&named(18, "127.0.0.1:50000".parse().unwrap(), "host"), now);
            let to_try: Vec<bool> = (9..=16)
                .map(|n| station.add_remote_candidate(&named(n, viewer(n), "srflx"), now))
                .collect();
            let seventeenth = station.add_remote_candidate(&named(17, viewer(17), "srflx"), now);
            let never_tried: Vec<bool> = (25..=48)
                .map(|n| station.add_remote_candidate(&never(n), now))
                .collect();
            let one_more = station.add_remote_candidate(&never(49), now);
            let sent = drive(&mut station, &mut now, Duration::from_secs(3), &[]);
            let checks = |at: SocketAddr| sent.iter().filter(|&&to| to == at).count();
            let untried: Vec<usize> = (1..=16).filter(|&n| checks(viewer(n)) == 0).collect();
            // CONTROL: the sixteen were taken and drew checks, beside the forty-eight.
            assert_eq!(
                (to_try, never_tried, untried),
                (vec![true; 8], vec![true; 24], vec![]),
                "(the trickle's eight to try, its twenty-four never tried, the sixteen without a \
                 check)"
            );
            assert_eq!(
                (seventeenth, loopback, one_more, station.refused()),
                (false, false, false, 3),
                "(taken: the seventeenth to try, loopback, the sixty-fifth; refused)"
            );
            assert!(!station.is_closed(), "{:?}", station.take_events());
        }

        /// ★ The candidates an offer carries count toward the same total, and one past it is
        /// refused at that door too (2026-10-10). The stream's limit on an offer's size leaves room
        /// for more than sixty-four short lines. The offer is answered, the session goes on, and
        /// the sixty-fifth, one the session could try, draws no check. CONTROL: the same line, as
        /// an offer's only candidate, draws checks.
        #[test]
        fn an_offer_carrying_more_than_sixty_four_candidates_has_the_rest_refused() {
            const ALL: usize = 64;
            let own = Counter::on(own_address());
            let mut now = Instant::now();
            let past = named(ALL + 1, own.at, "host");
            let (_page, offer, _pending) = page(now);
            let mut carried: Vec<String> = (1..=ALL).map(tcp).collect();
            carried.push(past.clone());
            let sdp = offer_carrying(&offer, &carried);
            assert!(
                sdp.len() <= crate::protocol::SDP_BYTES,
                "premise: the offer fits the stream's limit ({} bytes)",
                sdp.len()
            );
            let (mut station, _) =
                Session::accept(&sdp, BASE.parse().unwrap(), now).expect("the offer is answered");
            let sent = drive(&mut station, &mut now, Duration::from_secs(2), &[&own]);
            let at_own = own.stun();
            // CONTROL: an offer whose only candidate is that line has it tried.
            let (_page, offer, _pending) = page(now);
            let (mut only, _) =
                Session::accept(&offer_carrying(&offer, &[past]), BASE.parse().unwrap(), now)
                    .unwrap();
            drive(&mut only, &mut now, Duration::from_secs(1), &[&own]);
            assert!(
                own.stun() > 0,
                "control: an offer's only candidate drew no check"
            );
            assert_eq!(
                (station.refused(), at_own, sent.contains(&own.at)),
                (1, 0, false),
                "(refused, checks counted for the sixty-fifth, anything sent there)"
            );
            assert!(!station.is_closed(), "{:?}", station.take_events());
        }

        /// A candidate named by a host name (a browser's mDNS `.local` one, or any other name) is
        /// never looked up, trickled or in an offer: it is not taken, it is not a refusal, and
        /// nothing is sent for it. An offer carrying one is answered, as it was before this rule
        /// (measured). `localhost` names the loopback counter's port: before the rule, when the
        /// station took loopback, a check would have reached it had the name been resolved.
        /// CONTROL: the counter counts a STUN message sent to it straight.
        #[test]
        fn a_candidate_named_by_a_host_name_is_never_resolved_or_tried() {
            let mut now = Instant::now();
            let (_page, mut station, _, _) = answered(now);
            let loopback = Counter::on(Ipv4Addr::LOCALHOST.into());
            let names = [
                format!(
                    "candidate:1 1 udp 2122260223 localhost {} typ host",
                    loopback.at.port()
                ),
                "candidate:2 1 udp 2122260223 4d2a8f1e-8c1b-4f3a-9e2d-2c3b4a5d6e7f.local 54321 \
                 typ host"
                    .to_string(),
            ];
            let took: Vec<bool> = names
                .iter()
                .map(|line| station.add_remote_candidate(line, now))
                .collect();
            let sent = drive(&mut station, &mut now, Duration::from_secs(1), &[&loopback]);
            let (_page, offer, _pending) = page(now);
            let (mut offered, _) =
                Session::accept(&offer_carrying(&offer, &names), BASE.parse().unwrap(), now)
                    .expect("an offer carrying a host name is answered");
            let sent_for_offer =
                drive(&mut offered, &mut now, Duration::from_secs(1), &[&loopback]);
            assert_eq!(
                (
                    took,
                    sent,
                    sent_for_offer,
                    station.refused() + offered.refused(),
                    loopback.stun()
                ),
                (vec![false, false], vec![], vec![], 0, 0),
                "(taken, sent for the trickled, sent for the offer's, refused, counted at loopback)"
            );
            // CONTROL: a binding request's header, sent there straight.
            let mut header = [0u8; 20];
            header[1] = 1;
            header[4..8].copy_from_slice(&[0x21, 0x12, 0xa4, 0x42]);
            let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
            socket.send_to(&header, loopback.at).unwrap();
            assert_eq!(loopback.stun(), 1, "control: the counter missed a datagram");
        }

        /// A viewer only a TURN relay can reach, which trickles its relayed candidate, connects
        /// there.
        #[test]
        fn a_viewer_behind_a_relay_connects_over_its_relayed_candidate() {
            const RELAY: &str = "198.51.100.77:3478";
            let mut now = Instant::now();
            let (mut page, offer, pending, _) = page_on(now, false, RELAY);
            let (mut station, answer) =
                Session::accept(&offer_carrying(&offer, &[]), BASE.parse().unwrap(), now).unwrap();
            page.rtc
                .sdp_api()
                .accept_answer(pending, SdpAnswer::from_sdp_string(&answer).unwrap())
                .unwrap();
            page.drain();
            if let Some(host) = station.host_candidate() {
                page.rtc
                    .add_remote_candidate(Candidate::from_sdp_string(host).unwrap());
            }
            let line = station.add_reflexive(PUBLIC.parse().unwrap(), now).unwrap();
            page.rtc
                .add_remote_candidate(Candidate::from_sdp_string(&line).unwrap());
            page.drain();
            assert!(station.add_remote_candidate(&named(1, RELAY.parse().unwrap(), "relay"), now));
            for _ in 0..40 {
                travel(
                    &mut page,
                    &mut station,
                    &mut now,
                    Duration::from_millis(250),
                    PUBLIC,
                    RELAY,
                );
                if station.is_connected() {
                    break;
                }
            }
            assert!(
                station.is_connected(),
                "a viewer behind a relay found no path: {:?}",
                station.take_events()
            );
            assert!(page.rtc.is_connected());
        }
    }
}
