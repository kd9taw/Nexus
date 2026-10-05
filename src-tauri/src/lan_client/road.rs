//! The road to a paired station: this computer's end of the shack's LAN channel
//! (`remote_service::lan::channel`, whose wire this speaks), for the window's page.
//!
//! 1. **TLS 1.3 with the station's key pinned** (`tls::client::config`). Anything else the address
//!    presents is refused in the handshake, before this computer's own key is sent: TLS 1.3 proves
//!    the server's key first. That is "this station's key has changed" (`keyChanged`). A station
//!    that refuses this computer's key (removed there, or its identity reset) answers the handshake
//!    with an alert (`notPaired`).
//! 2. **The hello**, with the versions this build speaks. The station welcomes this computer under
//!    the device id it gave it at pairing and a session id of its own, or says which side to
//!    update (`updateStation`, `updateComputer`: "Refuse, say which", as ruled on 2026-10-04).
//! 3. **The lane.** The page's operation requests go on as the station's lane takes them: the five
//!    this road carries (state, acquire, heartbeat, release, Stop), anything else answered here as
//!    the station would (`stationUnsupported`), so a page can never close the road with a request
//!    the station cannot parse. Its stream signals go on with every offer signed here with this
//!    computer's key for this station and session (A5): the page never holds the key. What the
//!    station sends comes back: operation answers, stream state, the status line, and stream
//!    signals, the answer only once it is checked against the station's pinned key for that offer
//!    (S3-M1) and its candidates only behind it.
//!
//! Addresses are tried in order, the one the operator typed first, then the remembered ones, the
//! last that worked first; the first to welcome this computer is the road.
use std::collections::VecDeque;
use std::net::SocketAddrV4;
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::{SplitSink, SplitStream};
use futures_util::{SinkExt, StreamExt};
use ring::digest::{digest, SHA256};
use ring::signature::{UnparsedPublicKey, ECDSA_P256_SHA256_FIXED};
use rustls::ClientConfig;
use serde::Deserialize;
use serde_json::json;
use tempo_stream::protocol::{self, BrowserSignal, RoomToPage, StationSignal, Validate};
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use super::ComputerKey;
use crate::remote_service::lan::{tls, VERSIONS};
use crate::remote_service::vault::PairedStation;

/// A TCP connection to an address, before anything else: a station on the same network answers
/// at once.
const CONNECT: Duration = Duration::from_secs(3);
/// TLS, the upgrade and the station's first word: the station's own handshake deadline.
pub(super) const HANDSHAKE: Duration = Duration::from_secs(5);
/// How often this computer pings, so the station never takes it for gone (it waits 15 s).
pub(crate) const PING_EVERY: Duration = Duration::from_secs(5);
/// The largest message this computer sends: the station's own bound.
const TO_STATION_BYTES: usize = 8192;
/// The largest message the station may send: an operation answer is a few KiB.
const FROM_STATION_BYTES: usize = 64 * 1024;
/// The station's candidates kept while its answer is checked: the page's own bound.
const HELD_CANDIDATES: usize = 64;
/// The operation requests this road carries, as the station's lane does.
const OPERATIONS: [&str; 5] = ["state", "acquire", "heartbeat", "release", "stopTransmit"];
/// The operation version every request on this road is answered at.
const OPERATION_VERSION: u8 = VERSIONS.2;

pub(super) type Socket = WebSocketStream<tokio_rustls::client::TlsStream<TcpStream>>;

/// A TLS session with a station, upgraded: the socket, the key the station presented (SPKI) and
/// this session's exporter, which a pairing's proofs are bound to.
pub(super) struct Opened {
    pub socket: Socket,
    pub station: Vec<u8>,
    pub exporter: [u8; 32],
}

/// Why a connection did not open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Failed {
    /// Nothing answered, or it went away before the handshake was done.
    Unreachable,
    /// No TCP connection was made: how it ended (`TimedOut` when nothing answered in time), for
    /// `tempo_stream::lan::unreached` to name.
    NotConnected(std::io::ErrorKind),
    /// The address presented a key this computer did not pin.
    KeyChanged,
    /// The station refused this computer's key in the handshake.
    Refused,
}

