//! Remote as a stream, the station's side of it in the Remote service: the `streamSignal` lane
//! the relay socket feeds, and the one thread that carries a streamed session end to end.
//!
//! A streamed session shows the remote operator the station's own Nexus window over WebRTC and
//! carries their input back into it. The WebRTC session itself (the offer check, no LAN address,
//! the page's certificate) is `tempo_stream::session`; this file is where it meets the station.
//!
//! ## Where each rule is kept
//!
//! - **Admission (security test A3): [`admit`], before anything of the session exists.** The
//!   audio lane's rule, one definition in the operations authority: the operator's local control
//!   grant plus this browser's own live lease. Then the operator's switch, then the offer check
//!   and the platform. Only after all of them does the station open a socket, and only after that
//!   does a WebRTC session exist. It is asked again every second while the stream runs, so a
//!   lapsed lease or a revoked device ends it.
//! - **Transmit presence (S8) and a fresh picture (S9): [`Presence`].** Minted by the operations
//!   authority when the session connects and on every heartbeat whose picture is fresh, held by
//!   the engine, which stops every transmission at the station when it lapses. A heartbeat with a
//!   stale picture still renews the lease: the operator stays connected, just without the power
//!   to keep the station transmitting.
//! - **Input and the held PTT only while presence is live.** Blind means no authority, and that
//!   is where a streamed operator gains authority: a click on the picture, a held PTT.
//! - **The existing protocol on the data channel (S10).** `state`, `heartbeat`, `release` and
//!   `stopTransmit` go to the operations authority exactly as the relay's do, at operation version
//!   4, stamped with this session's identity from its admission, never from the channel.
//! - **Input into Nexus only (S11).** Admitted input goes to the station's own main window as the
//!   `remote-stream-input` event, through [`Host::input`]. There is no OS input call anywhere on
//!   this path.
//! - **Receive audio (S5)** is the relay's own audio lane, unaddressed, on the `audio` channel:
//!   the same encoder, the same bounds, the same "drop, never queue".
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ring::rand::{SecureRandom, SystemRandom};
use serde_json::Value;
use tempo_app::remote_control::ptt_hold::{HoldEnd, PttHold};
use tempo_app::remote_control::transmit::TransmitPermit;
use tempo_stream::protocol::{
    self, BrowserSignal, ControlIn, ControlOut, PttIn, PttReason, StationSignal, StationToRoom,
    StreamReason, WebviewInput,
};
use tempo_stream::session::{Lane, Session, SessionEvent};

use super::operations::{Authority, Request};

/// How often a running stream re-asks whether its browser still controls the station.
const RECHECK: Duration = Duration::from_secs(1);
/// The longest the session thread waits for a datagram before it looks at everything else.
const TICK: Duration = Duration::from_millis(10);
/// When to ask the STUN server again, after the first request, if no answer has come.
const STUN_RETRIES: [Duration; 4] = [
    Duration::from_millis(250),
    Duration::from_millis(750),
    Duration::from_millis(1750),
    Duration::from_millis(3750),
];
/// Every data-channel request is answered at the operation version that carries the stop token.
const OPERATION_VERSION: u8 = 4;

/// Hands one admitted input to the station's own main window.
pub type InputSink = Arc<dyn Fn(&WebviewInput) + Send + Sync>;

/// What the application gives the stream. Cheap to clone.
#[derive(Clone, Default)]
pub struct Host {
    /// Where admitted input goes: the station's own main window, as the `remote-stream-input`
    /// event. `None` in a build or a test with no window, and then input is dropped.
    pub input: Option<InputSink>,
    /// The held PTT the engine keys and releases. Installed into the engine when Remote is built.
    pub ptt: PttHold,
}

