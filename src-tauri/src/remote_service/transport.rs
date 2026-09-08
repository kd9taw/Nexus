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

pub async fn connected(
    client: &Client,
    station_id: &str,
    token: &str,
    mut stop: watch::Receiver<bool>,
    engine: &crate::SharedEngine,
    publisher: &crate::remote_monitor::Publisher,
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
    let config = WebSocketConfig::default()
        .max_message_size(Some(512))
        .max_frame_size(Some(512))
        .write_buffer_size(0)
        .max_write_buffer_size(tempo_app::remote_monitor::MAX_FRAME_BYTES + 1024);
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
    status.set("connected", None);
    let mut tick = tokio::time::interval(Duration::from_millis(tempo_app::remote_monitor::POLL_MS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut heartbeat = tokio::time::interval(Duration::from_secs(30));
    heartbeat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut pong_at = Instant::now();
    let mut pending = None;
    loop {
        tokio::select! {
            biased;
            _ = stop.changed() => { let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await; return Ok(()); }
            message = socket.next() => match message {
                Some(Ok(Message::Text(text))) => {
                    let message: ServerMessage = serde_json::from_str(&text).map_err(|_| "invalidResponse")?;
                    match message {
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
            _ = tick.tick(), if pending.is_some() => {
                if *stop.borrow() { return Ok(()); }
                if let Ok(frame) = publisher.read(engine, Instant::now()) {
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
    publisher: crate::remote_monitor::Publisher,
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
            &publisher,
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
