//! The loopback origin: where the window's page comes from, and the one WebSocket it talks on.
//!
//! **Why an origin of its own.** Tauri gives every Nexus command to a page it counts as the app's
//! own (its own scheme, its dev or dist address, a custom scheme the app registers), and this
//! computer may run a radio of its own. Served from `http://127.0.0.1:<port>` instead, the page is
//! a remote one to Tauri, and is refused every command, the app's own and every plugin's
//! (`remote_window`'s tests hold that against the access list this build compiles in).
//!
//! **What it serves.** The page (`lan.html`, from the same embedded files the app's own window
//! loads) and its `assets/`, nothing else; and the WebSocket at `socket`, one page session at a time
//! (a newer one ends the older). It listens on the loopback address only, and only while the
//! window is open.
//!
//! **The three gates** ([`Gates`]), checked on every request before anything else is read or
//! answered, each refusing on its own:
//! 1. the launch secret, the first segment of every path (refused: 404);
//! 2. `Host`, exactly `127.0.0.1:<port>`, against DNS rebinding (refused: 421);
//! 3. `Origin`, exactly `http://127.0.0.1:<port>` on the WebSocket, and never another origin on
//!    anything else (refused: 403).
//!
//! **The page's words** (JSON text frames). From the page: `stations`, `find`, `signIn` (the
//! `stationId` of a station on this computer, or none to add one: `super::account`), `pair`
//! (`address`, `code`, `name`), `forget` (`stationId`), `connect` (`stationId`, and an `address`
//! the operator typed, if any), `disconnect`, `operate` (`stationId`, and an `address` the
//! operator typed, if any), `keepAddress` (`stationId`, and the `address` Operate tries first, or
//! none to clear it), `chooseStation` (`stationId`, or none to cancel), `stationAddress` (where the
//! station is, or none to cancel), and on an open road the station's own `operationRequest` and
//! `streamSignal`. To the page: `stations` (`stations`, `computer`, `internet`: each station the
//! internet road has been added to, by `id`, with this computer's `key` for the road as the station
//! shows it; `account`: for each station added with the account, until when it admits this
//! computer on its network and whether to sign in again now; `away`: each station kept by its
//! service id until this computer first reaches it on its network, its `id`, `name` and LAN key's
//! fingerprint `key`; and after a forget the service could not be told of, `forgetUnreached`),
//! `signingIn`, `chooseStation` (`stations`: each `id` and `name`), `stationAddress` (`name`, and
//! the `reason` the last address did not take this computer, if one was tried), `signedIn`
//! (`stationId`, `waiting`, `away` for a station kept by its service id, and this computer's `key`
//! for the station's internet road as the station shows it) or `signInRefused`, `found` (`shacks`,
//! `available`), `paired` (`station`) or `pairRefused`, `connected` (`stationId`, `deviceId`,
//! `sessionId`, `stationKey`, `address`) or `connectRefused`, `closed` (`stationLeft`,
//! `connectionLost`), `answerRefused`, and the station's own `operationResponse`, `streamSignal`,
//! `streamState` and `status`. Every refusal is a code the page says in a sentence. Anything else
//! from the page ends the session.
//!
//! **A typed address** is a station's address as it shows it, or the name its computer goes by
//! (`tempo_stream::lan::Typed`), looked up when it is tried: a name that resolves to no address on
//! a private network is refused as `nameUnresolved`. Either is only where to try; the key pinned at
//! pairing is the station.
//!
//! **Found by name** (`find`, as ruled on 2026-10-04: "By name, or typed"). The stations Windows'
//! own DNS-SD finds on this computer's networks (`tempo_stream::lan::dnssd`), for the pairing
//! dialog's address field to offer: a hint, never an identity, since the road pins the key. The
//! look runs beside the session, never in its way, so an open road's Stop never waits for it.
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use futures_util::stream::SplitSink;
use futures_util::{SinkExt, StreamExt};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use serde_json::json;
use tempo_stream::lan::dnssd::{Found, Unavailable};
use tempo_stream::lan::{Lookup, Typed};
use tempo_stream::protocol::BrowserSignal;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::{Role, WebSocketConfig};
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::WebSocketStream;

use super::account::{Ask, Signed};
use super::internet::{Choice, Choose, CHOOSE_WAIT};
use super::road::{self, Road, ToPage};
use super::{code, hex, pairing, StationView, Stations, MAX_STATIONS};
use crate::remote_service::lan::valid_name;
use crate::remote_service::vault::PairedStation;

