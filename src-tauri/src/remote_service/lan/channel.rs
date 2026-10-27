//! One computer on the LAN road, from the accept to the close: the first steps of the ladder in
//! `super`'s header, for one connection, and then the lane that carries the lease, the stream's
//! signalling, the status line and Stop.
//!
//! 1. **The subnet and the gate**, before a byte is read: a source outside the shack's own subnet,
//!    or over the caps ([`super::gate`]), is dropped as it arrives.
//! 2. **The handshake, inside one deadline** ([`HANDSHAKE`]): TLS 1.3 with the computer's key held
//!    to the paired keys ([`super::tls`]), the WebSocket upgrade, and the hello. A computer that
//!    does not finish in time, or proves no paired key, counts against its address at the gate; one
//!    cut there to make room for newer handshakes does not. One that finishes then takes a
//!    session's place. A refused key reads its refusal and then an orderly close ([`drain`]).
//! 3. **The hello.** The LAN protocol, stream and operation versions must be the shack's own; a
//!    mismatch is refused saying which side to update (as ruled on 2026-10-04). Then the shack
//!    stamps the session: a session id of its own making, and the device the key belongs to.
//!    Nothing the computer says names either; that stamp is what the relay supplies on the hosted
//!    road. It opens the connection's own Stop road (`super::stop_road`) and audio road
//!    (`super::audio_road`), each keyed from this TLS session under its own label with the session
//!    id as the context, and the welcome names each one's port and tag. A
//!    first message that asks to pair instead goes, under the same version rule, to a
//!    pairing-only connection (`super::pairing`), which holds the pairing desk and nothing of
//!    this lane's: none of the steps below happen on it.
//! 4. **The lane.** Every operation request goes to the operations authority under this
//!    connection's own id, so the lease it takes is bound to this connection. Stop is answered
//!    on the reading task itself, before anything else it could queue behind, as on the relay's
//!    socket. Stream signals go to the same session thread the relay's do, on this road's socket.
//! 5. **The command lane** (protocol 2, `super::commands`): opened by the computer straight after
//!    the welcome, naming its kind of client, its build and its run of Nexus (`super::wire`). Any
//!    other build is refused, saying which side to update, and the connection closes before a call
//!    runs. A window's session never opens it. On it go the calls, routed by class, and their
//!    answers, the pushed feeds and the events. One writer writes everything on the connection
//!    (`super::outbox`), a Stop and its answer ahead of all else. Its `opened` names this station's
//!    key for the internet road (public half only) when the station has one, so a computer hears it
//!    from the station on a link already pinned to it, never from the service alone.
//! 6. **Internet roads**, in place of the LAN's: an `open` may ask for them on a connection whose
//!    welcome named no road of the LAN's, where the listener offers them (`InternetRoads`). Each
//!    is a socket, ICE and thread of its own at the shack (`super::stop_road`,
//!    `super::audio_road`), keyed from this TLS session under the internet roads' own labels; the
//!    computer's ICE offer comes in its `open`, the shack's goes back in its `opened`, and each
//!    end's later candidates follow on this connection, never on a relay lane.
//! 7. **The close.** However the connection ends (the computer closes it, it falls silent for
//!    [`SILENCE`], LAN is switched off, the computer is removed at the shack), its stream ends with
//!    it, the stream's presence at once, and the lease it holds goes with the connection, so the
//!    radio loop halts anything it was keeping on the air on its next tick.
use std::collections::VecDeque;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{json, Value};
use tempo_stream::protocol::{BrowserSignal, RoomToPage, StationToRoom, StreamReason, Validate};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::{protocol::WebSocketConfig, Message};

use super::super::operations::{Authority, LanConnection, Request};
use super::super::stream::{Station, StreamLane};
use super::super::transport::{identifier, random_secret, Feeds};
use super::commands::{Commands, Lane, Opening, OUTCOMES, WATCH};
use super::datagram;
use super::enrol;
use super::gate::Gate;
use super::outbox::Outbox;
use super::pairing::{self, Asked, Desk, Outcome};
use super::stop_road::{Opened as StopOpened, StateClock, StopRoad};
use super::tls::{pin, Identity};
use super::wire::{self, End, Whole};
use crate::command_dispatch::{Call, Dispatcher, Profile, Revision, ShackStores};
use tempo_client::ice::{Gather, IceRoad, Offer, Role};

/// The LAN protocol this shack speaks, and the operation version every request on this channel is
/// answered at: the wire's (`super::wire`), which the computer's end reads too.
pub use super::wire::{OPERATION_VERSION, PROTOCOL_VERSION};
/// TLS, the upgrade and the hello must all be done within this.
pub(super) const HANDSHAKE: Duration = Duration::from_secs(5);
/// What comes behind a refused key is read for this long in all ([`drain`]), and no more than
/// [`DRAIN_BYTES`] of it: a window sends about 200 bytes there.
const DRAIN_FOR: Duration = Duration::from_secs(1);
const DRAIN_BYTES: usize = 16 * 1024;
/// A computer that says nothing for this long is gone. Its own pings every 5 s keep it.
pub(super) const SILENCE: Duration = Duration::from_secs(15);
/// The largest message either way, the relay's own bound (the wire's).
pub(super) use super::wire::MESSAGE_BYTES;
/// The relay's budget for a page's stream signals: forty-eight in ten seconds, a close always
/// allowed (`ui/src/remote-web/stream-relay.ts`).
pub(super) const SIGNALS: usize = 48;
pub(super) const SIGNAL_WINDOW: Duration = Duration::from_secs(10);
/// How often the status line is read for a change: the observation's own cadence.
const STATUS_EVERY: Duration = Duration::from_millis(tempo_app::remote_monitor::POLL_MS);

