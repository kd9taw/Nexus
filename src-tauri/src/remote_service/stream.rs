//! Remote as a stream, the station's side of it in the Remote service: the `streamSignal` lane
//! the relay socket feeds, and the one thread that carries a streamed session end to end.
//!
//! A streamed session shows the remote operator the station's own Nexus window over WebRTC and
//! carries their input back into it. The WebRTC session itself (the offer check, the one address of
//! its own it offers, the addresses of the page's it will try, the page's certificate) is
//! `tempo_stream::session`; this file is where it meets the station.
//!
//! ## Where each rule is kept
//!
//! - **Admission (security test A3): [`admit`], before anything of the session exists.** The
//!   audio lane's rule, one definition in the operations authority: the operator's local control
//!   grant plus this browser's own live lease. Then the operator's switch, then the device key
//!   (A5), then the offer check and the platform. Only after all of them does the station open a
//!   socket, and only after that does a WebRTC session exist. The grant, the lease and the switch
//!   are asked again every second while the stream runs, so a lapsed lease, a revoked device or the
//!   switch turned off at the shack ends it ([`Streaming::still_admitted`]).
//! - **The browser's own device key (security test A5): [`verify`], then [`Streaming::connected`].**
//!   The relay stamps the device and session on every signal, so without this a relay could offer
//!   in any granted browser's name (security review M1). The operator pinned this browser's key
//!   (SHA-256 of its SPKI) when they approved it at the radio; the offer must carry that key and
//!   its signature over SHA-256 of the offer's DTLS fingerprint and the station's, device's and
//!   session's ids. A browser with no pin is refused `deviceNotPinned`; anything else that fails,
//!   an unsigned offer included, `deviceKeyMismatch`. Once DTLS is up, the certificate the page
//!   actually presented must be the one it signed, or the session ends there: before presence, and
//!   before any input or PTT is taken.
//! - **The station's own key (security review S3-M1): the answer, in [`run`].** The answer is signed
//!   with the key its pairing record at the service holds (`station_key`), over both DTLS
//!   fingerprints and the station's, device's and session's ids, so the page can tell the station
//!   from a relay that answers in its place. Without a key the answer goes unsigned, and the page
//!   refuses it.
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
//! - **Two roads, one chain (the operator's ruling of 2026-10-04, "both at once").** A session
//!   comes by the relay's road or, from a paired computer on the shack's own network, by the LAN
//!   road (`super::lan`). Everything above and below is the same for both; what differs is the
//!   socket ([`Road`]): on the LAN road it is on the shack's own address and port, it asks no STUN
//!   server, and nothing off that network's subnet is heard or tried. One stream at a time for the
//!   whole station, whichever road (`Authority::claim_stream`), because a stream's end is the
//!   station's.
//! - **Receive audio (S5)** is the relay's own audio lane, unaddressed, on the `audio` channel:
//!   the same encoder, the same bounds, the same "drop, never queue".
//! - **The page's microphone (S6)** arrives as the session's Opus track. Each packet is decoded
//!   here, on the session's thread, to the transmit route's 12 kHz and handed to [`Host::mic`],
//!   which takes it only while the streamed operator's over is armed. What keys, and when, is the
//!   engine's alone (`tempo_app::mic`, `engine/remote_mic.rs`): nothing on this thread keys a rig.
//! - **The picture (S1, S2) is the main window's, and nothing else's (A1).** The application
//!   resolves its `main` window's handle through [`Host::window`]; the station checks it can be
//!   captured before it opens a socket, starts capturing when the session connects, and stops when
//!   the session ends. `tempo_stream::video` does the capture and the encoding, no larger than the
//!   page says it shows the picture (`view` on `control`, [`Streaming::view`]), within the
//!   budget of the path the picture takes (`Session::path`), and at what the link to the page
//!   carries (`Session::estimate`, 2026-10-03), which the session is told to look for up to what
//!   the whole picture needs (`Session::want`): only its size, frame rate and bit rate change.
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ring::digest::{digest, SHA256};
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};
use serde_json::Value;
use tempo_app::remote_control::ptt_hold::{HoldEnd, PttHold};
use tempo_app::remote_control::transmit::TransmitPermit;
use tempo_stream::protocol::{
    self, BrowserSignal, ControlIn, ControlOut, PttIn, PttReason, StationSignal, StationToRoom,
    StreamReason, WebviewInput,
};
use tempo_stream::session::{receive_ends_session, Lane, Session, SessionEvent};

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