/// The port the origin asks for first, beside the station's own 42075: the same one each time, so
/// the page keeps its origin, and with it the microphone permission the operator gave it and the
/// Mic level it remembers. Taken, any free port.
pub(crate) const PREFERRED_PORT: u16 = 42076;
/// The launch secret, as it appears in a path: 32 random bytes in lowercase hex.
pub(crate) const SECRET_CHARS: usize = 64;
/// The largest request head, and how long it may take to arrive.
const HEAD_BYTES: usize = 8192;
const HEAD_WITHIN: Duration = Duration::from_secs(5);
/// Requests at once: room for the page's files as it loads, and its socket.
const CONNECTIONS: usize = 32;
/// The largest message the page may send: an offer is the largest, at the stream's own bound.
const FROM_PAGE_BYTES: usize = 16 * 1024;
/// How long a look for stations by name listens for their answers.
pub(crate) const FIND_FOR: Duration = Duration::from_secs(3);
/// The longest advertised name the page is shown, in UTF-16 units as the page counts them: a
/// station's own is "Nexus" and eight characters of its key.
const FOUND_NAME_UNITS: usize = 64;
/// The page's own policy: its own files and its own socket, nothing from anywhere else.
fn policy(port: u16) -> String {
    format!(
        "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; \
         img-src 'self' data: blob:; font-src 'self' data:; media-src 'self' blob:; \
         connect-src 'self' ws://127.0.0.1:{port}; worker-src 'self'; form-action 'none'; \
         manifest-src 'none'; frame-ancestors 'none'; base-uri 'none'; object-src 'none'"
    )
}

/// The page's own files, by path (`lan.html`, `assets/…`): the bytes and their media type.
pub(crate) type Assets = Arc<dyn Fn(&str) -> Option<(Vec<u8>, String)> + Send + Sync>;

/// The stations that answer by name within a wait (`tempo_stream::lan::dnssd::find`). It blocks
/// for that long, so it is only called on a thread that may wait.
pub(crate) type Find = Arc<dyn Fn(Duration) -> Result<Vec<Found>, Unavailable> + Send + Sync>;

/// All the page can reach through the origin: its own files, this computer's paired stations and
/// the stations found by name. No engine, no command and no other file.
#[derive(Clone)]
pub(crate) struct Reach {
    pub assets: Assets,
    pub stations: Arc<Stations>,
    pub find: Find,
    /// Where a typed host name is looked up (`tempo_stream::lan::lookup` in the app).
    pub lookup: Lookup,
    /// This computer's name, offered as the one a station shows beside its key.
    pub name: String,
    /// Operate from this computer: Nexus quits as itself and starts again driving the paired
    /// station this id names (`client_mode`), at the address the operator typed first, if any.
    pub operate: Arc<dyn Fn(String, Option<Typed>) + Send + Sync>,
    /// Sign in with the station's account (`super::account::sign_in`): for a new station (none),
    /// or for the station on this computer this id names. The sign-in waits in the system browser.
    pub sign_in: SignIn,
    /// End a station's internet road at the service (`internet::Service::forget`), as Forget here
    /// does before it forgets the road's key.
    pub forget_internet: super::internet::ForgetInternet,
    /// The clock a sign-in's age is read by: seconds since 1970.
    pub now_s: fn() -> u64,
}

/// Signing in, as the window asks it: the station on this computer, or none; where the operator
/// picks one of the account's stations; and where they say where the station is.
pub(crate) type SignIn = Arc<
    dyn Fn(
            Option<String>,
            Choose,
            Ask,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Signed, &'static str>> + Send>,
        > + Send
        + Sync,
>;

/// One request's head, read whole before anything is answered. Header names in lowercase.
#[derive(Debug)]
pub(crate) struct Head {
    pub method: String,
    pub target: String,
    pub headers: Vec<(String, String)>,
}

impl Head {
    /// Every value of header `name`.
    fn all(&self, name: &str) -> Vec<&str> {
        self.headers
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }

    /// The value of header `name` when it appears exactly once.
    fn one(&self, name: &str) -> Option<&str> {
        match self.all(name).as_slice() {
            [value] => Some(value),
            _ => None,
        }
    }

    /// Does this request ask for a WebSocket? Then the `Origin` gate is the strict one.
    fn wants_socket(&self) -> bool {
        self.headers
            .iter()
            .any(|(n, v)| n == "upgrade" && v.eq_ignore_ascii_case("websocket"))
    }
}

/// A request head's bytes, as `Head`. `None` for anything that is not one plain HTTP/1.1 request.
pub(crate) fn parse_head(bytes: &[u8]) -> Option<Head> {
    let text = std::str::from_utf8(bytes).ok()?;
    let mut lines = text.split("\r\n");
    let mut request = lines.next()?.split(' ');
    let (method, target, version) = (request.next()?, request.next()?, request.next()?);
    if request.next().is_some() || version != "HTTP/1.1" || !target.starts_with('/') {
        return None;
    }
    let mut headers = Vec::new();
    for line in lines {
        // No folded lines, and no space before the colon (RFC 9112).
        if line.starts_with([' ', '\t']) {
            return None;
        }
        let (name, value) = line.split_once(':')?;
        if name.is_empty() || name.contains([' ', '\t']) {
            return None;
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
    }
    Some(Head {
        method: method.to_string(),
        target: target.to_string(),
        headers,
    })
}

/// Which gate refused a request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refused {
    Secret,
    Host,
    Origin,
}

impl Refused {
    fn status(self) -> &'static str {
        match self {
            Refused::Secret => "404 Not Found",
            Refused::Host => "421 Misdirected Request",
            Refused::Origin => "403 Forbidden",
        }
    }
}