/// What every connection needs, cloned into each.
#[derive(Clone)]
pub(super) struct Shared {
    pub authority: Arc<Authority>,
    pub engine: crate::SharedEngine,
    pub feeds: Feeds,
    pub tls: Arc<rustls::ServerConfig>,
    pub identity: Arc<Identity>,
    /// Who may hold a session here, by the key its TLS proved, and what tells a session that its
    /// computer was removed ([`Roster`]).
    pub roster: Roster,
    /// Who is paired, and the pairing window: all a pairing-only connection keeps of this. `None`
    /// through the room's pipe, where no computer pairs: a request to pair there is refused.
    pub desk: Option<Desk>,
    /// The LAN's own network, whose subnet alone is heard. `None` through the room's pipe, which
    /// carries no address of anyone's and signals no stream.
    pub network: Option<tempo_stream::lan::Network>,
    pub port: u16,
    pub gate: Arc<Gate>,
    /// [`HANDSHAKE`] and [`SILENCE`]; a test's own, shorter, so it need not wait them out.
    pub handshake: Duration,
    pub silence: Duration,
    /// The command lane's dispatcher: one for the station, so a computer that reconnects finds the
    /// journal of its calls.
    pub dispatcher: Dispatcher<ShackStores>,
    /// How the lane runs a call, and the events it may forward.
    pub commands: Commands,
    /// This shack's build, as its welcome says it and an `open` must name it: the app's version and
    /// the contract (`super::wire`). A test's own stands for another build.
    pub build: (&'static str, &'static str),
    /// Stop's second road, where each connection's own road is opened as it is welcomed. `None`
    /// when it did not open: the welcome then names none, and Stop has this connection alone.
    pub stop_road: Option<StopRoad>,
    /// The audio road, where each connection's own is opened as it is welcomed. `None` when it did
    /// not open: the welcome then names none, and the computer hears nothing.
    #[cfg(feature = "radio")]
    pub audio_road: Option<super::audio_road::AudioRoad>,
    /// Where a connection's internet roads are opened, when its `open` asks for them and its
    /// welcome named no road of the LAN's. `None` on the LAN's own listener, where every road is
    /// the LAN's.
    pub internet: Option<InternetRoads>,
    /// This station's key for the internet road, the public half only (SPKI, lowercase hex, as the
    /// service lists it), read when an `open` is answered; `None` while the station has no pairing
    /// with the service. Its `opened` names it, so a computer learns it over a link already pinned.
    pub station_key: StationKeyOf,
}

/// Where a connection reads this station's key for the internet road: the public half alone. The
/// signer that holds the private half never reaches a channel.
pub(crate) type StationKeyOf = Arc<dyn Fn() -> Option<String> + Send + Sync>;

/// Who may hold a session on this channel, by the SHA-256 of the key its TLS proved: that
/// computer's device id, or `None`. And what tells a session its computer was removed, so it ends
/// at once. On the LAN, the computers paired there (`super::Book`); through the room's pipe, the
/// computers approved at the shack for hosted Remote, each by the key pinned as it was approved
/// (`super::super::tunnel`).
#[derive(Clone)]
pub(crate) struct Roster {
    pub paired: super::tls::Paired,
    /// Told each time who may changes.
    pub subscribe: Arc<dyn Fn() -> watch::Receiver<u64> + Send + Sync>,
}

impl Roster {
    /// The LAN's: the computers paired in `book`.
    pub(crate) fn lan(book: &Arc<super::Book>) -> Self {
        let (paired, book) = (book.verifier().0, book.clone());
        Self {
            paired,
            subscribe: Arc::new(move || book.subscribe()),
        }
    }

    fn paired(&self, pin: &[u8; 32]) -> Option<String> {
        (self.paired)(pin)
    }

    /// The same, asked on the runtime's blocking pool, for a session's own re-asks: the book's lock
    /// (and the room's) is a plain one, and a worker parked on it reads no socket and fires no timer
    /// for anything else on the runtime until it is let go.
    async fn asked(&self, pin: [u8; 32]) -> Option<String> {
        let paired = self.paired.clone();
        tokio::task::spawn_blocking(move || paired(&pin))
            .await
            .ok()
            .flatten()
    }
}

/// What a connection's internet roads are opened with: the listener's own Stop road and audio road
/// (whose `open_internet` gives each internet road a socket and a thread of its own), the address
/// those sockets bind, and the STUN server this end learns its server-reflexive address from.
#[derive(Clone)]
pub(crate) struct InternetRoads {
    pub stop: StopRoad,
    #[cfg(feature = "radio")]
    pub audio: Option<super::audio_road::AudioRoad>,
    pub bind: IpAddr,
    pub stun: Option<SocketAddr>,
}

/// The internet roads a computer asks for in its `open`: its ICE offer for each.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RoadsAsked {
    stop: Offer,
    #[serde(default)]
    audio: Option<Offer>,
}

/// A connection's internet roads, open: each road's handle (it closes as it drops) and its ICE.
struct Internet {
    stop: (StopOpened, Arc<IceRoad>),
    #[cfg(feature = "radio")]
    audio: Option<(super::audio_road::Opened, Arc<IceRoad>)>,
}