/// The station's own main window, as the handle its capture is built from (Windows' `HWND`), if
/// it has one right now.
pub type WindowHandle = Arc<dyn Fn() -> Option<isize> + Send + Sync>;

/// What the application gives the stream. Cheap to clone.
#[derive(Clone, Default)]
pub struct Host {
    /// Where admitted input goes: the station's own main window, as the `remote-stream-input`
    /// event. `None` in a build or a test with no window, and then input is dropped.
    pub input: Option<InputSink>,
    /// The held PTT the engine arms and releases. Installed into the engine when Remote is built.
    pub ptt: PttHold,
    /// The page's microphone, decoded, on its way to the engine's microphone over. Installed into
    /// the engine when Remote is built; it takes audio only while an over is armed.
    pub mic: tempo_app::mic::MicFeed,
    /// The one window a stream shows: the station's `main` window. `None` where there is none
    /// (a test, or a build without a window), and then a stream is answered unavailable.
    pub window: Option<WindowHandle>,
    /// The shack kept awake, system and display, while a stream is attached (the operator's pick
    /// "Keep awake during streams"). Held from a session's connection until its state is gone.
    pub awake: tempo_stream::keep_awake::KeepAwake,
}

/// The device key the operator pinned for a browser when they approved it at the radio (A5), as
/// the SHA-256 of its SPKI, or `None` when there is none. Read at admission, never cached.
pub type PinnedKeys = Arc<dyn Fn(&str) -> Option<[u8; 32]> + Send + Sync>;

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
    pub audio: Option<Arc<super::audio::ReceiveFanout>>,
    /// The station's own id, as its pairing holds it: one of the ids a browser signs (A5).
    pub station_id: String,
    pub pinned: PinnedKeys,
    /// The station's own key, which signs its answer (S3-M1). `None` without one: the answer then
    /// goes unsigned, and the page refuses it, saying to update Nexus at the shack.
    pub signer: Option<Arc<super::station_key::Signer>>,
}

/// An offer, and the identity the relay stamped on it.
#[derive(Clone)]
pub(super) struct Offer {
    pub session: String,
    pub device: String,
    pub lease: String,
    pub sdp: String,
    /// The browser's device key (SPKI, lowercase hex) and its signature over this offer (A5), as
    /// the page sent them: both or neither, which the parser has already held it to.
    pub public_key: Option<String>,
    pub signature: Option<String>,
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
    verify(station, offer)?;
    Session::precheck(&offer.sdp).map_err(|refusal| refusal.reason())
}

/// A5 at admission: the offer is signed by the device key the operator pinned for this browser at
/// the radio, over SHA-256 of the offer's DTLS fingerprint and the station's, device's and
/// session's ids (the contract's README). The device and session are the relay's stamp, the station
/// id this station's own: a signature made for any other session, browser or station is refused.
fn verify(station: &Station, offer: &Offer) -> Result<(), StreamReason> {
    let pinned = (station.pinned)(&offer.device).ok_or(StreamReason::DeviceNotPinned)?;
    let mismatch = StreamReason::DeviceKeyMismatch;
    let (Some(key), Some(signature)) = (offer.public_key.as_deref(), offer.signature.as_deref())
    else {
        return Err(mismatch);
    };
    let key = protocol::hex_bytes(key).ok_or(mismatch)?;
    let signature = protocol::hex_bytes(signature).ok_or(mismatch)?;
    if digest(&SHA256, &key).as_ref() != pinned {
        return Err(mismatch);
    }
    let fingerprint = protocol::offer_fingerprint(&offer.sdp).ok_or(StreamReason::InvalidOffer)?;
    let fingerprint: [u8; 32] = digest(&SHA256, &fingerprint)
        .as_ref()
        .try_into()
        .map_err(|_| mismatch)?;
    let signed = protocol::offer_binding(
        &fingerprint,
        &station.station_id,
        &offer.device,
        &offer.session,
    );
    // An SPKI ends with the uncompressed point, which is what ring verifies against.
    let point = key.get(key.len().saturating_sub(65)..).ok_or(mismatch)?;
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point)
        .verify(&signed, &signature)
        .map_err(|_| mismatch)
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