/// The three gates of the loopback origin (see the module header). Nothing else decides whether a
/// request is answered.
pub(crate) struct Gates {
    port: u16,
    host: String,
    origin: String,
    secret: String,
}

/// Equal, in time that does not depend on where they differ.
fn same(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

impl Gates {
    pub(crate) fn new(port: u16, secret: &str) -> Self {
        Self {
            port,
            host: format!("127.0.0.1:{port}"),
            origin: format!("http://127.0.0.1:{port}"),
            secret: secret.to_string(),
        }
    }

    /// The request's path past the secret, if it passes every gate. `socket`: it asks for the
    /// page's WebSocket, which must name the window's own origin; any other request may name none,
    /// as a browser's own same-origin requests do, but never another.
    pub(crate) fn admit<'h>(&self, head: &'h Head, socket: bool) -> Result<&'h str, Refused> {
        let past = head
            .target
            .strip_prefix('/')
            .and_then(|path| path.split_at_checked(SECRET_CHARS))
            .filter(|(secret, rest)| {
                same(secret.as_bytes(), self.secret.as_bytes()) && rest.starts_with('/')
            })
            .map(|(_, rest)| &rest[1..]);
        let Some(past) = past else {
            return Err(Refused::Secret);
        };
        if head.one("host") != Some(self.host.as_str()) {
            return Err(Refused::Host);
        }
        let origin = match head.all("origin").as_slice() {
            [] => !socket,
            [named] => *named == self.origin,
            _ => false,
        };
        if !origin {
            return Err(Refused::Origin);
        }
        Ok(past)
    }
}

/// The origin, running. It stops when this is dropped, and every page session with it.
pub(crate) struct Origin {
    port: u16,
    secret: String,
    stop: watch::Sender<bool>,
}

impl Origin {
    /// Listen on the loopback address, at [`PREFERRED_PORT`] or any free port, with a new secret,
    /// on a thread and a runtime of its own.
    pub(crate) fn start(reach: Reach) -> Result<Self, &'static str> {
        let listener = std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, PREFERRED_PORT))
            .or_else(|_| std::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)))
            .map_err(|_| "originUnavailable")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "originUnavailable")?;
        let port = listener
            .local_addr()
            .map_err(|_| "originUnavailable")?
            .port();
        let mut bytes = [0; 32];
        SystemRandom::new()
            .fill(&mut bytes)
            .map_err(|_| "originUnavailable")?;
        let secret = hex(&bytes);
        let gates = Gates::new(port, &secret);
        let (stop, stopped) = watch::channel(false);
        std::thread::Builder::new()
            .name("nexus-lan-window".into())
            .spawn(move || {
                if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    runtime.block_on(serve(listener, gates, reach, stopped));
                }
            })
            .map_err(|_| "originUnavailable")?;
        Ok(Self { port, secret, stop })
    }

    #[cfg(test)]
    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// The page's address, the launch secret in it: the window's, and nobody else's.
    pub(crate) fn page(&self) -> String {
        format!("http://127.0.0.1:{}/{}/lan.html", self.port, self.secret)
    }
}

impl Drop for Origin {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

async fn serve(
    listener: std::net::TcpListener,
    gates: Gates,
    reach: Reach,
    mut stopped: watch::Receiver<bool>,
) {
    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
        return;
    };
    let gates = Arc::new(gates);
    // The newest page session's number: a session that is not the newest ends.
    let (newest, _) = watch::channel(0u64);
    let newest = Arc::new(newest);
    let mut requests = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _ = stopped.changed() => break,
            Some(_) = requests.join_next(), if !requests.is_empty() => {}
            accepted = listener.accept() => match accepted {
                Ok((stream, peer)) if loopback(peer) && requests.len() < CONNECTIONS => {
                    requests.spawn(request(stream, gates.clone(), reach.clone(), newest.clone()));
                }
                Ok(_) => {}
                // Out of descriptors, say: wait a moment rather than spin.
                Err(_) => tokio::time::sleep(Duration::from_millis(100)).await,
            },
        }
    }
    drop(listener);
    requests.shutdown().await;
}

fn loopback(peer: SocketAddr) -> bool {
    peer.ip().to_canonical().is_loopback()
}

/// Read one request head, within [`HEAD_BYTES`]: the head, and whatever arrived after it.
async fn read_head(stream: &mut TcpStream) -> Option<(Head, Vec<u8>)> {
    let mut buffer = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        let n = stream.read(&mut chunk).await.ok()?;
        if n == 0 {
            return None;
        }
        buffer.extend_from_slice(&chunk[..n]);
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buffer.split_off(end + 4);
            buffer.truncate(end);
            return parse_head(&buffer).map(|head| (head, rest));
        }
        if buffer.len() > HEAD_BYTES {
            return None;
        }
    }
}

