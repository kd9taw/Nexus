//! Station-owned authority for manual logging and typed station controls.
//! Cloud admission routes an approved browser; only a local grant (given at the
//! shack, or restored for a still-approved browser) permits a lease. FT8/FT4 CQ
//! and TX On/Off require a separate local transmit grant, and no grant arms TX;
//! Stop has independent admission. Deferred hardware
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

mod export;
/// `pub(crate)` for the row key: the desktop's log commands find a row by the same
/// [`logging::Target`] the browser sends, so the two writers cannot disagree about identity.
pub(crate) mod logging;
mod program_edit;
mod program_export;
mod settings;
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
        action: Box<station::Action>,
    },
    /// Edit or delete a logged contact. Operation v4, the logging grant, and the manual log's own
    /// lease, window, sequence and receipt rules.
    LogChange {
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
        change: Box<logging::Change>,
    },
    /// Read one POTA/SOTA activation file, or the list of activations, under the logging grant and
    /// this browser's current lease. Operation v4. It spends no sequence and writes nothing.
    ActivationExport {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "stationBootId")]
        station_boot_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
        /// Null asks for the list. Required: serde would fill a missing `Option` with None.
        #[serde(deserialize_with = "Option::deserialize")]
        selection: Option<export::Selection>,
        index: u32,
    },
    /// Read the station's working channel list as a CHIRP or spreadsheet CSV, under STATION
    /// CONTROL and this browser's current lease. Operation v4. It spends no sequence and writes
    /// nothing — see `program_export`.
    ProgramExport {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "stationBootId")]
        station_boot_id: String,
        #[serde(rename = "leaseId")]
        lease_id: String,
        format: program_export::Format,
        #[serde(rename = "nameCap")]
        name_cap: u32,
        index: u32,
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
            | Self::StationControl { request_id, .. }
            | Self::LogChange { request_id, .. }
            | Self::ActivationExport { request_id, .. }
            | Self::ProgramExport { request_id, .. } => request_id,
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
                // The stored program as the log holds it (POTA, SOTA, or an ADIF SIG such as
                // WWFF kept verbatim), so an edit hands back what it read and rewrites nothing.
                // Mirrors `manualRecord` in ui/src/remote-web/operation-protocol.ts.
                (2..=16).contains(&o.their_program.len())
                    && o.their_program
                        .bytes()
                        .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
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
    /// `None` while the write is still in flight (a control receipt's answer is its completion
    /// and never lives here). See `value`.
    value: Option<Value>,
    control: Option<Completion>,
    // The result payload can require a newer decoder than the Result request.
    // Never send v4 QSO evidence to a browser which only understands v2/v3.
    result_version: u8,
}
impl Receipt {
    /// What a replay or a Result request is answered with. A receipt whose write has not come
    /// back — its file sync or network post runs with Core released — answers `remoteBusy`,
    /// exactly what the browser was told while that work held the lock, so it waits rather than
    /// appending or posting twice.
    fn value(&self) -> Result<Value, &'static str> {
        match (&self.control, &self.value) {
            (Some(control), _) => Ok(control_value(&self.id, control.outcome())),
            (None, Some(value)) => Ok(value.clone()),
            (None, None) => Err("remoteBusy"),
        }
    }
    fn pending(&self) -> bool {
        match &self.control {
            Some(control) => control.outcome() == Outcome::Pending,
            None => self.value.is_none(),
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
/// A control's outcome has settled: what the transport needs to tell the browser unprompted
/// (operation v5). Carries nothing the browser could not poll for — it is the poll, arriving
/// early — so a notice that is dropped costs a round trip and never an outcome.
#[derive(Clone, Debug)]
pub struct CompletionNotice {
    pub session: String,
    pub device: String,
    pub operation_id: String,
    pub version: u8,
    pub connection: u64,
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
impl Core {
    /// Record a receipt and spend the command window it was admitted under.
    fn open(&mut self, receipt: Receipt) {
        self.receipts.push_back(receipt);
        while self.receipts.len() > 1024 {
            self.receipts.pop_front();
        }
        self.windows.clear();
    }
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
/// Grants remembered from before a restart or a Turn on, per browser: remote logging, station
/// control and FT8/FT4 transmit. See `Authority::restore`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DurableGrants {
    pub logging: Vec<String>,
    pub control: Vec<String>,
    pub transmit: Vec<String>,
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
    /// Where a settled control's notice goes (see `CompletionNotice`): the live transport's
    /// bounded queue, set per socket. None, or a full queue, means the browser polls as it did
    /// before operation v5 — this is never load-bearing.
    completions: Mutex<Option<tokio::sync::mpsc::Sender<CompletionNotice>>>,
    #[cfg(test)]
    before_sync: Option<Box<dyn Fn() + Send + Sync>>,
    /// Tests post self-spots here, one poster per target. A test build has no path to pota.app or
    /// the shared cluster outbox at all.
    #[cfg(test)]
    spot_pota: Option<PotaPoster>,
    #[cfg(test)]
    spot_cluster: Option<ClusterPoster>,
    /// A test's own self-spot session (login latch, repeat limit), never the process's.
    #[cfg(test)]
    spot_gate: crate::self_spot::Gate,
    /// Tests turn self-spot off here, to prove `logging::SELF_SPOT` still refuses when false.
    #[cfg(test)]
    self_spot_off: bool,
}
#[cfg(test)]
type PotaPoster = Box<
    dyn Fn(
            &propagation::live::pota::SpotPost,
        ) -> Result<propagation::live::pota::SpotAnswer, String>
        + Send
        + Sync,
>;
#[cfg(test)]
type ClusterPoster = Box<dyn Fn(f64, &str, &str) -> Result<(), String> + Send + Sync>;
impl Authority {
    /// Self-spot posts publicly from the station's own call, to pota.app and its cluster login, so
    /// it has its own switch (`logging::SELF_SPOT`).
    fn self_spot_enabled(&self) -> bool {
        #[cfg(test)]
        if self.self_spot_off {
            return false;
        }
        logging::SELF_SPOT
    }
    #[cfg(not(test))]
    fn self_spot(&self, context: &crate::self_spot::Context) -> crate::self_spot::Report {
        crate::self_spot::send(context)
    }
    /// The test build, unit tests and the native harness alike, reaches neither network target:
    /// a target with no poster installed is refused (it reads `failed`), never posted.
    #[cfg(test)]
    fn self_spot(&self, context: &crate::self_spot::Context) -> crate::self_spot::Report {
        const NONE: &str = "test build: no spot poster installed";
        crate::self_spot::send_with(
            &self.spot_gate,
            Instant::now(),
            context,
            |spot| {
                self.spot_pota
                    .as_ref()
                    .map_or(Err(NONE.into()), |post| post(spot))
            },
            |freq, call, comment| {
                self.spot_cluster
                    .as_ref()
                    .map_or(Err(NONE.into()), |post| post(freq, call, comment))
            },
        )
    }
    /// A public spot of ANOTHER station, through the station's own `post_spot` verb — the same
    /// door the desktop Spot dialog uses, with its callsign rule and its no-node-connected refusal.
    #[cfg(not(test))]
    fn cluster_spot(&self, freq_mhz: f64, call: &str, comment: &str) -> Result<(), String> {
        crate::post_spot(freq_mhz, call.into(), comment.into())
    }
    /// The test build reaches no cluster at all: the poster a test installed answers, and with none
    /// installed the spot is refused. This is the self-spot's own hook, so a test can never put a
    /// real spot in front of the world.
    #[cfg(test)]
    fn cluster_spot(&self, freq_mhz: f64, call: &str, comment: &str) -> Result<(), String> {
        self.spot_cluster
            .as_ref()
            .map_or(Err("test build: no spot poster installed".into()), |post| {
                post(freq_mhz, call, comment)
            })
    }
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
        self.sync_stop_owner(c, now);
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
    /// Re-take Core to record what a write that ran with it released came back with. Waiting is
    /// fine here: nothing else holds Core across anything slow, and this thread holds nothing.
    fn record(&self, id: &str, value: Value) -> Result<(), &'static str> {
        let mut c = self.core.lock().map_err(|_| "authorityUnavailable")?;
        if let Some(receipt) = c.receipts.iter_mut().find(|r| r.id == id) {
            receipt.value = Some(value);
        }
        Ok(())
    }
    /// The transport's queue for settled controls. One socket at a time holds the station, so
    /// the newest simply replaces the last; a notice for an older connection is refused by
    /// `completion_event` anyway.
    pub fn watch_completions(&self, sink: tokio::sync::mpsc::Sender<CompletionNotice>) {
        if let Ok(mut current) = self.completions.lock() {
            *current = Some(sink);
        }
    }
    /// Arrange for `notice` to reach the transport when `completion` settles — only for a
    /// browser that negotiated v5, because an older page has no parser for the event and its
    /// socket would close on it. `try_send`, never a wait: the queue is bounded and a full one
    /// drops the notice, which the browser's own polling covers.
    fn notify_on_finish(&self, completion: &Completion, notice: CompletionNotice) {
        if notice.version < 5 {
            return;
        }
        let Some(sink) = self.completions.lock().ok().and_then(|s| s.clone()) else {
            return;
        };
        completion.on_finish(std::sync::Arc::new(move || {
            let _ = sink.try_send(notice.clone());
        }));
    }
    /// The unprompted `operationEvent` for a settled control: its outcome and the state a
    /// `State` request would return now — computed under the same locks, in the same order, as
    /// `handle_version`, so the browser can install it exactly as it installs a reply. None when
    /// there is nothing to say (the receipt is gone, the socket has changed) and the browser is
    /// left to its poll.
    ///
    /// Waits for both locks rather than answering `stationBusy`: the radio loop that settled
    /// the outcome still holds Engine when the notice arrives, and this runs on a blocking
    /// thread the transport does not wait on. Engine is taken first, as `handle_version` takes
    /// Core then tries Engine and every other Core user only tries, so nothing can wait on
    /// this thread while it waits.
    pub fn completion_event(
        &self,
        notice: &CompletionNotice,
        engine: &crate::SharedEngine,
    ) -> Option<String> {
        let now = Instant::now();
        let engine = engine.lock().ok()?;
        let mut c = self.core.lock().ok()?;
        self.reconcile(&mut c, now).ok()?;
        if notice.connection != self.connection.load(Ordering::SeqCst) {
            return None;
        }
        let value = c
            .receipts
            .iter()
            .find(|r| {
                r.id == notice.operation_id
                    && r.session == notice.session
                    && r.device == notice.device
            })?
            .value()
            .ok()?;
        self.context(&mut c, &engine).ok()?;
        let control = Some((
            notice.version,
            station::Context::capture(&engine),
            engine.remote_ft_available(),
            engine.remote_ft_tx_owned(),
        ));
        let state = self
            .state(&mut c, &notice.session, &notice.device, now, control)
            .ok()?;
        Some(
            json!({"type":"operationEvent","sessionId":notice.session,"operationId":notice.operation_id,"value":value,"state":state})
                .to_string(),
        )
    }
    /// The epoch every local grant belongs to. Any local decision that clears permissions
    /// (Turn off Remote, take over, revoking a browser, turning Remote on again) moves it.
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }
    /// Put back grants remembered for still-approved browsers when Remote comes on (at launch, or
    /// Turn on Remote).
    ///
    /// `epoch` is the epoch captured when Remote came on. If it has moved, a local decision has been
    /// made since and its answer stands: nothing is restored. The grants land in a fresh revision
    /// with no command window and no lease, so nothing issued before the restore can be used with
    /// them.
    ///
    /// FT8/FT4 transmit is restored only for a browser that also holds station control, exactly as
    /// `permit_transmit` requires. ⛔ Restoring it arms nothing: the TX-enable latch is untouched,
    /// the lease is dropped, and keying still needs the browser's own TX On under a fresh lease.
    pub fn restore(&self, epoch: u64, grants: &DurableGrants) -> Result<bool, &'static str> {
        let mut c = self.core.try_lock().map_err(|_| "remoteBusy")?;
        self.reconcile(&mut c, Instant::now())?;
        if c.epoch != epoch || self.epoch.load(Ordering::SeqCst) != epoch {
            return Ok(false);
        }
        for device in grants.logging.iter().filter(|d| identifier(d)) {
            if c.grants.len() < 16 {
                c.grants.insert(device.clone());
            }
        }
        for device in grants.control.iter().filter(|d| identifier(d)) {
            if c.control_grants.len() < 16 {
                c.control_grants.insert(device.clone());
            }
        }
        for device in &grants.transmit {
            if c.control_grants.contains(device) {
                c.transmit_grants.insert(device.clone());
            }
        }
        if c.lease.take().is_some() {
            self.revoke_execution();
        }
        c.windows.clear();
        self.advance(&mut c)?;
        self.sync_stop_owner(&c, Instant::now());
        Ok(true)
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
        self.sync_stop_owner(&c, Instant::now());
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
        self.sync_stop_owner(&c, Instant::now());
        Ok(())
    }
    /// Local permission, never implied by a receiver grant, and it arms nothing.
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
        self.sync_stop_owner(&c, Instant::now());
        Ok(())
    }
    /// May this browser listen to the station's receive audio right now?
    ///
    /// Two things, and both are the operator's: the local **control** grant, and this
    /// browser's own live lease. A logging-only browser holds `grants` and a perfectly
    /// valid lease and is refused here — hearing a shack is not a consequence of being
    /// allowed to write to its log, and the two permissions are given separately at the
    /// radio precisely so they can be withheld separately.
    ///
    /// Nothing here can key anything. It is a read of permission state, and the caller
    /// it serves holds no transmit authority at all.
    ///
    /// `remoteBusy` means the authority lock was held, not that the browser was refused.
    /// The audio lane re-asks on a cadence, and treating contention as a refusal would
    /// drop a listener every time an operation happened to be in flight.
    pub fn audio_admitted(
        &self,
        session: &str,
        device: &str,
        lease_id: &str,
        now: Instant,
    ) -> Result<(), &'static str> {
        if !identifier(session) || !identifier(device) || !identifier(lease_id) {
            return Err("invalidRequest");
        }
        let mut c = self.core.try_lock().map_err(|_| "remoteBusy")?;
        self.reconcile(&mut c, now)?;
        if !c.control_grants.contains(device) {
            return Err("notController");
        }
        let lease = c.lease.as_ref().ok_or("notController")?;
        if lease.id != lease_id || lease.session != session || lease.device != device {
            return Err("notController");
        }
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
        self.sync_stop_owner(c, now);
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
            // The stop token goes to any controlling browser (stop anything, 2026-09-14), and stays
            // with one whose lease has since run out (expired-lease stop, 2026-09-15) — that is what
            // `holds_stop_token` is, and for a live controller it is exactly the old
            // `owned && control_grants.contains(device)`. It starts nothing: every FT action also
            // checks the live lease, the transmit grant and its own capability.
            value["transmitEpoch"] = if self.holds_stop_token(session, device) {
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
                if c.grants.contains(device) {
                    capabilities.push("qsoLogging");
                    capabilities.extend(logging::CAPABILITIES);
                    if self.self_spot_enabled() {
                        capabilities.push("selfSpot");
                    }
                }
                // Station preferences are changed under station control (see `settings.rs`), and
                // so is a public spot of another station: it posts from this station's cluster
                // login rather than touching the log.
                if c.control_grants.contains(device) {
                    capabilities.push("settingsControl");
                    capabilities.push("postSpot");
                    // Rendering the working channel list to a CHIRP/CSV file the browser saves,
                    // and curating that list. The hints are what let an older station stay silent
                    // and an older page never offer the buttons.
                    capabilities.push("programExport");
                    capabilities.push("programEdit");
                    // Listening to the station's receive audio. The hint is what makes
                    // the negotiation work in BOTH directions: a browser that has never
                    // heard of it never offers a listen control, and a station built
                    // without the audio lane never advertises it, so nobody is ever
                    // asked for something the other end cannot do. It is under the
                    // CONTROL grant, not the logging one — see `audio_admitted`.
                    #[cfg(feature = "radio")]
                    capabilities.push("audioListen");
                    // Operation v5: this station tells the browser a control's outcome the
                    // moment it settles (`operationEvent`), so the page may stop re-reading
                    // after each command. NEGOTIATED, never assumed from the page's own
                    // version: `version` is the one the relay agreed for this request, so a
                    // v5 page on an older station never sees the hint and keeps its immediate
                    // re-read. A hint like `audioListen` - it names no action.
                    if version >= 5 {
                        capabilities.push("outcomePush");
                    }
                }
                value["txArmed"] = json!(owned && tx_owned);
                if ft_available
                    && c.control_grants.contains(device)
                    && c.transmit_grants.contains(device)
                {
                    capabilities.extend([
                        "ftOperate",
                        "ftCall",
                        "ftExchange",
                        "ftMessages",
                        "ftSettings",
                        "ftRuntime",
                    ]);
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
            transmit_stop::stop_station(engine);
            return Ok(json!({"stop":"accepted"}));
        }
        if !matches!(version, 1..=5)
            || matches!(request, Request::StationControl { action, .. } if version < action.minimum_version())
            || matches!(request, Request::LogChange { .. } | Request::ActivationExport { .. } | Request::ProgramExport { .. } if version < 4)
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
        // A permission refusal never depends on Engine contention: a device that
        // lost its local permission is told so even while Engine is busy. Each
        // request arm below repeats its own check, so nothing more is allowed.
        if !permitted(&c, version, device, request) {
            return Err("localPermissionRequired");
        }
        // Capture context and execute under the same engine lock. There is no
        // queue whose work could migrate into a later radio/profile context.
        let shared_engine = engine;
        let mut engine = shared_engine.try_lock().map_err(|_| "stationBusy")?;
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
            Request::ActivationExport {
                station_boot_id,
                lease_id,
                selection,
                index,
                ..
            } => {
                // A read under the logging grant (checked above) and this browser's own lease. It
                // spends no sequence or command window and writes nothing.
                if c.boot.as_deref() != Some(station_boot_id.as_str()) {
                    return Err("staleStation");
                }
                let l = c.lease.as_ref().ok_or("leaseExpired")?;
                if l.id != *lease_id || l.session != session || l.device != device {
                    return Err("notController");
                }
                export::respond(&engine, selection.as_ref(), *index)
            }
            Request::ProgramExport {
                station_boot_id,
                lease_id,
                format,
                name_cap,
                index,
                ..
            } => {
                // A read under station control (checked above) and this browser's own lease. Same
                // shape as the activation read beside it: no sequence, no command window, no write.
                if c.boot.as_deref() != Some(station_boot_id.as_str()) {
                    return Err("staleStation");
                }
                let l = c.lease.as_ref().ok_or("leaseExpired")?;
                if l.id != *lease_id || l.session != session || l.device != device {
                    return Err("notController");
                }
                program_export::respond(&crate::radioprog_path(), *format, *name_cap, *index)
            }
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
                    .ok_or("resultExpired")
                    .and_then(|receipt| {
                        if version < receipt.result_version {
                            Err("stationUnsupported")
                        } else {
                            receipt.value()
                        }
                    })
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
            }
            | Request::LogChange {
                station_boot_id,
                lease_id,
                expected_revision,
                command_window_id,
                client_sequence,
                ..
            } => {
                // The same local grant `permitted` checked above, read again under this lock.
                if !permitted(&c, version, device, request) {
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
                        r.value()
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
                if c.receipts.iter().any(Receipt::pending) {
                    return Err("remoteBusy");
                }
                if let Request::LogChange { change, .. } = request {
                    // Refused like an older station would, before the sequence advances.
                    if matches!(**change, logging::Change::SelfSpot { .. })
                        && !self.self_spot_enabled()
                    {
                        return Err("stationUnsupported");
                    }
                    if !change.valid(super::now_ms() / 1000) {
                        return Err("invalidRecord");
                    }
                    // The commit boundary, as for a manual entry: a rewrite begun here cannot be
                    // rolled back by a disconnect, and its receipt answers any replay.
                    self.advance(&mut c)?;
                    c.lease.as_mut().ok_or("leaseExpired")?.sequence = *client_sequence;
                    let prepared = logging::prepare_change(&mut engine, change);
                    drop(engine);
                    // Engine is released, and so is Core: the receipt goes in as in flight first,
                    // so a replay meanwhile is told the station is busy rather than posting twice,
                    // and a revoke, audio recheck or status read at the shack never waits behind
                    // the network post or the file sync that follow. A self-spot's pota.app post
                    // can take 15 s; while Core was held across it, a revoke silently did nothing.
                    c.open(Receipt {
                        id: request.id().into(),
                        session: session.into(),
                        device: device.into(),
                        fingerprint,
                        at: current,
                        value: None,
                        control: None,
                        result_version: 4,
                    });
                    drop(c);
                    #[cfg(test)]
                    if let Some(probe) = &self.before_sync {
                        probe();
                    }
                    let outcome = match prepared {
                        // Nothing is held; a spot is posted only now, and only once per receipt.
                        Ok(work) => work.finish(
                            |context| self.self_spot(context),
                            |freq_mhz, call, comment| self.cluster_spot(freq_mhz, call, comment),
                        ),
                        Err(reason) => logging::ChangeOutcome::Rejected { reason, spot: None },
                    };
                    let value = logging::change_value(request.id(), &outcome);
                    self.record(request.id(), value.clone())?;
                    return Ok(value);
                }
                if let Request::StationControl {
                    action, context, ..
                } = request
                {
                    if action.is_logging() {
                        self.advance(&mut c)?;
                        c.lease.as_mut().ok_or("leaseExpired")?.sequence = *client_sequence;
                        let prepared = logging::prepare(&mut engine, action);
                        drop(engine);
                        // The hardware receipt's own shape: pending under Core, finished once
                        // the file sync comes back with Core released (see the log change above).
                        let completion = Completion::default();
                        c.open(Receipt {
                            id: request.id().into(),
                            session: session.into(),
                            device: device.into(),
                            fingerprint,
                            at: current,
                            value: None,
                            control: Some(completion.clone()),
                            result_version: 4,
                        });
                        drop(c);
                        self.notify_on_finish(
                            &completion,
                            CompletionNotice {
                                session: session.into(),
                                device: device.into(),
                                operation_id: request.id().into(),
                                version,
                                connection,
                            },
                        );
                        #[cfg(test)]
                        if let Some(probe) = &self.before_sync {
                            probe();
                        }
                        let outcome = match prepared {
                            Ok(work) => work.finish(shared_engine),
                            Err(reason) => Outcome::Rejected { reason },
                        };
                        completion.finish(outcome.clone());
                        return Ok(control_value(request.id(), outcome));
                    }
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
                        station::execute(
                            &mut engine,
                            shared_engine,
                            context,
                            action,
                            permit,
                            self.spots.as_ref(),
                        )
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
                    c.open(Receipt {
                        id: request.id().into(),
                        session: session.into(),
                        device: device.into(),
                        fingerprint,
                        at: current,
                        value: None,
                        control: Some(completion.clone()),
                        result_version: 2,
                    });
                    // After the receipt is in, so a settlement that races this registration
                    // (the radio loop can be that fast on a rejected target) finds the receipt
                    // the event is built from. `on_finish` tells at once if it already settled.
                    self.notify_on_finish(
                        &completion,
                        CompletionNotice {
                            session: session.into(),
                            device: device.into(),
                            operation_id: request.id().into(),
                            version,
                            connection,
                        },
                    );
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
                // The file sync runs with Core released too (see the log change above).
                c.open(Receipt {
                    id: request.id().into(),
                    session: session.into(),
                    device: device.into(),
                    fingerprint,
                    at: current,
                    value: None,
                    control: None,
                    result_version: 1,
                });
                drop(c);
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
                self.record(request.id(), value.clone())?;
                Ok(value)
            }
        }
    }
}

/// The local grant a request needs, read before Engine is locked. Stop and the
/// lease-only requests need none here; their arms keep their own rules.
fn permitted(c: &Core, version: u8, device: &str, request: &Request) -> bool {
    match request {
        Request::Acquire { .. } | Request::Result { .. } => {
            c.grants.contains(device) || version >= 2 && c.control_grants.contains(device)
        }
        Request::LogManual { .. } | Request::ActivationExport { .. } => c.grants.contains(device),
        // The channel list itself is already readable by any observing browser (the `programming`
        // collection), so this read reveals nothing new — but it renders a station file through a
        // station writer, which is what station control means here, and it is the same grant the
        // channel TUNE beside it already needs.
        Request::ProgramExport { .. } => c.control_grants.contains(device),
        // A log change needs the logging grant; a preference change needs what its keys need.
        Request::LogChange { change, .. } => {
            let (logging, control) = change.grants();
            (!logging || c.grants.contains(device))
                && (!control || c.control_grants.contains(device))
        }
        Request::StationControl { action, .. } => {
            if action.is_logging() {
                c.grants.contains(device)
            } else {
                c.control_grants.contains(device)
            }
        }
        _ => true,
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