#[derive(Debug, PartialEq)]
enum Signal {
    Candidate(String),
    /// The session ends, and why: the page closed it, or the relay ended it.
    Close(StreamReason),
}

struct Live {
    session: String,
    signals: mpsc::Sender<Signal>,
    thread: std::thread::JoinHandle<()>,
}

/// The road a session came by (the operator's ruling of 2026-10-04, "both at once"). On either road
/// the session itself then tries, and hears, only an address a viewer could be at
/// (`tempo_stream::session::may_try`).
#[derive(Clone, Copy)]
enum Road {
    /// The relay's: a socket on the address the route to the STUN server leaves by, and the
    /// reflexive candidate that server reports.
    Relay,
    /// Remote over this network: a socket on the shack's own address and port, no STUN, and
    /// nothing tried, sent or heard off that network's subnet (`tempo_stream::lan::Network`).
    Lan {
        network: tempo_stream::lan::Network,
        port: u16,
    },
}

impl Road {
    /// May a datagram from `source` reach the session? On the LAN road, only one from the shack's
    /// own subnet.
    fn hears(&self, source: SocketAddr) -> bool {
        match self {
            Road::Relay => true,
            Road::Lan { network, .. } => network.contains(source.ip()),
        }
    }

    /// May the session try a candidate the page trickled? On the LAN road, only one on the shack's
    /// own subnet.
    fn tries(&self, candidate: &str) -> bool {
        match self {
            Road::Relay => true,
            Road::Lan { network, .. } => network.candidate(candidate).is_some(),
        }
    }
}

