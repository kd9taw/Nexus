//! One computer on the LAN road, from the accept to the close: the first steps of the ladder in
//! `super`'s header, for one connection, and then the lane that carries the lease, the stream's
//! signalling, the status line and Stop.
//!
//! 1. **The subnet and the gate**, before a byte is read: a source outside the shack's own subnet,
//!    or over the caps ([`super::gate`]), is dropped as it arrives.
//! 2. **The handshake, inside one deadline** ([`HANDSHAKE`]): TLS 1.3 with the computer's key held
//!    to the paired keys ([`super::tls`]), the WebSocket upgrade, and the hello. A computer that
//!    does not finish in time, or proves no paired key, counts against its address at the gate.
//! 3. **The hello.** The LAN protocol, stream and operation versions must be the shack's own; a
//!    mismatch is refused saying which side to update (as ruled on 2026-10-04). Then the shack
//!    stamps the session: a session id of its own making, and the device the key belongs to.
//!    Nothing the computer says names either; that stamp is what the relay supplies on the hosted
//!    road.
//! 4. **The lane.** Every operation request goes to the operations authority under this
//!    connection's own id, so the lease it takes is bound to this connection. Stop is answered
//!    on the reading task itself, before anything else it could queue behind, as on the relay's
//!    socket. Stream signals go to the same session thread the relay's do, on this road's socket.
//! 5. **The close.** However the connection ends (the computer closes it, it falls silent for
//!    [`SILENCE`], LAN is switched off), its stream ends with it, the stream's presence at once,
//!    and the lease it holds goes with the connection, so the radio loop halts anything it was
//!    keeping on the air on its next tick.
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
use super::super::transport::{identifier, random_secret, Feeds, Outbound};
use super::gate::Gate;
use super::tls::{pin, Identity, Paired};

/// The LAN protocol this shack speaks: the hello and everything after it on this channel.
pub const PROTOCOL_VERSION: u8 = 1;
/// The operation version every request on this channel is answered at: the one that carries the
/// stop token, as on the stream's own `control` channel.
pub const OPERATION_VERSION: u8 = 4;
/// TLS, the upgrade and the hello must all be done within this.
pub(super) const HANDSHAKE: Duration = Duration::from_secs(5);
/// A computer that says nothing for this long is gone. Its own pings every 5 s keep it.
pub(super) const SILENCE: Duration = Duration::from_secs(15);
/// The largest message either way, the relay's own bound.
pub(super) const MESSAGE_BYTES: usize = 8192;
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
    pub paired: Paired,
    pub network: tempo_stream::lan::Network,
    pub port: u16,
    pub gate: Arc<Gate>,
    /// [`HANDSHAKE`] and [`SILENCE`]; a test's own, shorter, so it need not wait them out.
    pub handshake: Duration,
    pub silence: Duration,
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
    OperationRequest {
        request: Box<Request>,
    },
    /// The page's own `streamSignal`, as it would hand it to the relay: the shack stamps it.
    StreamSignal {
        #[serde(rename = "leaseId")]
        lease_id: String,
        payload: BrowserSignal,
    },
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

/// A fresh session id, shaped as every id the authority takes is.
fn session_id() -> Option<String> {
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
    // 1. The subnet, then the caps. Nothing is read from a refused source.
    if !shared.network.contains(peer.ip()) {
        return;
    }
    let Ok(_ticket) = shared.gate.admit(peer.ip(), Instant::now()) else {
        return;
    };
    // 2 and 3. The handshake, inside one deadline.
    let handshake = tokio::time::timeout(shared.handshake, handshake(stream, &shared)).await;
    let (socket, pin, device) = match handshake {
        Ok(Ok(Some(done))) => done,
        // A paired computer on another version: told which side to update, and not counted.
        Ok(Ok(None)) => return,
        Ok(Err(())) | Err(_) => {
            shared.gate.failed(peer.ip(), Instant::now());
            return;
        }
    };
    session(socket, pin, device, shared, stop, peer.ip()).await;
}

type Socket<S> = tokio_tungstenite::WebSocketStream<tokio_rustls::server::TlsStream<S>>;

/// TLS, the upgrade and the hello. `Ok(None)`: a paired computer refused for its version, already
/// told why. `Err`: a failed handshake.
async fn handshake<S>(
    stream: S,
    shared: &Shared,
) -> Result<Option<(Socket<S>, [u8; 32], String)>, ()>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let tls = tokio_rustls::TlsAcceptor::from(shared.tls.clone())
        .accept(stream)
        .await
        .map_err(|_| ())?;
    let key = tls
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|keys| keys.first())
        .ok_or(())?;
    let pin = pin(key.as_ref());
    // Asked again here, after the verifier: a computer revoked during its own handshake is out.
    let device = (shared.paired)(&pin).filter(|d| identifier(d)).ok_or(())?;
    let mut socket = tokio_tungstenite::accept_async_with_config(tls, Some(socket_config()))
        .await
        .map_err(|_| ())?;
    let Some(Ok(Message::Text(text))) = socket.next().await else {
        return Err(());
    };
    let Ok(FromComputer::Hello {
        protocol,
        stream,
        operation,
    }) = serde_json::from_str(&text)
    else {
        return Err(());
    };
    if let Some(reason) = refusal((protocol, stream, operation)) {
        use futures_util::SinkExt;
        let refused = json!({"type":"refused","reason":reason}).to_string();
        let _ = socket.send(Message::Text(refused.into())).await;
        let _ = socket.close(None).await;
        return Ok(None);
    }
    Ok(Some((socket, pin, device)))
}