fn answer(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\n");
    for (name, value) in headers {
        out.push_str(&format!("{name}: {value}\r\n"));
    }
    out.push_str(&format!(
        "Content-Length: {}\r\nCache-Control: no-store\r\nReferrer-Policy: no-referrer\r\n\
         X-Content-Type-Options: nosniff\r\nCross-Origin-Resource-Policy: same-origin\r\n\
         Connection: close\r\n\r\n",
        body.len()
    ));
    let mut bytes = out.into_bytes();
    bytes.extend_from_slice(body);
    bytes
}

async fn reply(stream: &mut TcpStream, bytes: &[u8]) {
    let _ = stream.write_all(bytes).await;
    let _ = stream.shutdown().await;
}

/// The file a path names: the page, or one of its assets. `None` for anything else.
fn file(path: &str) -> Option<String> {
    let path = path.split('?').next()?;
    match path {
        "" | "lan.html" => Some("lan.html".into()),
        _ => {
            let name = path.strip_prefix("assets/")?;
            let plain = !name.is_empty()
                && !name.starts_with('.')
                && name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'));
            plain.then(|| format!("assets/{name}"))
        }
    }
}

async fn request(
    mut stream: TcpStream,
    gates: Arc<Gates>,
    reach: Reach,
    newest: Arc<watch::Sender<u64>>,
) {
    let Ok(Some((head, rest))) = tokio::time::timeout(HEAD_WITHIN, read_head(&mut stream)).await
    else {
        return;
    };
    let socket = head.wants_socket();
    let path = match gates.admit(&head, socket) {
        Ok(path) => path.to_string(),
        Err(refused) => {
            reply(&mut stream, &answer(refused.status(), &[], b"")).await;
            return;
        }
    };
    if head.method != "GET" {
        reply(&mut stream, &answer("405 Method Not Allowed", &[], b"")).await;
        return;
    }
    if path == "socket" {
        let key = head.one("sec-websocket-key");
        let upgrade = socket
            && head.one("sec-websocket-version") == Some("13")
            && head.one("connection").is_some_and(|c| {
                c.split(',')
                    .any(|t| t.trim().eq_ignore_ascii_case("upgrade"))
            });
        let (Some(key), true) = (key, upgrade) else {
            reply(&mut stream, &answer("400 Bad Request", &[], b"")).await;
            return;
        };
        let switching = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\
             Sec-WebSocket-Accept: {}\r\n\r\n",
            derive_accept_key(key.as_bytes())
        );
        if stream.write_all(switching.as_bytes()).await.is_err() {
            return;
        }
        let config = WebSocketConfig::default()
            .max_message_size(Some(FROM_PAGE_BYTES))
            .max_frame_size(Some(FROM_PAGE_BYTES));
        let socket =
            WebSocketStream::from_partially_read(stream, rest, Role::Server, Some(config)).await;
        let mut mine = 0;
        newest.send_modify(|n| {
            *n += 1;
            mine = *n;
        });
        page(socket, reach, newest.subscribe(), mine).await;
        return;
    }
    let found = file(&path).and_then(|name| (reach.assets)(&name));
    let Some((bytes, kind)) = found else {
        reply(&mut stream, &answer("404 Not Found", &[], b"")).await;
        return;
    };
    let policy = policy(gates.port);
    let mut headers = vec![("Content-Type", kind.as_str())];
    if kind.starts_with("text/html") {
        headers.push(("Content-Security-Policy", policy.as_str()));
        headers.push(("X-Frame-Options", "DENY"));
        headers.push((
            "Permissions-Policy",
            "camera=(), microphone=(self), geolocation=()",
        ));
    }
    reply(&mut stream, &answer("200 OK", &headers, &bytes)).await;
}

