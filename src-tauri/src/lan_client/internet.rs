//! The internet road, this computer's half of the Remote service (`remote/src/index.ts`, THE NATIVE
//! CLIENT): registering the road for a station, and opening it again each time client mode needs
//! it.
//!
//! ## Registering the road ([`Service::register`])
//!
//! In the same step as signing in with the station's account (`super::account`), with the token
//! that sign-in gave, kept in memory for that one exchange and then dropped: this computer's key for
//! the road (N3: a key of its own, never the LAN road's) registered as a device of the account's
//! station, signed by that key, with its LAN key and its sign-in's proof, which approve it at once.
//! A station paired with a code whose owner has not signed in at the shack lists no proof; there
//! the road is registered without one, for the station the operator picks ([`chosen`]), and waits
//! for Approve at the station as before. What is kept is the road's record (`vault::InternetRoad`),
//! in the OS credential store beside the station's: the service's station id, the device id it
//! gave, the station key (pinned from then on) and the road's key. No account credential is ever
//! kept.
//!
//! The station asks the operator to approve this computer only if the key it shows matches the one
//! Nexus shows on this computer, so the window shows this computer's key for each road too
//! ([`shown_key`]): the public half's fingerprint, as the station shows it, never the private key.
//!
//! ## Opening it ([`Service::pipe`], [`Service::turn`], [`Service::renew`])
//!
//! With no sign-in: every request names the device, and is signed by its key over a challenge the
//! service gives for that device alone (reopening). The pipe through the station's room is a signed
//! WebSocket the command link's TLS runs inside; the TURN credentials are for the two datagram
//! roads (audio and Stop) beside it; the pipe lives a minute unless renewed, so it is renewed every
//! [`RENEW_EVERY`] while the link lives.
//!
//! ## Held roads
//!
//! A road whose station key this computer has not heard from the station itself, nor seen bound to
//! its own account's sign-in, is held (`Stations::road_held`): client mode opens nothing for it, not
//! a request to the service, until the operator confirms the key, or a station paired here with a
//! code names that key on its own network; and one the station disowned on its own network stays
//! off (`stationKeyUnconfirmed`, `stationKeyDisagrees`).
//!
//! ## Refusals
//!
//! The service names what it refused; only the names below are taken from it ([`named`]), as the
//! shack's own transport does, and anything else is said by its status alone.
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tempo_client::native::{self, TurnServer};
use tempo_client::ComputerKey;
use tokio::net::TcpStream;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;
use tokio_tungstenite::tungstenite::protocol::WebSocketConfig;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream};

use super::Stations;
use crate::remote_service::vault::InternetRoad;

/// How often an open pipe is renewed. The room gives it a minute at a time.
pub(crate) const RENEW_EVERY: Duration = Duration::from_secs(20);
/// How long the pipe's socket may take to open: the room waits 10 s for the station's end.
const PIPE_OPEN: Duration = Duration::from_secs(12);
/// The most of a service answer read.
const ANSWER_LIMIT: usize = 16384;
/// The largest message either end of the pipe may send (the room's bound).
use tempo_client::tunnel::PIPE_MESSAGE_BYTES;

/// The pipe's socket, as it is opened.
pub(crate) type PipeSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;

/// An account with more than one station asks the operator which is the one being added: each
/// station's id and name are offered, and the one picked comes back, or none (cancelled, or the
/// window gone).
pub(crate) type Choose =
    Arc<dyn Fn(Vec<Choice>) -> Pin<Box<dyn Future<Output = Option<String>> + Send>> + Send + Sync>;

/// One of the account's stations, as the operator is offered it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub(crate) struct Choice {
    pub id: String,
    pub name: String,
}

/// How long the operator has to pick the station, as for the sign-in.
pub(crate) const CHOOSE_WAIT: Duration = crate::account_sign_in::SIGN_IN_WAIT;

/// The longest station name offered, in characters: the service's bound for a name (`label`).
const CHOICE_NAME_CHARS: usize = 48;
/// The most stations offered at once, as the page reads the list.
const MOST_CHOICES: usize = 16;

/// Forgetting a station's internet road at the service, as Forget on this computer does: the
/// device's approval ended there, or why the service was not told.
pub(crate) type ForgetInternet = Arc<
    dyn Fn(InternetRoad) -> Pin<Box<dyn Future<Output = Result<(), &'static str>> + Send>>
        + Send
        + Sync,
>;

/// The Remote service, as this computer's internet road speaks to it.
#[derive(Clone)]
pub(crate) struct Service {
    http: reqwest::Client,
    origin: String,
}