/// The `streamSignal` lane of one relay connection, or of one computer's connection on the LAN
/// road. One stream at a time for the whole station: the thread it starts owns the session, and
/// this only routes signals to it.
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
        ids: (String, String, String),
        payload: BrowserSignal,
    ) -> Option<String> {
        self.signal_on(station, to_relay, ids, payload, Road::Relay)
    }

    /// The same for a session on the LAN road, which the LAN channel stamped from the key its
    /// connection was opened with: the session's socket is on `network`'s address at `port`.
    pub fn signal_lan(
        &mut self,
        station: &Station,
        to_peer: &tokio::sync::mpsc::UnboundedSender<String>,
        ids: (String, String, String),
        payload: BrowserSignal,
        network: tempo_stream::lan::Network,
        port: u16,
    ) -> Option<String> {
        self.signal_on(station, to_peer, ids, payload, Road::Lan { network, port })
    }

    fn signal_on(
        &mut self,
        station: &Station,
        to_relay: &tokio::sync::mpsc::UnboundedSender<String>,
        (session, device, lease): (String, String, String),
        payload: BrowserSignal,
        road: Road,
    ) -> Option<String> {
        use tempo_stream::protocol::Validate;
        if self.live.as_ref().is_some_and(|l| l.thread.is_finished()) {
            self.live = None;
        }
        if !payload.valid() {
            return state(&session, false, Some(StreamReason::InvalidOffer));
        }
        match payload {
            // The device key and its signature (A5) are checked at admission, on the session thread.
            BrowserSignal::Offer {
                sdp,
                public_key,
                signature,
            } => {
                if self.live.is_some() {
                    return state(&session, false, Some(StreamReason::StreamInUse));
                }
                // One stream for the whole station, whichever road: another lane's stream
                // must have finished ending, its presence and the held PTT with it.
                let Some(claim) = station.authority.claim_stream() else {
                    return state(&session, false, Some(StreamReason::StreamInUse));
                };
                let (signals, inbox) = mpsc::channel();
                let offer = Offer {
                    session: session.clone(),
                    device,
                    lease,
                    sdp,
                    public_key,
                    signature,
                };
                let station = station.clone();
                let to_relay = to_relay.clone();
                let thread = std::thread::Builder::new()
                    .name("nexus-remote-stream".into())
                    .spawn(move || {
                        // Held until the session has finished ending, however it ends.
                        let _claim = claim;
                        run(station, offer, to_relay, inbox, road)
                    });
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
            let _ = live.signals.send(Signal::Close(StreamReason::StreamClosed));
        }
    }

    /// The relay ended this session's stream (`streamEnd`: Remote access switched off, operator
    /// decision 2026-09-27, "within about 2 s"). Its transmit presence ends HERE, on the relay's
    /// own thread and at once, so the radio loop halts on its next poll whatever the session thread
    /// is doing; the session thread is then told to tear the rest down, with the relay's reason.
    pub fn end(&mut self, station: &Station, session: &str, reason: StreamReason) {
        if let Some(live) = self.live.as_ref().filter(|l| l.session == session) {
            station.authority.end_stream_presence();
            let _ = live.signals.send(Signal::Close(reason));
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
    /// When the operator's switch was last read. A busy engine leaves it as it was, so the switch
    /// is read again next tick.
    switch_checked: Instant,
    /// Was presence live at the last look? Its lapse is when the window lets go of anything the
    /// streamed operator was holding down.
    was_live: bool,
    /// A5: the page's DTLS certificate is the one its offer was signed over. Until it is, nothing
    /// on `control` or `ptt` is taken.
    verified: bool,
    #[cfg(feature = "radio")]
    audio: super::audio::AudioLane,
    /// This session's decoder for the page's microphone. One per session: a decoder carries state
    /// from packet to packet. `None` if libopus would not start one, and then the audio is dropped.
    #[cfg(feature = "radio")]
    mic: Option<tempo_audio::mic_decode::MicDecoder>,
    /// The microphone over as the page was last told it (`micState`), so it is told of changes only.
    mic_told: tempo_app::mic::MicStatus,
    /// The shack held awake from the connection on. Dropped with this state, which the session
    /// loop drops however the stream ends, a panic included.
    awake: Option<tempo_stream::keep_awake::Attached>,
    /// The page's picture area in its own device pixels, once it has said (`view`).
    view: Option<(u32, u32)>,
}

impl Streaming {
    pub fn new(station: Station, offer: Offer, now: Instant) -> Self {
        // Whatever the over was before this session is not this page's to hear about.
        let mic_told = station.host.mic.status();
        Self {
            station,
            offer,
            presence: Presence::default(),
            checked: now,
            switch_checked: now,
            was_live: false,
            verified: false,
            #[cfg(feature = "radio")]
            audio: super::audio::AudioLane::unaddressed(),
            #[cfg(feature = "radio")]
            mic: tempo_audio::mic_decode::MicDecoder::new().ok(),
            mic_told,
            awake: None,
            view: None,
        }
    }

    /// The size the page last said it shows the picture at, in its own device pixels.
    pub fn view(&self) -> Option<(u32, u32)> {
        self.view
    }

    /// The microphone over's state for the page, when it has changed since the page was last told.
    pub fn mic_state(&mut self) -> Option<String> {
        let now = self.station.host.mic.status();
        if now == self.mic_told {
            return None;
        }
        self.mic_told = now;
        serde_json::to_string(&ControlOut::MicState {
            armed: now.armed,
            keyed: now.keyed,
            no_power_out: now.no_power_out,
            ended: now.ended.map(wire_ended),
        })
        .ok()
    }

    /// A packet of the page's microphone (S6): decoded to the transmit route's rate and offered to
    /// the feed, which keeps it only while an over is armed. Decoded either way, so the decoder's
    /// state is current when an over does arm. A packet libopus refuses is dropped: its moment is
    /// silence, as a lost one's is.
    #[cfg(feature = "radio")]
    pub fn mic(&mut self, packet: &tempo_stream::session::MicPacket) {
        use tempo_audio::mic_decode::media_position;
        let Some(samples) = self.mic.as_mut().and_then(|d| d.decode(&packet.payload)) else {
            return;
        };
        let _ = self.station.host.mic.push(tempo_app::mic::MicFrame {
            seq: packet.seq,
            media: media_position(packet.rtp, packet.clock_hz),
            arrived: packet.arrived,
            samples,
        });
    }

    /// The session is connected: DTLS is up. `remote` is the certificate fingerprint the page's
    /// DTLS actually presented. A5's second half: it must be the one the page signed, or the session
    /// ends here, before presence and before anything on `control` or `ptt` is taken. Then S8:
    /// presence from the moment the session is live.
    pub fn connected(
        &mut self,
        remote: Option<[u8; 32]>,
        now: Instant,
    ) -> Result<(), StreamReason> {
        let signed = protocol::offer_fingerprint(&self.offer.sdp);
        if signed.is_none() || remote != signed {
            return Err(StreamReason::DeviceKeyMismatch);
        }
        self.verified = true;
        self.presence.renew(&self.station, &self.offer, now);
        // Attached: the shack stays awake, system and display, until this stream is gone.
        self.awake = Some(self.station.host.awake.attach());
        Ok(())
    }

    /// Once per lapse of presence, the window's `reset`: a button or key a blind operator was
    /// holding down is released, as the contract promises, not left held until their next input.
    pub fn watch_presence(&mut self, now: Instant) {
        let live = self.presence.live(now);
        if self.was_live && !live {
            if let Some(deliver) = &self.station.host.input {
                deliver(&WebviewInput::Reset {});
            }
        }
        self.was_live = live;
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
        // A5: nothing from a page whose certificate has not been checked.
        if !self.verified {
            return (None, false);
        }
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
            // The size the page shows the picture at: not input, so it needs no presence.
            ControlIn::View { width, height } => {
                self.view = Some((width, height));
                (None, false)
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

    /// A message on `ptt` (S7). A hold, and the page's `held` set, are taken only while presence
    /// is live; a release always.
    pub fn ptt(&mut self, bytes: &[u8], now: Instant) {
        let Ok(message) = protocol::parse_ptt(bytes) else {
            return;
        };
        // A5: nothing from a page whose certificate has not been checked.
        if !self.verified {
            return;
        }
        match message {
            PttIn::PttHold { hold_id, .. } => {
                if self.presence.live(now) {
                    self.station.host.ptt.hold(&hold_id, now);
                }
            }
            PttIn::PttRelease { hold_id, .. } => self.station.host.ptt.release(&hold_id),
            // The page's held keys and buttons, re-asserted, to the window: input, so only while
            // presence is live (a lapse sends `reset` anyway).
            PttIn::Held(held) => {
                if self.presence.live(now) {
                    if let Some(deliver) = &self.station.host.input {
                        deliver(&WebviewInput::Held(held));
                    }
                }
            }
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

    /// Does the browser still control the station, and is the operator's switch ("Stream this
    /// station from my browser") still on? Each is asked once a second. A busy authority is not a
    /// refusal, and a busy engine is not the switch turned off: this thread only tries the engine's
    /// lock, as for presence, and tries again next tick. The switch turned off at the shack ends the
    /// stream as a revoked device does, and the page is told `streamDisabled`.
    pub fn still_admitted(&mut self, now: Instant) -> Result<(), StreamReason> {
        if now.saturating_duration_since(self.checked) >= RECHECK {
            self.checked = now;
            let admitted = self.station.authority.stream_admitted(
                &self.offer.session,
                &self.offer.device,
                &self.offer.lease,
                now,
            );
            if matches!(admitted, Err(reason) if reason != "remoteBusy") {
                return Err(StreamReason::NotController);
            }
        }
        if now.saturating_duration_since(self.switch_checked) < RECHECK {
            return Ok(());
        }
        let engine = match tempo_app::engine::engine_try_lock(&self.station.engine) {
            Ok(engine) => engine,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return Ok(()),
        };
        self.switch_checked = now;
        if engine.settings().remote_stream {
            Ok(())
        } else {
            Err(StreamReason::StreamDisabled)
        }
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

/// Why an over ended, in the contract's words: each end under its own name, so the page can say
/// the right thing (the audio design's §7).
fn wire_ended(why: tempo_app::mic::MicEnded) -> tempo_stream::protocol::MicEnded {
    use tempo_app::mic::MicEnded as Engine;
    use tempo_stream::protocol::MicEnded as Wire;
    match why {
        Engine::Released => Wire::Released,
        Engine::Stopped => Wire::Stopped,
        Engine::AudioGap => Wire::AudioGap,
        Engine::Presence => Wire::Presence,
        Engine::Ceiling => Wire::Ceiling,
        Engine::Watchdog => Wire::Watchdog,
        Engine::RouteChanged => Wire::RouteChanged,
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
/// base of its reflexive candidate, and its host candidate when it is the shack's LAN address or a
/// public one (`tempo_stream::lan::host`).
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

/// The session socket on the LAN road: the shack's own address and the port the operator set, so
/// the shack is reachable on two known ports and no other. Its host candidate is that address.
fn lan_socket(network: tempo_stream::lan::Network, port: u16) -> Option<UdpSocket> {
    if !tempo_stream::lan::listenable(network.address()) {
        return None;
    }
    UdpSocket::bind(SocketAddr::new(network.address().into(), port)).ok()
}

/// One streamed session, from an offer to its end, on its own thread.
fn run(
    station: Station,
    offer: Offer,
    to_relay: tokio::sync::mpsc::UnboundedSender<String>,
    inbox: mpsc::Receiver<Signal>,
    road: Road,
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
    // The picture: the main window, and it must be capturable, before anything opens.
    let Some(window) = station
        .host
        .window
        .as_ref()
        .and_then(|window| window())
        .filter(|&window| tempo_stream::video::available(window))
    else {
        send(state(
            &session_id,
            false,
            Some(StreamReason::StreamUnavailable),
        ));
        return;
    };
    let opened = match road {
        Road::Relay => open_socket().map(|(socket, reflexive)| (socket, Some(reflexive))),
        // No STUN on the shack's own network: its own address is the one candidate.
        Road::Lan { network, port } => lan_socket(network, port).map(|socket| (socket, None)),
    };
    let Some((socket, mut reflexive)) = opened else {
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
    // On the LAN road the offer goes in without any candidate off the shack's subnet.
    let sdp = match road {
        Road::Relay => std::borrow::Cow::Borrowed(offer.sdp.as_str()),
        Road::Lan { network, .. } => std::borrow::Cow::Owned(network.offer(&offer.sdp)),
    };
    let (mut session, answer) = match Session::accept(&sdp, base, Instant::now()) {
        Ok(accepted) => accepted,
        Err(refusal) => {
            send(state(&session_id, false, Some(refusal.reason())));
            return;
        }
    };
    // S3-M1: signed with the station's own key, for this offer, this browser and this session, so
    // the page can tell the station from a relay that answers in its place.
    let signed = station.signer.as_ref().and_then(|signer| {
        signer.sign_answer(
            &offer.sdp,
            &answer,
            &station.station_id,
            &offer.device,
            &session_id,
        )
    });
    if signed.is_none() {
        tempo_core::applog::info("remote", "stream: answer unsigned, no station key");
    }
    let answer = signed.unwrap_or(answer);
    send(
        StationToRoom::StreamSignal {
            session_id: session_id.clone(),
            payload: StationSignal::Answer { sdp: answer },
        }
        .to_wire(),
    );
    tempo_core::applog::info("remote", "stream: offer answered");
    // The shack's own LAN address, straight after the answer, so a browser on the same network
    // connects directly (the operator's ruling of 2026-10-03, `tempo_stream::lan`), or its public
    // address when no NAT stands in front of it. The reflexive candidate follows once STUN answers.
    if let Some(candidate) = session.host_candidate() {
        send(
            StationToRoom::StreamSignal {
                session_id: session_id.clone(),
                payload: StationSignal::Candidate {
                    candidate: candidate.to_string(),
                    sdp_mid: session.mid().to_string(),
                },
            }
            .to_wire(),
        );
    }
    let mut streaming = Streaming::new(station.clone(), offer, Instant::now());
    let mut video: Option<tempo_stream::video::Video> = None;
    let mut ended = StreamReason::StreamClosed;
    let mut buf = vec![0u8; 2048];
    let mut noted = false;
    loop {
        let now = Instant::now();
        if let Some(reflexive) = reflexive.as_mut() {
            if let Some(request) = reflexive.due(now) {
                let _ = socket.send_to(&request, reflexive.server);
            }
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
                let mapped = reflexive.as_mut().and_then(|reflexive| {
                    tempo_stream::stun::mapped(packet, &reflexive.transaction)
                        .filter(|_| source == reflexive.server)
                        .map(|public| (reflexive, public))
                });
                match mapped {
                    Some((reflexive, public)) => {
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
                    // On the LAN road nothing from off the shack's subnet reaches the session.
                    None if !road.hears(source) => {}
                    None => {
                        session.receive(now, source, packet);
                    }
                }
            }
            // A report about one datagram. One too long for `buf` is dropped here: the part of it
            // Windows hands back with the error never reaches the session.
            Err(e) if !receive_ends_session(&e) => {}
            Err(_) => session.close(StreamReason::ConnectionFailed, Instant::now()),
        }
        let now = Instant::now();
        loop {
            match inbox.try_recv() {
                Ok(Signal::Candidate(line)) => {
                    // …and no candidate off it is ever tried.
                    if road.tries(&line) {
                        session.add_remote_candidate(&line, now);
                    }
                }
                Ok(Signal::Close(reason)) => {
                    session.close(reason, now);
                    break;
                }
                Err(TryRecvError::Disconnected) => {
                    session.close(StreamReason::StreamClosed, now);
                    break;
                }
                Err(TryRecvError::Empty) => break,
            }
        }
        // A candidate the session refused is dropped and the stream goes on. The shack's log says
        // so once, in these words, and never names the address.
        if !noted && session.refused() > 0 {
            noted = true;
            tempo_core::applog::info(
                "remote",
                "stream: refused an address the page named, one no viewer could be at or one past \
                 the stream's limit",
            );
        }
        if now >= session.next_timeout() {
            session.timeout(now);
        }
        for event in session.take_events() {
            match event {
                SessionEvent::Connected => {
                    // A5, then S8: the page's certificate is the one it signed, and presence from
                    // the moment the session is live.
                    if let Err(reason) = streaming.connected(session.remote_fingerprint(), now) {
                        session.close(reason, now);
                        continue;
                    }
                    // S1, S2: the picture starts with the session. No picture, no stream.
                    video = tempo_stream::video::Video::start(window).ok();
                    if video.is_none() {
                        session.close(StreamReason::StreamUnavailable, now);
                    } else {
                        send(state(&session_id, true, None));
                        tempo_core::applog::info("remote", "stream: connected");
                    }
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
                SessionEvent::KeyframeRequest => {
                    if let Some(video) = &video {
                        video.request_keyframe();
                    }
                }
                SessionEvent::Mic(packet) => {
                    #[cfg(feature = "radio")]
                    streaming.mic(&packet);
                    #[cfg(not(feature = "radio"))]
                    let _ = packet;
                }
                SessionEvent::Closed(reason) => ended = reason,
            }
        }
        streaming.presence.install(&station, now);
        streaming.watch_presence(now);
        for report in streaming.ptt_reports() {
            session.send(Lane::Control, &report, now);
        }
        if let Some(state) = streaming.mic_state() {
            session.send(Lane::Control, &state, now);
        }
        #[cfg(feature = "radio")]
        if let Some(audio) = streaming.audio_due(now) {
            session.send(Lane::Audio, &audio, now);
        }
        if let Some(picture) = &video {
            picture.fit(session.path(), streaming.view(), session.estimate());
            if let Some(link) = picture.wanted() {
                session.want(link, now);
            }
            for frame in picture.take() {
                // A frame the transport could not take leaves the page's decoder without its
                // reference: the next frame is a keyframe, as when the page asks for one.
                if !session.send_video(now, frame.captured_at, &frame.data) {
                    picture.request_keyframe();
                }
            }
            // The window closed or the capture failed: the stream has nothing left to show.
            if picture.ended() {
                session.close(StreamReason::StreamUnavailable, now);
            }
        }
        if let Err(reason) = streaming.still_admitted(now) {
            session.close(reason, now);
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
    drop(video);
    station.host.ptt.end();
    if let Some(deliver) = &station.host.input {
        deliver(&WebviewInput::Reset {});
    }
    #[cfg(feature = "radio")]
    streaming.audio.stop(None);
    send(state(&session_id, false, Some(ended)));
    // The contract's closed vocabulary, so the shack's own log says why, as the page does.
    tempo_core::applog::info("remote", &format!("stream: ended ({ended:?})"));
}

#[cfg(test)]
mod lan_tests;
#[cfg(test)]
pub(super) mod tests;
