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
//! - **No LAN address leaves the shack (A8).** The only candidate the station ever signals is the
//!   server-reflexive address a STUN server reported ([`Session::add_reflexive`]), whose `raddr`
//!   str0m writes as `0.0.0.0 0`. str0m does need the socket's own (host) address as a local
//!   candidate: it answers connectivity checks only on a host or relay candidate, and discards
//!   every other one as an unknown interface (measured: srflx alone never connects). So the host
//!   candidate is added AFTER the answer is written, and is never trickled; nothing re-negotiates,
//!   so no later SDP can carry it, and ICE's own messages carry no local address. Every SDP and
//!   candidate line is read by [`crate::lan::leaks`] before it is handed out, and a leak is never
//!   handed out.
//! - **The page's certificate must match its offer.** str0m verifies the peer's DTLS certificate
//!   against the offer's fingerprint by default; [`Session::accept`] asserts that default rather than
//!   assuming it. The device-key binding (security test A5, a later piece) signs that same
//!   fingerprint, read with [`crate::offer::fingerprint`], and its check slots in between admission
//!   and `accept`.
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
    /// The local socket address. Never signalled: it is only the base of the reflexive candidate.
    base: SocketAddr,
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
    /// `host` exists for one test: A8's positive control, which shows what a host candidate WOULD
    /// put in the answer.
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
        if host.is_none() && crate::lan::leaks(&answer) {
            return Err(Refusal::Unavailable);
        }
        // Only now, with the answer written: the socket's own address, for ICE to use and never to
        // signal (see the module header for why str0m needs it and why it cannot leak from here).
        let own = Candidate::host(base, "udp").map_err(|_| Refusal::Unavailable)?;
        rtc.add_local_candidate(own);
        let mid = sdp
            .lines()
            .find_map(|l| l.trim().strip_prefix("a=mid:"))
            .unwrap_or("0")
            .to_string();
        let mut session = Session {
            rtc,
            base,
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
        (!crate::lan::leaks(&line)).then_some(line)
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
            Event::IceConnectionStateChange(IceConnectionState::Disconnected) => {
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

        /// The station's LAN address (never signalled) and what its NAT maps it to.
        const BASE: &str = "10.0.0.5:61000";
        const PUBLIC: &str = "203.0.113.7:61000";
        /// The page's own (public) candidate.
        const PAGE: &str = "198.51.100.23:51234";

        /// A page the way the contract describes it: VP8 video it receives, and the three
        /// channels with their reliability.
        fn page(now: Instant) -> (Page, String, SdpPendingOffer) {
            let (page, offer, pending, _) = page_with(now, false);
            (page, offer, pending)
        }

        /// The same page, and with `mic` its microphone too: an Opus line it sends (S6).
        fn page_with(now: Instant, mic: bool) -> (Page, String, SdpPendingOffer, Option<Mid>) {
            let mut rtc = Rtc::builder()
                .clear_codecs()
                .enable_vp8(true)
                .enable_opus(mic, false)
                .build(now);
            rtc.add_local_candidate(Candidate::host(PAGE.parse().unwrap(), "udp").unwrap());
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
            };
            page.drain();
            (page, offer.to_sdp_string(), pending, mic)
        }

        struct Page {
            rtc: Rtc,
            events: Vec<Event>,
            /// Datagrams the page has sent and the network has not yet delivered.
            outbox: Vec<(SocketAddr, Vec<u8>)>,
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
        /// BASE to PUBLIC, the way a home router would.
        fn run(page: &mut Page, station: &mut Session, now: &mut Instant, span: Duration) {
            let public: SocketAddr = PUBLIC.parse().unwrap();
            let page_addr: SocketAddr = PAGE.parse().unwrap();
            let end = *now + span;
            while *now < end {
                for (destination, packet) in std::mem::take(&mut page.outbox) {
                    if destination == public {
                        station.receive(*now, page_addr, &packet);
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

        /// ★ A8: no LAN address in anything the station signals; the control shows a host
        /// candidate WOULD have put one in the answer.
        #[test]
        fn no_lan_address_leaves_the_shack() {
            let mut now = Instant::now();
            let (_page, station, answer, line) = connect(&mut now);
            assert!(
                station.is_connected(),
                "premise: the session connected on srflx alone"
            );
            for text in [&answer, &line] {
                assert!(!text.contains("10.0.0.5"), "the LAN base leaked: {text}");
                assert!(!crate::lan::leaks(text), "{text}");
            }
            assert!(
                line.contains(" typ srflx ") && line.contains("raddr 0.0.0.0 rport 0"),
                "{line}"
            );
            // CONTROL: with a host candidate on, the answer carries a 192.168 address, and the
            // leak check catches it.
            let (_page, offer, _pending) = page(now);
            let (_s, leaky) = Session::build(
                &offer,
                BASE.parse().unwrap(),
                Some("192.168.1.20:61000".parse().unwrap()),
                now,
            )
            .unwrap();
            assert!(
                leaky.contains("192.168.1.20"),
                "control: no host candidate in {leaky}"
            );
            assert!(crate::lan::leaks(&leaky));
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