/// What one session needs from the station.
#[derive(Clone)]
pub(super) struct Station {
    pub authority: Arc<Authority>,
    pub engine: crate::SharedEngine,
    /// The relay connection the session was admitted on. Requests go to the authority under it,
    /// as the relay's own do, so a reconnect retires them as it retires the relay's.
    pub connection: u64,
    pub host: Host,
    #[cfg(feature = "radio")]
    pub audio: Option<Arc<tempo_audio::receive_audio::ReceiveAudioFeed>>,
}

/// An offer, and the identity the relay stamped on it.
#[derive(Clone)]
pub(super) struct Offer {
    pub session: String,
    pub device: String,
    pub lease: String,
    pub sdp: String,
}

/// Everything that must hold before the station opens a socket for an offer, in order.
pub(super) fn admit(station: &Station, offer: &Offer, now: Instant) -> Result<(), StreamReason> {
    // A3. A busy authority is contention, not a refusal: ask again, briefly.
    let mut admitted =
        station
            .authority
            .stream_admitted(&offer.session, &offer.device, &offer.lease, now);
    for _ in 0..20 {
        if admitted != Err("remoteBusy") {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
        admitted = station.authority.stream_admitted(
            &offer.session,
            &offer.device,
            &offer.lease,
            Instant::now(),
        );
    }
    admitted.map_err(|_| StreamReason::NotController)?;
    let enabled = tempo_app::engine::engine_lock(&station.engine)
        .settings()
        .remote_stream;
    if !enabled {
        return Err(StreamReason::StreamDisabled);
    }
    Session::precheck(&offer.sdp).map_err(|refusal| refusal.reason())
}

/// A message to the relay about one session, or nothing if it would break a bound.
fn state(session: &str, streaming: bool, reason: Option<StreamReason>) -> Option<String> {
    StationToRoom::StreamState {
        session_id: session.to_string(),
        streaming,
        reason,
    }
    .to_wire()
}

enum Signal {
    Candidate(String),
    Close,
}

struct Live {
    session: String,
    signals: mpsc::Sender<Signal>,
    thread: std::thread::JoinHandle<()>,
}

/// The `streamSignal` lane of one relay connection. One stream at a time: the thread it starts
/// owns the session, and this only routes signals to it.
#[derive(Default)]
pub(super) struct StreamLane {
    live: Option<Live>,
}

impl StreamLane {
    /// A `streamSignal` the relay stamped. What to tell the relay at once, if anything; the rest
    /// comes from the session thread through `to_relay`.
    pub fn signal(
        &mut self,
        station: &Station,
        to_relay: &tokio::sync::mpsc::UnboundedSender<String>,
        (session, device, lease): (String, String, String),
        payload: BrowserSignal,
    ) -> Option<String> {
        use tempo_stream::protocol::Validate;
        if self.live.as_ref().is_some_and(|l| l.thread.is_finished()) {
            self.live = None;
        }
        if !payload.valid() {
            return state(&session, false, Some(StreamReason::InvalidOffer));
        }
        match payload {
            BrowserSignal::Offer { sdp } => {
                if self.live.is_some() {
                    return state(&session, false, Some(StreamReason::StreamInUse));
                }
                let (signals, inbox) = mpsc::channel();
                let offer = Offer {
                    session: session.clone(),
                    device,
                    lease,
                    sdp,
                };
                let station = station.clone();
                let to_relay = to_relay.clone();
                let thread = std::thread::Builder::new()
                    .name("nexus-remote-stream".into())
                    .spawn(move || run(station, offer, to_relay, inbox));
                match thread {
                    Ok(thread) => {
                        self.live = Some(Live {
                            session,
                            signals,
                            thread,
                        });
                        None
                    }
                    Err(_) => state(&session, false, Some(StreamReason::StreamUnavailable)),
                }
            }
            BrowserSignal::Candidate { candidate, .. } => {
                if let Some(live) = self.live.as_ref().filter(|l| l.session == session) {
                    let _ = live.signals.send(Signal::Candidate(candidate));
                }
                None
            }
            BrowserSignal::Close {} => {
                self.session_gone(&session);
                None
            }
        }
    }

    /// The relay says this session is gone, or the page closed its stream: the stream ends.
    pub fn session_gone(&mut self, session: &str) {
        if let Some(live) = self.live.as_ref().filter(|l| l.session == session) {
            let _ = live.signals.send(Signal::Close);
        }
    }
}

/// The streamed session's transmit presence, as this thread holds it: the permit the engine
/// holds, or the one it is about to.
#[derive(Default)]
pub(super) struct Presence {
    installed: Option<TransmitPermit>,
    pending: Option<TransmitPermit>,
}

impl Presence {
    /// Mint and install a fresh presence permit (S8). A refusal leaves presence to lapse on its own.
    pub fn renew(&mut self, station: &Station, offer: &Offer, now: Instant) {
        if let Ok(permit) =
            station
                .authority
                .stream_presence(&offer.session, &offer.device, &offer.lease, now)
        {
            self.pending = Some(permit);
            self.install(station, now);
        }
    }

    /// Hand a minted permit to the engine. The engine's lock is only tried, never waited for, on
    /// this thread; a busy engine is tried again next tick, and until then presence is not live.
    pub fn install(&mut self, station: &Station, now: Instant) {
        let Some(permit) = self.pending.take() else {
            return;
        };
        let engine = match tempo_app::engine::engine_try_lock(&station.engine) {
            Ok(engine) => engine,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                self.pending = Some(permit);
                return;
            }
        };
        let mut engine = engine;
        engine.hold_remote_presence(permit.clone(), now);
        self.installed = Some(permit);
    }

    /// Is presence live: installed in the engine, and neither lapsed nor revoked?
    pub fn live(&self, now: Instant) -> bool {
        self.pending.is_none() && self.installed.as_ref().is_some_and(|p| p.valid(now))
    }
}