/// The rustls error inside an I/O error from the TLS stream, if there is one.
fn tls_error(error: &std::io::Error) -> Option<&rustls::Error> {
    error.get_ref()?.downcast_ref::<rustls::Error>()
}

/// What a failed read or write says about the handshake: refused by the station, or anything else.
fn refused_or_gone(error: &std::io::Error) -> Failed {
    match tls_error(error) {
        // What a station's verifier answers a key it does not take (`tls::PinnedClients`).
        Some(rustls::Error::AlertReceived(rustls::AlertDescription::AccessDenied)) => {
            Failed::Refused
        }
        Some(rustls::Error::InvalidCertificate(_)) => Failed::KeyChanged,
        _ => Failed::Unreachable,
    }
}

fn socket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(FROM_STATION_BYTES))
        .max_frame_size(Some(FROM_STATION_BYTES))
}

/// TCP, TLS with `config`, and the upgrade, at `at`.
pub(super) async fn open(at: SocketAddrV4, config: Arc<ClientConfig>) -> Result<Opened, Failed> {
    let tcp = match tokio::time::timeout(CONNECT, TcpStream::connect(at)).await {
        Ok(Ok(tcp)) => tcp,
        Ok(Err(error)) => return Err(Failed::NotConnected(error.kind())),
        Err(_) => return Err(Failed::NotConnected(std::io::ErrorKind::TimedOut)),
    };
    let name = rustls::pki_types::ServerName::try_from("nexus-station")
        .map_err(|_| Failed::Unreachable)?;
    let tls = tokio_rustls::TlsConnector::from(config)
        .connect(name, tcp)
        .await
        .map_err(|e| refused_or_gone(&e))?;
    let station = tls
        .get_ref()
        .1
        .peer_certificates()
        .and_then(|keys| keys.first())
        .ok_or(Failed::Unreachable)?
        .as_ref()
        .to_vec();
    let exporter = tls
        .get_ref()
        .1
        .export_keying_material([0; 32], crate::remote_service::lan::pairing::EXPORTER, None)
        .map_err(|_| Failed::Unreachable)?;
    // In TLS 1.3 the station judges this computer's key after this computer's side of the
    // handshake is done, so its refusal is the first thing read here.
    let (socket, _) = tokio_tungstenite::client_async_with_config(
        "ws://nexus-station/",
        tls,
        Some(socket_config()),
    )
    .await
    .map_err(|e| match e {
        tokio_tungstenite::tungstenite::Error::Io(io) => refused_or_gone(&io),
        _ => Failed::Unreachable,
    })?;
    Ok(Opened {
        socket,
        station,
        exporter,
    })
}

/// Send one text message.
pub(super) async fn say(socket: &mut Socket, text: String) -> Result<(), ()> {
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

/// What the station says first on a road.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum First {
    Welcome {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        #[serde(rename = "stationId")]
        station_id: String,
    },
    Refused {
        reason: String,
    },
}

/// The reasons that say nothing answered (`tempo_stream::lan::unreached`'s, and `unreachable` for a
/// connection that went away before the handshake was done), as against a station that answered
/// and said no.
pub(crate) const UNREACHED: [&str; 4] = ["unreachable", "otherNetwork", "refused", "noAnswer"];

/// How much a reason says, for the one to tell when no address welcomed this computer. Nothing
/// answering says least, and of its words a refusal says most: the station's computer answered.
pub(crate) fn weight(reason: &str) -> u8 {
    match reason {
        "updateStation" | "updateComputer" => 7,
        "notPaired" => 6,
        "keyChanged" => 5,
        "notStation" => 4,
        "refused" => 3,
        "otherNetwork" => 2,
        "noAnswer" => 1,
        _ => 0,
    }
}

/// The ids a road was opened under: the station's, the device id it gave this computer, and the
/// session it stamped.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ids {
    pub station: String,
    pub device: String,
    pub session: String,
}

