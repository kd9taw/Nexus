//! Local approval and the Remote background task. Engine access is restricted to
//! bounded observation, reviewed application reads and separately approved manual logging.
mod application;
mod aprs;
mod operations;
pub(crate) mod query;
pub(crate) mod sstv;
#[cfg(test)]
mod tests;
mod transport;
mod vault;

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot, watch};
use transport::{credential, empty, identifier, station_path, Client, REMOTE_ORIGIN};
use vault::{Binding, SystemVault, Vault};

#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Status {
    phase: String,
    origin: String,
    station_id: Option<String>,
    account_id: Option<String>,
    pairing_id: Option<String>,
    pairing_code: Option<String>,
    expires_at: Option<u64>,
    devices: Vec<Device>,
    error: Option<&'static str>,
    observation_generation: Option<String>,
    logging_permissions: Vec<String>,
    station_permissions: Vec<String>,
    logging_controller: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Device {
    id: String,
    name: String,
    approved: u8,
    expires_at: u64,
}
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
// Empty struct variants preserve deny_unknown_fields; Serde unit variants ignore extra fields.
pub enum Action {
    LoggingPermission {
        #[serde(rename = "deviceId")]
        device_id: String,
        allow: bool,
    },
    StationPermission {
        #[serde(rename = "deviceId")]
        device_id: String,
        allow: bool,
    },
    TakeOverLogging {},
    Begin {
        name: String,
    },
    Refresh {},
    Cancel {},
    Approve {
        #[serde(rename = "enrollmentId")]
        enrollment_id: String,
        #[serde(rename = "accountId")]
        account_id: String,
    },
    Enable {},
    Disable {},
    Forget {},
    Device {
        #[serde(rename = "deviceId")]
        device_id: String,
        approve: bool,
    },
}
type Reply = oneshot::Sender<Result<(), &'static str>>;
#[derive(Default)]
struct Control {
    operations: Arc<operations::Authority>,
    memories: query::memories::Bank,
    enabled: bool,
    cancel: Option<watch::Sender<bool>>,
    generation: u64,
}
impl Control {
    fn stop(&mut self) {
        self.operations.invalidate();
        self.enabled = false;
        if let Ok(mut bank) = self.memories.lock() {
            *bank = None;
        }
        self.generation = self.generation.wrapping_add(1);
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(true);
        }
    }
}
pub struct Service {
    commands: mpsc::Sender<(Action, u64, Reply)>,
    status: Arc<Mutex<Status>>,
    control: Arc<Mutex<Control>>,
}
// A superseded connection may finish an I/O operation after a newer local
// decision. Its status update must not undo that decision in the desktop UI.
#[derive(Clone)]
struct SessionStatus {
    status: Arc<Mutex<Status>>,
    control: Arc<Mutex<Control>>,
    generation: u64,
}
impl SessionStatus {
    fn set(&self, value: &str, error: Option<&'static str>) {
        if let Ok(mut control) = self.control.lock() {
            if !control.enabled || control.generation != self.generation {
                return;
            }
            if value == "disabled" {
                control.stop();
            }
            phase(&self.status, value, error);
        }
    }
}
pub(super) fn phase(status: &Arc<Mutex<Status>>, value: &str, error: Option<&'static str>) {
    if let Ok(mut status) = status.lock() {
        status.phase = value.into();
        status.error = error;
    }
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 48 && !name.chars().any(char::is_control)
}
impl Service {
    pub fn new(
        engine: crate::SharedEngine,
        publisher: crate::remote_monitor::Publisher,
        spectrum: tempo_app::engine::SpectrumFeed,
        meters: tempo_app::engine::MeterFeed,
        sources: Option<query::Sources>,
    ) -> Self {
        tempo_app::engine::engine_lock(&engine)
            .configure_remote_settings_store(crate::settings_path());
        Self::start(
            REMOTE_ORIGIN.to_string(),
            Box::new(SystemVault),
            engine,
            publisher,
            Some(spectrum),
            meters,
            sources,
        )
    }
    #[cfg(test)]
    fn configured(
        origin: String,
        vault: Box<dyn Vault>,
        engine: crate::SharedEngine,
        publisher: crate::remote_monitor::Publisher,
    ) -> Self {
        Self::start(
            origin,
            vault,
            engine,
            publisher,
            None,
            Default::default(),
            None,
        )
    }
    fn start(
        origin: String,
        vault: Box<dyn Vault>,
        engine: crate::SharedEngine,
        publisher: crate::remote_monitor::Publisher,
        spectrum: Option<tempo_app::engine::SpectrumFeed>,
        meters: tempo_app::engine::MeterFeed,
        sources: Option<query::Sources>,
    ) -> Self {
        let status = Arc::new(Mutex::new(Status {
            phase: "unpaired".into(),
            origin: origin.clone(),
            ..Default::default()
        }));
        let control = Arc::new(Mutex::new(Control {
            memories: sources
                .as_ref()
                .map(|s| s.memories.clone())
                .unwrap_or_default(),
            ..Default::default()
        }));
        let (commands, receiver) = mpsc::channel(1);
        let task_status = status.clone();
        let task_control = control.clone();
        let thread = std::thread::Builder::new()
            .name("nexus-remote".into())
            .spawn(move || {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build();
                let client = Client::new(&origin);
                match (runtime, client) {
                    (Ok(runtime), Ok(client)) => runtime.block_on(
                        Controller {
                            client,
                            vault,
                            binding: None,
                            pending: None,
                            status: task_status,
                            control: task_control,
                            engine,
                            feeds: transport::Feeds {
                                monitor: publisher,
                                spectrum,
                                meters,
                                sources,
                            },
                        }
                        .run(receiver),
                    ),
                    _ => phase(&task_status, "unpaired", Some("serviceUnavailable")),
                }
            });
        if thread.is_err() {
            phase(&status, "unpaired", Some("serviceUnavailable"));
        }
        Self {
            commands,
            status,
            control,
        }
    }
    pub fn status(&self) -> Result<Status, &'static str> {
        let mut status = self
            .status
            .lock()
            .map_err(|_| "serviceUnavailable")?
            .clone();
        let control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        let enabled = control.enabled;
        let operations = control.operations.local_status();
        status.logging_permissions =
            serde_json::from_value(operations["devices"].clone()).unwrap_or_default();
        status.logging_controller = operations["controller"].as_str().map(str::to_string);
        status.station_permissions =
            serde_json::from_value(operations["controlDevices"].clone()).unwrap_or_default();
        status.observation_generation = enabled.then(|| control.generation.to_string());
        if !enabled && ["connected", "connecting", "reconnecting"].contains(&status.phase.as_str())
        {
            status.phase = "disabled".into();
        }
        Ok(status)
    }
    fn publish_memories(&self, generation: &str, bank: Option<&str>) -> bool {
        // Parse outside the control lock. Recheck the local enable generation at
        // admission so an old WebView reply cannot revive a disabled snapshot.
        let value = bank.and_then(query::memories::parse);
        let Ok(control) = self.control.try_lock() else {
            return false;
        };
        if !control.enabled || control.generation.to_string() != generation {
            return false;
        }
        let Ok(mut cache) = control.memories.try_lock() else {
            return false;
        };
        *cache = value.map(|v| (std::time::Instant::now(), Arc::new(v)));
        cache.is_some()
    }
    pub async fn action(&self, action: Action) -> Result<Status, &'static str> {
        match &action {
            Action::LoggingPermission { device_id, allow }
            | Action::StationPermission { device_id, allow } => {
                let status = self.status()?;
                if *allow
                    && (status.observation_generation.is_none()
                        || !status
                            .devices
                            .iter()
                            .any(|d| d.id == *device_id && d.approved == 1))
                {
                    return Err("accessDenied");
                }
                let operations = self
                    .control
                    .lock()
                    .map_err(|_| "serviceUnavailable")?
                    .operations
                    .clone();
                if matches!(&action, Action::StationPermission { .. }) {
                    operations.permit_station(device_id, *allow)?;
                } else {
                    operations.permit(device_id, *allow)?;
                }
                return self.status();
            }
            Action::TakeOverLogging {} => {
                self.control
                    .lock()
                    .map_err(|_| "serviceUnavailable")?
                    .operations
                    .invalidate();
                return self.status();
            }
            _ => {}
        }
        let (reply, response) = oneshot::channel();
        {
            let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
            if matches!(&action, Action::Device { approve: false, .. }) {
                control.operations.invalidate();
            }
            if matches!(
                action,
                Action::Disable {} | Action::Forget {} | Action::Cancel {}
            ) {
                control.stop();
            }
            // A refused Enable must not leave enabled authority behind. Stop
            // actions remain immediate even when the command queue is full.
            let permit = self.commands.try_reserve().map_err(|_| "remoteBusy")?;
            if matches!(action, Action::Enable {}) {
                control.stop();
                control.enabled = true;
            }
            permit.send((action, control.generation, reply));
        }
        response.await.map_err(|_| "serviceUnavailable")??;
        self.status()
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        if let Ok(mut control) = self.control.lock() {
            control.stop();
        }
    }
}