impl Internet {
    /// Each road's ICE, by the road's name on the link.
    fn ice(&self) -> Vec<(&'static str, &Arc<IceRoad>)> {
        let mut ice = vec![("stop", &self.stop.1)];
        #[cfg(feature = "radio")]
        if let Some((_, audio)) = &self.audio {
            ice.push(("audio", audio));
        }
        ice
    }

    /// What the `opened` names of them: each road's tag, as lowercase hex, and this end's offer.
    fn named(&self) -> Value {
        let hex = |tag: &[u8]| -> String { tag.iter().map(|b| format!("{b:02x}")).collect() };
        let mut roads = json!({"stop": {"tag": hex(&self.stop.0.tag), "ice": self.stop.1.offer()}});
        #[cfg(feature = "radio")]
        if let Some((opened, ice)) = &self.audio {
            roads["audio"] = json!({"tag": hex(&opened.tag), "ice": ice.offer()});
        }
        roads
    }
}

/// Opens a connection's internet roads, as its `open` asked: each its own socket at `net.bind`
/// and ICE (controlled: the computer nominates), the computer's offer taken, keys from this
/// connection's TLS session under the internet roads' own labels (`keys`). `None`, and no road
/// open, if the Stop road's could not be had: Stop then has the link alone.
#[allow(clippy::too_many_arguments)]
fn open_internet(
    net: &InternetRoads,
    asked: RoadsAsked,
    keys: (
        Option<[u8; datagram::EXPORTED]>,
        Option<[u8; datagram::EXPORTED]>,
    ),
    session: &str,
    device: &str,
    pin: [u8; 32],
    clock: Arc<StateClock>,
) -> Option<Internet> {
    let bind = |tick| {
        let gather = Gather {
            direct: true,
            stun: net.stun,
            relay: None,
        };
        IceRoad::bind(SocketAddr::new(net.bind, 0), Role::Shack, gather, tick).map(Arc::new)
    };
    let stop_ice = bind(super::stop_road::TICK)?;
    if !stop_ice.answer(&asked.stop) {
        return None;
    }
    let stop = net
        .stop
        .open_internet(stop_ice.clone(), &keys.0?, session, device, pin, clock)?;
    #[cfg(feature = "radio")]
    let audio = match (asked.audio, &net.audio, keys.1) {
        (Some(offer), Some(road), Some(key)) => bind(super::audio_road::TICK).and_then(|ice| {
            ice.write_timeout(Some(super::audio_road::SEND_WAIT));
            ice.answer(&offer).then_some(())?;
            let opened = road.open_internet(ice.clone(), &key, session, device, Some(stop.tag))?;
            Some((opened, ice))
        }),
        _ => None,
    };
    Some(Internet {
        stop: (stop, stop_ice),
        #[cfg(feature = "radio")]
        audio,
    })
}

/// What a remote PC sends. Parsed whole and exactly: anything else closes the connection.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum FromComputer {
    Hello {
        protocol: u8,
        stream: u8,
        operation: u8,
    },
    /// A computer asking to pair, inside an open window (`super::pairing`): the first message
    /// only, and it never reaches the lane.
    Pair {
        protocol: u8,
        stream: u8,
        operation: u8,
        name: String,
        nonce: String,
    },
    /// A computer enrolling by its own sign-in to the station's account (`super::enrol`): the
    /// first message only, and it never reaches the lane.
    Enrol {
        protocol: u8,
        stream: u8,
        operation: u8,
        name: String,
        kind: String,
        #[serde(rename = "publicKey")]
        public_key: String,
        proof: String,
    },
    OperationRequest {
        request: Box<Request>,
    },
    /// The page's own `streamSignal`, as it would hand it to the relay: the shack stamps it.
    StreamSignal {
        #[serde(rename = "leaseId")]
        lease_id: String,
        payload: BrowserSignal,
    },
    /// Opens the command lane, once, after the welcome: the kind of client, its build, and the run
    /// of Nexus on it whose calls are journaled together.
    Open {
        profile: String,
        app: String,
        contract: String,
        run: u64,
        /// Internet roads in place of the LAN's: the computer's ICE offer for each.
        #[serde(default)]
        roads: Option<RoadsAsked>,
    },
    /// One call on the open lane: its id, rising within the run, the command and its arguments,
    /// for a whole save the revision of the copy it was made from, and how many Stops the computer
    /// had pressed when it made the call (a Stop counting itself).
    Call {
        id: u64,
        cmd: String,
        args: Value,
        #[serde(default)]
        base: Option<String>,
        #[serde(default)]
        stops: u64,
    },
    /// What became of these calls: asked after a reconnect.
    Outcomes {
        ids: Vec<u64>,
    },
    /// The feeds to push from now on, in place of the ones before.
    Watch {
        feeds: Vec<String>,
    },
    /// One more of the computer's candidates for one of this connection's internet roads, named
    /// `stop` or `audio`, as it finds it.
    Candidate {
        road: String,
        candidate: String,
    },
}

/// A call as the shack reads it off the link: its id and the count of Stops it carries, or `None`
/// for a message the shack would close the connection over. For a test that holds the computer's
/// writing of a call to the shack's reading of it.
#[cfg(test)]
pub(super) fn read_call(text: &str) -> Option<(u64, u64)> {
    match serde_json::from_str::<FromComputer>(text).ok()? {
        FromComputer::Call { id, stops, .. } => Some((id, stops)),
        _ => None,
    }
}