/// The session, from the welcome to the close.
async fn session<S>(
    socket: Socket<S>,
    pin: [u8; 32],
    device: String,
    shared: Shared,
    mut stop: watch::Receiver<bool>,
    source: IpAddr,
) where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    let Some(session) = session_id() else {
        return;
    };
    // The lease this computer takes is bound to this connection, and goes with it.
    let connection = LanConnection::new(shared.authority.clone());
    let paired = shared.paired.clone();
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
            (offered == signed_by && paired(&pin).as_deref() == Some(offered)).then_some(pin)
        }),
        signer: Some(shared.identity.signer.clone()),
    };
    let (sink, mut reader) = socket.split();
    let (outbound, mut writer) = Outbound::spawn(sink);
    let welcome = json!({"type":"welcome","sessionId":session,"deviceId":device,
        "stationId":shared.identity.station_id})
    .to_string();
    if outbound.send(Message::Text(welcome.into())).is_err() {
        return;
    }
    let mut lane = StreamLane::default();
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
            _ = &mut writer => break StreamReason::ConnectionFailed,
            done = async { operation.as_mut().expect("guarded operation task").await }, if operation.is_some() => {
                operation = None;
                let Ok(text) = done else { break StreamReason::ConnectionFailed };
                if outbound.send(Message::Text(text.into())).is_err() { break StreamReason::ConnectionFailed }
            }
            message = reader.next() => {
                heard = tokio::time::Instant::now();
                let text = match message {
                    Some(Ok(Message::Text(text))) => text,
                    // tungstenite answers a ping itself; either kind of control frame only says
                    // the computer is still there.
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    _ => break StreamReason::StreamClosed,
                };
                let Ok(message) = serde_json::from_str::<FromComputer>(&text) else {
                    break StreamReason::StreamClosed;
                };
                match message {
                    FromComputer::Hello { .. } => break StreamReason::StreamClosed,
                    FromComputer::OperationRequest { request } => {
                        let id = request.id().to_string();
                        if !identifier(&id) { break StreamReason::StreamClosed }
                        let answer = if matches!(*request, Request::StopTransmit { .. }) {
                            // The second road for Stop: admitted here, on the reading task, never
                            // behind an operation or a send in flight. Its path never waits on the
                            // engine's lock or the authority's (`transmit_stop`).
                            Some(response(&id, shared.authority.handle_version((connection.id, OPERATION_VERSION),
                                &session, &device, &request, &shared.engine, Instant::now())))
                        } else if !matches!(*request, Request::State { .. } | Request::Acquire { .. }
                            | Request::Heartbeat { .. } | Request::Release { .. }) {
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
                            if outbound.send(Message::Text(answer.into())).is_err() { break StreamReason::ConnectionFailed }
                        }
                    }
                    FromComputer::StreamSignal { lease_id, payload } => {
                        if !identifier(&lease_id) || !payload.valid() { break StreamReason::StreamClosed }
                        let now = Instant::now();
                        // The relay's budget, kept here, since this channel is this road's relay.
                        if !matches!(payload, BrowserSignal::Close {}) {
                            while signals.front().is_some_and(|at| now.saturating_duration_since(*at) >= SIGNAL_WINDOW) {
                                signals.pop_front();
                            }
                            if signals.len() >= SIGNALS {
                                let refused = RoomToPage::StreamState { streaming: false, reason: Some(StreamReason::TryLater) };
                                let Ok(text) = serde_json::to_string(&refused) else { break StreamReason::StreamClosed };
                                if outbound.send(Message::Text(text.into())).is_err() { break StreamReason::ConnectionFailed }
                                continue;
                            }
                            signals.push_back(now);
                        }
                        let said_now = lane.signal_lan(&station, &to_peer, (session.clone(), device.clone(), lease_id),
                            payload, shared.network, shared.port);
                        if let Some(text) = said_now.as_deref().and_then(to_page) {
                            if outbound.send(Message::Text(text.into())).is_err() { break StreamReason::ConnectionFailed }
                        }
                    }
                }
            }
            text = from_stream.recv() => {
                if let Some(text) = text.as_deref().and_then(to_page) {
                    if outbound.send(Message::Text(text.into())).is_err() { break StreamReason::ConnectionFailed }
                }
            }
            _ = status_tick.tick() => {
                if let Ok(frame) = shared.feeds.monitor.read(&shared.engine, Instant::now()) {
                    let keyed = frame.station.radio.rig_keyed;
                    if said != Some(keyed) {
                        said = Some(keyed);
                        if outbound.send(Message::Text(status(keyed).into())).is_err() { break StreamReason::ConnectionFailed }
                    }
                }
            }
            _ = tokio::time::sleep_until(heard + shared.silence) => break StreamReason::ConnectionFailed,
        }
    };
    // The stream ends with the connection, its presence at once, here; the lease and anything it
    // keeps on the air end as `connection` is dropped.
    lane.end(&station, &session, ended);
    drop(lane);
    drop(connection);
    writer.abort();
    tempo_core::applog::info(
        "remote",
        &format!("on this network: a computer at {source} left ({ended:?})"),
    );
}