struct Pending {
    id: String,
    proof: String,
    code: String,
    expires_at: u64,
    account_id: Option<String>,
    station_credential: Option<String>,
}
struct Controller {
    client: Client,
    vault: Box<dyn Vault>,
    binding: Option<Binding>,
    pending: Option<Pending>,
    status: Arc<Mutex<Status>>,
    control: Arc<Mutex<Control>>,
    engine: crate::SharedEngine,
    feeds: transport::Feeds,
}
impl Controller {
    async fn run(mut self, mut receiver: mpsc::Receiver<(Action, u64, Reply)>) {
        match self.vault.binding() {
            Ok(Some(binding))
                if binding.origin == self.client.origin()
                    && identifier(&binding.station_id)
                    && identifier(&binding.account_id) =>
            {
                self.binding = Some(binding);
                // The observation pilot requires local enable after each launch.
                // Pairing persists; unattended reconnection across an app restart does not.
                self.reflect("disabled");
            }
            Ok(None) => {}
            _ => phase(&self.status, "unpaired", Some("credentialStoreUnavailable")),
        }
        while let Some((action, generation, reply)) = receiver.recv().await {
            let result = self.handle(action, generation).await;
            if let Err(error) = result {
                if let Ok(mut status) = self.status.lock() {
                    status.error = Some(error);
                }
            }
            let _ = reply.send(result);
        }
        if let Ok(mut control) = self.control.lock() {
            control.stop();
        }
    }
    fn reflect(&self, value: &str) {
        if let Ok(mut status) = self.status.lock() {
            status.phase = value.into();
            status.error = None;
            status.station_id = self.binding.as_ref().map(|b| b.station_id.clone());
            status.account_id = self
                .binding
                .as_ref()
                .map(|b| b.account_id.clone())
                .or_else(|| self.pending.as_ref().and_then(|p| p.account_id.clone()));
            status.pairing_id = self.pending.as_ref().map(|p| p.id.clone());
            status.pairing_code = self.pending.as_ref().map(|p| p.code.clone());
            status.expires_at = self.pending.as_ref().map(|p| p.expires_at);
            if self.binding.is_none() {
                status.devices.clear();
            }
        }
    }
    fn bound(&self) -> Result<(Binding, String), &'static str> {
        let binding = self.binding.as_ref().ok_or("stationNotPaired")?.clone();
        let token = self.vault.credential(&binding)?;
        if !credential(&token) {
            return Err("credentialStoreUnavailable");
        }
        Ok((binding, token))
    }
    async fn handle(&mut self, action: Action, generation: u64) -> Result<(), &'static str> {
        match action {
            Action::LoggingPermission { .. }
            | Action::StationPermission { .. }
            | Action::TakeOverLogging {} => return Err("invalidRequest"),
            Action::Begin { name } => {
                if self.binding.is_some() || self.pending.is_some() || !valid_name(&name) {
                    return Err("invalidRequest");
                }
                let value = self
                    .client
                    .post("enroll", None, json!({ "name": name.trim() }))
                    .await?;
                #[derive(Deserialize)]
                #[serde(rename_all = "camelCase", deny_unknown_fields)]
                struct Enrollment {
                    id: String,
                    proof: String,
                    code: String,
                    expires_at: u64,
                }
                let e: Enrollment = serde_json::from_value(value).map_err(|_| "invalidResponse")?;
                if !identifier(&e.id)
                    || !credential(&e.proof)
                    || e.code.len() != 16
                    || !e.code.bytes().all(|b| b.is_ascii_hexdigit())
                    || e.expires_at <= now_ms()
                    || e.expires_at > now_ms() + 660000
                {
                    return Err("invalidResponse");
                }
                self.pending = Some(Pending {
                    id: e.id,
                    proof: e.proof,
                    code: e.code,
                    expires_at: e.expires_at,
                    account_id: None,
                    station_credential: None,
                });
                self.reflect("pairing");
            }
            Action::Cancel {} => {
                self.pending = None;
                self.reflect(if self.binding.is_some() {
                    "disabled"
                } else {
                    "unpaired"
                });
            }
            Action::Refresh {} => {
                if let Some(pending) = &mut self.pending {
                    if pending.expires_at <= now_ms() {
                        self.pending = None;
                        self.reflect("unpaired");
                        return Err("pairingExpired");
                    }
                    let value = self
                        .client
                        .post(
                            "enroll/check",
                            None,
                            json!({ "id": pending.id, "proof": pending.proof }),
                        )
                        .await?;
                    #[derive(Deserialize)]
                    #[serde(rename_all = "camelCase", deny_unknown_fields)]
                    struct Check {
                        account_id: Option<String>,
                        approved: bool,
                    }
                    let check: Check =
                        serde_json::from_value(value).map_err(|_| "invalidResponse")?;
                    if check
                        .account_id
                        .as_deref()
                        .is_some_and(|value| !identifier(value))
                    {
                        return Err("invalidResponse");
                    }
                    // A successful approve whose response was lost may be retried
                    // with the same in-memory credential; never issue a new one.
                    if check.approved && pending.station_credential.is_none() {
                        return Err("invalidResponse");
                    }
                    pending.account_id = check.account_id;
                    self.reflect(
                        if self
                            .pending
                            .as_ref()
                            .is_some_and(|p| p.account_id.is_some())
                        {
                            "approval"
                        } else {
                            "pairing"
                        },
                    );
                } else if self.binding.is_some() {
                    let (binding, token) = self.bound()?;
                    let value = self
                        .client
                        .post(&station_path(&binding, "devices"), Some(&token), empty())
                        .await?;
                    #[derive(Deserialize)]
                    #[serde(deny_unknown_fields)]
                    struct Devices {
                        devices: Vec<Device>,
                    }
                    let value: Devices =
                        serde_json::from_value(value).map_err(|_| "invalidResponse")?;
                    if value.devices.len() > 8
                        || value
                            .devices
                            .iter()
                            .any(|d| !identifier(&d.id) || !valid_name(&d.name) || d.approved > 1)
                    {
                        return Err("invalidResponse");
                    }
                    if let Ok(mut status) = self.status.lock() {
                        status.devices = value.devices;
                        status.error = None;
                    }
                }
            }
            Action::Approve {
                enrollment_id,
                account_id,
            } => {
                let pending = self.pending.as_mut().ok_or("pairingExpired")?;
                if pending.id != enrollment_id
                    || pending.account_id.as_deref() != Some(&account_id)
                    || pending.expires_at <= now_ms()
                {
                    return Err("pairingExpired");
                }
                let token = pending
                    .station_credential
                    .get_or_insert(transport::random_secret()?)
                    .clone();
                let binding = Binding {
                    origin: self.client.origin().into(),
                    station_id: pending.id.clone(),
                    account_id: account_id.clone(),
                };
                // Confirm the OS vault works before creating cloud station authority.
                self.vault.stage(&binding, &token)?;
                let result = self
                    .client
                    .post(
                        "enroll/approve",
                        None,
                        json!({ "id": pending.id, "proof": pending.proof, "credential": token }),
                    )
                    .await;
                let value = match result {
                    Ok(value) => value,
                    Err(error) => {
                        let _ = self.vault.remove(&binding);
                        return Err(error);
                    }
                };
                if value.get("stationId").and_then(|v| v.as_str()) != Some(&binding.station_id)
                    || value.get("accountId").and_then(|v| v.as_str()) != Some(&binding.account_id)
                {
                    let _ = self.vault.remove(&binding);
                    return Err("invalidResponse");
                }
                self.vault.save(&binding, &token)?;
                self.binding = Some(binding);
                self.pending = None;
                self.reflect("disabled");
            }
            Action::Enable {} => {
                let bound = self.bound();
                let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
                if !control.enabled || control.generation != generation {
                    return Ok(());
                }
                let (binding, token) = match bound {
                    Ok(value) => value,
                    Err(error) => {
                        control.stop();
                        return Err(error);
                    }
                };
                if let Some(cancel) = control.cancel.take() {
                    let _ = cancel.send(true);
                }
                let (cancel, receiver) = watch::channel(false);
                control.cancel = Some(cancel);
                phase(&self.status, "connecting", None);
                tokio::spawn(transport::supervise(
                    self.client.clone(),
                    binding,
                    token,
                    receiver,
                    self.engine.clone(),
                    self.feeds.clone(),
                    SessionStatus {
                        status: self.status.clone(),
                        control: self.control.clone(),
                        generation,
                    },
                ));
            }
            Action::Disable {} => {
                self.reflect(if self.binding.is_some() {
                    "disabled"
                } else {
                    "unpaired"
                });
            }
            Action::Forget {} => {
                let (binding, token) = self.bound()?;
                self.client
                    .post(&station_path(&binding, "revoke"), Some(&token), empty())
                    .await?;
                self.vault.remove(&binding)?;
                self.binding = None;
                self.pending = None;
                self.reflect("unpaired");
            }
            Action::Device { device_id, approve } => {
                if !identifier(&device_id) {
                    return Err("invalidRequest");
                }
                let (binding, token) = self.bound()?;
                self.client
                    .post(
                        &station_path(
                            &binding,
                            if approve {
                                "approve-device"
                            } else {
                                "revoke-device"
                            },
                        ),
                        Some(&token),
                        json!({ "deviceId": device_id }),
                    )
                    .await?;
                if let Ok(mut status) = self.status.lock() {
                    status.devices.clear();
                }
            }
        }
        Ok(())
    }
}

#[tauri::command]
pub fn get_remote_station_status(
    service: tauri::State<'_, Service>,
) -> Result<Status, &'static str> {
    service.status()
}
#[tauri::command]
pub async fn remote_station_action(
    service: tauri::State<'_, Service>,
    action: Action,
) -> Result<Status, &'static str> {
    service.action(action).await
}

// The canonical desktop WebView owns the in-memory bank. Detached windows and
// the hosted browser cannot publish it or send a generic durable-state map.
#[tauri::command]
pub fn publish_remote_memory_bank(
    service: tauri::State<'_, Service>,
    window: tauri::WebviewWindow,
    generation: String,
    bank: Option<String>,
) -> bool {
    window.label() == "main" && service.publish_memories(&generation, bank.as_deref())
}