/// The hello at `at`: the socket and the session the station stamped, or why not.
async fn hello(
    at: SocketAddrV4,
    config: Arc<ClientConfig>,
    record: &PairedStation,
) -> Result<(Socket, String), &'static str> {
    let Opened { mut socket, .. } = open(at, config).await.map_err(|failed| match failed {
        Failed::Unreachable => "unreachable",
        // Nothing answered at the address: why, in the words the card uses for it.
        Failed::NotConnected(how) => {
            tempo_stream::lan::unreached(&how.into(), tempo_stream::lan::here(*at.ip()))
        }
        Failed::KeyChanged => "keyChanged",
        Failed::Refused => "notPaired",
    })?;
    let (protocol, stream, operation) = VERSIONS;
    let hello = json!({"type":"hello","protocol":protocol,"stream":stream,"operation":operation});
    say(&mut socket, hello.to_string())
        .await
        .map_err(|_| "unreachable")?;
    let first = loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => break text,
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            // Through TLS and then closed without a word: the station no longer has this key
            // paired (removed while this computer connected).
            _ => return Err("notPaired"),
        }
    };
    match serde_json::from_str::<First>(&first) {
        Ok(First::Welcome {
            session_id,
            device_id,
            station_id,
        }) if device_id == record.device_id
            && station_id == record.station_id
            && protocol::identifier(&session_id) =>
        {
            Ok((socket, session_id))
        }
        Ok(First::Refused { reason }) if reason == "updateStation" => Err("updateStation"),
        Ok(First::Refused { reason }) if reason == "updateComputer" => Err("updateComputer"),
        _ => Err("notStation"),
    }
}

/// The addresses the road to the station `record` names is tried at, in turn: `typed` first when
/// the operator gave an address, then each remembered one, the last that worked first.
pub(crate) fn order(record: &PairedStation, typed: Option<SocketAddrV4>) -> Vec<SocketAddrV4> {
    let mut order: Vec<SocketAddrV4> = typed.into_iter().collect();
    for remembered in record
        .addresses
        .iter()
        .filter_map(|a| tempo_stream::lan::typed(a))
    {
        if !order.contains(&remembered) {
            order.push(remembered);
        }
    }
    order
}

/// Open the road to the station `record` names: at `typed` first when the operator gave an
/// address, then at each remembered one in turn. The road and the address that welcomed this
/// computer, or the most telling reason none did.
pub(crate) async fn connect(
    record: &PairedStation,
    typed: Option<SocketAddrV4>,
) -> Result<(Road, SocketAddrV4), &'static str> {
    connect_at(record, &order(record, typed)).await
}

/// Open the road to the station `record` names at each of `order` in turn: the road and the
/// address that welcomed this computer, or the most telling reason none did (`unreachable`, with
/// nowhere to try).
pub(crate) async fn connect_at(
    record: &PairedStation,
    order: &[SocketAddrV4],
) -> Result<(Road, SocketAddrV4), &'static str> {
    let key = ComputerKey::restore(&record.pkcs8).ok_or("storeUnavailable")?;
    let config =
        tls::client::config(&record.pkcs8, &record.station_key).ok_or("storeUnavailable")?;
    let mut told = "unreachable";
    for &at in order {
        let reached = tokio::time::timeout(HANDSHAKE, hello(at, config.clone(), record))
            .await
            .unwrap_or(Err("unreachable"));
        match reached {
            Ok((socket, session)) => {
                let ids = Ids {
                    station: record.station_id.clone(),
                    device: record.device_id.clone(),
                    session,
                };
                return Ok((Road::new(socket, key, record.station_key.clone(), ids), at));
            }
            Err(reason) if weight(reason) > weight(told) => told = reason,
            Err(_) => {}
        }
    }
    Err(told)
}

