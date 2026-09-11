//! Station-owned authority for manual logging and typed station controls.
//! Cloud admission routes an approved browser; only a local, boot-scoped grant
//! permits a lease. FT8/FT4 CQ and TX On/Off require a separate local transmit
//! grant; Stop has independent admission. Deferred hardware
//! writes carry a revocable permit and a separate completion receipt.
//! A log append already begun cannot be rolled back on disconnect. Its bounded
//! receipt remains queryable by the same device while locally permitted.
use super::transport::identifier;
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeSet, VecDeque};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};
use tempo_app::remote_control::{transmit::TransmitAuthority, Completion, Outcome, Revocation};

mod station;
mod transmit_stop;

const LEASE: Duration = Duration::from_secs(5);
const WINDOW: Duration = Duration::from_secs(2);
const RESULT_AGE: Duration = Duration::from_secs(600);
const MAX_COUNTER: u64 = 9_007_199_254_740_991;

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum Request {
    State {
        #[serde(rename = "requestId")]
        request_id: String,
    },
    Acquire {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "stationBootId")]
        station_boot_id: String,
    },
    Heartbeat {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
    },
    Release {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
    },
    StopTransmit {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "stationBootId")]
        station_boot_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
        #[serde(rename = "transmitEpoch")]
        transmit_epoch: String,
    },
    Result {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "operationId")]
        operation_id: String,
    },
    LogManual {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "stationBootId")]
        station_boot_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: u64,
        #[serde(rename = "commandWindowId")]
        command_window_id: String,
        #[serde(rename = "clientSequence")]
        client_sequence: u64,
        record: Box<ManualRecord>,
    },
    StationControl {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "stationBootId")]
        station_boot_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
        #[serde(rename = "expectedRevision")]
        expected_revision: u64,
        #[serde(rename = "commandWindowId")]
        command_window_id: String,
        #[serde(rename = "clientSequence")]
        client_sequence: u64,
        context: station::Context,
        action: station::Action,
    },
}
impl Request {
    pub fn id(&self) -> &str {
        match self {
            Self::State { request_id }
            | Self::Acquire { request_id, .. }
            | Self::Heartbeat { request_id, .. }
            | Self::Release { request_id, .. }
            | Self::StopTransmit { request_id, .. }
            | Self::Result { request_id, .. }
            | Self::LogManual { request_id, .. }
            | Self::StationControl { request_id, .. } => request_id,
        }
    }
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManualRecord {
    call: String,
    grid: Option<String>,
    country: Option<String>,
    state: Option<String>,
    band: String,
    freq_mhz: f64,
    mode: String,
    rst_sent: Option<String>,
    rst_rcvd: Option<String>,
    name: Option<String>,
    qth: Option<String>,
    comment: Option<String>,
    notes: Option<String>,
    when_unix: Option<u64>,
    confirmed: bool,
    award_confirmed: bool,
    #[serde(default)]
    ota: Option<ManualOta>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManualOta {
    their_program: String,
    their_ref: String,
}
impl ManualRecord {
    fn valid(&self, now_unix: u64) -> bool {
        let text = |s: &str, max: usize| {
            s.len() <= max && !s.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
        };
        let field = |s: &Option<String>, max| s.as_ref().is_none_or(|s| text(s, max));
        self.call.len() >= 3
            && self.call.len() <= 32
            && self
                .call
                .bytes()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == b'/')
            && !self.confirmed
            && !self.award_confirmed
            && self.freq_mhz.is_finite()
            && self.freq_mhz > 0.0
            && self.freq_mhz <= 250_000.0
            && self
                .when_unix
                .is_none_or(|at| at > 0 && at <= now_unix.saturating_add(300))
            && text(&self.band, 16)
            && !self.band.is_empty()
            && text(&self.mode, 32)
            && !self.mode.is_empty()
            && field(&self.grid, 16)
            && field(&self.country, 96)
            && field(&self.state, 16)
            && field(&self.rst_sent, 16)
            && field(&self.rst_rcvd, 16)
            && field(&self.name, 128)
            && field(&self.qth, 256)
            && field(&self.comment, 512)
            && field(&self.notes, 1024)
            && self.ota.as_ref().is_none_or(|o| {
                ["POTA", "SOTA"].contains(&o.their_program.as_str())
                    && !o.their_ref.is_empty()
                    && o.their_ref.len() <= 32
                    && o.their_ref
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'/' || b == b'-')
            })
    }
    fn record(&self) -> Result<tempo_app::dto::LoggedQso, &'static str> {
        let mut value = serde_json::to_value(self).map_err(|_| "invalidRequest")?;
        value["whenUnix"] = json!(self.when_unix.unwrap_or_else(|| super::now_ms() / 1000));
        if self.ota.is_none() {
            value.as_object_mut().ok_or("invalidRequest")?.remove("ota");
        }
        serde_json::from_value(value).map_err(|_| "invalidRequest")
    }
}
struct Window {
    id: String,
    at: Instant,
}
struct Lease {
    id: String,
    session: String,
    device: String,
    until: Instant,
    sequence: u64,
}
struct Receipt {
    id: String,
    session: String,
    device: String,
    fingerprint: Vec<u8>,
    at: Instant,
    value: Value,
    control: Option<Completion>,
}
impl Receipt {
    fn value(&self) -> Value {
        if let Some(control) = &self.control {
            control_value(&self.id, control.outcome())
        } else {
            self.value.clone()
        }
    }
}
fn control_value(id: &str, outcome: Outcome) -> Value {
    let mut value = serde_json::to_value(outcome)
        .unwrap_or_else(|_| json!({"outcome":"unknown","reason":"hardwareUnconfirmed"}));
    value["operation"] = json!("stationControl");
    value["operationId"] = json!(id);
    value
}
struct Core {
    epoch: u64,
    connection: u64,
    lease_epoch: u64,
    boot: Option<String>,
    grants: BTreeSet<String>,
    control_grants: BTreeSet<String>,
    transmit_grants: BTreeSet<String>,
    lease: Option<Lease>,
    revision: u64,
    context: Option<Vec<u8>>,
    windows: VecDeque<Window>,
    receipts: VecDeque<Receipt>,
}
impl Default for Core {
    fn default() -> Self {
        Self {
            epoch: 0,
            connection: 0,
            lease_epoch: 0,
            boot: super::query::snapshot_id().ok(),
            grants: BTreeSet::new(),
            control_grants: BTreeSet::new(),
            transmit_grants: BTreeSet::new(),
            lease: None,
            revision: 0,
            context: None,
            windows: VecDeque::new(),
            receipts: VecDeque::new(),
        }
    }
}
#[derive(Default)]
pub struct Authority {
    spots: Option<crate::SharedSpots>,
    epoch: AtomicU64,
    connection: AtomicU64,
    lease_epoch: AtomicU64,
    core: Mutex<Core>,
    hardware: Revocation,
    transmit: TransmitAuthority,
    stop_owner: Mutex<Option<transmit_stop::Owner>>,
    transmit_revocations: Mutex<BTreeSet<String>>,
    #[cfg(test)]
    before_sync: Option<Box<dyn Fn() + Send + Sync>>,
}
impl Authority {
    fn revoke_execution(&self) {
        self.hardware.revoke();
        self.transmit.revoke();
    }
    pub fn with_spots(spots: Option<crate::SharedSpots>) -> Self {
        Self {
            spots,
            ..Self::default()
        }
    }
    /// Synchronous invalidation does not wait for an in-flight file operation.
    /// Its epoch is reconciled before any next request or local grant.
    pub fn invalidate(&self) {
        self.revoke_execution();
        let _ = self
            .epoch
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1));
    }
    pub fn start_connection(&self) -> u64 {
        self.revoke_execution();
        self.connection
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1))
            .map_or(u64::MAX, |n| n + 1)
    }
    pub fn retire_connection(&self, id: u64) {
        if self
            .connection
            .compare_exchange(id, id.saturating_add(1), Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.revoke_execution();
        }
    }
    fn reconcile(&self, c: &mut Core, now: Instant) -> Result<(), &'static str> {
        let epoch = self.epoch.load(Ordering::SeqCst);
        if epoch == u64::MAX {
            return Err("authorityUnavailable");
        }
        let connection = self.connection.load(Ordering::SeqCst);
        let lease_epoch = self.lease_epoch.load(Ordering::SeqCst);
        if connection == u64::MAX || lease_epoch == u64::MAX {
            return Err("authorityUnavailable");
        }
        if c.connection != connection || c.lease_epoch != lease_epoch {
            c.lease_epoch = lease_epoch;
            c.connection = connection;
            c.lease = None;
            c.windows.clear();
            self.advance(c)?;
        }
        if c.epoch != epoch {
            c.epoch = epoch;
            c.grants.clear();
            c.control_grants.clear();
            c.transmit_grants.clear();
            c.lease = None;
            c.windows.clear();
            c.context = None;
            self.advance(c)?;
        }
        // A local revoke cannot wait behind a durable append. Retire its grant
        // before any later heartbeat, state or command can consume Core again.
        let revoked = std::mem::take(
            &mut *self
                .transmit_revocations
                .lock()
                .map_err(|_| "authorityUnavailable")?,
        );
        for device in revoked {
            c.transmit_grants.remove(&device);
            if c.lease.as_ref().is_some_and(|l| l.device == device) {
                self.transmit.revoke();
            }
        }
        if c.lease.as_ref().is_some_and(|l| now >= l.until) {
            self.revoke_execution();
            c.lease = None;
            c.windows.clear();
            self.advance(c)?;
        }
        while c
            .windows
            .front()
            .is_some_and(|w| now.saturating_duration_since(w.at) >= WINDOW)
        {
            c.windows.pop_front();
        }
        while c
            .receipts
            .front()
            .is_some_and(|r| now.saturating_duration_since(r.at) >= RESULT_AGE)
        {
            c.receipts.pop_front();
        }
        self.sync_stop_owner(c);
        Ok(())
    }
    fn advance(&self, c: &mut Core) -> Result<(), &'static str> {
        if c.revision >= MAX_COUNTER {
            self.revoke_execution();
            c.windows.clear();
            c.lease = None;
            return Err("authorityUnavailable");
        }
        c.revision += 1;
        Ok(())
    }
    pub fn permit(&self, device: &str, allow: bool) -> Result<(), &'static str> {
        if !identifier(device) {
            return Err("invalidRequest");
        }
        let mut c = self.core.try_lock().map_err(|_| "remoteBusy")?;
        self.reconcile(&mut c, Instant::now())?;
        if allow {
            if c.grants.len() >= 16 && !c.grants.contains(device) {
                return Err("remoteBusy");
            }
            c.grants.insert(device.to_string());
        } else {
            c.grants.remove(device);
            if c.lease.as_ref().is_some_and(|l| l.device == device) {
                self.revoke_execution();
                c.lease = None;
                c.windows.clear();
                self.advance(&mut c)?;
            }
        }
        self.sync_stop_owner(&c);
        Ok(())
    }
    pub fn permit_station(&self, device: &str, allow: bool) -> Result<(), &'static str> {
        if !identifier(device) {
            return Err("invalidRequest");
        }
        let mut c = self.core.try_lock().map_err(|_| "remoteBusy")?;
        self.reconcile(&mut c, Instant::now())?;
        if allow {
            if c.control_grants.len() >= 16 && !c.control_grants.contains(device) {
                return Err("remoteBusy");
            }
            c.control_grants.insert(device.to_string());
        } else {
            c.control_grants.remove(device);
            c.transmit_grants.remove(device);
            if c.lease.as_ref().is_some_and(|l| l.device == device) {
                self.revoke_execution();
                c.lease = None;
                c.windows.clear();
                self.advance(&mut c)?;
            }
        }
        self.sync_stop_owner(&c);
        Ok(())
    }
    /// Local permission is boot-scoped and never implied by a receiver grant.
    /// Removing it revokes the current TX permit without waiting for Engine.
    pub fn permit_transmit(&self, device: &str, allow: bool) -> Result<(), &'static str> {
        if !identifier(device) {
            return Err("invalidRequest");
        }
        if !allow {
            self.revoke_transmit_device(device)?;
        }
        let mut c = match self.core.try_lock() {
            Ok(c) => c,
            Err(std::sync::TryLockError::WouldBlock) if !allow => return Ok(()),
            Err(_) => return Err("remoteBusy"),
        };
        self.reconcile(&mut c, Instant::now())?;
        if allow {
            if !c.control_grants.contains(device) {
                return Err("localPermissionRequired");
            }
            c.transmit_grants.insert(device.to_string());
        } else {
            c.transmit_grants.remove(device);
            if c.lease.as_ref().is_some_and(|l| l.device == device) {
                self.transmit.revoke();
            }
        }
        self.sync_stop_owner(&c);
        Ok(())
    }
    /// Any admitted browser departure ends the shared logging lease. This is
    /// deliberately conservative and cannot wait behind a file append. Grants
    /// survive; a controller must explicitly acquire a fresh lease afterward.
    pub fn disconnect_session(&self, session: &str) {
        if identifier(session) {
            self.revoke_execution();
            let _ = self
                .lease_epoch
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_add(1));
        }
    }
    pub fn local_status(&self) -> Value {
        let Ok(mut c) = self.core.try_lock() else {
            return json!({"devices":[],"controller":null});
        };
        if self.reconcile(&mut c, Instant::now()).is_err() {
            return json!({"devices":[],"controller":null});
        }
        json!({"devices":c.grants,"controlDevices":c.control_grants,"transmitDevices":c.transmit_grants,"controller":c.lease.as_ref().map(|l|l.device.as_str())})
    }
    fn context(
        &self,
        c: &mut Core,
        engine: &tempo_app::engine::Engine,
    ) -> Result<(), &'static str> {
        let s = engine.settings();
        let value = json!([
            engine.remote_log_context_generation(),
            engine.remote_receiver_context_generation(),
            engine.remote_actuation_context_generation(),
            s.active_radio,
            s.mycall,
            s.mygrid,
            s.fd_operator,
            s.fd_active,
            s.dial_mhz,
            s.band,
            s.operating_mode
        ]);
        let next = digest(
            &SHA256,
            &serde_json::to_vec(&value).map_err(|_| "authorityUnavailable")?,
        )
        .as_ref()
        .to_vec();
        if c.context.as_ref() != Some(&next) {
            c.context = Some(next);
            c.windows.clear();
            self.advance(c)?;
        }
        Ok(())
    }
    fn state(
        &self,
        c: &mut Core,
        session: &str,
        device: &str,
        now: Instant,
        control: Option<(u8, station::Context, bool, bool)>,
    ) -> Result<Value, &'static str> {
        self.sync_stop_owner(c);
        let allowed =
            c.grants.contains(device) || (control.is_some() && c.control_grants.contains(device));
        let owned = allowed
            && c.lease
                .as_ref()
                .is_some_and(|l| l.session == session && l.device == device);
        if owned
            && c.windows
                .back()
                .is_none_or(|w| now.saturating_duration_since(w.at) >= Duration::from_millis(500))
        {
            c.windows.push_back(Window {
                id: super::query::snapshot_id()?,
                at: now,
            });
            while c.windows.len() > 4 {
                c.windows.pop_front();
            }
        }
        let mut value = json!({"stationBootId":c.boot.as_deref().ok_or("authorityUnavailable")?,"allowed":allowed,
            "phase":if owned{"controlling"}else if c.lease.is_some(){"occupied"}else if allowed{"available"}else{"localPermissionRequired"},
            "leaseId":if owned{c.lease.as_ref().map(|l|l.id.as_str())}else{None},"revision":c.revision,
            "commandWindowId":if owned{c.windows.back().map(|w|w.id.as_str())}else{None},
            "nextSequence":if owned{c.lease.as_ref().map(|l|l.sequence+1)}else{None},
            "leaseRemainingMs":if owned{c.lease.as_ref().map(|l|l.until.saturating_duration_since(now).as_millis() as u64)}else{None},
            "actions":if c.grants.contains(device){vec!["log.manual"]}else{vec![]},"txArmed":false});
        if control
            .as_ref()
            .is_some_and(|(version, _, _, _)| *version >= 4)
        {
            value["transmitEpoch"] = if owned && c.transmit_grants.contains(device) {
                json!(format!("{:016x}", self.transmit.generation()))
            } else {
                Value::Null
            };
        }
        if let Some((version, context, ft_available, tx_owned)) = control {
            let mut capabilities = if c.control_grants.contains(device) {
                station::capabilities(version)
            } else {
                vec![]
            };
            if version >= 4 {
                value["txArmed"] = json!(owned && tx_owned);
                if ft_available
                    && c.control_grants.contains(device)
                    && c.transmit_grants.contains(device)
                {
                    capabilities.extend(["ftOperate", "ftCall", "ftExchange"]);
                }
            }
            value["controls"] = json!({"context":context,"capabilities":capabilities});
        }
        Ok(value)
    }
    pub fn handle(
        &self,
        connection: u64,
        session: &str,
        device: &str,
        request: &Request,
        engine: &crate::SharedEngine,
        now: Instant,
    ) -> Result<Value, &'static str> {
        self.handle_version((connection, 1), session, device, request, engine, now)
    }
    pub fn handle_version(
        &self,
        (connection, version): (u64, u8),
        session: &str,
        device: &str,
        request: &Request,
        engine: &crate::SharedEngine,
        now: Instant,
    ) -> Result<Value, &'static str> {
        if matches!(request, Request::StopTransmit { .. }) {
            if version != 4 {
                return Err("stationUnsupported");
            }
            self.stop_transmit(connection, session, device, request, now)?;
            return Ok(json!({"stop":"accepted"}));
        }
        if !matches!(version, 1..=4)
            || matches!(request, Request::StationControl { action, .. } if version < action.minimum_version())
        {
            return Err("stationUnsupported");
        }
        if connection != self.connection.load(Ordering::SeqCst) {
            return Err("staleConnection");
        }
        if !identifier(session) || !identifier(device) || !identifier(request.id()) {
            return Err("invalidRequest");
        }
        let mut c = self.core.try_lock().map_err(|_| "remoteBusy")?;
        self.reconcile(&mut c, now)?;
        // Capture context and execute under the same engine lock. There is no
        // queue whose work could migrate into a later radio/profile context.
        let mut engine = engine.try_lock().map_err(|_| "stationBusy")?;
        self.context(&mut c, &engine)?;
        let control = (version >= 2).then(|| {
            (
                version,
                station::Context::capture(&engine),
                engine.remote_ft_available(),
                engine.remote_ft_tx_owned(),
            )
        });
        match request {
            Request::StopTransmit { .. } => unreachable!("handled before ordinary operations"),
            Request::State { .. } => self.state(&mut c, session, device, now, control),
            Request::Acquire {
                station_boot_id, ..
            } => {
                if !(c.grants.contains(device) || version >= 2 && c.control_grants.contains(device))
                {
                    return Err("localPermissionRequired");
                }
                if c.boot.as_deref() != Some(station_boot_id.as_str()) {
                    return Err("staleStation");
                }
                if let Some(l) = &c.lease {
                    if l.session != session || l.device != device {
                        return Err("controllerBusy");
                    }
                } else {
                    c.lease = Some(Lease {
                        id: super::query::snapshot_id()?,
                        session: session.into(),
                        device: device.into(),
                        until: now + LEASE,
                        sequence: 0,
                    });
                    self.advance(&mut c)?;
                }
                self.state(&mut c, session, device, now, control)
            }
            Request::Heartbeat { lease_id, .. } => {
                let l = c.lease.as_mut().ok_or("leaseExpired")?;
                if l.id != *lease_id || l.session != session || l.device != device {
                    return Err("notController");
                }
                l.until = now + LEASE;
                let until = l.until;
                if c.transmit_grants.contains(device) {
                    if let Some(permit) = self.transmit.permit(until) {
                        // An accepted heartbeat extends only an already-owned,
                        // unexpired transmission. It cannot arm or take over TX.
                        engine.renew_remote_transmit(permit, now);
                    }
                }
                engine.poll_remote_transmit(now);
                self.state(&mut c, session, device, now, control)
            }
            Request::Release { lease_id, .. } => {
                if c.lease.as_ref().is_some_and(|l| {
                    l.id == *lease_id && l.session == session && l.device == device
                }) {
                    self.revoke_execution();
                    c.lease = None;
                    c.windows.clear();
                    self.advance(&mut c)?;
                }
                self.state(&mut c, session, device, now, control)
            }
            Request::Result { operation_id, .. } => {
                if !identifier(operation_id)
                    || !(c.grants.contains(device)
                        || version >= 2 && c.control_grants.contains(device))
                {
                    return Err("localPermissionRequired");
                }
                c.receipts
                    .iter()
                    .find(|r| {
                        r.id == *operation_id
                            && r.device == device
                            && (version >= 2 || r.control.is_none())
                    })
                    .map(Receipt::value)
                    .ok_or("resultExpired")
            }
            Request::LogManual {
                station_boot_id,
                lease_id,
                expected_revision,
                command_window_id,
                client_sequence,
                ..
            }
            | Request::StationControl {
                station_boot_id,
                lease_id,
                expected_revision,
                command_window_id,
                client_sequence,
                ..
            } => {
                let is_control = matches!(request, Request::StationControl { .. });
                if !(if is_control {
                    c.control_grants.contains(device)
                } else {
                    c.grants.contains(device)
                }) {
                    return Err("localPermissionRequired");
                }
                let bytes = serde_json::to_vec(request).map_err(|_| "invalidRequest")?;
                if bytes.len() > 4096 {
                    return Err("invalidRequest");
                }
                let fingerprint = digest(&SHA256, &bytes).as_ref().to_vec();
                if let Some(r) = c.receipts.iter().find(|r| r.id == request.id()) {
                    return if r.session == session
                        && r.device == device
                        && r.fingerprint == fingerprint
                    {
                        Ok(r.value())
                    } else {
                        Err("requestConflict")
                    };
                }
                if c.boot.as_deref() != Some(station_boot_id.as_str()) {
                    return Err("staleStation");
                }
                let l = c.lease.as_ref().ok_or("leaseExpired")?;
                if l.id != *lease_id || l.session != session || l.device != device {
                    return Err("notController");
                }
                if *client_sequence == 0 || *client_sequence > MAX_COUNTER {
                    return Err("invalidRequest");
                }
                if *client_sequence <= l.sequence {
                    return Err("resultExpired");
                }
                if *client_sequence != l.sequence + 1 {
                    return Err("sequenceConflict");
                }
                if *expected_revision != c.revision {
                    return Err("staleContext");
                }
                let window = c
                    .windows
                    .iter()
                    .find(|w| w.id == *command_window_id)
                    .ok_or("windowExpired")?;
                let current = now.max(Instant::now());
                if current.saturating_duration_since(window.at) >= WINDOW
                    || current >= l.until
                    || self.epoch.load(Ordering::SeqCst) != c.epoch
                    || connection != self.connection.load(Ordering::SeqCst)
                    || c.lease_epoch != self.lease_epoch.load(Ordering::SeqCst)
                {
                    return Err("windowExpired");
                }
                if c.receipts.iter().any(|r| {
                    r.control
                        .as_ref()
                        .is_some_and(|r| r.outcome() == Outcome::Pending)
                }) {
                    return Err("remoteBusy");
                }
                if let Request::StationControl {
                    action, context, ..
                } = request
                {
                    let deadline = (current + Duration::from_secs(5)).min(l.until);
                    let transmit_permit = if let Some(epoch) = action.transmit_epoch() {
                        if !c.transmit_grants.contains(device) {
                            return Err("localPermissionRequired");
                        }
                        if epoch.len() != 16
                            || !epoch
                                .bytes()
                                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                        {
                            return Err("invalidRequest");
                        }
                        let expected =
                            u64::from_str_radix(epoch, 16).map_err(|_| "invalidRequest")?;
                        Some(
                            self.transmit
                                .permit_generation(expected, deadline)
                                .ok_or("staleContext")?,
                        )
                    } else {
                        None
                    };
                    let permit = self
                        .hardware
                        .permit(deadline)
                        .ok_or("authorityUnavailable")?;
                    self.advance(&mut c)?;
                    c.lease.as_mut().ok_or("leaseExpired")?.sequence = *client_sequence;
                    let executed = if let Some(transmit_permit) = transmit_permit {
                        station::execute_transmit(&mut engine, context, action, transmit_permit)
                    } else {
                        station::execute(&mut engine, context, action, permit, self.spots.as_ref())
                    };
                    let completion = match executed {
                        Ok(result) => result,
                        Err(reason) => {
                            let result = Completion::default();
                            result.finish(Outcome::Rejected { reason });
                            result
                        }
                    };
                    let value = control_value(request.id(), completion.outcome());
                    c.receipts.push_back(Receipt {
                        id: request.id().into(),
                        session: session.into(),
                        device: device.into(),
                        fingerprint,
                        at: current,
                        value: value.clone(),
                        control: Some(completion),
                    });
                    while c.receipts.len() > 1024 {
                        c.receipts.pop_front();
                    }
                    c.windows.clear();
                    return Ok(value);
                }
                let Request::LogManual { record, .. } = request else {
                    return Err("invalidRequest");
                };
                if !record.valid(super::now_ms() / 1000) {
                    return Err("invalidRecord");
                }
                if engine.settings().fd_active {
                    return Err("fieldDayUnsupported");
                }
                if engine.settings().save_qso_wav {
                    return Err("recordingUnsupported");
                }
                let rec = record.record()?;
                // This is the operation's commit boundary. After this point an
                // append may exist even if its response or file sync is lost.
                self.advance(&mut c)?;
                c.lease.as_mut().ok_or("leaseExpired")?.sequence = *client_sequence;
                let outcome = engine.log_qso_for_sync(rec.into());
                drop(engine);
                let value = match outcome {
                    tempo_app::engine::LogWriteOutcome::PendingSync(receipts) => {
                        #[cfg(test)]
                        if let Some(probe) = &self.before_sync {
                            probe();
                        }
                        if receipts
                            .into_iter()
                            .try_for_each(|receipt| receipt.sync())
                            .is_ok()
                        {
                            json!({"outcome":"applied","evidence":"fileSynced","uploads":"stationPipeline","operationId":request.id()})
                        } else {
                            json!({"outcome":"unknown","reason":"persistenceUnconfirmed","operationId":request.id()})
                        }
                    }
                    tempo_app::engine::LogWriteOutcome::Unconfirmed => {
                        json!({"outcome":"unknown","reason":"persistenceUnconfirmed","operationId":request.id()})
                    }
                    tempo_app::engine::LogWriteOutcome::Duplicate => {
                        json!({"outcome":"rejected","reason":"alreadyPresent","operationId":request.id()})
                    }
                };
                c.receipts.push_back(Receipt {
                    id: request.id().into(),
                    session: session.into(),
                    device: device.into(),
                    fingerprint,
                    at: current,
                    value: value.clone(),
                    control: None,
                });
                while c.receipts.len() > 1024 {
                    c.receipts.pop_front();
                }
                c.windows.clear();
                Ok(value)
            }
        }
    }
}

/// Retire the lease on every socket exit, including malformed input and timeout.
/// Local device permission survives a network interruption; a lease never does.
pub struct Connection {
    pub authority: std::sync::Arc<Authority>,
    pub id: u64,
}
impl Connection {
    pub fn new(authority: std::sync::Arc<Authority>) -> Self {
        let id = authority.start_connection();
        Self { authority, id }
    }
}
impl Drop for Connection {
    fn drop(&mut self) {
        self.authority.retire_connection(self.id)
    }
}

#[cfg(test)]
mod tests;
