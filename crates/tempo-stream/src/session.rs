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
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use str0m::change::SdpOffer;
use str0m::channel::{ChannelId, Reliability};
use str0m::format::Codec;
use str0m::media::{MediaKind, MediaTime, Mid, Pt};
use str0m::net::{Protocol, Receive};
use str0m::{Candidate, Event, IceConnectionState, Input, Output, Rtc};

use crate::frame_clock::VideoClock;
use crate::offer::{self, OfferRefusal};
use crate::protocol::{StreamReason, AUDIO_CHANNEL, CONTROL_CHANNEL, PTT_CHANNEL};

/// How long ICE and DTLS may take before the session is given up on.
pub const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

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
        let parsed = SdpOffer::from_sdp_string(sdp)
            .map_err(|_| Refusal::Offer(OfferRefusal::Unparseable))?;
        let config = Rtc::builder()
            .clear_codecs()
            .enable_vp8(true)
            // The page's microphone (S6). No RED: the page sends plain Opus.
            .enable_opus(true, false)
            .set_ice_lite(false);
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
            started: now,
            connected: false,
            closed: false,
            timeout: now,
            transmits: Vec::new(),
            events: Vec::new(),
        };
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

    /// A candidate the page trickled. One the station cannot use (an mDNS host name it cannot
    /// resolve, a malformed line) is ignored: ICE works with whatever candidates remain.
    pub fn add_remote_candidate(&mut self, line: &str, now: Instant) -> bool {
        if self.closed {
            return false;
        }
        let line = line.trim();
        let line = line.strip_prefix("a=").unwrap_or(line);
        let Ok(candidate) = Candidate::from_sdp_string(line) else {
            return false;
        };
        self.rtc.add_remote_candidate(candidate);
        self.pump(now);
        true
    }

    /// A datagram that arrived on the session's socket. Returns false when it was not the
    /// session's (a STUN answer to the station's own binding request, say).
    pub fn receive(&mut self, now: Instant, source: SocketAddr, data: &[u8]) -> bool {
        if self.closed {
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

    /// Drain str0m to its next timeout: the one rule its API has.
    fn pump(&mut self, now: Instant) {
        loop {
            match self.rtc.poll_output() {
                Ok(Output::Timeout(at)) => {
                    self.timeout = at;
                    return;
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

    /// The acceptance tests that need a real WebRTC session: Windows only, because that is where
    /// the approved crypto backend exists. A browser-shaped str0m peer plays the page, the network
    /// is an in-memory NAT, and time is driven by the test.
    #[cfg(windows)]
    mod windows {
        use super::*;
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
        }

        /// A browser elsewhere is handed the shack's LAN address too, and cannot reach it: its
        /// checks there are lost. It connects over the reflexive candidate, as every browser did
        /// before the station offered its LAN address.
        #[test]
        fn a_browser_that_cannot_reach_the_lan_address_connects_over_the_reflexive_one() {
            let mut now = Instant::now();
            let (page, station, _, _) = connect(&mut now);
            let base: SocketAddr = BASE.parse().unwrap();
            assert!(
                page.lost.contains(&base),
                "premise: the page tried the LAN address, {:?}",
                page.lost
            );
            assert!(station.is_connected(), "the session never connected");
            assert!(page.rtc.is_connected());
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
    }
}