/// Is `answer` the station's own (S3-M1): signed with `station_key` (SPKI, lowercase hex, the key
/// this computer pinned) over both DTLS fingerprints and the three ids, for exactly `offer`?
/// `stationNotSigned` with no signature line, `stationKeyMismatch` for any other failure: the
/// page's own words for the same two refusals (`station-key.ts`).
pub(crate) fn check_answer(
    station_key: &str,
    answer: &str,
    offer: &str,
    ids: &Ids,
) -> Result<(), &'static str> {
    let mismatch = "stationKeyMismatch";
    let signature = answer
        .lines()
        .find_map(|line| line.strip_prefix(protocol::ANSWER_SIGNATURE_ATTRIBUTE))
        .ok_or("stationNotSigned")?;
    if signature.len() != protocol::SIGNATURE_HEX_CHARS {
        return Err(mismatch);
    }
    let hashed = |sdp: &str| -> Option<[u8; 32]> {
        digest(&SHA256, &protocol::offer_fingerprint(sdp)?)
            .as_ref()
            .try_into()
            .ok()
    };
    let (answered, offered) = (
        hashed(answer).ok_or(mismatch)?,
        hashed(offer).ok_or(mismatch)?,
    );
    let bound =
        protocol::answer_binding(&answered, &offered, &ids.station, &ids.device, &ids.session);
    let spki = protocol::hex_bytes(station_key).ok_or(mismatch)?;
    let point = spki.get(spki.len().saturating_sub(65)..).ok_or(mismatch)?;
    let signature = protocol::hex_bytes(signature).ok_or(mismatch)?;
    UnparsedPublicKey::new(&ECDSA_P256_SHA256_FIXED, point)
        .verify(&bound, &signature)
        .map_err(|_| mismatch)
}

/// What the page is told from the road.
#[derive(Debug, PartialEq)]
pub(crate) enum ToPage {
    /// One of the station's own messages, as it sent it.
    Station(String),
    /// The station's answer did not hold, and was not passed on: the stream is ended at the
    /// station too.
    AnswerRefused(&'static str),
}

/// Where the answer to the last offer stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    /// No offer made, or its answer not here yet.
    Awaited,
    /// Checked, and it held: its candidates go to the page.
    Held,
    /// Checked, and refused: nothing more of that negotiation goes to the page.
    Refused,
}

/// The road, open: this computer welcomed by the station, under [`Ids`].
pub(crate) struct Road {
    sink: SplitSink<Socket, Message>,
    stream: SplitStream<Socket>,
    key: ComputerKey,
    /// The station's key, SPKI as lowercase hex, as this computer pinned it.
    station_key: String,
    pub(crate) ids: Ids,
    /// The last offer signed here, and the lease it went under.
    offered: Option<(String, String)>,
    answer: Answer,
    /// The station's candidates for the last offer, while its answer is checked.
    held: Vec<String>,
    /// What the page is to be told next, in order.
    outbox: VecDeque<ToPage>,
    /// When to ping next.
    ping: tokio::time::Interval,
}

impl Road {
    fn new(socket: Socket, key: ComputerKey, station_key: String, ids: Ids) -> Self {
        let (sink, stream) = socket.split();
        let mut ping = tokio::time::interval(PING_EVERY);
        ping.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        Self {
            sink,
            stream,
            key,
            station_key,
            ids,
            offered: None,
            answer: Answer::Awaited,
            held: Vec::new(),
            outbox: VecDeque::new(),
            ping,
        }
    }

    async fn say(&mut self, text: String) -> Result<(), ()> {
        if text.len() > TO_STATION_BYTES {
            return Err(());
        }
        self.sink
            .send(Message::Text(text.into()))
            .await
            .map_err(|_| ())
    }

    /// One of the page's operation requests, on to the station. `Ok(Some(answer))` for a request
    /// answered here instead, as the station would answer it: one this road does not carry, or
    /// one asked at another operation version. `Err`: the road has gone.
    pub(crate) async fn operation(
        &mut self,
        version: Option<u8>,
        request: &serde_json::Value,
    ) -> Result<Option<String>, ()> {
        let (Some(kind), Some(request_id)) = (
            request["type"].as_str(),
            request["requestId"]
                .as_str()
                .filter(|id| protocol::identifier(id)),
        ) else {
            return Ok(None);
        };
        if !OPERATIONS.contains(&kind) || version.is_some_and(|v| v != OPERATION_VERSION) {
            let answer = json!({"type":"operationResponse","requestId":request_id,
                "error":"stationUnsupported"});
            return Ok(Some(answer.to_string()));
        }
        let text = json!({"type":"operationRequest","request":request}).to_string();
        self.say(text).await.map(|_| None)
    }