/// A reply on `control`.
fn reply(
    request_id: String,
    result: Result<Value, &'static str>,
    presence: Option<bool>,
) -> String {
    let (value, error) = match result {
        Ok(value) => (Some(value), None),
        Err(error) => (None, Some(error.to_string())),
    };
    serde_json::to_string(&ControlOut::OperationResponse {
        request_id,
        value,
        error,
        presence,
    })
    .unwrap_or_default()
}

/// One session's state on its thread, apart from the WebRTC session itself.
pub(super) struct Streaming {
    pub station: Station,
    pub offer: Offer,
    pub presence: Presence,
    checked: Instant,
    #[cfg(feature = "radio")]
    audio: super::audio::AudioLane,
}

impl Streaming {
    pub fn new(station: Station, offer: Offer, now: Instant) -> Self {
        Self {
            station,
            offer,
            presence: Presence::default(),
            checked: now,
            #[cfg(feature = "radio")]
            audio: super::audio::AudioLane::unaddressed(),
        }
    }

    fn handle(&self, request: &Request, now: Instant) -> Result<Value, &'static str> {
        self.station.authority.handle_version(
            (self.station.connection, OPERATION_VERSION),
            &self.offer.session,
            &self.offer.device,
            request,
            &self.station.engine,
            now,
        )
    }

    /// A message on `control`. `fresh` is whether the picture a heartbeat names is fresh, which
    /// only the WebRTC session can say. Returns the reply to send, if any, and whether the page
    /// released its lease (the stream ends then).
    pub fn control(
        &mut self,
        bytes: &[u8],
        fresh: impl FnOnce(Option<u32>) -> bool,
        now: Instant,
    ) -> (Option<String>, bool) {
        // A malformed message is dropped, not answered: the page's own parser is the one that
        // refused to write it, and nothing here depends on hearing it.
        let Ok(message) = protocol::parse_control(bytes) else {
            return (None, false);
        };
        match message {
            ControlIn::State { request_id } => {
                let result = self.handle(
                    &Request::State {
                        request_id: request_id.clone(),
                    },
                    now,
                );
                (Some(reply(request_id, result, None)), false)
            }
            ControlIn::Heartbeat {
                request_id,
                lease_id,
                decoded_frame_at,
            } => {
                let result = self.handle(
                    &Request::Heartbeat {
                        request_id: request_id.clone(),
                        lease_id: lease_id.clone(),
                    },
                    now,
                );
                // S9: the lease is renewed above whatever the picture; presence only for a fresh
                // one, and only under the lease this stream was admitted with.
                if result.is_ok() && lease_id == self.offer.lease && fresh(decoded_frame_at) {
                    self.presence.renew(&self.station, &self.offer, now);
                }
                let live = self.presence.live(now);
                (Some(reply(request_id, result, Some(live))), false)
            }
            ControlIn::Release {
                request_id,
                lease_id,
            } => {
                let result = self.handle(
                    &Request::Release {
                        request_id: request_id.clone(),
                        lease_id,
                    },
                    now,
                );
                (Some(reply(request_id, result, None)), true)
            }
            ControlIn::StopTransmit {
                request_id,
                station_boot_id,
                lease_id,
                transmit_epoch,
            } => {
                let result = self.handle(
                    &Request::StopTransmit {
                        request_id: request_id.clone(),
                        station_boot_id,
                        lease_id,
                        transmit_epoch,
                    },
                    now,
                );
                (Some(reply(request_id, result, None)), false)
            }
            input => {
                // S11: into Nexus only, and only while presence is live.
                if self.presence.live(now) {
                    if let (Some(deliver), Some(input)) = (&self.station.host.input, input.input())
                    {
                        deliver(&input);
                    }
                }
                (None, false)
            }
        }
    }

    /// A message on `ptt` (S7). A hold is taken only while presence is live; a release always.
    pub fn ptt(&mut self, bytes: &[u8], now: Instant) {
        let Ok(message) = protocol::parse_ptt(bytes) else {
            return;
        };
        match message {
            PttIn::PttHold { hold_id, .. } => {
                if self.presence.live(now) {
                    self.station.host.ptt.hold(&hold_id, now);
                }
            }
            PttIn::PttRelease { hold_id, .. } => self.station.host.ptt.release(&hold_id),
        }
    }

    /// What the held PTT did since the last call, for the page.
    pub fn ptt_reports(&self) -> Vec<String> {
        self.station
            .host
            .ptt
            .reports()
            .into_iter()
            .filter_map(|r| {
                serde_json::to_string(&ControlOut::PttState {
                    hold_id: r.hold_id,
                    keyed: r.keyed,
                    reason: r.end.map(|end| match end {
                        HoldEnd::Refused => PttReason::Refused,
                        HoldEnd::Lapsed => PttReason::Lapsed,
                        HoldEnd::Released => PttReason::Released,
                        HoldEnd::Stopped => PttReason::Stopped,
                    }),
                })
                .ok()
            })
            .collect()
    }

    /// Does the browser still control the station? Asked once a second; a busy authority is not
    /// a refusal.
    pub fn still_admitted(&mut self, now: Instant) -> bool {
        if now.saturating_duration_since(self.checked) < RECHECK {
            return true;
        }
        self.checked = now;
        let admitted = self.station.authority.stream_admitted(
            &self.offer.session,
            &self.offer.device,
            &self.offer.lease,
            now,
        );
        !matches!(admitted, Err(reason) if reason != "remoteBusy")
    }

    /// The page opened its `audio` channel: start feeding it, and say whether that worked.
    #[cfg(feature = "radio")]
    fn audio_open(&mut self, now: Instant) -> String {
        let started = match self.station.audio.as_ref() {
            None => Err("audioUnavailable"),
            Some(feed) => self.audio.start(
                feed,
                &self.offer.session,
                &self.offer.device,
                &self.offer.lease,
                now,
            ),
        };
        let value = match started {
            Ok(()) => super::audio::audio_state_value(true, None),
            Err(reason) => {
                super::audio::audio_state_value(false, Some(super::audio::shared_reason(reason)))
            }
        };
        value.to_string()
    }

    /// Receive audio due now, as messages for the `audio` channel.
    #[cfg(feature = "radio")]
    fn audio_due(&mut self, now: Instant) -> Option<String> {
        if !self.audio.listening() {
            return None;
        }
        let pump = self.audio.poll(now, true);
        match pump.ended {
            Some(reason) => Some(
                super::audio::audio_state_value(false, Some(super::audio::shared_reason(reason)))
                    .to_string(),
            ),
            None => pump.message,
        }
    }
}