/// What the page says. Parsed whole and exactly: anything else ends its session.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum FromPage {
    Stations {},
    Find {},
    Pair {
        address: String,
        code: String,
        name: String,
    },
    Forget {
        #[serde(rename = "stationId")]
        station_id: String,
    },
    Connect {
        #[serde(rename = "stationId")]
        station_id: String,
        #[serde(default)]
        address: Option<String>,
    },
    Disconnect {},
    Operate {
        #[serde(rename = "stationId")]
        station_id: String,
        #[serde(default)]
        address: Option<String>,
    },
    /// Sign in with the station's account: for the station on this computer this id names, or
    /// none to add one.
    SignIn {
        #[serde(rename = "stationId")]
        station_id: Option<String>,
    },
    /// The operator compared the station's key with the station's own screen and it matches: its
    /// internet road is no longer held.
    KeyMatches {
        #[serde(rename = "stationId")]
        station_id: String,
    },
    /// The operator compared the station's key with the station's own screen and it does not
    /// match: for a station paired here with a code, its internet road goes and its pairing on its
    /// network stays; any other is forgotten, as Forget does (`Stations::code_paired`).
    KeyDiffers {
        #[serde(rename = "stationId")]
        station_id: String,
    },
    /// The address Operate tries first for a station, changed, or cleared with none.
    KeepAddress {
        #[serde(rename = "stationId")]
        station_id: String,
        address: Option<String>,
    },
    /// The station the operator picked while signing in, or none (cancelled).
    ChooseStation {
        #[serde(rename = "stationId")]
        station_id: Option<String>,
    },
    /// Where the station is, as the operator typed it while signing in, or none (cancelled).
    StationAddress {
        address: Option<String>,
    },
    OperationRequest {
        #[serde(rename = "operationVersion", default)]
        operation_version: Option<u8>,
        // A `Value`, not raw text: a tagged enum's fields are read from serde's own copy, which
        // a `RawValue` cannot be taken from.
        request: serde_json::Value,
    },
    StreamSignal {
        #[serde(rename = "leaseId")]
        lease_id: String,
        payload: BrowserSignal,
    },
}

type ToPageSink = SplitSink<WebSocketStream<TcpStream>, Message>;

async fn tell(to: &mut ToPageSink, text: String) -> Result<(), ()> {
    to.send(Message::Text(text.into())).await.map_err(|_| ())
}

/// The keychain can block: off the session's thread.
async fn store<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, &'static str> + Send + 'static,
) -> Result<T, &'static str> {
    tokio::task::spawn_blocking(work)
        .await
        .unwrap_or(Err("storeUnavailable"))
}

/// The stations, as the page shows them, which of them have the internet road (each with this
/// computer's key for it as the station shows it, `internet::shown_key`: its public half only), for
/// each added with the account until when it admits this computer on its network, and those kept by
/// their service id until this computer first reaches them there. And for every internet road, the
/// station's key it is pinned to, as the station shows its own (`remote_service::fingerprint`), and
/// why it is held, when it is (`Stations::road_held`).
async fn stations(reach: &Reach) -> String {
    listed(reach).await.to_string()
}

async fn listed(reach: &Reach) -> serde_json::Value {
    let kept = reach.stations.clone();
    let listed = store(move || {
        let list = kept.list()?;
        let internet: Vec<serde_json::Value> = list
            .iter()
            .filter_map(|station| {
                let road = kept.internet(&station.id).ok().flatten()?;
                Some(json!({"id":station.id,"key":super::internet::shown_key(&road)?}))
            })
            .collect();
        let signed: Vec<(String, u64)> = list
            .iter()
            .filter_map(|station| Some((station.id.clone(), kept.signed(&station.id)?)))
            .collect();
        let away = kept.list_away()?;
        let mut keys = Vec::new();
        for id in list.iter().map(|s| &s.id).chain(away.iter().map(|a| &a.id)) {
            let Some(road) = kept.internet(id)? else {
                continue;
            };
            let Some(key) = crate::remote_service::fingerprint(&road.station_key) else {
                continue;
            };
            let mut shown = json!({"id":id,"key":key});
            if let Some(why) = kept.road_held(id)? {
                shown["held"] = json!(why);
            }
            // Whether It doesn't match drops this road alone, keeping the pairing.
            if kept.code_paired(id)? {
                shown["codePaired"] = json!(true);
            }
            keys.push(shown);
        }
        Ok((list, internet, signed, away, keys))
    });
    match listed.await {
        Ok((list, internet, signed, away, keys)) => {
            let now = (reach.now_s)();
            let account: Vec<serde_json::Value> = signed
                .into_iter()
                .map(|(id, issued_at)| {
                    json!({"stationId":id,
                        "until":tempo_client::account::admitted_until(issued_at),
                        "renew":tempo_client::account::renew_due(issued_at, now)})
                })
                .collect();
            json!({"type":"stations","stations":list,"internet":internet,"account":account,
                "away":away,"stationKeys":keys,"computer":reach.name})
        }
        Err(reason) => {
            json!({"type":"stations","stations":[],"computer":reach.name,"error":reason})
        }
    }
}

/// What a look by name found, as the page is told it: each station's advertised name (one the page
/// can show), address, protocol and key tag, and whether the look could be made at all.
fn found(looked: Result<Vec<Found>, Unavailable>) -> String {
    match looked {
        Ok(mut shacks) => {
            shacks.retain(|shack| shack.name.encode_utf16().count() <= FOUND_NAME_UNITS);
            json!({"type":"found","shacks":shacks,"available":true})
        }
        Err(Unavailable) => json!({"type":"found","shacks":[],"available":false}),
    }
    .to_string()
}

