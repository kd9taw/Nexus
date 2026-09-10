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
            .json(&body);
        if let Some(token) = token {
            request = request.bearer_auth(token);
        }
        let mut response = request.send().await.map_err(|_| "serviceUnavailable")?;
        if !response.status().is_success() {
            return Err(match response.status().as_u16() {
                401 | 403 => "accessDenied",
                410 => "pairingExpired",
                429 => "tryLater",
                _ => "serviceUnavailable",
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
        request: super::operations::Request,
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
    request.headers_mut().insert(
        "x-nexus-operation-version",
        "1".parse().map_err(|_| "invalidResponse")?,
    );
    let config = WebSocketConfig::default()
        .max_message_size(Some(8192))
        .max_frame_size(Some(8192))
        .write_buffer_size(0)
        .max_write_buffer_size(super::application::MAX_BYTES + 1024);
    let connect = tokio_tungstenite::connect_async_with_config(request, Some(config), false);
    let (mut socket, _) = tokio::select! {
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
    let authority = status
        .control
        .lock()
        .map_err(|_| "serviceUnavailable")?
        .operations
        .clone();
    let operation_connection = super::operations::Connection::new(authority);
    let mut operation_task: Option<tokio::task::JoinHandle<String>> = None;
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
    loop {
        tokio::select! {
            biased;
            _ = stop.changed() => { let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await; return Ok(()); }
            response = async { operation_task.as_mut().expect("guarded operation task").await }, if operation_task.is_some() => {
                operation_task=None;
                let data=response.map_err(|_|"serviceUnavailable")?;
                tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Text(data.into()))).await.map_err(|_|"serviceUnavailable")?.map_err(|_|"serviceUnavailable")?;
            }
            message = socket.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    let message: ServerMessage = serde_json::from_str(&text).map_err(|_| "invalidResponse")?;
                    match message {
                        ServerMessage::OperationDisconnect{session_id}=>{if !identifier(&session_id){return Err("invalidResponse")}operation_connection.authority.disconnect_session(&session_id);},
                        ServerMessage::OperationRequest{session_id,device_id,request}=>{
                            if !identifier(&session_id)||!identifier(&device_id)||!identifier(request.id()){return Err("invalidResponse")}
                            if operation_task.is_some(){
                                let data=json!({"type":"operationResponse","sessionId":session_id,"requestId":request.id(),"error":"stationBusy"}).to_string();
                                tokio::time::timeout(Duration::from_secs(2),socket.send(Message::Text(data.into()))).await.map_err(|_|"serviceUnavailable")?.map_err(|_|"serviceUnavailable")?;
                            }else{
                                let authority=operation_connection.authority.clone();let connection=operation_connection.id;let engine=engine.clone();
                                operation_task=Some(tokio::task::spawn_blocking(move||{
                                    match authority.handle(connection,&session_id,&device_id,&request,&engine,Instant::now()){
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
                                tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Text(data.into()))).await.map_err(|_| "serviceUnavailable")?.map_err(|_| "serviceUnavailable")?;
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
                            tokio::select! {
                                biased;
                                _ = stop.changed() => return Ok(()),
                                result = tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Text(data.into()))) => {
                                    result.map_err(|_| "serviceUnavailable")?.map_err(|_| "serviceUnavailable")?;
                                }
                            }
                        }
                        ServerMessage::Watch { enabled, request_id } => {
                            if enabled && !request_id.as_deref().is_some_and(identifier) { return Err("invalidResponse"); }
                            if !enabled && request_id.is_some() { return Err("invalidResponse"); }
                            pending = if enabled { request_id } else { None };
                        }
                    }
                },
                Some(Ok(Message::Pong(_))) => pong_at = Instant::now(),
                Some(Ok(Message::Ping(_))) => {
                    tokio::time::timeout(Duration::from_secs(2), socket.flush()).await
                        .map_err(|_| "serviceUnavailable")?.map_err(|_| "serviceUnavailable")?;
                },
                Some(Ok(Message::Close(_))) | None => return Err("serviceUnavailable"),
                _ => return Err("invalidResponse"),
            },
            result = async { match query_task.as_mut() { Some(task) => task.await, None => std::future::pending().await } }, if query_task.is_some() => {
                query_task = None;
                let data = result.map_err(|_| "applicationUnavailable")?;
                tokio::select! {
                    biased;
                    _ = stop.changed() => return Ok(()),
                    result = tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Text(data.into()))) => {
                        result.map_err(|_| "serviceUnavailable")?.map_err(|_| "serviceUnavailable")?;
                    }
                }
            },
            _ = application_tick.tick(), if stream.active() => {
                if let Some(data) = stream.next(&mut application, engine, Instant::now())? {
                    tokio::select! {
                        biased;
                        _ = stop.changed() => return Ok(()),
                        result = tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Text(data.into()))) => {
                            result.map_err(|_| "serviceUnavailable")?.map_err(|_| "serviceUnavailable")?;
                        }
                    }
                }
            },
            _ = tick.tick(), if pending.is_some() => {
                if *stop.borrow() { return Ok(()); }
                if let Ok(frame) = feeds.monitor.read(engine, Instant::now()) {
                    let data = serde_json::to_string(&Publication { r#type: "publication", request_id: pending.take().unwrap(), frame })
                        .map_err(|_| "invalidResponse")?;
                    if data.len() > tempo_app::remote_monitor::MAX_FRAME_BYTES + 256 { return Err("invalidResponse"); }
                    tokio::select! {
                        biased;
                        _ = stop.changed() => return Ok(()),
                        result = tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Text(data.into()))) => {
                            result.map_err(|_| "serviceUnavailable")?.map_err(|_| "serviceUnavailable")?;
                        }
                    }
                }
            },
            _ = heartbeat.tick() => {
                if pong_at.elapsed() > Duration::from_secs(65) { return Err("serviceUnavailable"); }
                tokio::time::timeout(Duration::from_secs(2), socket.send(Message::Ping(Vec::new().into())))
                    .await.map_err(|_| "serviceUnavailable")?.map_err(|_| "serviceUnavailable")?;
            }
        }
    }
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
        if matches!(
            result,
            Err("accessDenied" | "invalidResponse" | "credentialStoreUnavailable")
        ) {
            status.set("disabled", result.err());
            return;
        }
        if started.elapsed() > Duration::from_secs(60) {
            attempts = 0;
        }
        attempts = attempts.saturating_add(1).min(6);
        status.set("reconnecting", None);
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