impl Service {
    /// The service at `origin`: the one Nexus is built for, or in a test an `http://127.0.0.1:port`
    /// stand-in.
    pub(crate) fn new(origin: &str) -> Result<Self, &'static str> {
        let allowed = origin == crate::remote_service::REMOTE_ORIGIN;
        #[cfg(test)]
        let allowed = allowed
            || origin
                .strip_prefix("http://127.0.0.1:")
                .is_some_and(|port| port.parse::<u16>().is_ok());
        if !allowed {
            return Err("originDenied");
        }
        let http = reqwest::Client::builder()
            .https_only(!cfg!(test))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "serviceUnavailable")?;
        Ok(Self {
            http,
            origin: origin.to_string(),
        })
    }

    /// The path a request's signature covers: the URL's own.
    fn path(verb: &str) -> String {
        format!("/api/remote/{verb}")
    }

    /// A POST of `body` to `verb`, with `headers`: its answer, or the refusal it names.
    async fn post(
        &self,
        verb: &str,
        headers: &[(&str, &str)],
        body: Vec<u8>,
    ) -> Result<Value, &'static str> {
        let mut request = self
            .http
            .post(format!("{}{}", self.origin, Self::path(verb)))
            .header("content-type", "application/json")
            .body(body);
        for (name, value) in headers {
            request = request.header(*name, *value);
        }
        answer(request.send().await.map_err(|_| "serviceUnavailable")?).await
    }

    /// A challenge for the next signed request: the signed-in account's with `token`, or with none
    /// this device's own, reopening the station `road` names.
    async fn challenge(
        &self,
        token: Option<&str>,
        road: Option<&InternetRoad>,
    ) -> Result<String, &'static str> {
        let given = match (token, road) {
            (Some(token), _) => {
                let bearer = format!("Bearer {token}");
                self.post(
                    "client/challenge",
                    &[("authorization", &bearer)],
                    b"{}".to_vec(),
                )
                .await?
            }
            (None, Some(road)) => {
                self.post(
                    &format!("stations/{}/client/challenge", road.station_id),
                    &[("x-nexus-device-id", &road.device_id)],
                    b"{}".to_vec(),
                )
                .await?
            }
            (None, None) => return Err("signInRequired"),
        };
        given["challenge"]
            .as_str()
            .filter(|c| c.len() == 64 && c.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_string)
            .ok_or("invalidResponse")
    }

    /// The station's verb `verb` (`turn`, `renew`, `forget`), as the device `road` names, signed by
    /// its key over a challenge given for it, with no sign-in.
    async fn reopened(&self, road: &InternetRoad, verb: &str) -> Result<Value, &'static str> {
        let key = ComputerKey::restore(&road.pkcs8).ok_or("storeUnavailable")?;
        let challenge = self.challenge(None, Some(road)).await?;
        let verb = format!("stations/{}/client/{verb}", road.station_id);
        let body = b"{}".to_vec();
        let signature = key
            .sign_request(&challenge, "POST", &Self::path(&verb), &body)
            .ok_or("serviceUnavailable")?;
        self.post(
            &verb,
            &[
                ("x-nexus-device-id", &road.device_id),
                ("x-nexus-challenge", &challenge),
                ("x-nexus-signature", &signature),
            ],
            body,
        )
        .await
    }

    /// TURN servers for this device's two datagram roads, UDP only.
    pub(crate) async fn turn(&self, road: &InternetRoad) -> Result<Vec<TurnServer>, &'static str> {
        Ok(native::turn_servers(&self.reopened(road, "turn").await?))
    }

    /// The open pipe's minute renewed.
    pub(crate) async fn renew(&self, road: &InternetRoad) -> Result<(), &'static str> {
        self.reopened(road, "renew").await.map(|_| ())
    }

    /// This device's approval at the station ended, as the device itself asks (Forget on this
    /// computer): signed by its key, with no sign-in.
    pub(crate) async fn forget(&self, road: &InternetRoad) -> Result<(), &'static str> {
        self.reopened(road, "forget").await.map(|_| ())
    }

    /// The pipe to the station through its room: a WebSocket, signed by this device's key over a
    /// challenge given for it, which the command link's TLS then runs inside.
    pub(crate) async fn pipe(&self, road: &InternetRoad) -> Result<PipeSocket, &'static str> {
        let key = ComputerKey::restore(&road.pkcs8).ok_or("storeUnavailable")?;
        let challenge = self.challenge(None, Some(road)).await?;
        let path = Self::path(&format!("stations/{}/client/pipe", road.station_id));
        let signature = key
            .sign_request(&challenge, "GET", &path, b"")
            .ok_or("serviceUnavailable")?;
        let origin = self
            .origin
            .replacen("https://", "wss://", 1)
            .replacen("http://", "ws://", 1);
        let mut request = format!("{origin}{path}")
            .into_client_request()
            .map_err(|_| "invalidRequest")?;
        let headers = request.headers_mut();
        for (name, value) in [
            ("x-nexus-device-id", road.device_id.as_str()),
            ("x-nexus-challenge", &challenge),
            ("x-nexus-signature", &signature),
        ] {
            headers.insert(name, value.parse().map_err(|_| "invalidRequest")?);
        }
        // `true`: no Nagle. A Stop's frame, and its answer, go as they are written.
        let connect = tokio_tungstenite::connect_async_with_config(request, Some(config()), true);
        match tokio::time::timeout(PIPE_OPEN, connect).await {
            Ok(Ok((socket, _))) => Ok(socket),
            Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) => {
                let status = response.status().as_u16();
                let said = response
                    .body()
                    .as_deref()
                    .and_then(|body| serde_json::from_slice::<Value>(body).ok());
                Err(named(status, said.as_ref()))
            }
            _ => Err("serviceUnavailable"),
        }
    }

    /// The account's stations, each with its station key, its LAN key and the proof that vouches
    /// for both, and its entitlement, for the signed-in account `token` is for.
    pub(crate) async fn session(&self, token: &str) -> Result<Value, &'static str> {
        let bearer = format!("Bearer {token}");
        self.post(
            "client/session",
            &[("authorization", &bearer)],
            b"{}".to_vec(),
        )
        .await
    }

    /// This computer registered as a device of the account's station `station_id`, named `name`,
    /// by its road key `key`, signed by that key; with `vouched`, also its LAN key and its sign-in's
    /// proof, which approve it at once when the proof is the station owner's or an invited person's.
    /// The device id the service gave, and whether it is approved already (on the proof, or a key
    /// registered before and approved since).
    pub(crate) async fn register(
        &self,
        token: &str,
        station_id: &str,
        key: &ComputerKey,
        vouched: Option<(&ComputerKey, &str)>,
        name: &str,
    ) -> Result<(String, bool), &'static str> {
        if !tempo_stream::protocol::identifier(station_id) {
            return Err("invalidResponse");
        }
        let verb = format!("stations/{station_id}/client/device");
        let mut asked = json!({"name":name,"kind":"computer","platform":platform(),
            "publicKey":key.public_key()});
        if let Some((lan, proof)) = vouched {
            asked["lanKey"] = lan.public_key().into();
            asked["proof"] = proof.into();
        }
        let body = serde_json::to_vec(&asked).map_err(|_| "serviceUnavailable")?;
        let challenge = self.challenge(Some(token), None).await?;
        let signature = key
            .sign_request(&challenge, "POST", &Self::path(&verb), &body)
            .ok_or("serviceUnavailable")?;
        let bearer = format!("Bearer {token}");
        let registered = self
            .post(
                &verb,
                &[
                    ("authorization", &bearer),
                    ("x-nexus-challenge", &challenge),
                    ("x-nexus-signature", &signature),
                ],
                body,
            )
            .await?;
        let device_id = registered["deviceId"]
            .as_str()
            .filter(|id| tempo_stream::protocol::identifier(id))
            .ok_or("invalidResponse")?;
        Ok((
            device_id.to_string(),
            registered["approved"].as_bool() == Some(true),
        ))
    }
}