/// A pairing the page asked for, with what the operator typed: the station paired and kept, or
/// why not, by name.
async fn pair(
    reach: &Reach,
    typed_address: &str,
    typed_code: &str,
    name: &str,
) -> Result<StationView, &'static str> {
    let at = tempo_stream::lan::typed(typed_address).ok_or("badAddress")?;
    let code = code(typed_code).ok_or("badCode")?;
    if !valid_name(name) {
        return Err("badName");
    }
    // Room on this computer first: a pairing it could not keep is never made at the station. The
    // stations it is paired with already go with the pairing, which may be with one of them again.
    let kept = reach.stations.clone();
    let known = store(move || kept.records()).await?;
    if known.len() >= MAX_STATIONS {
        return Err("stationsFull");
    }
    let record = pairing::pair(at, code, name, &known).await?;
    let view = StationView::of(&record).ok_or("notStation")?;
    let kept = reach.stations.clone();
    store(move || kept.keep_coded(&record)).await?;
    Ok(view)
}

/// One page session, from its socket's opening to its close, or until a newer page opens one.
async fn page(
    socket: WebSocketStream<TcpStream>,
    reach: Reach,
    mut newest: watch::Receiver<u64>,
    mine: u64,
) {
    let (mut to_page, mut from_page) = socket.split();
    let mut road: Option<Road> = None;
    // A look by name under way, on a thread that may wait: a second `find` has its answer.
    let mut finding: Option<tokio::task::JoinHandle<Result<Vec<Found>, Unavailable>>> = None;
    // A sign-in, while it waits for the browser and for the operator: one at a time.
    let mut signing: Option<tokio::task::JoinHandle<Result<Signed, &'static str>>> = None;
    // An account with more than one station: the stations the sign-in offers the page, and where
    // the operator's pick goes back to it.
    let (offering, mut offered) =
        tokio::sync::mpsc::channel::<(Vec<Choice>, tokio::sync::oneshot::Sender<String>)>(1);
    let mut choosing: Option<tokio::sync::oneshot::Sender<String>> = None;
    // A station found nowhere on this network: the sign-in asks where it is, and where the
    // operator's answer goes back to it.
    type Asked = (
        String,
        Option<&'static str>,
        tokio::sync::oneshot::Sender<String>,
    );
    let (asking, mut asked) = tokio::sync::mpsc::channel::<Asked>(1);
    let mut placing: Option<tokio::sync::oneshot::Sender<String>> = None;
    loop {
        let told = tokio::select! {
            biased;
            changed = newest.changed() => {
                if changed.is_err() || *newest.borrow_and_update() != mine { break }
                continue
            }
            told = async { road.as_mut().expect("guarded road").next().await }, if road.is_some() => {
                match told {
                    Some(ToPage::Station(text)) => text,
                    Some(ToPage::AnswerRefused(reason)) => {
                        json!({"type":"answerRefused","reason":reason}).to_string()
                    }
                    None => {
                        road = None;
                        json!({"type":"closed","reason":"stationLeft"}).to_string()
                    }
                }
            }
            looked = async { finding.as_mut().expect("guarded look").await }, if finding.is_some() => {
                finding = None;
                found(looked.unwrap_or(Err(Unavailable)))
            }
            done = async { signing.as_mut().expect("guarded sign-in").await }, if signing.is_some() => {
                signing = None;
                (choosing, placing) = (None, None);
                match done {
                    Ok(Ok(Signed { station, waiting, away, key })) => {
                        json!({"type":"signedIn","stationId":station,"waiting":waiting,
                            "away":away,"key":key})
                    }
                    Ok(Err(reason)) => json!({"type":"signInRefused","reason":reason}),
                    Err(_) => json!({"type":"signInRefused","reason":"signInFailed"}),
                }
                .to_string()
            }
            offer = offered.recv(), if signing.is_some() => {
                let Some((stations, answer)) = offer else { continue };
                choosing = Some(answer);
                json!({"type":"chooseStation","stations":stations}).to_string()
            }
            where_ = asked.recv(), if signing.is_some() => {
                let Some((name, reason, answer)) = where_ else { continue };
                placing = Some(answer);
                let mut told = json!({"type":"stationAddress","name":name});
                if let Some(reason) = reason {
                    told["reason"] = json!(reason);
                }
                told.to_string()
            }
            message = from_page.next() => {
                let text = match message {
                    Some(Ok(Message::Text(text))) => text,
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
                    _ => break,
                };
                let Ok(asked) = serde_json::from_str::<FromPage>(&text) else { break };
                match asked {
                    FromPage::Stations {} => stations(&reach).await,
                    FromPage::Find {} => {
                        if finding.is_none() {
                            let find = reach.find.clone();
                            finding = Some(tokio::task::spawn_blocking(move || find(FIND_FOR)));
                        }
                        continue
                    }
                    FromPage::Pair { address, code, name } => {
                        if let Some(open) = road.take() { open.close().await }
                        match pair(&reach, &address, &code, &name).await {
                            Ok(station) => json!({"type":"paired","station":station}),
                            Err(reason) => json!({"type":"pairRefused","reason":reason}),
                        }
                        .to_string()
                    }
                    FromPage::Forget { station_id } => {
                        if let Some(open) = road.take() { open.close().await }
                        forget(&reach, station_id, false).await
                    }
                    FromPage::KeyDiffers { station_id } => {
                        let kept = reach.stations.clone();
                        let wanted = station_id.clone();
                        let road_only = store(move || kept.code_paired(&wanted)).await == Ok(true);
                        if !road_only {
                            if let Some(open) = road.take() { open.close().await }
                        }
                        forget(&reach, station_id, road_only).await
                    }
                    FromPage::KeyMatches { station_id } => {
                        let kept = reach.stations.clone();
                        match store(move || kept.confirm_road(&station_id)).await {
                            Ok(()) => listed(&reach).await.to_string(),
                            Err(reason) => json!({"type":"connectRefused","reason":reason}).to_string(),
                        }
                    }
                    FromPage::KeepAddress { station_id, address } => {
                        let typed = match address.as_deref().map(Typed::read) {
                            Some(None) => Err("badAddress"),
                            typed => Ok(typed.flatten()),
                        };
                        let kept = reach.stations.clone();
                        let saved = match typed {
                            Ok(typed) => store(move || kept.keep_typed(&station_id, typed.as_ref())).await,
                            Err(reason) => Err(reason),
                        };
                        match saved {
                            Ok(()) => listed(&reach).await.to_string(),
                            Err(reason) => json!({"type":"connectRefused","reason":reason}).to_string(),
                        }
                    }
                    FromPage::Connect { station_id, address: typed } => {
                        if let Some(open) = road.take() { open.close().await }
                        match connect(&reach, &station_id, typed.as_deref()).await {
                            Ok((opened, told)) => { road = Some(opened); told }
                            Err(reason) => json!({"type":"connectRefused","reason":reason}).to_string(),
                        }
                    }
                    FromPage::Disconnect {} => {
                        if let Some(open) = road.take() { open.close().await }
                        json!({"type":"closed","reason":"disconnected"}).to_string()
                    }
                    FromPage::Operate { station_id, address } => {
                        if let Some(open) = road.take() { open.close().await }
                        match operate(&reach, station_id, address.as_deref()).await {
                            Ok(()) => json!({"type":"operating"}).to_string(),
                            Err(reason) => json!({"type":"connectRefused","reason":reason}).to_string(),
                        }
                    }
                    FromPage::SignIn { station_id } => {
                        if signing.is_some() {
                            json!({"type":"signInRefused","reason":"signInUnderWay"}).to_string()
                        } else {
                            let sign_in = reach.sign_in.clone();
                            let offering = offering.clone();
                            let choose: Choose = Arc::new(move |stations| {
                                let offering = offering.clone();
                                Box::pin(async move {
                                    let (answer, picked) = tokio::sync::oneshot::channel();
                                    offering.send((stations, answer)).await.ok()?;
                                    tokio::time::timeout(CHOOSE_WAIT, picked).await.ok()?.ok()
                                })
                            });
                            let asking = asking.clone();
                            let ask: Ask = Arc::new(move |name, reason| {
                                let asking = asking.clone();
                                Box::pin(async move {
                                    let (answer, typed) = tokio::sync::oneshot::channel();
                                    asking.send((name, reason, answer)).await.ok()?;
                                    tokio::time::timeout(CHOOSE_WAIT, typed).await.ok()?.ok()
                                })
                            });
                            signing = Some(tokio::spawn(sign_in(station_id, choose, ask)));
                            json!({"type":"signingIn"}).to_string()
                        }
                    }
                    FromPage::ChooseStation { station_id } => {
                        // None, or nothing being chosen: the pick is dropped, and the sign-in told
                        // so.
                        if let (Some(answer), Some(id)) = (choosing.take(), station_id) {
                            let _ = answer.send(id);
                        }
                        continue
                    }
                    FromPage::StationAddress { address } => {
                        // None, or nothing being asked: the answer is dropped, and the sign-in told
                        // so.
                        if let (Some(answer), Some(address)) = (placing.take(), address) {
                            let _ = answer.send(address);
                        }
                        continue
                    }
                    FromPage::OperationRequest { operation_version, request } => {
                        let Some(open) = road.as_mut() else { continue };
                        match open.operation(operation_version, &request).await {
                            Ok(Some(answered)) => answered,
                            Ok(None) => continue,
                            Err(()) => {
                                road = None;
                                json!({"type":"closed","reason":"connectionLost"}).to_string()
                            }
                        }
                    }
                    FromPage::StreamSignal { lease_id, payload } => {
                        let Some(open) = road.as_mut() else { continue };
                        if open.signal(lease_id, payload).await.is_ok() { continue }
                        road = None;
                        json!({"type":"closed","reason":"connectionLost"}).to_string()
                    }
                }
            }
        };
        if tell(&mut to_page, told).await.is_err() {
            break;
        }
    }
    if let Some(open) = road.take() {
        open.close().await;
    }
    if let Some(signing) = signing.take() {
        signing.abort();
    }
}