/// Which side to update, when the two ends do not speak the same versions (as ruled on
/// 2026-10-04).
fn refusal(hello: (u8, u8, u8)) -> Option<&'static str> {
    let ours = (
        PROTOCOL_VERSION,
        tempo_stream::protocol::STREAM_VERSION,
        OPERATION_VERSION,
    );
    match hello.cmp(&ours) {
        std::cmp::Ordering::Equal => None,
        // The computer speaks a newer contract than this shack.
        std::cmp::Ordering::Greater => Some("updateStation"),
        std::cmp::Ordering::Less => Some("updateComputer"),
    }
}

fn socket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(MESSAGE_BYTES))
        .max_frame_size(Some(MESSAGE_BYTES))
}

/// A fresh session id, shaped as every id the authority takes is (the LAN station id is made the
/// same way).
pub(super) fn session_id() -> Option<String> {
    let hex = random_secret().ok()?;
    Some(format!(
        "{}-{}-{}-{}-{}",
        &hex[..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    ))
}

/// What the station tells the relay about a session, as the relay would hand it to the page:
/// the session id stripped, since this channel is the session.
fn to_page(station: &str) -> Option<String> {
    let message = match serde_json::from_str::<StationToRoom>(station).ok()? {
        StationToRoom::StreamSignal { payload, .. } => RoomToPage::StreamSignal { payload },
        StationToRoom::StreamState {
            streaming, reason, ..
        } => RoomToPage::StreamState { streaming, reason },
    };
    serde_json::to_string(&message).ok()
}

fn response(request_id: &str, result: Result<Value, &'static str>) -> String {
    match result {
        Ok(value) => json!({"type":"operationResponse","requestId":request_id,"value":value}),
        Err(error) => json!({"type":"operationResponse","requestId":request_id,"error":error}),
    }
    .to_string()
}

/// The status line: whether the rig is keyed, as the observation reads it (`null` when it cannot
/// tell). The page's ▲ TX and its microphone's mute (M9) read it, as they read the relay's
/// observation on the hosted road.
fn status(rig_keyed: Option<bool>) -> String {
    json!({"type":"status","rigKeyed":rig_keyed}).to_string()
}

/// One connection, from the accept to its end. Generic over the transport so a test can drive the
/// whole of it, handshake included, over any stream, with `peer` the address it came from.
pub(super) async fn connection<S>(
    stream: S,
    peer: SocketAddr,
    shared: Shared,
    stop: watch::Receiver<bool>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    // 1. The subnet, then the caps. Nothing is read from a refused source, and a listener with no
    // network of its own (the room's pipe has none) takes nothing here.
    if !shared
        .network
        .is_some_and(|network| network.contains(peer.ip()))
    {
        return;
    }
    let Ok(mut ticket) = shared.gate.admit(peer.ip(), Instant::now()) else {
        return;
    };
    // A first read of the signed-in computers that did not complete admits nobody: read again
    // before this one is asked (at most once a while), off the runtime, since the store can block,
    // and inside the handshake's deadline.
    let enrolment = shared.desk.as_ref().map(|desk| desk.enrolment.clone());
    let opening = async {
        if let Some(enrolment) = enrolment.filter(|enrolment| !enrolment.is_read()) {
            let now = Instant::now();
            let _ = tokio::task::spawn_blocking(move || enrolment.retry(now)).await;
        }
        handshake(stream, &shared, Some(peer)).await
    };
    // 2 and 3. The handshake, inside one deadline, unless enough newer ones arrive meanwhile to cut
    // it: a handshake holds a handshake's place, never a session's.
    let handshake = tokio::select! {
        done = tokio::time::timeout(shared.handshake, opening) => done.ok(),
        // ⛔ Cut to make room: newer arrivals did it, not this address, so it is never counted
        // against it. Counted, a flood could cut a paired computer's handshakes until its own
        // address was ignored.
        () = ticket.cut() => return,
    };
    match handshake {
        // Refused for its version, and told which side to update: not counted.
        Some(Ok(Opened::Told)) => {}
        // Done, and only now holding one of the sessions' places: with every one taken, it is closed
        // before its welcome, not counted.
        Some(Ok(_)) if ticket.opened().is_err() => {}
        Some(Ok(Opened::Session(socket, pin, device))) => {
            session(socket, pin, device, shared, stop, Some(peer.ip())).await;
        }
        Some(Ok(Opened::Pairing(socket, asked))) => {
            // From here on this connection holds the pairing desk and nothing else: no operations
            // authority, no engine, no stream.
            let (gate, desk, deadline) =
                (shared.gate.clone(), shared.desk.clone(), shared.handshake);
            drop(shared);
            if let Some(desk) = desk {
                if pairing::session(socket, asked, desk, deadline, stop).await == Outcome::Failed {
                    gate.failed(peer.ip(), Instant::now());
                }
            }
        }
        Some(Ok(Opened::Enrol(socket, asked))) => {
            // As a pairing: this connection holds the desk and nothing else.
            let (gate, desk, deadline) =
                (shared.gate.clone(), shared.desk.clone(), shared.handshake);
            drop(shared);
            if let Some(desk) = desk {
                if enrol::session(socket, asked, desk, deadline, stop).await == Outcome::Failed {
                    gate.failed(peer.ip(), Instant::now());
                }
            }
        }
        Some(Err(())) | None => shared.gate.failed(peer.ip(), Instant::now()),
    }
}

/// One computer through the room's pipe (`super::super::tunnel`), from its first byte to its end:
/// the ladder from step 2 on, held to this channel's roster. No address is anyone's there, so no
/// subnet or gate is read: the room has admitted the computer by its account and its approval, and
/// limits its pipes. A request to pair is refused, and no stream is signalled.
pub(super) async fn tunnel<S>(stream: S, shared: Shared, stop: watch::Receiver<bool>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let handshake = tokio::time::timeout(shared.handshake, handshake(stream, &shared, None)).await;
    if let Ok(Ok(Opened::Session(socket, pin, device))) = handshake {
        session(socket, pin, device, shared, stop, None).await;
    }
}

pub(super) type Socket<S> = tokio_tungstenite::WebSocketStream<tokio_rustls::server::TlsStream<S>>;

/// What a handshake opened.
enum Opened<S> {
    /// A paired computer, welcomed next: its key's pin and its device.
    Session(Socket<S>, [u8; 32], String),
    /// A computer asking to pair, for a pairing-only connection.
    Pairing(Socket<S>, Asked),
    /// A computer asking to enrol, for an enrol-only connection.
    Enrol(Socket<S>, enrol::Asked),
    /// Refused for its version, and already told which side to update.
    Told,
}

/// TLS, the upgrade and the first message: a hello from a paired computer, or a request to pair.
/// `Err`: a failed handshake. A computer refused for its key is written to the diagnostic log with
/// the address it came from (`peer`, none through the room's pipe) and why, never the key.
async fn handshake<S>(stream: S, shared: &Shared, peer: Option<SocketAddr>) -> Result<Opened<S>, ()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let tls = match tokio_rustls::TlsAcceptor::from(shared.tls.clone())
        .accept(stream)
        .into_fallible()
        .await
    {
        Ok(tls) => tls,
        Err((error, io)) => {
            // Every other failure closes at once.
            if refused_key(&error) {
                refused(peer, "its key is not paired here");
                drain(io).await;
            }
            return Err(());
        }
    };
    let key = tls
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|keys| keys.first())
        .ok_or(())?
        .as_ref()
        .to_vec();
    // What a pairing's proofs are bound to: this one session.
    let exporter = tls
        .get_ref()
        .1
        .export_keying_material([0; 32], pairing::EXPORTER, None)
        .map_err(|_| ())?;
    let pin = pin(&key);
    let mut socket = tokio_tungstenite::accept_async_with_config(tls, Some(socket_config()))
        .await
        .map_err(|_| ())?;
    let Some(Ok(Message::Text(text))) = socket.next().await else {
        return Err(());
    };
    match serde_json::from_str(&text) {
        Ok(FromComputer::Hello {
            protocol,
            stream,
            operation,
        }) => {
            // Asked again here, after the verifier: a computer revoked during its own handshake
            // is out, and a key the verifier took only for the pairing window is no paired one.
            let Some(device) = shared.roster.paired(&pin).filter(|d| identifier(d)) else {
                refused(peer, "its key is not paired here, or no longer");
                return Err(());
            };
            match refusal((protocol, stream, operation)) {
                Some(reason) => told(socket, reason).await,
                None => Ok(Opened::Session(socket, pin, device)),
            }
        }
        // Nothing pairs through the room's pipe.
        Ok(FromComputer::Pair { .. }) if shared.desk.is_none() => Err(()),
        Ok(FromComputer::Pair {
            protocol,
            stream,
            operation,
            name,
            nonce,
        }) => match refusal((protocol, stream, operation)) {
            Some(reason) => told(socket, reason).await,
            None => Ok(Opened::Pairing(
                socket,
                Asked::new(&name, &nonce, key, exporter).ok_or(())?,
            )),
        },
        // Nor does anything enrol through it.
        Ok(FromComputer::Enrol { .. }) if shared.desk.is_none() => Err(()),
        Ok(FromComputer::Enrol {
            protocol,
            stream,
            operation,
            name,
            kind,
            public_key,
            proof,
        }) => match refusal((protocol, stream, operation)) {
            Some(reason) => told(socket, reason).await,
            None => Ok(Opened::Enrol(
                socket,
                enrol::Asked::new(&name, &kind, &public_key, &proof, key).ok_or(())?,
            )),
        },
        _ => Err(()),
    }
}