/// The STUN exchange that learns the session socket's reflexive address.
struct Reflexive {
    server: SocketAddr,
    transaction: [u8; 12],
    started: Instant,
    tries: usize,
    found: bool,
}

impl Reflexive {
    /// Ask now if it is time to (again). Returns the datagram to send.
    fn due(&mut self, now: Instant) -> Option<[u8; 20]> {
        if self.found || self.tries > STUN_RETRIES.len() {
            return None;
        }
        let at = match self.tries {
            0 => self.started,
            n => self.started + STUN_RETRIES[n - 1],
        };
        if now < at {
            return None;
        }
        self.tries += 1;
        Some(tempo_stream::stun::request(&self.transaction))
    }
}

/// The session socket: bound to the address the station reaches the internet from, which is the
/// base of its reflexive candidate and is never signalled.
fn open_socket() -> Option<(UdpSocket, Reflexive)> {
    let server = tempo_stream::stun::SERVER
        .to_socket_addrs()
        .ok()?
        .find(SocketAddr::is_ipv4)?;
    // A connected socket learns which interface the route to the server leaves by; nothing is sent.
    let probe = UdpSocket::bind("0.0.0.0:0").ok()?;
    probe.connect(server).ok()?;
    let ip = probe.local_addr().ok()?.ip();
    let socket = UdpSocket::bind(SocketAddr::new(ip, 0)).ok()?;
    let mut transaction = [0u8; 12];
    SystemRandom::new().fill(&mut transaction).ok()?;
    Some((
        socket,
        Reflexive {
            server,
            transaction,
            started: Instant::now(),
            tries: 0,
            found: false,
        },
    ))
}

