//! Local approval and the Remote background task. Engine access is restricted to
//! bounded observation, reviewed application reads and separately approved manual logging.
mod application;
mod aprs;
/// Receive audio for a listening browser. Gated with the radio feature because the
/// encoder lives in tempo-audio, which a build without it does not have at all.
#[cfg(feature = "radio")]
mod audio;
pub(crate) mod operations;
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
    transmit_permissions: Vec<String>,
    logging_controller: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Device {
    id: String,
    name: String,
    approved: u8,
    expires_at: u64,
    /// The approval generation. Listed only by a service with the browser approval lifetime, and only
    /// to a Nexus that asks (`transport::Client::post` does).
    #[serde(default)]
    generation: Option<u64>,
    /// The end that use cannot move, ninety days after the shack approved it. None for an approval that
    /// never renews.
    #[serde(default)]
    renews_until: Option<u64>,
}
impl Device {
    fn approval(&self) -> Approval {
        Approval {
            expires_at: self.expires_at,
            generation: self.generation,
        }
    }
}
/// One browser approval as the service lists it.
#[derive(Clone, Copy, PartialEq)]
struct Approval {
    expires_at: u64,
    generation: Option<u64>,
}
impl Approval {
    /// Was `grant` given against this approval? By generation when both sides know it: a renewal on
    /// use moves the expiry and keeps the generation, while approving again or revoking moves it.
    /// Otherwise by expiry, which every approval and revocation rewrites: a record 1.12.0 wrote, or a
    /// service without the approval lifetime, which never renews.
    fn holds(&self, grant: &vault::Grant) -> bool {
        match (grant.generation, self.generation) {
            (Some(held), Some(listed)) => held == listed,
            _ => grant.expires_at == self.expires_at,
        }
    }
    /// Rebind a grant this approval holds to the approval as listed now, so a record written before
    /// the service reported a generation carries one from here on. Grants nothing.
    fn bind(&self, grant: &mut vault::Grant) {
        if self.generation.is_some() {
            grant.generation = self.generation;
            grant.expires_at = self.expires_at;
        }
    }
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
    TransmitPermission {
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
        /// "Also allow FT8/FT4 transmit" for the browser that did the pairing.
        #[serde(default)]
        transmit: bool,
    },
    Enable {},
    Disable {},
    Forget {},
    Device {
        #[serde(rename = "deviceId")]
        device_id: String,
        approve: bool,
        /// "Also allow FT8/FT4 transmit" on this approval. Ignored when revoking.
        #[serde(default)]
        transmit: bool,
    },
}
type Reply = oneshot::Sender<Result<(), &'static str>>;
/// The most browsers a restart remembers grants for. The service lists at most eight per station,
/// and the record has to fit the smallest OS credential blob (Windows).
const MAX_REMEMBERED: usize = 8;
/// A decision a restart must remember. Operator decisions 2026-09-13: Remote remembers being on;
/// approving a browser grants it station control and logging, plus FT8/FT4 transmit when the
/// approval ticks it, and each is kept for as long as that approval stands, until revoked.
///
/// Every decision is applied to `Control::remembered` under the `Control` lock at the moment it is
/// made, so the record always holds the operator's decisions in the order they were made, and Turn
/// on reads exactly what has been decided. Writing it happens on the vault writer's own thread,
/// never under a lock and never on the path that revokes transmission, because an OS credential
/// store can block (a locked keychain can prompt).
enum Persist {
    /// The controller loaded or created this pairing; later decisions apply to it.
    Bound(Binding, Option<vault::State>),
    /// Remote was turned on and is connecting.
    Enabled,
    /// Remote went off for a reason that ends access: Revoke station access, a cancelled pairing,
    /// or the connection refusing itself. The permissions it cleared are remembered as cleared.
    Off,
    /// Turn off Remote. It only pauses (operator decision 2026-09-14): every browser is
    /// disconnected and every live permission cleared, but the remembered grants stay, so Turn on
    /// or a restart restores them for browsers still approved at the same approval.
    Paused,
    /// A local decision cleared every browser permission (take over, revoke a browser).
    ClearGrants,
    /// One browser's grants changed. `approval` is the approval the change was made against: a grant
    /// is only ever created against a known approval, and a grant against a different approval (not
    /// merely a renewed one) starts that browser afresh. A `None` field is left as it was.
    Browser {
        device_id: String,
        approval: Option<Approval>,
        logging: Option<bool>,
        control: Option<bool>,
        transmit: Option<bool>,
    },
    /// The approvals the service lists right now. A grant remembered against any other approval
    /// (a browser revoked, re-approved, expired or gone) is forgotten; the rest are rebound to the
    /// approval as listed.
    Approvals(Vec<(String, Approval)>),
    /// The pairing was removed.
    Removed,
}
/// What the vault must hold next. Each write is the whole record, so only the latest matters.
enum Write {
    Save(vault::State),
    Remove,
}
/// The record for the current pairing, kept in step with every decision.
#[derive(Default)]
struct Remembered {
    binding: Option<Binding>,
    state: Option<vault::State>,
}
impl Remembered {
    /// Apply one decision. Returns the write it calls for, or `None` when nothing changed.
    fn apply(&mut self, decision: Persist) -> Option<Write> {
        match decision {
            Persist::Bound(bound, state) => {
                self.state = state.filter(|s| s.binding == bound);
                self.binding = Some(bound);
                return None;
            }
            Persist::Removed => {
                self.binding = None;
                self.state = None;
                return Some(Write::Remove);
            }
            _ => {}
        }
        let bound = self.binding.clone()?;
        let mut state = self.state.clone().unwrap_or(vault::State {
            binding: bound,
            enabled: false,
            grants: Vec::new(),
        });
        match decision {
            Persist::Enabled => state.enabled = true,
            Persist::Off => {
                state.enabled = false;
                state.grants.clear();
            }
            Persist::Paused => state.enabled = false,
            Persist::ClearGrants => state.grants.clear(),
            Persist::Browser {
                device_id,
                approval,
                logging,
                control,
                transmit,
            } => {
                let at = state.grants.iter().position(|g| g.device_id == device_id);
                let mut grant = match (at.map(|i| state.grants[i].clone()), approval) {
                    (Some(mut grant), Some(approval)) if approval.holds(&grant) => {
                        approval.bind(&mut grant);
                        grant
                    }
                    (_, Some(approval)) => vault::Grant {
                        device_id,
                        expires_at: approval.expires_at,
                        generation: approval.generation,
                        logging: false,
                        control: false,
                        transmit: false,
                    },
                    (Some(grant), None) => grant,
                    // Nothing remembered for this browser, and no approval to bind a grant to.
                    (None, None) => return None,
                };
                grant.logging = logging.unwrap_or(grant.logging);
                grant.control = control.unwrap_or(grant.control);
                // FT8/FT4 transmit is only ever held alongside station control.
                grant.transmit = transmit.unwrap_or(grant.transmit) && grant.control;
                let keep = grant.logging || grant.control;
                match at {
                    Some(i) if keep => state.grants[i] = grant,
                    Some(i) => {
                        state.grants.remove(i);
                    }
                    None if keep && state.grants.len() < MAX_REMEMBERED => state.grants.push(grant),
                    None => {}
                }
            }
            Persist::Approvals(approvals) => state.grants.retain_mut(|g| {
                let listed = approvals
                    .iter()
                    .find(|(id, approval)| *id == g.device_id && approval.holds(g));
                if let Some((_, approval)) = listed {
                    approval.bind(g);
                }
                listed.is_some()
            }),
            Persist::Bound(..) | Persist::Removed => return None,
        }
        if self.state.as_ref() == Some(&state) {
            return None;
        }
        self.state = Some(state.clone());
        Some(Write::Save(state))
    }
    fn grants(&self) -> &[vault::Grant] {
        self.state.as_ref().map_or(&[], |s| s.grants.as_slice())
    }
}
#[derive(Default)]
struct Control {
    operations: Arc<operations::Authority>,
    memories: query::memories::Bank,
    enabled: bool,
    cancel: Option<watch::Sender<bool>>,
    generation: u64,
    vault_writer: Option<mpsc::UnboundedSender<Write>>,
    remembered: Remembered,
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
    /// Hand a decision to the vault writer. Never called from `stop`: `stop` also runs when Nexus
    /// exits, and an exit must not be remembered as the operator turning Remote off.
    fn remember(&mut self, decision: Persist) {
        if let Some(write) = self.remembered.apply(decision) {
            if let Some(writer) = &self.vault_writer {
                let _ = writer.send(write);
            }
        }
    }
}
/// A command, the Remote generation it was issued under, the authority epoch at that moment (so a
/// local decision made after it wins), and where to reply.
type Command = (Action, u64, u64, Reply);
pub struct Service {
    commands: mpsc::Sender<Command>,
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
                // The service refused this station (access denied) or its credential is unusable.
                // Remote is off now, so the next launch must not turn it back on. A malformed
                // service reply is deliberately NOT this: it reconnects (see `supervise`), because
                // an off that is remembered can only be undone at the shack.
                control.remember(Persist::Off);
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
        // `audio`: the station's bounded receive-audio copy, for a listening browser.
        // `None` leaves the audio lane unadvertised, so nothing is ever offered it.
        #[cfg(feature = "radio")] audio: Option<Arc<tempo_audio::receive_audio::ReceiveAudioFeed>>,
    ) -> Self {
        tempo_app::engine::engine_lock(&engine)
            .configure_remote_settings_store(crate::settings_path());
        Self::start(
            REMOTE_ORIGIN.to_string(),
            Box::new(SystemVault),
            engine,
            transport::Feeds {
                monitor: publisher,
                spectrum: Some(spectrum),
                meters,
                sources,
                #[cfg(feature = "radio")]
                audio,
            },
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
            transport::Feeds {
                monitor: publisher,
                spectrum: None,
                meters: Default::default(),
                sources: None,
                #[cfg(feature = "radio")]
                audio: None,
            },
        )
    }
    /// `feeds` arrives whole rather than as four parameters. They were four until the audio
    /// lane made it five, and a five-feed argument list is the shape that gets called with two
    /// of them transposed — they already travel together into the controller.
    fn start(
        origin: String,
        vault: Box<dyn Vault>,
        engine: crate::SharedEngine,
        feeds: transport::Feeds,
    ) -> Self {
        let vault: Arc<dyn Vault> = Arc::from(vault);
        let status = Arc::new(Mutex::new(Status {
            phase: "unpaired".into(),
            origin: origin.clone(),
            ..Default::default()
        }));
        // The vault writer runs on a thread of its own, so remembering a decision never waits on
        // the OS credential store while a lock is held or while the controller is mid-request. If
        // it cannot start, nothing is remembered, and the next launch leaves Remote off.
        let (writer, decisions) = mpsc::unbounded_channel();
        let writer_vault = vault.clone();
        let vault_writer = std::thread::Builder::new()
            .name("nexus-remote-vault".into())
            .spawn(move || write_remembered(writer_vault, decisions))
            .ok()
            .map(|_| writer);
        let control = Arc::new(Mutex::new(Control {
            operations: Arc::new(operations::Authority::with_spots(
                feeds.sources.as_ref().map(|s| s.spots.clone()),
            )),
            memories: feeds
                .sources
                .as_ref()
                .map(|s| s.memories.clone())
                .unwrap_or_default(),
            vault_writer,
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
                            feeds,
                            restore: None,
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
        status.transmit_permissions =
            serde_json::from_value(operations["transmitDevices"].clone()).unwrap_or_default();
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
            | Action::StationPermission { device_id, allow }
            | Action::TransmitPermission { device_id, allow } => {
                // Keep admission and grant under the same local control lock
                // as Disable/Take over. A prior status snapshot must not install
                // a new grant after that local authority has been stopped.
                let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
                // The approval being granted against. A restart checks it (by generation where the
                // service reports one) before giving the grant back, so a revoked or re-approved
                // browser gets nothing, and a renewed one keeps what it had.
                let mut approval = None;
                if *allow {
                    let status = self.status.lock().map_err(|_| "serviceUnavailable")?;
                    let Some(device) = status
                        .devices
                        .iter()
                        .find(|d| d.id == *device_id && d.approved == 1)
                        .filter(|_| control.enabled)
                    else {
                        return Err("accessDenied");
                    };
                    approval = Some(device.approval());
                }
                // These are the restrict switches: each allows or revokes one permission for one
                // browser, live first, and the record follows only what took effect. Revoking
                // station control revokes transmit with it.
                let operations = control.operations.clone();
                let (logging, station, transmit) = match &action {
                    Action::StationPermission { .. } => {
                        operations.permit_station(device_id, *allow)?;
                        (None, Some(*allow), (!*allow).then_some(false))
                    }
                    Action::TransmitPermission { .. } => {
                        operations.permit_transmit(device_id, *allow)?;
                        (None, None, Some(*allow))
                    }
                    _ => {
                        operations.permit(device_id, *allow)?;
                        (Some(*allow), None, None)
                    }
                };
                control.remember(Persist::Browser {
                    device_id: device_id.clone(),
                    approval,
                    logging,
                    control: station,
                    transmit,
                });
                drop(control);
                return self.status();
            }
            Action::TakeOverLogging {} => {
                let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
                control.operations.invalidate();
                control.remember(Persist::ClearGrants);
                drop(control);
                return self.status();
            }
            _ => {}
        }
        let (reply, response) = oneshot::channel();
        {
            let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
            if matches!(&action, Action::Device { approve: false, .. }) {
                control.operations.invalidate();
                control.remember(Persist::ClearGrants);
            }
            if matches!(
                action,
                Action::Disable {} | Action::Forget {} | Action::Cancel {}
            ) {
                control.stop();
                // Remembered here rather than in the controller: this stop is immediate even when
                // the command queue is full, and what a restart remembers must match it. The stop
                // clears every live permission either way; only Turn off keeps the remembered ones.
                control.remember(if matches!(action, Action::Disable {}) {
                    Persist::Paused
                } else {
                    Persist::Off
                });
            }
            // A refused Enable must not leave enabled authority behind. Stop
            // actions remain immediate even when the command queue is full.
            let permit = self.commands.try_reserve().map_err(|_| "remoteBusy")?;
            if matches!(action, Action::Enable {}) {
                control.stop();
                control.enabled = true;
                // Turning on starts a fresh authority: every permission in memory is cleared. The
                // grants remembered for still-approved browsers come back once the service lists
                // them (see `Controller::apply_restore`). "On" itself is remembered by the
                // controller, once the connection has actually started.
            }
            permit.send((
                action,
                control.generation,
                control.operations.epoch(),
                reply,
            ));
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
    vault: Arc<dyn Vault>,
    binding: Option<Binding>,
    pending: Option<Pending>,
    status: Arc<Mutex<Status>>,
    control: Arc<Mutex<Control>>,
    engine: crate::SharedEngine,
    feeds: transport::Feeds,
    restore: Option<Restore>,
}
/// Remote came on with grants remembered; they wait for the service's browser list.
struct Restore {
    /// The authority epoch when Remote came on. Any local decision since then (Turn off, take over,
    /// revoking a browser) moves it, and that decision wins.
    epoch: u64,
    at: tokio::time::Instant,
    attempts: u32,
}
impl Controller {
    async fn run(mut self, mut receiver: mpsc::Receiver<Command>) {
        match self.vault.binding() {
            Ok(Some(binding))
                if binding.origin == self.client.origin()
                    && identifier(&binding.station_id)
                    && identifier(&binding.account_id) =>
            {
                self.binding = Some(binding.clone());
                self.resume(binding).await;
            }
            Ok(None) => {}
            _ => phase(&self.status, "unpaired", Some("credentialStoreUnavailable")),
        }
        loop {
            let retry = self.restore.as_ref().map(|r| r.at);
            tokio::select! {
                next = receiver.recv() => {
                    let Some((action, generation, epoch, reply)) = next else { break };
                    let result = self.handle(action, generation, epoch).await;
                    if let Err(error) = result {
                        if let Ok(mut status) = self.status.lock() {
                            status.error = Some(error);
                        }
                    }
                    let _ = reply.send(result);
                }
                _ = async {
                    match retry {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => self.retry_restore().await,
            }
        }
        if let Ok(mut control) = self.control.lock() {
            control.stop();
        }
    }
    /// Remote remembers being on. If it was on when Nexus last exited, turn it on again exactly as
    /// Turn on Remote does: a fresh local authority (every in-memory permission cleared, transmit
    /// included), a fresh generation, and the remembered grants restored for still-approved
    /// browsers. A locked or unreadable store, a record written for another pairing, or a record
    /// that says off all leave it off.
    async fn resume(&mut self, binding: Binding) {
        let remembered = match self.vault.state() {
            Ok(state) => state.filter(|s| {
                s.binding == binding
                    && s.grants.len() <= MAX_REMEMBERED
                    && s.grants.iter().all(|g| identifier(&g.device_id))
            }),
            Err(error) => {
                if let Ok(mut control) = self.control.lock() {
                    control.remember(Persist::Bound(binding, None));
                }
                self.reflect("disabled");
                phase(&self.status, "disabled", Some(error));
                return;
            }
        };
        // Decide under the lock BEFORE the pairing becomes visible, so a Turn off that arrives
        // afterwards is ordered after this decision and wins.
        let resumed = {
            let Ok(mut control) = self.control.lock() else {
                return;
            };
            let enabled = remembered.as_ref().is_some_and(|s| s.enabled);
            control.remember(Persist::Bound(binding, remembered));
            if enabled {
                control.stop();
                control.enabled = true;
                Some(control.generation)
            } else {
                None
            }
        };
        self.reflect("disabled");
        let Some(generation) = resumed else {
            return;
        };
        if let Err(error) = self.enable(generation) {
            if let Ok(mut status) = self.status.lock() {
                status.error = Some(error);
            }
        }
    }
    /// Nobody opens Settings after an unattended restart, so the station fetches its own browser
    /// list to restore grants, backing off while the service is unreachable.
    async fn retry_restore(&mut self) {
        let current = self.restore.as_ref().is_some_and(|restore| {
            self.control
                .lock()
                .is_ok_and(|c| c.enabled && c.operations.epoch() == restore.epoch)
        });
        if !current {
            self.restore = None;
            return;
        }
        match self.devices().await {
            Ok(devices) => {
                self.apply_restore(&devices);
                if let Ok(mut status) = self.status.lock() {
                    status.devices = devices;
                }
            }
            Err(_) => {
                if let Some(restore) = &mut self.restore {
                    restore.attempts = restore.attempts.saturating_add(1).min(5);
                    restore.at = tokio::time::Instant::now()
                        + std::time::Duration::from_secs(1 << restore.attempts);
                }
            }
        }
    }
    /// Put back remembered grants for each browser the service still lists as approved at the SAME
    /// approval (`Approval::holds`: the same generation, so a renewal on use still matches), under the
    /// epoch captured when Remote came on. The grants are read from the record now, under the lock, so
    /// a revocation made while the list was on its way is honoured. FT8/FT4 transmit comes back only
    /// where it was granted, only with station control, and arms nothing.
    fn apply_restore(&mut self, devices: &[Device]) {
        let Some(restore) = self.restore.take() else {
            return;
        };
        let now = now_ms();
        let approvals: Vec<(String, Approval)> = devices
            .iter()
            .filter(|d| d.approved == 1 && d.expires_at > now)
            .map(|d| (d.id.clone(), d.approval()))
            .collect();
        let Ok(mut control) = self.control.lock() else {
            return;
        };
        if !control.enabled || control.operations.epoch() != restore.epoch {
            return;
        }
        let kept: Vec<vault::Grant> = control
            .remembered
            .grants()
            .iter()
            .filter(|g| {
                approvals
                    .iter()
                    .any(|(id, approval)| *id == g.device_id && approval.holds(g))
            })
            .cloned()
            .collect();
        let ids = |held: fn(&vault::Grant) -> bool| {
            kept.iter()
                .filter(|g| held(g))
                .map(|g| g.device_id.clone())
                .collect()
        };
        let grants = operations::DurableGrants {
            logging: ids(|g| g.logging),
            control: ids(|g| g.control),
            transmit: ids(|g| g.control && g.transmit),
        };
        let operations = control.operations.clone();
        match operations.restore(restore.epoch, &grants) {
            // Browsers no longer approved at the same approval are forgotten.
            Ok(true) => control.remember(Persist::Approvals(approvals)),
            Ok(false) => {}
            Err(_) => {
                drop(control);
                self.restore = Some(Restore {
                    at: tokio::time::Instant::now() + std::time::Duration::from_secs(1),
                    ..restore
                });
            }
        }
    }
    async fn devices(&self) -> Result<Vec<Device>, &'static str> {
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
        let value: Devices = serde_json::from_value(value).map_err(|_| "invalidResponse")?;
        if value.devices.len() > 8
            || value
                .devices
                .iter()
                .any(|d| !identifier(&d.id) || !valid_name(&d.name) || d.approved > 1)
        {
            return Err("invalidResponse");
        }
        Ok(value.devices)
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
    /// ONE APPROVAL (operator decision 2026-09-13): approving a browser grants it station control
    /// and logging, and FT8/FT4 transmit when the approval ticked it. With Remote on they take
    /// effect at once; with Remote off they wait in the record for Turn on. A local decision made
    /// after the approval was asked for (Turn off or on, take over, revoking a browser) wins, and
    /// nothing is granted. ⛔ Granting transmit arms nothing: the browser still has to press TX On.
    fn grant_approved(
        &self,
        devices: &[Device],
        device_id: String,
        transmit: bool,
        (generation, epoch): (u64, u64),
    ) -> Result<(), &'static str> {
        let approval = devices
            .iter()
            .find(|d| d.id == device_id && d.approved == 1 && d.expires_at > now_ms())
            .map(Device::approval)
            .ok_or("invalidResponse")?;
        let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        if control.generation != generation || control.operations.epoch() != epoch {
            return Ok(());
        }
        if control.enabled {
            let operations = control.operations.clone();
            operations.permit(&device_id, true)?;
            operations.permit_station(&device_id, true)?;
            // A fresh approval without the tick holds no transmit, even if an earlier one did.
            operations.permit_transmit(&device_id, transmit)?;
        }
        control.remember(Persist::Browser {
            device_id,
            approval: Some(approval),
            logging: Some(true),
            control: Some(true),
            transmit: Some(transmit),
        });
        Ok(())
    }
    /// Connect under `generation`, which the caller has already made enabled with a fresh
    /// authority (Turn on Remote, a launch that remembers being on, or approving the pairing).
    fn enable(&mut self, generation: u64) -> Result<(), &'static str> {
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
        // Remote is on and connecting: the next launch turns it back on.
        control.remember(Persist::Enabled);
        // Remembered grants come back once the service confirms each browser is still approved at
        // the same approval. The epoch is read here, under the lock: a later take over, revocation
        // or Turn off moves it, and that decision wins.
        if !control.remembered.grants().is_empty() {
            self.restore = Some(Restore {
                epoch: control.operations.epoch(),
                at: tokio::time::Instant::now(),
                attempts: 0,
            });
        }
        Ok(())
    }
    async fn handle(
        &mut self,
        action: Action,
        generation: u64,
        epoch: u64,
    ) -> Result<(), &'static str> {
        match action {
            Action::LoggingPermission { .. }
            | Action::StationPermission { .. }
            | Action::TransmitPermission { .. }
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
                    let devices = self.devices().await?;
                    // A fresh list is also what a restore after a restart is waiting for.
                    self.apply_restore(&devices);
                    if let Ok(mut status) = self.status.lock() {
                        status.devices = devices;
                        status.error = None;
                    }
                }
            }
            Action::Approve {
                enrollment_id,
                account_id,
                transmit,
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
                // One approval: the service approved the browser that confirmed this pairing along
                // with the station. It gets what any approved browser gets, waiting in the record
                // for Turn on. A service that reports no browser pairs exactly as it used to.
                let paired = value
                    .get("device")
                    .and_then(|d| {
                        Some((
                            d.get("id")?.as_str()?.to_string(),
                            Approval {
                                expires_at: d.get("expiresAt")?.as_u64()?,
                                generation: d.get("generation").and_then(|g| g.as_u64()),
                            },
                        ))
                    })
                    .filter(|(id, approval)| identifier(id) && approval.expires_at > now_ms());
                if let Ok(mut control) = self.control.lock() {
                    // A new pairing starts with nothing remembered: off, and no grants.
                    control.remember(Persist::Bound(binding.clone(), None));
                    if let Some((device_id, approval)) = paired {
                        control.remember(Persist::Browser {
                            device_id,
                            approval: Some(approval),
                            logging: Some(true),
                            control: Some(true),
                            transmit: Some(transmit),
                        });
                    }
                }
                self.binding = Some(binding);
                self.pending = None;
                self.reflect("disabled");
                // Approving the pairing also turns Remote on (operator decision 2026-09-14), exactly
                // as Turn on Remote does: a fresh authority, and the TX-enable latch untouched. A
                // Turn off or Cancel made while the approval was on its way moved the generation,
                // and wins.
                let turned_on = self.control.lock().ok().and_then(|mut control| {
                    (control.generation == generation).then(|| {
                        control.stop();
                        control.enabled = true;
                        control.generation
                    })
                });
                if let Some(generation) = turned_on {
                    self.enable(generation)?;
                }
            }
            Action::Enable {} => self.enable(generation)?,
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
                if let Ok(mut control) = self.control.lock() {
                    control.remember(Persist::Removed);
                }
                self.binding = None;
                self.pending = None;
                self.reflect("unpaired");
            }
            Action::Device {
                device_id,
                approve,
                transmit,
            } => {
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
                if approve {
                    // The list names the approval the service just wrote; the grant binds to it.
                    let devices = self.devices().await?;
                    let granted =
                        self.grant_approved(&devices, device_id, transmit, (generation, epoch));
                    if let Ok(mut status) = self.status.lock() {
                        status.devices = devices;
                    }
                    granted?;
                }
            }
        }
        Ok(())
    }
}

/// The vault writer. Each write is the whole record for the current pairing, produced in the order
/// the decisions were made, so a backlog collapses to its latest. A failed save is followed by
/// deleting the record: a record that could not be updated must not survive to be restored, and no
/// record at all means Remote stays off at the next launch.
fn write_remembered(vault: Arc<dyn Vault>, mut writes: mpsc::UnboundedReceiver<Write>) {
    while let Some(mut write) = writes.blocking_recv() {
        while let Ok(next) = writes.try_recv() {
            write = next;
        }
        match write {
            Write::Save(state) => {
                if vault.save_state(&state).is_err() {
                    let _ = vault.remove_state();
                }
            }
            Write::Remove => {
                let _ = vault.remove_state();
            }
        }
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