/// Why the station did not take a computer, for the diagnostic log: where it came from and why.
fn refused(peer: Option<SocketAddr>, why: &str) {
    let from = peer.map_or_else(|| "through the room".to_string(), |at| format!("at {at}"));
    tempo_core::applog::warn(
        "remote",
        &format!("on this network: refused a computer {from}: {why}"),
    );
}

/// Did the handshake fail on the computer's key: one no paired computer holds, outside the pairing
/// window, or one that is not a P-256 key (`tls::PinnedClients`)?
fn refused_key(error: &std::io::Error) -> bool {
    error
        .get_ref()
        .and_then(|inner| inner.downcast_ref::<rustls::Error>())
        .is_some_and(|tls| matches!(tls, rustls::Error::InvalidCertificate(_)))
}

/// The close after a refused key, in order: the refusal TLS has already written, then the end of
/// the stream, never a reset. In TLS 1.3 the computer writes its upgrade straight after its side of
/// the handshake, before its key is judged, so those bytes are here unread, or on their way, when
/// it is refused. Closed with them unread, or answered after the close, the connection ends in a
/// reset, and Windows drops the refusal its computer has not read yet: the window then says that
/// nothing answered, where the truth is that this computer is not paired. So the write side is shut
/// first, and what comes is read and thrown away until the computer closes, for [`DRAIN_FOR`] in
/// all (one deadline, never one per read, so a computer that drips bytes is let go in time), or
/// [`DRAIN_BYTES`], whichever comes first. Nothing read here reaches TLS or the WebSocket. Inside
/// the handshake's own deadline, and counted against the address as any failed handshake is.
async fn drain<S>(mut io: S)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin,
{
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let _ = tokio::time::timeout(DRAIN_FOR, async {
        let _ = io.shutdown().await;
        let (mut discard, mut read) = ([0; 4096], 0);
        while read < DRAIN_BYTES {
            match io.read(&mut discard).await {
                Ok(0) | Err(_) => break,
                Ok(n) => read += n,
            }
        }
    })
    .await;
}

