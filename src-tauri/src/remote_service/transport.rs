//! Bounded outbound HTTPS/WSS. Errors are fixed codes: transport Display strings
//! can contain URLs and credentials and must never reach logs or the webview.
use futures_util::{SinkExt, StreamExt};
use ring::rand::{SecureRandom, SystemRandom};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::time::{Duration, Instant};
use tokio::sync::watch;
use tokio_tungstenite::tungstenite::{
    client::IntoClientRequest, protocol::WebSocketConfig, Message,
};

pub const REMOTE_ORIGIN: &str = "https://remote-staging.hamradiotools.io";
const HTTP_LIMIT: usize = 16384;
/// The RX DSP tick, which is also one Opus frame. A literal here so the socket loop is
/// one piece of code in a build without tempo-audio; the assertion below pins it to the
/// encoder's own definition, so the two cannot drift apart silently.
const AUDIO_TICK_MS: u64 = 20;
#[cfg(feature = "radio")]
const _: () = assert!(AUDIO_TICK_MS == tempo_audio::receive_encode::FRAME_MS);

pub fn identifier(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit() && !b.is_ascii_uppercase()
            }
        })
}
pub fn credential(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
pub fn random_secret() -> Result<String, &'static str> {
    let mut bytes = [0; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| "serviceUnavailable")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    origin: String,
}
impl Client {
    pub fn new(origin: &str) -> Result<Self, &'static str> {
        let allowed = origin == REMOTE_ORIGIN;
        #[cfg(test)]
        let allowed = allowed
            || reqwest::Url::parse(origin).is_ok_and(|url| {
                url.scheme() == "http"
                    && url.host_str() == Some("127.0.0.1")
                    && url.port().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.path() == "/"
                    && url.query().is_none()
                    && url.fragment().is_none()
            });
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
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub async fn post(
        &self,
        path: &str,
        token: Option<&str>,
        body: Value,
    ) -> Result<Value, &'static str> {
        let mut request = self
            .http
            .post(format!("{}/api/remote/{path}", self.origin))
            // This Nexus binds remembered grants to the approval generation, so the service may list
            // the generation to it and renew the browser approvals it gives. A service without the
            // browser approval lifetime ignores the header and answers exactly as before.
            .header("x-nexus-device-lifetime", "1")
            .json(&body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let mut response = request.send().await.map_err(|_| "serviceUnavailable")?;
        if !response.status().is_success() {
            let status = response.status().as_u16();
            // The service names what it refused. Mapping on the status alone threw that away, so a
            // station whose access had been revoked reported the same `accessDenied` as a bad
            // token, and the shack could only show that it had stopped working.
            //
            // An ALLOW-LIST, not a pass-through: the error type is &'static str, and a refusal code
            // is attacker-adjacent input that has no business becoming a string this side renders
            // verbatim. Anything unrecognised keeps the old status mapping exactly.
            let named = response
                .chunk()
                .await
                .ok()
                .flatten()
                .and_then(|chunk| serde_json::from_slice::<Value>(&chunk).ok())
                .and_then(|body| body.get("error")?.as_str().map(str::to_owned));
            return Err(match named.as_deref() {
                Some("stationRevoked") => "stationRevoked",
                Some("trialEnded") => "trialEnded",
                Some("trialDisabled") => "trialDisabled",
                // Approve refused because the browser has not yet agreed to attach this station.
                // Unnamed, a 409 falls through to serviceUnavailable - which tells an operator
                // standing at the radio that the service is down, when the fix is one click in
                // their browser.
                Some("awaitingConfirmation") => "awaitingConfirmation",
                _ => match status {
                    401 | 403 => "accessDenied",
                    410 => "pairingExpired",
                    429 => "tryLater",
                    _ => "serviceUnavailable",
                },
            });
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| "serviceUnavailable")? {
            if bytes.len() + chunk.len() > HTTP_LIMIT {
                return Err("invalidResponse");
            }
            bytes.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| "invalidResponse")
    }
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum ServerMessage {
    OperationDisconnect {
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    OperationRequest {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        #[serde(rename = "operationVersion")]
        operation_version: Option<u8>,
        request: Box<super::operations::Request>,
    },
    ApplicationQuery {
        #[serde(flatten)]
        request: super::query::Request,
    },
    ApplicationWatch {
        #[serde(rename = "watchId")]
        watch_id: String,
        topics: Vec<super::application::Command>,
        #[serde(rename = "requestId")]
        request_id: Option<String>,
    },
    ApplicationCredit {
        #[serde(rename = "watchId")]
        watch_id: String,
        #[serde(rename = "previousRequestId")]
        previous_request_id: String,
        #[serde(rename = "requestId")]
        request_id: String,
    },
    ApplicationRead {
        #[serde(rename = "requestId")]
        request_id: String,
        command: super::application::Command,
        revision: Option<u64>,
    },
    Watch {
        enabled: bool,
        #[serde(rename = "requestId")]
        request_id: Option<String>,
    },
    /// A browser asking to start or stop listening. The session and device are stamped
    /// by the relay from its own admission record, never asserted by the browser.
    AudioListen {
        #[serde(rename = "sessionId")]
        session_id: String,
        #[serde(rename = "deviceId")]
        device_id: String,
        listening: bool,
        #[serde(rename = "leaseId")]
        lease_id: String,
    },
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct Publication {
    r#type: &'static str,
    request_id: String,
    frame: tempo_app::remote_monitor::Frame,
}

#[derive(Clone)]
pub struct Feeds {
    pub monitor: crate::remote_monitor::Publisher,
    pub spectrum: Option<tempo_app::engine::SpectrumFeed>,
    pub meters: tempo_app::engine::MeterFeed,
    pub sources: Option<super::query::Sources>,
    /// The bounded copy of receive audio, for a listening browser. `None` on a build or
    /// a launch with no capture path, and then the audio lane is never advertised at all
    /// — so a browser is never offered a control the station cannot honour.
    ///
    /// Holding this grants nothing: it is a read of already-captured receive audio, and
    /// while nobody is listening it copies nothing.
    #[cfg(feature = "radio")]
    pub audio: Option<std::sync::Arc<tempo_audio::receive_audio::ReceiveAudioFeed>>,
}
pub async fn connected(
    client: &Client,
    station_id: &str,
    token: &str,
    mut stop: watch::Receiver<bool>,
    engine: &crate::SharedEngine,
    feeds: &Feeds,
    status: &super::SessionStatus,
) -> Result<(), &'static str> {
    if !identifier(station_id) || !credential(token) {
        return Err("credentialStoreUnavailable");
    }
    let origin = client
        .origin
        .replacen("https://", "wss://", 1)
        .replacen("http://", "ws://", 1);
    let mut request = format!("{origin}/api/remote/stations/{station_id}/connect")
        .into_client_request()
        .map_err(|_| "invalidResponse")?;
    request.headers_mut().insert(
        "authorization",
        format!("Bearer {token}")
            .parse()
            .map_err(|_| "credentialStoreUnavailable")?,
    );
    request.headers_mut().insert(
        "x-nexus-application-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    // Old services still recognize the legacy contract during rollback. Only a
    // stream-aware service consumes the separate v2 capability advertisement.
    request.headers_mut().insert(
        "x-nexus-application-stream-version",
        "2".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-application-query-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    // Preserve all previous advertisements for rollback to a v1/v2/v3 service.
    request.headers_mut().insert(
        "x-nexus-application-recall-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    // Keyboard samples are separately advertised; older services keep their
    // exact v1/v2/v3/v4 contracts when rolling back the hosted deployment.
    request.headers_mut().insert(
        "x-nexus-application-keyboard-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-application-insights-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-application-dxpeditions-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-application-memories-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-application-ota-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-application-field-day-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-application-js8-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-application-station-modes-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-application-configuration-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-application-navigation-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    // Park directory search and confirmation diagnostics. A v14 service ignores
    // this header and keeps the exact v14 contract.
    request.headers_mut().insert(
        "x-nexus-application-lookups-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    // The rare-DX alerts the station's Pounce detector raised. A v15 service ignores this
    // header and keeps the exact v15 contract.
    request.headers_mut().insert(
        "x-nexus-application-alerts-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    // Where the rotator is pointing. A v16 service ignores this header and keeps the exact v16
    // contract, so a browser on an older service simply never sees a heading.
    request.headers_mut().insert(
        "x-nexus-application-rotator-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-operation-version",
        "2".parse().map_err(|_| "invalidResponse")?,
    );
    // Older cloud builds still see the v2 baseline. A compatible cloud can
    // negotiate expanded radio actions without removing legacy operations.
    request.headers_mut().insert(
        "x-nexus-operation-max-version",
        "3".parse().map_err(|_| "invalidResponse")?,
    );
    request.headers_mut().insert(
        "x-nexus-operation-ft-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    // Operation v5: this station tells a v5 browser a control's outcome the moment it settles
    // (`operationEvent`), instead of leaving it to poll. A service without the header keeps the
    // exact v4 contract and the browser keeps polling; nothing is pushed to a browser that did
    // not negotiate it.
    request.headers_mut().insert(
        "x-nexus-operation-push-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    // Receive audio, advertised only when this station actually has a capture copy to
    // send. A service that does not see this header never routes an `audioListen` here,
    // which matters: the message parser above rejects unknown fields, so being handed
    // one would take the whole control socket down rather than declining a feature.
    #[cfg(feature = "radio")]
    if feeds.audio.is_some() {
        request.headers_mut().insert(
            "x-nexus-audio-version",
            "1".parse().map_err(|_| "invalidResponse")?,
        );
    }
    let connect =
        tokio_tungstenite::connect_async_with_config(request, Some(socket_config()), false);
    let (socket, _) = tokio::select! {
        biased;
        _ = stop.changed() => return Ok(()),
        result = tokio::time::timeout(Duration::from_secs(10), connect) => {
            match result {
                Ok(Ok(value)) => value,
                Ok(Err(tokio_tungstenite::tungstenite::Error::Http(response))) if [401, 403].contains(&response.status().as_u16()) => return Err("accessDenied"),
                _ => return Err("serviceUnavailable"),
            }
        }
    };
    if *stop.borrow() {
        return Ok(());
    }
    serve(socket, stop, engine, feeds, status).await
}

/// The station socket configuration, one definition for the real connection and the tests: the
/// relay's messages are small, and the write buffer holds one full application batch.
pub(super) fn socket_config() -> WebSocketConfig {
    WebSocketConfig::default()
        .max_message_size(Some(8192))
        .max_frame_size(Some(8192))
        .write_buffer_size(0)
        .max_write_buffer_size(super::application::MAX_BYTES + 1024)
}

/// The station's side of an open socket, until the service ends it or Remote stops. Generic over
/// the transport so a test can drive it over an in-memory pipe of a chosen size.
pub(super) async fn serve<S>(
    socket: tokio_tungstenite::WebSocketStream<S>,
    mut stop: watch::Receiver<bool>,
    engine: &crate::SharedEngine,
    feeds: &Feeds,
    status: &super::SessionStatus,
) -> Result<(), &'static str>
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
{
    // Reading and writing are decoupled: the loop below only ever reads the socket, and
    // everything it sends goes through `outbound`, whose own task writes. A send that is slow
    // to drain (a full application batch on a slow shack uplink) therefore never stops the
    // loop reading — and Stop is read, and admitted, inline, exactly as before.
    let (sink, mut socket) = socket.split();
    let (outbound, mut writer) = Outbound::spawn(sink);
    let authority = status
        .control
        .lock()
        .map_err(|_| "serviceUnavailable")?
        .operations
        .clone();
    let operation_connection = super::operations::Connection::new(authority);
    let mut operation_task: Option<tokio::task::JoinHandle<String>> = None;
    // Settled controls, for operation v5 (`operationEvent`). Bounded, and the authority
    // `try_send`s into it and drops on full: a notice is the poll arriving early, never the
    // only way an outcome reaches a browser. The loop keeps a sender of its own so `recv`
    // below cannot see the channel close while the loop runs.
    let (completion_sink, mut completions) =
        tokio::sync::mpsc::channel::<super::operations::CompletionNotice>(8);
    operation_connection
        .authority
        .watch_completions(completion_sink.clone());
    let mut event_task: Option<tokio::task::JoinHandle<Option<String>>> = None;
    status.set("connected", None);
    let mut tick = tokio::time::interval(Duration::from_millis(tempo_app::remote_monitor::POLL_MS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut pong_at = Instant::now();
    let mut pending = None;
    let mut application =
        super::application::Publisher::with_feeds(feeds.spectrum.clone(), feeds.meters.clone());
    let queries = std::sync::Arc::new(std::sync::Mutex::new(super::query::Publisher::default()));
    application.sstv_images = queries
        .lock()
        .map_err(|_| "applicationUnavailable")?
        .sstv_images
        .clone();
    application.journal = Some(
        queries
            .lock()
            .map_err(|_| "applicationUnavailable")?
            .journal
            .clone(),
    );
    let mut query_task: Option<tokio::task::JoinHandle<String>> = None;
    let mut stream = super::application::Stream::default();
    let mut application_tick = tokio::time::interval(Duration::from_millis(100));
    application_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // The receive-audio lane. It holds no encoder until a browser asks to listen, and
    // its select branch below is guarded on that, so on a station nobody is listening to
    // this timer is never even polled.
    #[cfg(feature = "radio")]
    let mut audio_lane = super::audio::AudioLane::default();
    // A plain bool rather than `audio_lane.listening()`, because a `select!` branch
    // condition cannot carry a `#[cfg]` and the lane type does not exist in a build
    // without tempo-audio. INVARIANT: every block that can change the lane's state
    // re-reads it into this on the way out. There are three, and each is marked.
    #[cfg_attr(not(feature = "radio"), allow(unused_mut))]
    let mut audio_listening = false;
    let mut audio_tick = tokio::time::interval(Duration::from_millis(AUDIO_TICK_MS));
    audio_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let result: Result<(), &'static str> = async { loop {
        tokio::select! {
            biased;
            _ = stop.changed() => return Ok(()),
            // The writer gave up on a send (see `send_deadline`) or the socket failed under it.
            ended = &mut writer => return Err(match ended { Ok(Err(reason)) => reason, _ => "serviceUnavailable" }),
            response = async { operation_task.as_mut().expect("guarded operation task").await }, if operation_task.is_some() => {
                operation_task=None;
                let data=response.map_err(|_|"serviceUnavailable")?;
                outbound.send(Message::Text(data.into()))?;
            }
            message = socket.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    let message: ServerMessage = serde_json::from_str(&text).map_err(|_| "invalidResponse")?;
                    match message {
                        ServerMessage::OperationDisconnect{session_id}=>{
                            if !identifier(&session_id){return Err("invalidResponse")}
                            // A browser that has gone cannot still be listening. This is
                            // also the tab-hide and lease-loss path in practice: the
                            // browser stops its own audio first, and this is the backstop
                            // for the times it cannot.
                            #[cfg(feature = "radio")]
                            { audio_lane.stop(Some(&session_id)); audio_listening = audio_lane.listening(); }
                            operation_connection.authority.disconnect_session(&session_id);
                        },
                        ServerMessage::AudioListen{session_id,device_id,listening,lease_id}=>{
                            if !identifier(&session_id)||!identifier(&device_id)||!identifier(&lease_id){return Err("invalidResponse")}
                            // A build without the audio lane never advertises it, so the
                            // service never routes one here. If one arrives anyway it is
                            // ignored rather than treated as a protocol error: dropping a
                            // working control socket over a message this build simply
                            // cannot serve would be far worse than a listen control that
                            // never lights up.
                            #[cfg(feature = "radio")]
                            {
                                let now = Instant::now();
                                let result = if listening {
                                    match feeds.audio.as_ref() {
                                        None => Err("audioUnavailable"),
                                        Some(feed) => operation_connection.authority
                                            .audio_admitted(&session_id, &device_id, &lease_id, now)
                                            .and_then(|()| audio_lane.start(feed, &session_id, &device_id, &lease_id, now)),
                                    }
                                } else {
                                    audio_lane.stop(Some(&session_id));
                                    Ok(())
                                };
                                let data = match result {
                                    Ok(()) => super::audio::audio_state(&session_id, listening, None),
                                    Err(reason) => super::audio::audio_state(&session_id, false, Some(super::audio::shared_reason(reason))),
                                };
                                audio_listening = audio_lane.listening();
                                outbound.send(Message::Text(data.into()))?;
                            }
                        },
                        ServerMessage::OperationRequest{session_id,device_id,operation_version,request}=>{
                            if !identifier(&session_id)||!identifier(&device_id)||!identifier(request.id()){return Err("invalidResponse")}
                            if matches!(request.as_ref(), super::operations::Request::StopTransmit { .. }) {
                                // Stop must not queue behind a disk append, another Engine
                                // operation or a send in flight. Its authority path uses neither
                                // of those locks, and it is admitted here, on the reading task,
                                // before its acceptance is even handed to the writer.
                                let result = operation_connection.authority.handle_version(
                                    (operation_connection.id, operation_version.unwrap_or(1)),
                                    &session_id, &device_id, &request, engine, Instant::now());
                                let data = match result {
                                    Ok(value) => json!({"type":"operationResponse","sessionId":session_id,"requestId":request.id(),"value":value}),
                                    Err(error) => json!({"type":"operationResponse","sessionId":session_id,"requestId":request.id(),"error":error}),
                                }.to_string();
                                outbound.send(Message::Text(data.into()))?;
                            } else if operation_task.is_some(){
                                let data=json!({"type":"operationResponse","sessionId":session_id,"requestId":request.id(),"error":"stationBusy"}).to_string();
                                outbound.send(Message::Text(data.into()))?;
                            }else{
                                let authority=operation_connection.authority.clone();let connection=operation_connection.id;let engine=engine.clone();
                                operation_task=Some(tokio::task::spawn_blocking(move||{
                                    let result=if let Some(version)=operation_version { authority.handle_version((connection,version),&session_id,&device_id,&request,&engine,Instant::now()) } else { authority.handle(connection,&session_id,&device_id,&request,&engine,Instant::now()) };
                                    match result{
                                        Ok(value)=>json!({"type":"operationResponse","sessionId":session_id,"requestId":request.id(),"value":value}),
                                        Err(error)=>json!({"type":"operationResponse","sessionId":session_id,"requestId":request.id(),"error":error}),
                                    }.to_string()
                                }));
                            }
                        },
                        ServerMessage::ApplicationQuery { request } => {
                            if !request.valid() { return Err("invalidResponse"); }
                            if query_task.is_some() {
                                let data = json!({ "type": "applicationQueryError", "requestId": request.request_id, "error": "applicationBusy" }).to_string();
                                outbound.send(Message::Text(data.into()))?;
                            } else {
                                let engine = engine.clone(); let queries = queries.clone(); let sources = feeds.sources.clone();
                                query_task = Some(tokio::task::spawn_blocking(move || {
                                    let now = Instant::now();
                                    queries.lock().map_err(|_| "applicationUnavailable").and_then(|mut q| q.read(&request, &engine, sources.as_ref(), now))
                                        .unwrap_or_else(|error| json!({ "type": "applicationQueryError", "requestId": request.request_id, "error": error }).to_string())
                                }));
                            }
                        }
                        ServerMessage::ApplicationWatch { watch_id, topics, request_id } => stream.watch(watch_id, topics, request_id)?,
                        ServerMessage::ApplicationCredit { watch_id, previous_request_id, request_id } => stream.credit(&watch_id, &previous_request_id, request_id)?,
                        ServerMessage::ApplicationRead { request_id, command, revision } => {
                            if !identifier(&request_id) || !command.legacy() { return Err("invalidResponse"); }
                            let data = application.read(engine, command, &request_id, revision, Instant::now())
                                .unwrap_or_else(|error| json!({ "type": "applicationError", "requestId": request_id, "error": error }).to_string());
                            outbound.send(Message::Text(data.into()))?;
                        }
                        ServerMessage::Watch { enabled, request_id } => {
                            if enabled && !request_id.as_deref().is_some_and(identifier) { return Err("invalidResponse"); }
                            if !enabled && request_id.is_some() { return Err("invalidResponse"); }
                            pending = if enabled { request_id } else { None };
                        }
                    }
                },
                Some(Ok(Message::Pong(_))) => pong_at = Instant::now(),
                // tungstenite has already queued the pong; the next read poll writes it.
                Some(Ok(Message::Ping(_))) => {},
                Some(Ok(Message::Close(_))) | None => return Err("serviceUnavailable"),
                _ => return Err("invalidResponse"),
            },
            result = async { match query_task.as_mut() { Some(task) => task.await, None => std::future::pending().await } }, if query_task.is_some() => {
                query_task = None;
                let data = result.map_err(|_| "applicationUnavailable")?;
                outbound.send(Message::Text(data.into()))?;
            },
            // A control settled (operation v5). The event is built on its own blocking task,
            // never on this loop and never in the operation task's slot - so a browser request
            // arriving meanwhile is not told `stationBusy` for it, and Stop above is read exactly
            // as before. One at a time; the queue holds the rest.
            notice = completions.recv(), if event_task.is_none() => {
                if let Some(notice) = notice {
                    let authority = operation_connection.authority.clone(); let engine = engine.clone();
                    event_task = Some(tokio::task::spawn_blocking(move || authority.completion_event(&notice, &engine)));
                }
            },
            event = async { event_task.as_mut().expect("guarded event task").await }, if event_task.is_some() => {
                event_task = None;
                // Nothing to say (the receipt or the connection is gone) is not an error: the
                // browser still has its poll.
                if let Ok(Some(data)) = event {
                    outbound.send(Message::Text(data.into()))?;
                }
            },
            _ = application_tick.tick(), if stream.active() => {
                if let Some(data) = stream.next(&mut application, engine, Instant::now())? {
                    outbound.send(Message::Text(data.into()))?;
                }
            },
            _ = tick.tick(), if pending.is_some() => {
                if *stop.borrow() { return Ok(()); }
                if let Ok(frame) = feeds.monitor.read(engine, Instant::now()) {
                    let data = serde_json::to_string(&Publication { r#type: "publication", request_id: pending.take().unwrap(), frame })
                        .map_err(|_| "invalidResponse")?;
                    if data.len() > tempo_app::remote_monitor::MAX_FRAME_BYTES + 256 { return Err("invalidResponse"); }
                    outbound.send(Message::Text(data.into()))?;
                }
            },
            _ = heartbeat.tick() => {
                if pong_at.elapsed() > Duration::from_secs(65) { return Err("serviceUnavailable"); }
                outbound.send(Message::Ping(Vec::new().into()))?;
            }
            // LAST, deliberately. Every branch above shares one writer, so audio has to
            // be the thing that yields: it sits after the stop signal, every operation
            // response, the observation publication and the application batch, and it is
            // guarded on somebody actually listening so an idle station never polls it.
            //
            // It also never queues. `Outbound::idle` asks whether anything is still on its
            // way out, and a "yes" discards the bundle rather than holding it - the
            // listener hears a 60 ms gap and the control channel is untouched. The
            // alternative, queueing, would put band noise in front of Stop TX.
            _ = audio_tick.tick(), if audio_listening => {
                if *stop.borrow() { return Ok(()); }
                #[cfg(feature = "radio")]
                {
                let now = Instant::now();
                let session = audio_lane.session().unwrap_or_default().to_owned();
                let authority = operation_connection.authority.clone();
                // A lapsed lease or a withdrawn control grant stops the audio. On a slow
                // cadence, so the authority lock stays out of the 20 ms path.
                let lost = audio_lane.recheck(now, |s, d, l| authority.audio_admitted(s, d, l, now));
                let data = if let Some(reason) = lost {
                    Some(super::audio::audio_state(&session, false, Some(super::audio::shared_reason(reason))))
                } else {
                    let writable = outbound.idle();
                    let pump = audio_lane.poll(now, writable);
                    match pump.ended {
                        Some(reason) => Some(super::audio::audio_state(&session, false, Some(super::audio::shared_reason(reason)))),
                        None => pump.message,
                    }
                };
                audio_listening = audio_lane.listening();
                if let Some(data) = data {
                    outbound.send(Message::Text(data.into()))?;
                }
                }
            }
        }
    } }.await;
    // The loop is over. Remote stopping closes the socket as it always did - a second for the
    // writer to drain and send the close frame; anything else just lets the socket go.
    if result.is_ok() {
        drop(outbound);
        let _ = tokio::time::timeout(Duration::from_secs(1), &mut writer).await;
    }
    writer.abort();
    result
}

/// The station's outbound lane: one bounded queue, drained onto the socket by its own task, so a
/// send that is slow to reach the relay never stops the loop reading.
///
/// BACKPRESSURE. Every lane that can produce more than one message is already flow-controlled
/// upstream of this queue: an application batch spends a relay credit and the next waits for it
/// (`application::Stream`), a monitor publication spends its watch request, at most one operation
/// and one query task run at a time, a ping goes every 30 s, and the audio lane sends only into
/// an idle queue and DISCARDS otherwise (`idle`). So the queue holds a handful of messages in
/// normal use, and a full one means the relay has fallen `CAPACITY` whole messages behind - not
/// a slow link, a stalled one - and the session ends (`serviceUnavailable`) to be rebuilt, as a
/// 2 s stall ended it before. Nothing is ever dropped here: every message queued is one the
/// relay or a browser is waiting for.
struct Outbound {
    queue: tokio::sync::mpsc::Sender<Message>,
    /// Messages handed over and not yet on the wire, the one being written included.
    outstanding: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
impl Outbound {
    const CAPACITY: usize = 64;
    fn spawn<S>(
        mut sink: futures_util::stream::SplitSink<tokio_tungstenite::WebSocketStream<S>, Message>,
    ) -> (Self, tokio::task::JoinHandle<Result<(), &'static str>>)
    where
        S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send + 'static,
    {
        let (queue, mut messages) = tokio::sync::mpsc::channel::<Message>(Self::CAPACITY);
        let outstanding = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let written = outstanding.clone();
        let writer = tokio::spawn(async move {
            while let Some(message) = messages.recv().await {
                let deadline = send_deadline(message.len());
                if !matches!(
                    tokio::time::timeout(deadline, sink.send(message)).await,
                    Ok(Ok(()))
                ) {
                    return Err("serviceUnavailable");
                }
                written.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            }
            // The loop has let go of its end: Remote is stopping. Close as the loop used to.
            let _ = sink.send(Message::Close(None)).await;
            Ok(())
        });
        (Self { queue, outstanding }, writer)
    }
    /// Hand a message to the writer without waiting. A full queue is the stalled relay
    /// described above, and the caller's `?` ends the session.
    fn send(&self, message: Message) -> Result<(), &'static str> {
        self.outstanding
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        self.queue.try_send(message).map_err(|_| {
            self.outstanding
                .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
            "serviceUnavailable"
        })
    }
    /// Is nothing on its way out? The audio lane's question, asked every 20 ms: a "no" means
    /// the write path is behind a relay that is not keeping up, and the lane's answer to that
    /// is to DROP the bundle. Queueing instead would let receive audio delay an operation
    /// response on the same writer, which is the one thing that lane must never do.
    #[cfg(feature = "radio")]
    fn idle(&self) -> bool {
        self.outstanding.load(std::sync::atomic::Ordering::SeqCst) == 0
    }
}

/// How long one message may take to reach the wire before the relay is judged gone: the 2 s
/// every send used to get, plus a second per 32 KiB, so an 8 KB publication still has about 2 s
/// and a full 768 KB application batch has 26 s - a 256 kbit/s floor - where 2 s flat tore the
/// session down on any uplink under 3 Mbit/s and sent the same batch again on reconnect. Each
/// message is timed on its own, so a small one is never charged for the large one ahead of it. A
/// relay that has stopped reading altogether is also caught by the 65 s pong deadline above.
fn send_deadline(bytes: usize) -> Duration {
    Duration::from_secs(2) + Duration::from_millis((bytes / 32) as u64)
}

pub async fn supervise(
    client: Client,
    binding: super::vault::Binding,
    token: String,
    mut stop: watch::Receiver<bool>,
    engine: crate::SharedEngine,
    feeds: Feeds,
    status: super::SessionStatus,
) {
    let mut attempts = 0_u32;
    while !*stop.borrow() {
        let started = Instant::now();
        let result = connected(
            &client,
            &binding.station_id,
            &token,
            stop.clone(),
            &engine,
            &feeds,
            &status,
        )
        .await;
        if *stop.borrow() {
            break;
        }
        // Only a refusal of THIS STATION ends Remote: `disabled` is remembered as off across
        // restarts, and the remote operator is then told to turn Remote on at the shack. A
        // message this build cannot parse (`invalidResponse`) is not that - it is a newer
        // service's shape or a corrupt frame, and it used to turn Remote off for good, which
        // the one person affected could not undo. It ends this connection and backs off.
        if matches!(result, Err("accessDenied" | "credentialStoreUnavailable")) {
            status.set("disabled", result.err());
            return;
        }
        if started.elapsed() > Duration::from_secs(60) {
            attempts = 0;
        }
        attempts = attempts.saturating_add(1).min(6);
        // Carry the reason. `disabled` has always said why; `reconnecting` said nothing, so a
        // station backing off because the service is refusing it (tryLater, serviceUnavailable)
        // looked identical to a flaky network from the operator's chair.
        status.set("reconnecting", result.err());
        // Bounded exponential backoff with OS randomness, so a service restart does
        // not make every shack reconnect on the same second.
        let mut jitter = [0; 2];
        let _ = SystemRandom::new().fill(&mut jitter);
        let wait = Duration::from_millis(
            (1000_u64 << attempts) + u64::from(u16::from_le_bytes(jitter) % 1000),
        );
        tokio::select! { _ = stop.changed() => break, _ = tokio::time::sleep(wait) => {} }
    }
}

pub fn station_path(binding: &super::vault::Binding, action: &str) -> String {
    format!("stations/{}/native/{action}", binding.station_id)
}
pub fn empty() -> Value {
    json!({})
}