/// The pipe's socket: binary messages up to the room's bound, written as they come.
fn config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(PIPE_MESSAGE_BYTES))
        .max_frame_size(Some(PIPE_MESSAGE_BYTES))
        .write_buffer_size(0)
}

/// This computer's platform, as the station shows it at approval.
fn platform() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else {
        "linux"
    }
}

/// A service answer: its JSON, or the refusal it names.
async fn answer(mut response: reqwest::Response) -> Result<Value, &'static str> {
    let status = response.status().as_u16();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "serviceUnavailable")? {
        if bytes.len() + chunk.len() > ANSWER_LIMIT {
            return Err("invalidResponse");
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = serde_json::from_slice::<Value>(&bytes).ok();
    if !(200..300).contains(&status) {
        return Err(named(status, body.as_ref()));
    }
    body.ok_or("invalidResponse")
}

/// What the service refused, by the name it gave when it is one of these (an ALLOW-LIST: a code
/// from the service never becomes text this side shows), otherwise by its status alone.
pub(crate) fn named(status: u16, body: Option<&Value>) -> &'static str {
    match body.and_then(|b| b["error"].as_str()) {
        // N4: the internet road is the paid service's; the trial or the plan does not take it in.
        Some("trialRequired" | "trialEnded" | "trialDisabled") => "internetNotIncluded",
        // Reopening is refused alike for a device removed, waiting, unknown or never approved.
        Some("signInRequired") => "notApprovedThere",
        Some("deviceNotApproved") => "waitingApproval",
        Some("deviceLimit") => "deviceLimit",
        Some("stationUnavailable") => "stationUnavailable",
        Some("serviceNotConfigured") => "internetNotReady",
        Some("relayNotConfigured" | "relayUnavailable") => "relayUnavailable",
        Some("pipeClosed") => "pipeClosed",
        _ => match status {
            401 | 403 => "accessDenied",
            404 => "stationUnavailable",
            429 => "tryLater",
            _ => "serviceUnavailable",
        },
    }
}