    /// One of the page's stream signals, on to the station: an offer signed here first, with this
    /// computer's key for this station and this session (A5).
    pub(crate) async fn signal(&mut self, lease: String, payload: BrowserSignal) -> Result<(), ()> {
        if !protocol::identifier(&lease) {
            return Err(());
        }
        let payload = match payload {
            BrowserSignal::Offer { sdp, .. } => {
                let signature = self.key.sign_offer(
                    &sdp,
                    &self.ids.station,
                    &self.ids.device,
                    &self.ids.session,
                );
                self.offered = Some((sdp.clone(), lease.clone()));
                self.answer = Answer::Awaited;
                self.held.clear();
                BrowserSignal::Offer {
                    sdp,
                    public_key: signature
                        .as_ref()
                        .map(|_| self.key.public_key().to_string()),
                    signature,
                }
            }
            other => other,
        };
        if !payload.valid() {
            return Err(());
        }
        let text = json!({"type":"streamSignal","leaseId":lease,"payload":payload}).to_string();
        self.say(text).await
    }

    /// The next thing to tell the page, or `None` once the station has gone (or said something no
    /// station says, which ends the road as surely). While it waits, it pings the station every
    /// [`PING_EVERY`], so the station never takes this computer for gone.
    pub(crate) async fn next(&mut self) -> Option<ToPage> {
        loop {
            if let Some(told) = self.outbox.pop_front() {
                return Some(told);
            }
            let text = tokio::select! {
                message = self.stream.next() => match message {
                    Some(Ok(Message::Text(text))) => text.to_string(),
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    _ => return None,
                },
                _ = self.ping.tick() => {
                    if self.sink.send(Message::Ping(Vec::new().into())).await.is_err() {
                        return None;
                    }
                    continue;
                }
            };
            self.heard(text).await?;
        }
    }

    /// One message from the station, sorted into the outbox. `None`: not a station's message.
    async fn heard(&mut self, text: String) -> Option<()> {
        #[derive(Deserialize)]
        struct Typed {
            #[serde(rename = "type")]
            kind: String,
        }
        let kind = serde_json::from_str::<Typed>(&text).ok()?.kind;
        match kind.as_str() {
            "operationResponse" | "status" | "streamState" => {
                self.outbox.push_back(ToPage::Station(text));
            }
            "streamSignal" => {
                let RoomToPage::StreamSignal { payload } = serde_json::from_str(&text).ok()? else {
                    return None;
                };
                if !payload.valid() {
                    return None;
                }
                match payload {
                    StationSignal::Answer { sdp } => self.answered(&sdp, text).await,
                    StationSignal::Candidate { .. } => match self.answer {
                        Answer::Held => self.outbox.push_back(ToPage::Station(text)),
                        Answer::Awaited if self.held.len() < HELD_CANDIDATES => {
                            self.held.push(text)
                        }
                        Answer::Awaited | Answer::Refused => {}
                    },
                }
            }
            _ => return None,
        }
        Some(())
    }

    /// The station's answer to the last offer: on to the page if it holds, with the candidates
    /// that came before it was checked; otherwise refused, and the stream ended at the station.
    async fn answered(&mut self, sdp: &str, text: String) {
        let Some((offer, lease)) = self.offered.clone() else {
            return;
        };
        if self.answer != Answer::Awaited {
            return;
        }
        match check_answer(&self.station_key, sdp, &offer, &self.ids) {
            Ok(()) => {
                self.answer = Answer::Held;
                self.outbox.push_back(ToPage::Station(text));
                for candidate in std::mem::take(&mut self.held) {
                    self.outbox.push_back(ToPage::Station(candidate));
                }
            }
            Err(reason) => {
                // The page is told first: whatever happens to the close, it never takes this
                // negotiation further.
                self.answer = Answer::Refused;
                self.held.clear();
                self.outbox.push_back(ToPage::AnswerRefused(reason));
                let close = json!({"type":"streamSignal","leaseId":lease,"payload":BrowserSignal::Close {}});
                let _ = self.say(close.to_string()).await;
            }
        }
    }

    /// Close the road. The station ends this computer's stream and lease with it.
    pub(crate) async fn close(mut self) {
        let _ = tokio::time::timeout(Duration::from_secs(1), self.sink.close()).await;
    }
}