/// Forget station `id` on this computer, or with `road_only` its internet road alone
/// (`Stations::drop_road`), and the stations as the page then shows them. The internet road's
/// approval at the service ends first, while this computer still holds the key that signs for it.
/// The service out of reach is said, and never keeps the forget here from going on.
async fn forget(reach: &Reach, id: String, road_only: bool) -> String {
    let kept = reach.stations.clone();
    let wanted = id.clone();
    let unreached = match store(move || kept.internet(&wanted)).await {
        Ok(Some(internet)) => (reach.forget_internet)(internet).await.err(),
        Ok(None) => None,
        Err(reason) => Some(reason),
    };
    let kept = reach.stations.clone();
    let _ = store(move || {
        if road_only {
            kept.drop_road(&id)
        } else {
            kept.forget(&id)
        }
    })
    .await;
    let mut told = listed(reach).await;
    if let Some(reason) = unreached {
        let said = if road_only {
            "dropUnreached"
        } else {
            "forgetUnreached"
        };
        told[said] = json!(reason);
    }
    told.to_string()
}

/// Operate from this computer: Nexus starts again driving station `id` (one on this network, or
/// one kept by its service id, which it drives over the internet road), at the address the operator
/// typed first, if any, read as [`connect`] reads one. The launch proves the key pinned for the
/// station wherever it reaches it, so a typed address is never taken as the station.
async fn operate(reach: &Reach, id: String, typed: Option<&str>) -> Result<(), &'static str> {
    let typed = match typed {
        Some(text) => Some(resolved(reach, text).await?.0),
        None => None,
    };
    let kept = reach.stations.clone();
    let wanted = id.clone();
    let here = store(move || Ok(kept.get(&wanted)?.is_some() || kept.away(&wanted)?.is_some()));
    if !here.await? {
        return Err("unknownStation");
    }
    (reach.operate)(id, typed);
    Ok(())
}