/// Whether a refusal will not change by asking again: the operator must act first.
pub(crate) fn lasting(reason: &str) -> bool {
    matches!(
        reason,
        "internetNotIncluded"
            | "notApprovedThere"
            | "waitingApproval"
            | "stationUnavailable"
            | "internetNotReady"
            | "accessDenied"
            // Held until the operator checks the station's key, or for good (`Stations::road_held`).
            | "stationKeyUnconfirmed"
            | "stationKeyDisagrees"
    )
}

/// This computer's key for `road` as the station shows it when it asks to approve this computer,
/// and under Remote access ▸ Your computers: the station's own fingerprint
/// (`remote_service::fingerprint`) of the key this computer registered, its public half. The
/// private half is never shown, sent or logged. `None` for a record with no key in it.
pub(crate) fn shown_key(road: &InternetRoad) -> Option<String> {
    crate::remote_service::fingerprint(ComputerKey::restore(&road.pkcs8)?.public_key())
}

/// For a road registered without a proof (see the header), from the account's `session`: the
/// station, its only one, or the one the operator picks through `choose`, as the service listed it.
/// `internetNotIncluded` when the account's entitlement does not take in the internet road (N4):
/// nothing is registered then.
pub(crate) async fn chosen(session: &Value, choose: Choose) -> Result<Value, &'static str> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if !native::internet_included(&session["entitlement"], now) {
        return Err("internetNotIncluded");
    }
    let listed = session["stations"].as_array().cloned().unwrap_or_default();
    match listed.as_slice() {
        [] => Err("stationUnavailable"),
        [only] => Ok(only.clone()),
        _ => {
            // Only a station the service listed can be picked: the answer is looked up in the
            // list, never taken as an id of its own.
            let offered: Vec<Choice> = listed
                .iter()
                .filter_map(|station| {
                    let id = station["id"]
                        .as_str()
                        .filter(|id| tempo_stream::protocol::identifier(id))?;
                    let name = station["name"].as_str().unwrap_or("");
                    Some(Choice {
                        id: id.to_string(),
                        name: name.chars().take(CHOICE_NAME_CHARS).collect(),
                    })
                })
                .take(MOST_CHOICES)
                .collect();
            let picked = choose(offered).await.ok_or("stationNotChosen")?;
            listed
                .iter()
                .find(|station| {
                    station["id"].as_str() == Some(picked.as_str())
                        && tempo_stream::protocol::identifier(&picked)
                })
                .cloned()
                .ok_or("stationNotChosen")
        }
    }
}

/// The internet road as client mode opens it (`client_mode::Afar`): each road's record read from
/// this computer's store, the Remote service asked for the pipe, the relay and each renewal.
pub(crate) struct Roads {
    stations: Arc<Stations>,
    service: Service,
}

impl Roads {
    pub(crate) fn new(stations: Arc<Stations>) -> Result<Self, &'static str> {
        Self::at(stations, crate::remote_service::REMOTE_ORIGIN)
    }

    /// The same, with the Remote service at `origin` (a test's stand-in).
    pub(crate) fn at(stations: Arc<Stations>, origin: &str) -> Result<Self, &'static str> {
        Ok(Self {
            stations,
            service: Service::new(origin)?,
        })
    }

    /// The road kept for the station paired here as `id`, unless it is held, read off the async
    /// threads.
    async fn unheld(stations: Arc<Stations>, id: String) -> Result<InternetRoad, &'static str> {
        tokio::task::spawn_blocking(move || match stations.road_held(&id)? {
            Some(why) => Err(why),
            None => stations.internet(&id)?.ok_or("unknownStation"),
        })
        .await
        .unwrap_or(Err("storeUnavailable"))
    }

    /// The road kept for the station paired here as `id`, read off the async threads.
    async fn road(stations: Arc<Stations>, id: String) -> Result<InternetRoad, &'static str> {
        tokio::task::spawn_blocking(move || stations.internet(&id))
            .await
            .unwrap_or(Err("storeUnavailable"))?
            .ok_or("unknownStation")
    }
}