/// One streamed session, from an offer to its end, on its own thread.
fn run(
    station: Station,
    offer: Offer,
    to_relay: tokio::sync::mpsc::UnboundedSender<String>,
    inbox: mpsc::Receiver<Signal>,
) {
    let send = |text: Option<String>| {
        if let Some(text) = text {
            let _ = to_relay.send(text);
        }
    };
    let session_id = offer.session.clone();
    if let Err(reason) = admit(&station, &offer, Instant::now()) {
        send(state(&session_id, false, Some(reason)));
        return;
    }
    let Some((socket, mut reflexive)) = open_socket() else {
        send(state(
            &session_id,
            false,
            Some(StreamReason::StreamUnavailable),
        ));
        return;
    };
    let Ok(base) = socket.local_addr() else {
        send(state(
            &session_id,
            false,
            Some(StreamReason::StreamUnavailable),
        ));
        return;
    };
    let (mut session, answer) = match Session::accept(&offer.sdp, base, Instant::now()) {
        Ok(accepted) => accepted,
        Err(refusal) => {
            send(state(&session_id, false, Some(refusal.reason())));
            return;
        }
    };
    send(
        StationToRoom::StreamSignal {
            session_id: session_id.clone(),
            payload: StationSignal::Answer { sdp: answer },
        }
        .to_wire(),
    );
    tempo_core::applog::info("remote", "stream: offer answered");
    let mut streaming = Streaming::new(station.clone(), offer, Instant::now());
    let mut ended = StreamReason::StreamClosed;
    let mut buf = vec![0u8; 2048];
    loop {
        let now = Instant::now();
        if let Some(request) = reflexive.due(now) {
            let _ = socket.send_to(&request, reflexive.server);
        }
        let wait = session
            .next_timeout()
            .saturating_duration_since(now)
            .clamp(Duration::from_millis(1), TICK);
        let _ = socket.set_read_timeout(Some(wait));
        match socket.recv_from(&mut buf) {
            Ok((n, source)) => {
                let packet = &buf[..n];
                let now = Instant::now();
                match tempo_stream::stun::mapped(packet, &reflexive.transaction) {
                    Some(public) if source == reflexive.server => {
                        reflexive.found = true;
                        if let Some(candidate) = session.add_reflexive(public, now) {
                            send(
                                StationToRoom::StreamSignal {
                                    session_id: session_id.clone(),
                                    payload: StationSignal::Candidate {
                                        candidate,
                                        sdp_mid: session.mid().to_string(),
                                    },
                                }
                                .to_wire(),
                            );
                        }
                    }
                    _ => {
                        session.receive(now, source, packet);
                    }
                }
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => session.close(StreamReason::ConnectionFailed, Instant::now()),
        }
        let now = Instant::now();
        loop {
            match inbox.try_recv() {
                Ok(Signal::Candidate(line)) => {
                    session.add_remote_candidate(&line, now);
                }
                Ok(Signal::Close) | Err(TryRecvError::Disconnected) => {
                    session.close(StreamReason::StreamClosed, now);
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        if now >= session.next_timeout() {
            session.timeout(now);
        }
        for event in session.take_events() {
            match event {
                SessionEvent::Connected => {
                    // S8: presence from the moment the session is live.
                    streaming.presence.renew(&station, &streaming.offer, now);
                    send(state(&session_id, true, None));
                    tempo_core::applog::info("remote", "stream: connected");
                }
                SessionEvent::Message(Lane::Control, bytes) => {
                    let (answer, released) =
                        streaming.control(&bytes, |decoded| session.fresh(decoded, now), now);
                    if let Some(answer) = answer {
                        session.send(Lane::Control, &answer, now);
                    }
                    if released {
                        session.close(StreamReason::StreamClosed, now);
                    }
                }
                SessionEvent::Message(Lane::Ptt, bytes) => streaming.ptt(&bytes, now),
                SessionEvent::Message(Lane::Audio, _) => {}
                SessionEvent::AudioOpen => {
                    #[cfg(feature = "radio")]
                    {
                        let said = streaming.audio_open(now);
                        session.send(Lane::Audio, &said, now);
                    }
                }
                SessionEvent::AudioClosed => {
                    #[cfg(feature = "radio")]
                    streaming.audio.stop(None);
                }
                SessionEvent::KeyframeRequest => {}
                SessionEvent::Closed(reason) => ended = reason,
            }
        }
        streaming.presence.install(&station, now);
        for report in streaming.ptt_reports() {
            session.send(Lane::Control, &report, now);
        }
        #[cfg(feature = "radio")]
        if let Some(audio) = streaming.audio_due(now) {
            session.send(Lane::Audio, &audio, now);
        }
        if !streaming.still_admitted(now) {
            session.close(StreamReason::NotController, now);
        }
        for transmit in session.take_transmits() {
            let _ = socket.send_to(&transmit.contents, transmit.destination);
        }
        if session.is_closed() {
            for event in session.take_events() {
                if let SessionEvent::Closed(reason) = event {
                    ended = reason;
                }
            }
            break;
        }
    }
    // The session is over, however it ended. Its presence goes (the engine halts on its next tick
    // if it held any), a held PTT is released, and the window lets go of anything still pressed.
    station.authority.end_stream_presence();
    station.host.ptt.end();
    if let Some(deliver) = &station.host.input {
        deliver(&WebviewInput::Reset {});
    }
    #[cfg(feature = "radio")]
    streaming.audio.stop(None);
    send(state(&session_id, false, Some(ended)));
    tempo_core::applog::info("remote", "stream: ended");
}

#[cfg(test)]
mod tests;