/// What the operator typed, read (`badAddress` when it is neither an address nor a host name), and
/// where it is now: a name looked up through [`Reach::lookup`], `nameUnresolved` when it resolves
/// to no address on a private network.
async fn resolved(reach: &Reach, text: &str) -> Result<(Typed, SocketAddrV4), &'static str> {
    let typed = Typed::read(text).ok_or("badAddress")?;
    let (asked, lookup) = (typed.clone(), reach.lookup.clone());
    let at = tokio::task::spawn_blocking(move || asked.resolve(&lookup))
        .await
        .ok()
        .flatten()
        .ok_or("nameUnresolved")?;
    Ok((typed, at))
}

/// The road to station `id`, opened: the road, and what the page is told of it. It is tried at the
/// address the operator typed, then at the remembered ones, the last that worked first, and only
/// when none of them welcomed it because nothing answered, or because another key did (another
/// station at an address that was this one's), where a look by name finds the station now
/// ([`found_at`]).
async fn connect(
    reach: &Reach,
    id: &str,
    typed: Option<&str>,
) -> Result<(Road, String), &'static str> {
    let typed = match typed {
        Some(text) => Some(resolved(reach, text).await?.1),
        None => None,
    };
    let kept = reach.stations.clone();
    let wanted = id.to_string();
    let record = store(move || kept.get(&wanted))
        .await?
        .ok_or("unknownStation")?;
    let (opened, at) = match road::connect(&record, typed).await {
        Err(why) if road::look_further(why) => {
            let tried = road::order(&record, typed);
            let found = found_at(reach, &record, &tried).await;
            // Nothing found by name: the page hears why the addresses tried did not welcome this
            // computer (`road::told`).
            road::connect_at(&record, &found)
                .await
                .map_err(|later| road::told(why, later, !found.is_empty()))?
        }
        reached => reached?,
    };
    // Remembered first for next time. A store that will not keep it changes nothing now.
    let kept = reach.stations.clone();
    let station = record.station_id.clone();
    let _ = store(move || kept.worked(&station, at)).await;
    let told = json!({"type":"connected","stationId":opened.ids.station,
        "deviceId":opened.ids.device,"sessionId":opened.ids.session,
        "stationKey":record.station_key,"address":at.to_string()});
    Ok((opened, told.to_string()))
}

/// Where a look by name finds the station `record` names, at addresses not in `tried`: each advert
/// whose key tag starts the fingerprint of the key pinned for it, so another station's is never
/// tried (it could only fail the handshake, and tell the page the key had changed).
async fn found_at(
    reach: &Reach,
    record: &PairedStation,
    tried: &[SocketAddrV4],
) -> Vec<SocketAddrV4> {
    let find = reach.find.clone();
    let looked = tokio::task::spawn_blocking(move || find(FIND_FOR)).await;
    road::found_for(
        record,
        tried,
        looked.ok().and_then(Result::ok).unwrap_or_default(),
    )
}