impl crate::client_mode::Afar for Roads {
    fn has_road(&self, id: &str) -> bool {
        matches!(self.stations.internet(id), Ok(Some(_)))
    }

    fn open(
        &self,
        id: String,
        headset: Option<Arc<dyn tempo_client::audio_road::Headset>>,
    ) -> Pin<Box<dyn Future<Output = Result<tempo_client::link::Link, &'static str>> + Send>> {
        let (stations, service) = (Arc::clone(&self.stations), self.service.clone());
        // A held road opens nothing: not a request to the service, let alone the pipe.
        Box::pin(async move {
            let road = Self::unheld(stations, id).await?;
            open(&service, &road, headset).await
        })
    }

    fn renew(&self, id: String) -> Pin<Box<dyn Future<Output = Result<(), &'static str>> + Send>> {
        let (stations, service) = (Arc::clone(&self.stations), self.service.clone());
        Box::pin(async move {
            let road = Self::road(stations, id).await?;
            service.renew(&road).await
        })
    }

    fn heard_key(&self, id: &str, key: &str) {
        if let Ok(true) = self.stations.disown_road(id, key) {
            tempo_core::applog::warn(
                "client",
                "the station named another key for the internet road than the one kept here: that road is off",
            );
        }
    }
}

/// How long the STUN server's address may take to look up before the roads go without it.
const STUN_LOOKUP: Duration = Duration::from_secs(2);

/// The command link to the station `road` names, through its room: the TURN credentials asked
/// (the roads go direct only, said in the log, when the service has none to give), the STUN
/// server's address looked up, the signed pipe opened, and the link's TLS, hello and `open` run
/// inside it with the two internet roads beside it, each on a socket of its own.
pub(crate) async fn open(
    service: &Service,
    road: &InternetRoad,
    headset: Option<Arc<dyn tempo_client::audio_road::Headset>>,
) -> Result<tempo_client::link::Link, &'static str> {
    let relay = match service.turn(road).await {
        Ok(servers) => relay(servers).await,
        Err(why) => {
            tempo_core::applog::warn(
                "client",
                &format!("no relay for the internet roads ({why}); they try direct paths only"),
            );
            None
        }
    };
    let lookup = tokio::net::lookup_host(tempo_stream::stun::SERVER);
    let stun = match tokio::time::timeout(STUN_LOOKUP, lookup).await {
        Ok(Ok(mut found)) => found.find(SocketAddr::is_ipv4),
        _ => None,
    };
    let socket = service.pipe(road).await?;
    let (pipe, _pumps) = tempo_client::tunnel::bridge(socket);
    let record = tempo_client::PairedStation {
        station_id: road.station_id.clone(),
        device_id: road.device_id.clone(),
        station_key: road.station_key.clone(),
        pkcs8: road.pkcs8.clone(),
        addresses: Vec::new(),
    };
    let internet = tempo_client::link::Internet {
        bind: Ipv4Addr::UNSPECIFIED.into(),
        direct: true,
        stun,
        relay,
    };
    tempo_client::link::Link::connect_tunnel(
        pipe,
        &record,
        tempo_client::link::run_id(),
        headset,
        &internet,
    )
    .await
}

/// The TURN servers' addresses, IPv4, each looked up within a second, with their credentials, as
/// the internet roads' relay takes them: the first that resolves.
pub(crate) async fn relay(servers: Vec<TurnServer>) -> Option<tempo_client::ice::Relay> {
    for server in servers {
        let lookup = tokio::net::lookup_host((server.host.as_str(), server.port));
        let Ok(Ok(mut found)) = tokio::time::timeout(Duration::from_secs(1), lookup).await else {
            continue;
        };
        if let Some(at) = found.find(SocketAddr::is_ipv4) {
            return Some(tempo_client::ice::Relay {
                server: at,
                username: server.username,
                password: server.password,
            });
        }
    }
    None
}

#[cfg(test)]
mod tests;