/// Refused for its versions, saying which side to update (as ruled on 2026-10-04), and closed.
async fn told<S>(mut socket: Socket<S>, reason: &str) -> Result<Opened<S>, ()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    use futures_util::SinkExt;
    let refused = json!({"type":"refused","reason":reason}).to_string();
    let _ = socket.send(Message::Text(refused.into())).await;
    let _ = socket.close(None).await;
    Ok(Opened::Told)
}

/// The session, from the welcome to the close.
async fn session<S>(
    socket: Socket<S>,
    pin: [u8; 32],
    device: String,
    shared: Shared,
    mut stop: watch::Receiver<bool>,
    source: Option<IpAddr>,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let roster = shared.roster.clone();
    // Told when who is paired changes: removed at the shack, this computer is out at once. Asked
    // once more now, for a removal since the handshake asked.
    let mut revised = (roster.subscribe)();
    let Some(session) =
        session_id().filter(|_| roster.paired(&pin).as_deref() == Some(device.as_str()))
    else {
        return;
    };
    // This connection's own Stop road: its keys from this TLS session, under the road's label with
    // the session as the context, so only this computer and this shack can make them. Opened before
    // the welcome names it; it closes with the connection, its keys kept a minute for a late Stop.
    let clock = Arc::new(StateClock::default());
    let road = shared.stop_road.as_ref().and_then(|road| {
        let exported = socket
            .get_ref()
            .get_ref()
            .1
            .export_keying_material(
                [0; datagram::EXPORTED],
                datagram::STOP_LABEL,
                Some(session.as_bytes()),
            )
            .ok()?;
        road.open(&exported, &session, &device, pin, clock.clone())
    });
    // Its audio road the same way, under the audio road's own label, so its keys are never Stop's.
    // The Stop road's tag goes with it: a press is named by it at the held PTT.
    #[cfg(feature = "radio")]
    let audio = shared.audio_road.as_ref().and_then(|audio| {
        let exported = socket
            .get_ref()
            .get_ref()
            .1
            .export_keying_material(
                [0; datagram::EXPORTED],
                datagram::AUDIO_LABEL,
                Some(session.as_bytes()),
            )
            .ok()?;
        audio.open(&exported, &session, &device, road.as_ref().map(|r| r.tag))
    });
    // An internet road's keys, made now from this TLS session (the link's halves part below), and
    // kept for its `open` alone: used only if the computer asks for the roads there, and only on a
    // connection the welcome named no road of the LAN's.
    let mut internet_keys = (road.is_none() && shared.internet.is_some()).then(|| {
        let export = |label: &[u8]| {
            socket
                .get_ref()
                .get_ref()
                .1
                .export_keying_material([0; datagram::EXPORTED], label, Some(session.as_bytes()))
                .ok()
        };
        (
            export(datagram::INTERNET_STOP_LABEL),
            export(datagram::INTERNET_AUDIO_LABEL),
        )
    });
    let mut internet: Option<Internet> = None;
    // The lease this computer takes is bound to this connection, and goes with it.
    let connection = LanConnection::new(shared.authority.clone());
    let paired = roster.clone();
    let signed_by = device.clone();
    let station = Station {
        authority: shared.authority.clone(),
        engine: shared.engine.clone(),
        connection: connection.id,
        host: shared.feeds.stream.clone(),
        #[cfg(feature = "radio")]
        audio: shared.feeds.audio.clone(),
        station_id: shared.identity.station_id.clone(),
        // A5 on this road: the offer is signed by the key this connection was opened with, and
        // only while that key is still paired to this computer, read at each admission.
        pinned: Arc::new(move |offered: &str| {
            (offered == signed_by && paired.paired(&pin).as_deref() == Some(offered)).then_some(pin)
        }),
        signer: Some(shared.identity.signer.clone()),
    };
    let (sink, mut reader) = socket.split();
    let (outbox, mut writer) = Outbox::spawn(sink);
    // Each snapshot's number, and when its state was taken: the clock the road reads a state echo by.
    outbox.keep_time("snapshot/1", clock.clone());
    let (app, contract) = shared.build;
    let mut welcome = json!({"type":"welcome","sessionId":session,"deviceId":device,
        "stationId":shared.identity.station_id,"app":app,"contract":contract});
    if let Some(road) = &road {
        welcome["stopRoad"] = road.offer();
    }
    #[cfg(feature = "radio")]
    if let Some(audio) = &audio {
        welcome["audioRoad"] = audio.offer();
    }
    if outbox.text(welcome.to_string()).is_err() {
        return;
    }
    // The command lane, once this computer opens it with the same build.
    let mut lane: Option<Lane> = None;
    let mut stream_lane = StreamLane::default();
    let (to_peer, mut from_stream) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mut operation: Option<tokio::task::JoinHandle<String>> = None;
    let mut signals: VecDeque<Instant> = VecDeque::new();
    let mut status_tick = tokio::time::interval(STATUS_EVERY);
    status_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut said: Option<Option<bool>> = None;
    let mut heard = tokio::time::Instant::now();
    let ended: StreamReason = loop {
        tokio::select! {
            biased;
            _ = stop.changed() => break StreamReason::RemoteOff,
            // Removed at the shack, or the station's network identity reset: out at once, before
            // anything else it sent is answered.
            Ok(()) = revised.changed() => {
                if roster.asked(pin).await.as_deref() != Some(device.as_str()) {
                    break StreamReason::NotController;
                }
            }
            _ = &mut writer => break StreamReason::ConnectionFailed,
            done = async { operation.as_mut().expect("guarded operation task").await }, if operation.is_some() => {
                operation = None;
                let Ok(text) = done else { break StreamReason::ConnectionFailed };
                if outbox.text(text).is_err() { break StreamReason::ConnectionFailed }
            }
            message = reader.next() => {
                heard = tokio::time::Instant::now();
                // A message of text, or a call put back together from its pieces on the open lane.
                let (text, pieced) = match message {
                    Some(Ok(Message::Text(text))) => (text.to_string(), false),
                    Some(Ok(Message::Binary(frame))) => match lane.as_mut().map(|lane| lane.piece(&frame)) {
                        Some(Ok(Some(text))) => (text, true),
                        Some(Ok(None)) => continue,
                        // Bytes before the lane is open, or a piece no computer sends.
                        None | Some(Err(())) => break StreamReason::StreamClosed,
                    },
                    // tungstenite answers a ping itself; either kind of control frame only says
                    // the computer is still there.
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    _ => break StreamReason::StreamClosed,
                };
                let Ok(message) = serde_json::from_str::<FromComputer>(&text) else {
                    break StreamReason::StreamClosed;
                };
                // Only a call comes in pieces.
                if pieced && !matches!(message, FromComputer::Call { .. }) {
                    break StreamReason::StreamClosed;
                }
                match message {
                    FromComputer::Hello { .. } | FromComputer::Pair { .. } | FromComputer::Enrol { .. } => break StreamReason::StreamClosed,
                    FromComputer::Open { profile, app, contract, run, roads } => {
                        if lane.is_some() || run == 0 { break StreamReason::StreamClosed }
                        let profile = Profile::named(&profile);
                        let refused = match profile {
                            // A kind of client this build does not know; a later shack may.
                            None => Some("updateStation"),
                            // A phone, known and refused by name: a later shack serves it, so the
                            // phone is told to update the station, as for a kind not known.
                            Some(profile) if !profile.served() => Some("updateStation"),
                            Some(_) => wire::refusal(End::Shack, shared.build, (&app, &contract)),
                        };
                        let (Some(profile), None) = (profile, refused) else {
                            // Said, and the connection closed, before any call of its own runs.
                            let reason = refused.unwrap_or("updateStation");
                            let _ = outbox.text(json!({"type":"refused","reason":reason}).to_string());
                            outbox.written(DRAIN_FOR).await;
                            break StreamReason::StreamClosed;
                        };
                        // Internet roads in place of the LAN's, when asked for on a connection the
                        // welcome named none of the LAN's: each its own socket, ICE and thread.
                        internet = match (roads, internet_keys.take(), shared.internet.as_ref()) {
                            (Some(asked), Some(keys), Some(net)) => {
                                open_internet(net, asked, keys, &session, &device, pin, clock.clone())
                            }
                            _ => None,
                        };
                        let ready = road.as_ref().map(|road| road.ready())
                            .or_else(|| internet.as_ref().map(|net| net.stop.0.ready()));
                        lane = Some(Lane::open(Opening {
                            dispatcher: shared.dispatcher.clone(),
                            commands: shared.commands.clone(),
                            authority: shared.authority.clone(),
                            connection: connection.id,
                            device: device.clone(),
                            run,
                            profile,
                            outbox: outbox.clone(),
                            ready,
                        }));
                        let mut opened = json!({"type":"opened"});
                        if let Some(net) = &internet {
                            opened["roads"] = net.named();
                        }
                        // This station's key for the internet road, on a link already pinned to
                        // this station: what lets a computer pin it without taking the service's
                        // word for it. Never in the welcome, which a computer of another build
                        // reads whole and exactly.
                        if let Some(key) = (shared.station_key)() {
                            opened["stationKey"] = key.into();
                        }
                        if outbox.text(opened.to_string()).is_err() { break StreamReason::ConnectionFailed }
                    }
                    FromComputer::Call { id, cmd, args, base, stops } => {
                        let Some(lane) = lane.as_mut() else { break StreamReason::StreamClosed };
                        lane.call(Call { id, cmd, args, base: base.map(Revision::new), stops });
                    }
                    FromComputer::Outcomes { ids } => {
                        let Some(lane) = lane.as_ref().filter(|_| ids.len() <= OUTCOMES) else { break StreamReason::StreamClosed };
                        lane.outcomes(ids);
                    }
                    FromComputer::Watch { feeds } => {
                        let Some(lane) = lane.as_mut().filter(|_| feeds.len() <= WATCH) else { break StreamReason::StreamClosed };
                        lane.watch(feeds);
                    }
                    // Only for a road this connection opened, and only a line its ICE reads. One it
                    // reads but will not probe (`tempo_client::ice`'s rule) is dropped there, and the
                    // connection goes on.
                    FromComputer::Candidate { road: name, candidate } => {
                        let taken = internet.as_ref().is_some_and(|net| {
                            net.ice().iter().any(|(road, ice)| *road == name && ice.add_candidate(&candidate))
                        });
                        if !taken { break StreamReason::StreamClosed }
                    }
                    FromComputer::OperationRequest { request } => {
                        let id = request.id().to_string();
                        if !identifier(&id) { break StreamReason::StreamClosed }
                        if matches!(*request, Request::StopTransmit { .. }) {
                            // The second road for Stop: admitted here, on the reading task, never
                            // behind an operation or a send in flight, and its answer ahead of
                            // everything waiting to be written. Its path never waits on the
                            // engine's lock or the authority's (`transmit_stop`).
                            let answer = response(&id, shared.authority.handle_version((connection.id, OPERATION_VERSION),
                                &session, &device, &request, &shared.engine, Instant::now()));
                            if outbox.urgent(Whole::Text(answer)).is_err() { break StreamReason::ConnectionFailed }
                            continue;
                        }
                        let answer = if !matches!(*request, Request::State { .. } | Request::Acquire { .. }
                            | Request::Heartbeat { .. } | Request::Release { .. }
                            | Request::TakeControl { .. }) {
                            // The relay's other lanes (logging, station controls, exports) stay on
                            // the relay: on this road everything else goes through the picture.
                            Some(response(&id, Err("stationUnsupported")))
                        } else if operation.is_some() {
                            Some(response(&id, Err("stationBusy")))
                        } else {
                            let authority = shared.authority.clone();
                            let engine = shared.engine.clone();
                            let (session, device, connection) = (session.clone(), device.clone(), connection.id);
                            operation = Some(tokio::task::spawn_blocking(move || {
                                response(&id, authority.handle_version((connection, OPERATION_VERSION),
                                    &session, &device, &request, &engine, Instant::now()))
                            }));
                            None
                        };
                        if let Some(answer) = answer {
                            if outbox.text(answer).is_err() { break StreamReason::ConnectionFailed }
                        }
                    }
                    FromComputer::StreamSignal { lease_id, payload } => {
                        if !identifier(&lease_id) || !payload.valid() { break StreamReason::StreamClosed }
                        // Through the room's pipe there is no picture: the hosted stream has its own road.
                        let Some(network) = shared.network else {
                            let refused = RoomToPage::StreamState { streaming: false, reason: Some(StreamReason::StreamUnavailable) };
                            let Ok(text) = serde_json::to_string(&refused) else { break StreamReason::StreamClosed };
                            if outbox.text(text).is_err() { break StreamReason::ConnectionFailed }
                            continue;
                        };
                        let now = Instant::now();
                        // The relay's budget, kept here, since this channel is this road's relay.
                        if !matches!(payload, BrowserSignal::Close {}) {
                            while signals.front().is_some_and(|at| now.saturating_duration_since(*at) >= SIGNAL_WINDOW) {
                                signals.pop_front();
                            }
                            if signals.len() >= SIGNALS {
                                let refused = RoomToPage::StreamState { streaming: false, reason: Some(StreamReason::TryLater) };
                                let Ok(text) = serde_json::to_string(&refused) else { break StreamReason::StreamClosed };
                                if outbox.text(text).is_err() { break StreamReason::ConnectionFailed }
                                continue;
                            }
                            signals.push_back(now);
                        }
                        let said_now = stream_lane.signal_lan(&station, &to_peer, (session.clone(), device.clone(), lease_id),
                            payload, network, shared.port);
                        if let Some(text) = said_now.as_deref().and_then(to_page) {
                            if outbox.text(text).is_err() { break StreamReason::ConnectionFailed }
                        }
                    }
                }
            }
            text = from_stream.recv() => {
                if let Some(text) = text.as_deref().and_then(to_page) {
                    if outbox.text(text).is_err() { break StreamReason::ConnectionFailed }
                }
            }
            _ = status_tick.tick() => {
                // Asked again at each tick, for what tells no session: an approval that reached its
                // end (the room's tunnel holds each one's), or a roster that cannot be read.
                if roster.asked(pin).await.as_deref() != Some(device.as_str()) {
                    break StreamReason::NotController;
                }
                // Each internet road's candidates as it finds them (its server-reflexive address
                // once the STUN server answers), on the link.
                if let Some(net) = &internet {
                    for (name, ice) in net.ice() {
                        for line in ice.trickle() {
                            let said = json!({"type":"candidate","road":name,"candidate":line});
                            if outbox.text(said.to_string()).is_err() { break }
                        }
                    }
                }
                if let Ok(frame) = shared.feeds.monitor.read(&shared.engine, Instant::now()) {
                    let keyed = frame.station.radio.rig_keyed;
                    if said != Some(keyed) {
                        said = Some(keyed);
                        if outbox.text(status(keyed)).is_err() { break StreamReason::ConnectionFailed }
                    }
                }
            }
            _ = tokio::time::sleep_until(heard + shared.silence) => break StreamReason::ConnectionFailed,
        }
    };
    // The stream ends with the connection, its presence at once, here; the lane's feeds and events
    // with it; and the lease and anything it keeps on the air as `connection` is dropped.
    stream_lane.end(&station, &session, ended);
    drop(stream_lane);
    drop(lane);
    drop(connection);
    // Its Stop road closes now, and keeps its keys for a Stop that comes late: on the internet too.
    drop(road);
    drop(internet);
    writer.abort();
    tempo_core::applog::info(
        "remote",
        &match source {
            Some(source) => format!("on this network: a computer at {source} left ({ended:?})"),
            None => format!("through the room: a computer left ({ended:?})"),
        },
    );
}
