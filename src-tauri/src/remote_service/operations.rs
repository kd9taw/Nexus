//! Station-owned authority for the lease and Stop.
//! Cloud admission routes an approved browser; only a local grant (given at the
//! shack, or restored for a still-approved browser) permits a lease, and no grant
//! arms TX. Stop has independent admission. The lane takes no station action: the
//! old Remote page's FT8/FT4 and held-contact actions, their receipts and its
//! transmit grant are gone, and a browser that still sends one is answered
//! `stationUnsupported` (`Request::StationControl`, `Request::Result`).
use super::transport::identifier;
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Mutex,
};
use std::time::{Duration, Instant};
use tempo_app::engine::remote_transmit::RemoteTxHold;
use tempo_app::remote_control::transmit::TransmitAuthority;

mod station;
mod transmit_stop;

/// A fresh random id in the UUID shape: the station's boot, a lease, a command window.
fn snapshot_id() -> Result<String, &'static str> {
    let s = super::transport::random_secret()?;
    Ok(format!(
        "{}-{}-{}-{}-{}",
        &s[..8],
        &s[8..12],
        &s[12..16],
        &s[16..20],
        &s[20..32]
    ))
}

const LEASE: Duration = Duration::from_secs(5);
const WINDOW: Duration = Duration::from_secs(2);
/// How long the shack's grant waits for Core before it is refused `remoteBusy`: past the brief
/// holds of a live computer's traffic and the radio loop's tick, short enough that a stuck Core
/// never hangs an Approve.
const GRANT_WAIT: Duration = Duration::from_millis(250);
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
    /// A paired computer takes control from the one holding it: that computer's over ends as a
    /// Stop ends it, its lease ends, and this one's starts, with TX off. Only on the LAN road (the
    /// computers' own, through the station's room as well); the relay's browsers never send it.
    TakeControl {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "stationBootId")]
        station_boot_id: String,
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
    /// The old Remote page's read of a station control's receipt. Retired with the controls; still
    /// read, so that either road refuses it by name (`stationUnsupported`) as it always did.
    Result {
        #[serde(rename = "requestId")]
        request_id: String,
        #[serde(rename = "operationId")]
        operation_id: String,
    },
    /// The old Remote page's FT8/FT4 and held-contact actions. Retired: read only so that either
    /// road refuses one by name (`stationUnsupported`), whatever its action, and nothing runs.
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
        context: Value,
        action: Value,
    },
}
impl Request {
    pub fn id(&self) -> &str {
        match self {
            Self::State { request_id }
            | Self::Acquire { request_id, .. }
            | Self::Heartbeat { request_id, .. }
            | Self::Release { request_id, .. }
            | Self::TakeControl { request_id, .. }
            | Self::StopTransmit { request_id, .. }
            | Self::Result { request_id, .. }
            | Self::StationControl { request_id, .. } => request_id,
        }
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
    /// The LAN connection that took it, or `None` for the relay's. The lease is bound to that
    /// connection: it ends with it, and nothing on the other road ends it (the operator's ruling of
    /// 2026-10-04, "both at once").
    lan: Option<u64>,
}
struct Core {
    epoch: u64,
    connection: u64,
    lease_epoch: u64,
    boot: Option<String>,
    control_grants: BTreeSet<String>,
    lease: Option<Lease>,
    revision: u64,
    context: Option<Vec<u8>>,
    windows: VecDeque<Window>,
    /// The transmit WAIVER STATE the station recorded for each sign-in auto-granted device (the
    /// 2026-10-23 sign-in decision), by device id. FAIL-CLOSED: this map does not decide whether a
    /// device is sign-in-granted — the durable service list does, through
    /// [`Authority::signin_devices`] — it only records a POSITIVE outcome. A sign-in-granted device
    /// with an entry here is held or waived per the entry; a sign-in-granted device with NO entry is
    /// held (its grant never recorded a positive waiver/window). Written by
    /// `Controller::grant_vouched` (held with its window) and
    /// `grant_approved` (waived at the shack); a waiver is cleared on revoke. The engine's arm gate is
    /// set from the CONTROLLING device's state at each lease event ([`Authority::apply_tx_hold`]). A
    /// restart restores only the windows of the devices the service had confirmed it notified
    /// ([`Authority::restore_transmit_windows`]) and the waivers given at the shack
    /// ([`Authority::restore_transmit_waivers`]), and every other sign-in device is held.
    tx_grants: BTreeMap<String, TxGrant>,
}
/// What the station recorded for a sign-in auto-granted device's transmit (the 2026-10-23 sign-in
/// decision). Presence of a value is a POSITIVE record; its absence for a sign-in-granted device is
/// a hold (fail-closed).
#[derive(Clone, Copy)]
enum TxGrant {
    /// Held from transmit until this monotonic moment (its 15-minute window); past it, free.
    Held { until: Instant },
    /// Waived: an Approve at the shack. Free to transmit.
    Waived,
    /// Waived at the shack before a restart, and kept for it (`vault::Waivers`). Free to transmit
    /// only once the service's list is heard (`tx_held_for`): held until then.
    Restored,
}
impl Default for Core {
    fn default() -> Self {
        Self {
            epoch: 0,
            connection: 0,
            lease_epoch: 0,
            boot: snapshot_id().ok(),
            control_grants: BTreeSet::new(),
            lease: None,
            revision: 0,
            context: None,
            windows: VecDeque::new(),
            tx_grants: BTreeMap::new(),
        }
    }
}
/// Grants remembered from before a restart or a Turn on, per browser: station control. See
/// `Authority::restore`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DurableGrants {
    pub control: Vec<String>,
}
#[derive(Default)]
pub struct Authority {
    epoch: AtomicU64,
    connection: AtomicU64,
    lease_epoch: AtomicU64,
    core: Mutex<Core>,
    transmit: TransmitAuthority,
    /// A streamed session's transmit presence (see `stream_presence`). Its own authority, not
    /// `transmit`'s: the engine's stand-down (`halt_tx`) moves `transmit`, and a Stop ends a
    /// transmission, not the session that is watching it. Revoked with every other execution
    /// authority when a lease ends, which stops everything at the station on the next tick.
    presence: TransmitAuthority,
    stop_owner: Mutex<Option<transmit_stop::Owner>>,
    /// Station-control revokes, by device, queued so a revoke never waits for Core; see
    /// `revoke_grant_device`.
    grant_revocations: Mutex<BTreeSet<String>>,
    /// The LAN road's open connections. Its own lock, never held while another is taken, so
    /// asking whether a connection is open can wait on nothing.
    lan: Mutex<LanRoad>,
    /// The LAN connection that holds the lease, or 0: a mirror of the lease, kept under Core, for
    /// the relay's departures, which revoke without Core and must leave a LAN controller's
    /// transmission alone. Without a LAN connection it is always 0, and they revoke as before.
    lan_lease: AtomicU64,
    /// One stream at a time for the whole station, whichever road it came by: a stream's end
    /// is the station's (its presence, the held PTT, the window's reset), so a second may not start
    /// while the first is still ending. See `claim_stream`.
    streaming: std::sync::atomic::AtomicBool,
    /// The LAN road's paired computers, who hold station control while LAN is on (see
    /// `hold_lan_devices`). Unset in a build of the service without a LAN road.
    lan_devices: std::sync::OnceLock<LanDevices>,
    /// The computers approved for the room's tunnel, who hold station control while hosted Remote
    /// is on (see `hold_tunnel_devices`). Unset in a build of the service without the tunnel.
    tunnel_devices: std::sync::OnceLock<LanDevices>,
    /// The device ids the service lists as added by a sign-in (`addedBy == "account"`): the
    /// PROVENANCE a transmit hold keys to (the 2026-10-23 sign-in decision). The controller replaces
    /// it from the durable service list on every list, so it does not depend on any station write
    /// surviving — that is what makes a sign-in-granted device fail CLOSED (held) when its waiver
    /// record is missing. Empty (no list heard yet) names nobody, so nobody is held. Its own leaf
    /// lock, only ever taken alone (the controller's replace) or after Core (the gate's read).
    /// `None` until the first list is heard — the UNKNOWN state, which holds an account-road
    /// controller fail-closed rather than reading an empty set as "not sign-in-added".
    signin_devices: Mutex<Option<BTreeSet<String>>>,
    /// The subset of [`Authority::signin_devices`] the service confirms a durable owner-notice
    /// record exists for (the 2026-10-23 sign-in decision). A HELD sign-in device's transmit lifts
    /// (once its window elapses) only while it is here; a device not confirmed is held even past
    /// its window. Replaced from the durable list on every list, beside `signin_devices`.
    notified_devices: Mutex<BTreeSet<String>>,
    /// The service's ids for each computer (or phone) on the room's tunnel, by the name the tunnel
    /// takes it as (`lan::tunnel_device_id`): one device under two names. The transmit hold, its
    /// records and the sign-in provenance all key by the service id, so the hold reads a tunnel name
    /// as those ids ([`Authority::hold_names`]). More than one only for listed devices that share a
    /// key, and then the name is held while any of them is. Replaced beside the tunnel's own list
    /// (`Control::sync_approved`). Its own leaf lock, taken alone or after Core.
    tunnel_owners: Mutex<BTreeMap<String, BTreeSet<String>>>,
}
/// The device ids of the LAN road's paired computers while LAN is on, and none while it is off.
/// Called under Core: it may take no lock that is ever held while Core is taken.
pub type LanDevices = Box<dyn Fn() -> Vec<String> + Send + Sync>;
impl Authority {
    /// The authority's lock, held for as long as a test keeps what this returns: every request
    /// that comes meanwhile is answered `remoteBusy`, as it is while another request holds it.
    #[cfg(test)]
    pub(crate) fn busy_for_a_test(&self) -> impl Sized + '_ {
        self.core
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// The transmit authority's revocation, for the ENGINE to hold so that a local stand-down
    /// (`Engine::halt_tx`) moves the counter a browser is shown as `transmitEpoch`, its Stop's
    /// token, as Remote's own Stop does.
    pub fn transmit_revocation(&self) -> tempo_app::remote_control::Revocation {
        self.transmit.revocation()
    }
    fn revoke_execution(&self) {
        self.transmit.revoke();
        self.presence.revoke();
    }
    /// The relay's road moved (its socket replaced or gone, or a browser on it left): what was
    /// issued under its lease ends at once, before Core is free, as it always has. A lease held on
    /// the LAN is not the relay's, and what it keeps on the air goes on. With no LAN
    /// connection there is no LAN lease, and this is `revoke_execution`.
    fn revoke_relay_execution(&self) {
        if self.lan_lease.load(Ordering::SeqCst) == 0 {
            self.revoke_execution();
        }
    }
    /// The LAN road's paired computers hold station control while LAN is on, whatever clears
    /// grants: `reconcile` reads `devices` each time and gives each one it names its grant back,
    /// so a local decision that clears every grant (`invalidate`: Turn Remote on or off, take over)
    /// leaves them theirs before any next request is answered, as `restore`
    /// does for hosted browsers once the service lists them. It only ever gives: a computer
    /// removed at the shack, or LAN going off, is first dropped from `devices` and then revoked
    /// (`permit_station`), and the revoke stands. Set once, by the LAN road at its start; with no
    /// LAN road, or nobody paired, or LAN off, `reconcile` is as it was.
    pub fn hold_lan_devices(&self, devices: LanDevices) {
        let _ = self.lan_devices.set(devices);
    }
    /// The computers approved here for the room's tunnel hold station control as the LAN's paired
    /// computers do (operator decision 2026-10-10, one rule for every device): `reconcile` reads
    /// `devices` each time and gives each name it lists its grant back, so Turn Remote on, or a
    /// revoke of someone else, leaves them theirs. It only ever gives: a computer removed or no
    /// longer approved is first dropped from `devices` and then revoked (`permit_station`), and the
    /// revoke stands; with hosted Remote off `devices` lists nobody, so the clear that turning it off
    /// makes (`invalidate`) stands too. Names are the shack's own (`lan::tunnel_device_id`), never a
    /// service id. Set once, by the service at its start; `devices` takes only locks of its own,
    /// since it runs under Core.
    pub fn hold_tunnel_devices(&self, devices: LanDevices) {
        let _ = self.tunnel_devices.set(devices);
    }
    /// Is `device` one of the computers that hold station control here (the LAN's paired ones
    /// while LAN is on, the tunnel's approved ones while hosted Remote is on)? A computer reads
    /// only over its own pinned link, so the relay's lane answers no read in its name.
    pub fn is_computer(&self, device: &str) -> bool {
        [&self.lan_devices, &self.tunnel_devices]
            .into_iter()
            .filter_map(|devices| devices.get())
            .any(|devices| devices().iter().any(|d| d == device))
    }
    /// Replace the set of device ids the service lists as added by a sign-in (`addedBy ==
    /// "account"`): the provenance a transmit hold keys to (the 2026-10-23 sign-in decision). The
    /// controller calls this from the durable list on every list it hears, so the provenance cannot
    /// be lost by a station write failing. See [`Authority::signin_devices`].
    pub fn set_signin_devices(&self, devices: BTreeSet<String>) {
        if let Ok(mut held) = self.signin_devices.lock() {
            *held = Some(devices);
        }
    }
    /// Forget the sign-in provenance list, back to the pre-list (`None`) state: for a test to
    /// exercise the restart/startup window where an account-road controller is held fail-closed.
    #[cfg(test)]
    pub(crate) fn forget_signin_devices(&self) {
        if let Ok(mut held) = self.signin_devices.lock() {
            *held = None;
        }
    }
    /// Whether `device` is a computer the station paired or enrolled on its OWN network (the LAN
    /// book). RECORDED provenance (the book), not the lease's transport: a genuinely on-network
    /// device, so its transmit is never held (the 2026-10-23 sign-in decision's LAN-enrolment
    /// waiver). The tunnel's approved computers are NOT included here — a tunnelled connection can
    /// never take the LAN waiver, even presenting a LAN marker.
    fn is_lan_device(&self, device: &str) -> bool {
        self.lan_devices
            .get()
            .is_some_and(|devices| devices().iter().any(|d| d == device))
    }
    /// Replace the subset of sign-in devices the service confirms a durable owner-notice record for
    /// (the 2026-10-23 sign-in decision). The controller calls this from the list beside
    /// `set_signin_devices`. See [`Authority::notified_devices`].
    pub fn set_notified_devices(&self, devices: BTreeSet<String>) {
        if let Ok(mut held) = self.notified_devices.lock() {
            *held = devices;
        }
    }
    /// Replace the service's ids for each computer on the room's tunnel, by its tunnel name. See
    /// [`Authority::tunnel_owners`].
    pub fn set_tunnel_owners(&self, owners: BTreeMap<String, BTreeSet<String>>) {
        if let Ok(mut held) = self.tunnel_owners.lock() {
            *held = owners;
        }
    }
    /// The names the transmit hold keys `device` by: a computer on the room's tunnel by the service's
    /// ids for it, so its tunnel name is held exactly as they are; a device the service names by its
    /// own. FAIL-CLOSED: `None`, which holds, for a name of the shack's own (version 8: a tunnel
    /// name or a LAN id, `lan::tunnel_device_id`) that the map does not place, so one that has
    /// just left the tunnel, or whose revoke was lost, is held; and while the map's lock is
    /// poisoned. A computer in the LAN book never gets here (`tx_held_for`).
    fn hold_names(&self, device: &str) -> Option<BTreeSet<String>> {
        if let Some(owners) = self.tunnel_owners.lock().ok()?.get(device) {
            return (!owners.is_empty()).then(|| owners.clone());
        }
        let shacks = identifier(device) && device.as_bytes()[14] == b'8';
        (!shacks).then(|| BTreeSet::from([device.to_string()]))
    }
    /// Whether the service confirms a durable owner-notice record exists for `device`: the
    /// precondition a held sign-in device's transmit lifts against.
    fn is_notified(&self, device: &str) -> bool {
        self.notified_devices
            .lock()
            .is_ok_and(|devices| devices.contains(device))
    }
    /// Record that a sign-in auto-granted `device` is HELD from transmit until `until` (its
    /// 15-minute window). A positive record; absence of one for a sign-in device also holds (the
    /// 2026-10-23 sign-in decision, fail-closed).
    pub fn hold_transmit_until(&self, device: &str, until: Instant) {
        self.set_tx_grant(device, TxGrant::Held { until });
    }
    /// Record that a sign-in auto-granted `device` is WAIVED (an Approve at the shack): free to
    /// transmit.
    pub fn waive_transmit(&self, device: &str) {
        self.set_tx_grant(device, TxGrant::Waived);
    }
    /// Record the station's POSITIVE transmit outcome for a sign-in auto-granted `device` (the
    /// 2026-10-23 sign-in decision): `Held { until }` for its 15-minute window, or `Waived` for an
    /// Approve at the shack. Writing nothing here for a sign-in-granted
    /// device leaves it HELD (fail-closed): the gate reads `is_signin_granted` and finds no waiver.
    fn set_tx_grant(&self, device: &str, grant: TxGrant) {
        if let Ok(mut c) = self.core.lock() {
            // Kept, never pruned: an elapsed `Held` is still a positive record (granted, window
            // passed) and must stay distinct from "no record" — dropping it would turn an
            // elapsed-and-notified device back into the fail-closed held case. The map is at most
            // the station's device count.
            c.tx_grants.insert(device.to_string(), grant);
        }
    }
    /// What is left at `now` of the transmit window the station recorded for each of `devices`
    /// that has one: zero once it has elapsed, and for an Approve at the shack. A device with no
    /// record is left out. What the station keeps across a restart for the sign-in devices the
    /// service confirmed it notified ([`Authority::restore_transmit_windows`]).
    pub fn transmit_windows(
        &self,
        devices: &BTreeSet<String>,
        now: Instant,
    ) -> BTreeMap<String, Duration> {
        let Ok(c) = self.core.lock() else {
            return BTreeMap::new();
        };
        devices
            .iter()
            .filter_map(|device| {
                let left = match c.tx_grants.get(device)? {
                    TxGrant::Held { until } => until.saturating_duration_since(now),
                    TxGrant::Waived | TxGrant::Restored => Duration::ZERO,
                };
                Some((device.clone(), left))
            })
            .collect()
    }
    /// Restore, after a restart, the transmit window kept for each sign-in device the service had
    /// confirmed it notified: held for what was `left` of it from `now`, never longer than a whole
    /// window (`account::TX_HOLD`). Only ever a `Held` — an
    /// Approve at the shack comes back as an elapsed window, which still waits for the list to
    /// confirm the device and its notice (`tx_held_for`) — and never over a record this run made.
    pub fn restore_transmit_windows(
        &self,
        windows: impl IntoIterator<Item = (String, Duration)>,
        now: Instant,
    ) {
        if let Ok(mut c) = self.core.lock() {
            for (device, left) in windows {
                c.tx_grants.entry(device).or_insert(TxGrant::Held {
                    until: now + left.min(super::account::TX_HOLD),
                });
            }
        }
    }
    /// Restore, at launch, the waivers given at the shack before the restart for `devices`, each
    /// already checked against what the station remembers of it (`Controller::resume`). Over the
    /// window kept with it, since the waiver came after that window. Each waits for the list
    /// ([`TxGrant::Restored`]).
    pub fn restore_transmit_waivers(&self, devices: impl IntoIterator<Item = String>) {
        if let Ok(mut c) = self.core.lock() {
            for device in devices {
                c.tx_grants.insert(device, TxGrant::Restored);
            }
        }
    }
    /// Forget each waiver restored at launch whose device is not in `kept`: what the station keeps of
    /// its waivers now (`Controller::vouch`). A waiver given in this run is left as it is.
    pub fn keep_restored(&self, kept: &BTreeSet<String>) {
        if let Ok(mut c) = self.core.lock() {
            c.tx_grants.retain(|device, grant| {
                !matches!(grant, TxGrant::Restored) || kept.contains(device)
            });
        }
    }
    /// Whether the station holds `device` from transmit at `now`. Every controller is asked here;
    /// candidacy comes from RECORDED provenance, never the connection's transport: a device in the
    /// LAN book (`is_lan_device`) is never held. FAIL-CLOSED on every uncertainty:
    /// - a positive local `Waived` record (an Approve at the shack) frees
    ///   at once, even before the provenance list has loaded; one restored after a restart
    ///   (`Restored`) frees only once the list has loaded;
    /// - the list UNKNOWN (`None`, no list heard yet) → HELD: an empty set must not read as "not
    ///   sign-in-added", so an account-road controller waits until the list confirms it;
    /// - the list loaded and this device NOT in it → free (confirmed not sign-in-added);
    /// - in it, `Held` → held until the window has elapsed AND the owner notice is confirmed durable
    ///   (the 2026-10-23 sign-in decision: an absent/unconfirmed notice holds it even past 15 min);
    ///   in it with NO record → HELD; a poisoned lock → HELD.
    ///
    /// A computer on the room's tunnel is asked under the service's ids for it
    /// ([`Self::hold_names`]), whatever name holds the lease, and held while any of them is; a name
    /// of the shack's own whose service id is not known is HELD.
    fn tx_held_for(&self, device: &str, grants: &BTreeMap<String, TxGrant>, now: Instant) -> bool {
        // A device paired or enrolled on the station's OWN network (the LAN book) is locally
        // trusted — recorded provenance, not transport — so it is never held, even before the
        // account list loads.
        if self.is_lan_device(device) {
            return false;
        }
        self.hold_names(device)
            .is_none_or(|names| names.iter().any(|name| self.held_as(name, grants, now)))
    }
    /// [`Self::tx_held_for`] for one name the hold keys by.
    fn held_as(&self, device: &str, grants: &BTreeMap<String, TxGrant>, now: Instant) -> bool {
        // A local positive waiver (an Approve at the shack) frees at once.
        if matches!(grants.get(device), Some(TxGrant::Waived)) {
            return false;
        }
        match self.signin_devices.lock().as_deref() {
            Ok(Some(set)) if !set.contains(device) => false,
            Ok(Some(_)) => match grants.get(device) {
                Some(TxGrant::Waived | TxGrant::Restored) => false,
                Some(TxGrant::Held { until }) => now < *until || !self.is_notified(device),
                None => true,
            },
            Ok(None) => true,
            Err(_) => true,
        }
    }
    /// Whether the station holds `device` from transmit at `now`, as the grants and the sign-in
    /// provenance decide, without driving the engine: what the shack shows as held.
    pub(crate) fn tx_held(&self, device: &str, now: Instant) -> bool {
        // A poisoned lock holds, as `tx_held_for` does.
        let Ok(grants) = self.core.lock().map(|c| c.tx_grants.clone()) else {
            return true;
        };
        self.tx_held_for(device, &grants, now)
    }

    /// Set the engine's transmit-arm gate from the CONTROLLING device's fail-closed hold state
    /// (the 2026-10-23 sign-in decision). Called at every lease event under the Engine lock, so the
    /// gate always reflects who holds the station now: a held device's hold never leaks onto the
    /// next controller or the local operator, and a sign-in controller without a positive waiver
    /// cannot start a transmission. With no controller nobody is held. The hold itself stops and
    /// disarms nothing — the engine side refuses only a START (an arm, a key, a send, an over).
    fn apply_tx_hold(&self, c: &Core, engine: &mut tempo_app::engine::Engine, now: Instant) {
        // EVERY controller goes through `tx_held_for`: the decision is a pure function of RECORDED
        // provenance, never the lease's transport (a tunnelled connection that looks LAN must not
        // skip the hold — the 2026-10-23 sign-in decision). With no controller, nobody is held.
        let held = c
            .lease
            .as_ref()
            .is_some_and(|l| self.tx_held_for(&l.device, &c.tx_grants, now));
        engine.set_remote_transmit_held(held);
    }

    /// Whether the device driving the station from afar is held from transmit right now — the value
    /// [`Self::apply_tx_hold`] sets, re-derived from the live lease so the engine's hold can track
    /// the CURRENT controller on the radio loop's own tick (the 2026-10-23 sign-in decision). It
    /// reconciles first, so a lease ended WITHOUT a further request — Remote turned off, a retired
    /// connection, a revoke, or expiry — is cleared here and the hold lifts; a still-controlling
    /// held device keeps its lease and stays held. Returns `None` while a request holds Core (it is
    /// setting the gate itself), so the tick leaves the flag be; it never blocks the radio loop and
    /// cannot deadlock a request, which takes Core then the engine with the same `try` discipline.
    ///
    /// Beside the gate's answer it says, for the remote PC's display only, whether the owner of a
    /// held device was notified and, when only its window holds it, what is left of that window
    /// ([`Self::hold_shown`]). Only the gate's answer is decided under Core; the display is worked
    /// out after Core is free, from the grants as the gate read them, so the tick holds Core no
    /// longer than the gate needs and a request (the shack's grant among them) is not kept waiting.
    pub fn current_transmit_hold(&self, now: Instant) -> Option<RemoteTxHold> {
        let (device, grants) = {
            let mut c = self.core.try_lock().ok()?;
            // Authority unavailable (Remote off past the counter, or saturated): no device can
            // operate the station, so none is held and the local operator is free.
            if self.reconcile(&mut c, now).is_err() {
                return Some(RemoteTxHold::default());
            }
            match c.lease.as_ref() {
                Some(l) if self.tx_held_for(&l.device, &c.tx_grants, now) => {
                    (l.device.clone(), c.tx_grants.clone())
                }
                _ => return Some(RemoteTxHold::default()),
            }
        };
        Some(self.hold_shown(&device, &grants, now))
    }

    /// [`Self::hold_shown`] for each of `devices` at `now`, as the remote PC holding the lease under
    /// it would be told, for the shack's Devices list: DISPLAY ONLY, never read to decide anything.
    /// One snapshot of the grants for them all, so a status read takes Core once however many
    /// devices are held, and none with nobody held. One not held, or a poisoned lock, says nothing.
    pub(crate) fn hold_display(
        &self,
        devices: &[String],
        now: Instant,
    ) -> BTreeMap<String, RemoteTxHold> {
        if devices.is_empty() {
            return BTreeMap::new();
        }
        let Ok(grants) = self.core.lock().map(|c| c.tx_grants.clone()) else {
            return BTreeMap::new();
        };
        devices
            .iter()
            .filter(|device| self.tx_held_for(device, &grants, now))
            .map(|device| (device.clone(), self.hold_shown(device, &grants, now)))
            .collect()
    }

    /// What the remote PC may be told of `device`, which the gate holds: notified only when the
    /// service confirms a notice for every name it is held under ([`Self::hold_names`]), and the
    /// time left only then, and only when each of those names is held by its window alone (the
    /// provenance list loaded and naming it), the longest of them. Anything less says no time.
    /// Display only, and taken without Core: it never decides the gate.
    fn hold_shown(
        &self,
        device: &str,
        grants: &BTreeMap<String, TxGrant>,
        now: Instant,
    ) -> RemoteTxHold {
        let held: Vec<String> = self
            .hold_names(device)
            .unwrap_or_default()
            .into_iter()
            .filter(|name| self.held_as(name, grants, now))
            .collect();
        let owner_notified = !held.is_empty() && held.iter().all(|name| self.is_notified(name));
        let listed = |name: &String| {
            self.signin_devices
                .lock()
                .is_ok_and(|set| set.as_ref().is_some_and(|set| set.contains(name)))
        };
        let left = held
            .iter()
            .map(|name| match grants.get(name) {
                Some(TxGrant::Held { until }) if *until > now && listed(name) => Some(*until - now),
                _ => None,
            })
            .collect::<Option<Vec<_>>>()
            .and_then(|lefts| lefts.into_iter().max())
            .filter(|_| owner_notified);
        RemoteTxHold {
            held: true,
            owner_notified,
            left,
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
        self.revoke_relay_execution();
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
            self.revoke_relay_execution();
        }
    }
    /// A computer's connection on the LAN road opened. It disturbs nothing: not the relay's
    /// connection, nor a lease held on either road. Its id carries [`LAN_ROAD`].
    pub fn start_lan_connection(&self) -> u64 {
        let Ok(mut lan) = self.lan.lock() else {
            return u64::MAX;
        };
        if lan.next >= LAN_ROAD - 1 {
            return u64::MAX;
        }
        lan.next += 1;
        let id = LAN_ROAD | lan.next;
        lan.open.insert(id);
        id
    }
    /// That connection ended. If it held the lease, what the lease kept on the air ends at once,
    /// before Core is free, and the lease itself at the next `reconcile`. Any other connection's
    /// lease, on either road, is untouched.
    pub fn retire_lan_connection(&self, id: u64) {
        if let Ok(mut lan) = self.lan.lock() {
            lan.open.remove(&id);
        }
        if self
            .lan_lease
            .compare_exchange(id, 0, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            self.revoke_execution();
        }
    }
    fn lan_open(&self, id: u64) -> bool {
        self.lan.lock().is_ok_and(|lan| lan.open.contains(&id))
    }
    /// May a request on `connection` be answered: the relay's current socket, or a LAN connection
    /// still open?
    fn current(&self, connection: u64) -> bool {
        match lan_road(connection) {
            Some(id) => self.lan_open(id),
            None => connection == self.connection.load(Ordering::SeqCst),
        }
    }
    /// Hold the station's one stream slot (see `streaming`), or `None` while a stream on either
    /// road still holds it. Given back when the claim is dropped, however its stream ends.
    pub fn claim_stream(self: &std::sync::Arc<Self>) -> Option<StreamClaim> {
        self.streaming
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
            .then(|| StreamClaim(self.clone()))
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
            // The relay's road moved. A lease the LAN holds is not the relay's to end.
            if c.lease.as_ref().is_none_or(|l| l.lan.is_none()) {
                c.lease = None;
                c.windows.clear();
                self.advance(c)?;
            }
        }
        // A lease taken on the LAN ends with its connection. `retire_lan_connection` has
        // already revoked what it kept on the air, unless it was taken while the connection ended.
        if c.lease
            .as_ref()
            .and_then(|l| l.lan)
            .is_some_and(|id| !self.lan_open(id))
        {
            self.revoke_execution();
            c.lease = None;
            c.windows.clear();
            self.advance(c)?;
        }
        if c.epoch != epoch {
            c.epoch = epoch;
            c.control_grants.clear();
            c.lease = None;
            c.windows.clear();
            c.context = None;
            self.advance(c)?;
        }
        // The LAN road's paired computers keep station control while LAN is on (see
        // `hold_lan_devices`). Before the queued revocations below, so a revoke wins.
        if let Some(devices) = self.lan_devices.get() {
            c.control_grants
                .extend(devices().into_iter().filter(|d| identifier(d)));
        }
        // The tunnel's approved computers the same way, while hosted Remote is on (see
        // `hold_tunnel_devices`).
        if let Some(devices) = self.tunnel_devices.get() {
            c.control_grants
                .extend(devices().into_iter().filter(|d| identifier(d)));
        }
        // A local revoke never waits for Core (#318): its grant is retired here, before any
        // later heartbeat or state can consume Core again. A revoked controller loses its
        // lease here, as it always has when Core was free.
        let grants_revoked = std::mem::take(
            &mut *self
                .grant_revocations
                .lock()
                .map_err(|_| "authorityUnavailable")?,
        );
        let mut controller_revoked = false;
        for device in grants_revoked {
            c.control_grants.remove(&device);
            // A revoke ends a waiver given at the shack with the grant (its record forgets it too,
            // `Persist::Browser`): granted again, the device is held until it is waived again.
            if matches!(
                c.tx_grants.get(&device),
                Some(TxGrant::Waived | TxGrant::Restored)
            ) {
                c.tx_grants.remove(&device);
            }
            controller_revoked |= c.lease.as_ref().is_some_and(|l| l.device == device);
        }
        if controller_revoked {
            self.revoke_execution();
            c.lease = None;
            c.windows.clear();
            self.advance(c)?;
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
    /// The epoch every local grant belongs to. Any local decision that clears permissions
    /// (Turn off Remote, take over, turning Remote on again) moves it. Revoking one browser does not:
    /// it takes that browser's grant alone (`permit_station`).
    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::SeqCst)
    }
    /// Put back grants remembered for still-approved browsers when Remote comes on (at launch, or
    /// Turn on Remote).
    ///
    /// `epoch` is the epoch captured when Remote came on. If it has moved, a local decision has been
    /// made since and its answer stands: nothing is restored. The grants land in a fresh revision
    /// with no command window and no lease, so nothing issued before the restore can be used with
    /// them. ⛔ Restoring arms nothing: the TX-enable latch is untouched and the lease is dropped.
    pub fn restore(&self, epoch: u64, grants: &DurableGrants) -> Result<bool, &'static str> {
        let mut c = self.core.try_lock().map_err(|_| "remoteBusy")?;
        self.reconcile(&mut c, Instant::now())?;
        if c.epoch != epoch || self.epoch.load(Ordering::SeqCst) != epoch {
            return Ok(false);
        }
        for device in grants.control.iter().filter(|d| identifier(d)) {
            if c.control_grants.len() < 16 {
                c.control_grants.insert(device.clone());
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
    /// A revoke never waits for Core (#318): it is queued for `reconcile`, which applies it
    /// before anything else consumes Core. A grant waits for Core up to [`GRANT_WAIT`], past the
    /// brief holds of a live computer's traffic and the radio loop's tick, and is refused
    /// `remoteBusy` only when Core is held longer. Its callers hold only Control, which no Core
    /// holder ever takes, so the wait cannot deadlock.
    pub fn permit_station(&self, device: &str, allow: bool) -> Result<(), &'static str> {
        if !identifier(device) {
            return Err("invalidRequest");
        }
        if !allow {
            self.revoke_grant_device(device)?;
        }
        let waited = Instant::now() + GRANT_WAIT;
        let mut c = loop {
            match self.core.try_lock() {
                Ok(c) => break c,
                Err(std::sync::TryLockError::WouldBlock) if !allow => return Ok(()),
                Err(std::sync::TryLockError::WouldBlock) if Instant::now() < waited => {
                    std::thread::sleep(Duration::from_millis(1));
                }
                Err(_) => return Err("remoteBusy"),
            }
        };
        self.reconcile(&mut c, Instant::now())?;
        if allow {
            if c.control_grants.len() >= 16 && !c.control_grants.contains(device) {
                return Err("remoteBusy");
            }
            c.control_grants.insert(device.to_string());
        }
        self.sync_stop_owner(&c, Instant::now());
        Ok(())
    }
    /// May this browser listen to the station's receive audio right now?
    ///
    /// Two things, and both are the operator's: the local **control** grant, and this
    /// browser's own live lease.
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
    /// May this browser stream the station (security acceptance test A3)? Exactly the audio lane's
    /// rule, deliberately one definition: the operator's local control grant plus this browser's
    /// own live lease. Asked before any WebRTC state exists, and again on a slow cadence while a
    /// stream runs, so a lapsed lease or a revoked device ends it.
    pub fn stream_admitted(
        &self,
        session: &str,
        device: &str,
        lease_id: &str,
        now: Instant,
    ) -> Result<(), &'static str> {
        self.audio_admitted(session, device, lease_id, now)
    }

    /// S8: the streamed session's transmit presence, minted under the lease that admitted it. The
    /// same checks as admission; a deadline of five seconds from now, and never past the lease.
    ///
    /// ⛔ IT GRANTS NOTHING: nothing is armed or keyed with it and no browser gesture is bound to
    /// it, so there is no displayed generation to check it against. Held by the engine, its only power is to stop every transmission at the
    /// station when it lapses (`engine/remote_stream.rs`).
    pub fn stream_presence(
        &self,
        session: &str,
        device: &str,
        lease_id: &str,
        now: Instant,
    ) -> Result<tempo_app::remote_control::transmit::TransmitPermit, &'static str> {
        self.presence_until(session, device, lease_id, now)
            .map(|(permit, _)| permit)
    }

    /// [`Self::stream_presence`], with the deadline it was given: what Stop's second road tells the
    /// computer whose heartbeat renewed it.
    pub(crate) fn presence_until(
        &self,
        session: &str,
        device: &str,
        lease_id: &str,
        now: Instant,
    ) -> Result<(tempo_app::remote_control::transmit::TransmitPermit, Instant), &'static str> {
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
        let deadline = (now + LEASE).min(lease.until);
        self.presence
            .permit(deadline)
            .map(|permit| (permit, deadline))
            .ok_or("authorityUnavailable")
    }

    /// The authority's lock, held by a test as a slow request would hold it, until the guard goes.
    #[cfg(test)]
    pub(crate) fn held(&self) -> impl Sized + '_ {
        self.core
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Does the LAN connection `connection` hold the station's lease now, for `device`, which holds
    /// station control? What the command lane asks before it admits any call but a Stop, and before
    /// it pushes a feed or an event. Read under Core after a reconcile, so a lease that ran out or
    /// went with a revoke reads as gone even when nothing else has asked since. Off the radio's
    /// path: Core is never held across anything slow, and a Stop never asks.
    pub fn lan_controls(&self, connection: u64, device: &str, now: Instant) -> bool {
        let Ok(mut c) = self.core.lock() else {
            return false;
        };
        if self.reconcile(&mut c, now).is_err() {
            return false;
        }
        c.control_grants.contains(device)
            && c.lease
                .as_ref()
                .is_some_and(|lease| lease.lan == Some(connection) && lease.device == device)
    }

    /// The stream ended: its presence ends with it, and the engine halts on its next tick if it
    /// held any. Ending a session that never connected stops nothing, because it held none.
    pub fn end_stream_presence(&self) {
        self.presence.revoke();
    }

    /// Any admitted browser departure ends the shared lease. This is
    /// deliberately conservative and cannot wait behind a file append. Grants
    /// survive; a controller must explicitly acquire a fresh lease afterward.
    pub fn disconnect_session(&self, session: &str) {
        if identifier(session) {
            self.revoke_relay_execution();
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
        json!({"controlDevices":c.control_grants,"controller":c.lease.as_ref().map(|l|l.device.as_str())})
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
        control: Option<(u8, station::Context, bool)>,
    ) -> Result<Value, &'static str> {
        self.sync_stop_owner(c, now);
        let allowed = control.is_some() && c.control_grants.contains(device);
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
                id: snapshot_id()?,
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
            // `actions` names no action any more, and stays: every shipped page requires the field.
            "actions":Vec::<&str>::new(),"txArmed":false});
        if control
            .as_ref()
            .is_some_and(|(version, _, _)| *version >= 4)
        {
            // The stop token goes to any controlling browser (stop anything, 2026-09-14), and stays
            // with one whose lease has since run out (expired-lease stop, 2026-09-15) — that is what
            // `holds_stop_token` is, and for a live controller it is exactly the old
            // `owned && control_grants.contains(device)`. It starts nothing: it is only a Stop's.
            value["transmitEpoch"] = if self.holds_stop_token(session, device) {
                json!(format!("{:016x}", self.transmit.generation()))
            } else {
                Value::Null
            };
        }
        if let Some((version, context, tx_on)) = control {
            let mut capabilities = vec![];
            if version >= 4 {
                if c.control_grants.contains(device) {
                    // Listening to the station's receive audio. The hint is what makes
                    // the negotiation work in BOTH directions: a browser that has never
                    // heard of it never offers a listen control, and a station built
                    // without the audio lane never advertises it, so nobody is ever
                    // asked for something the other end cannot do. It is under the
                    // control grant — see `audio_admitted`.
                    #[cfg(feature = "radio")]
                    capabilities.push("audioListen");
                }
                // The station's TX-enable latch, for display only: nothing on either end decides a
                // transmission on it. Only with the lease and the stop token beside it, as the page
                // requires of a `txArmed` that is true.
                value["txArmed"] = json!(owned && tx_on && value["transmitEpoch"].is_string());
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
        if matches!(
            request,
            Request::StationControl { .. } | Request::Result { .. }
        ) {
            return Err("stationUnsupported");
        }
        if !matches!(version, 1..=5) {
            return Err("stationUnsupported");
        }
        if !self.current(connection) {
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
        let mut engine = tempo_app::engine::engine_try_lock(engine).map_err(|_| "stationBusy")?;
        self.context(&mut c, &engine)?;
        let control = (version >= 2).then(|| {
            (
                version,
                station::Context::capture(&engine),
                engine.tx_enabled(),
            )
        });
        match request {
            Request::StopTransmit { .. }
            | Request::Result { .. }
            | Request::StationControl { .. } => {
                unreachable!("handled before ordinary operations")
            }
            Request::State { .. } => {
                self.apply_tx_hold(&c, &mut engine, now);
                self.state(&mut c, session, device, now, control)
            }
            Request::Acquire {
                station_boot_id, ..
            } => {
                if !(version >= 2 && c.control_grants.contains(device)) {
                    return Err("localPermissionRequired");
                }
                if c.boot.as_deref() != Some(station_boot_id.as_str()) {
                    return Err("staleStation");
                }
                if let Some(l) = &c.lease {
                    // One controller for the whole station, on either road.
                    if l.session != session || l.device != device || l.lan != lan_road(connection) {
                        return Err("controllerBusy");
                    }
                } else {
                    c.lease = Some(Lease {
                        id: snapshot_id()?,
                        session: session.into(),
                        device: device.into(),
                        until: now + LEASE,
                        sequence: 0,
                        lan: lan_road(connection),
                    });
                    self.advance(&mut c)?;
                }
                self.apply_tx_hold(&c, &mut engine, now);
                self.state(&mut c, session, device, now, control)
            }
            Request::Heartbeat { lease_id, .. } => {
                let l = c.lease.as_mut().ok_or("leaseExpired")?;
                if l.id != *lease_id || l.session != session || l.device != device {
                    return Err("notController");
                }
                l.until = now + LEASE;
                engine.poll_remote_transmit(now);
                self.apply_tx_hold(&c, &mut engine, now);
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
                self.apply_tx_hold(&c, &mut engine, now);
                self.state(&mut c, session, device, now, control)
            }
            Request::TakeControl {
                station_boot_id, ..
            } => {
                if !(version >= 2 && c.control_grants.contains(device)) {
                    return Err("localPermissionRequired");
                }
                if lan_road(connection).is_none() {
                    return Err("stationUnsupported");
                }
                if c.boot.as_deref() != Some(station_boot_id.as_str()) {
                    return Err("staleStation");
                }
                if c.lease.as_ref().is_some_and(|l| {
                    l.session != session || l.device != device || l.lan != lan_road(connection)
                }) {
                    // The other computer's over ends as its Stop would end it, everything issued
                    // under its lease with it (its presence, so what it started ends too), and
                    // the Stop leaves the TX-enable latch off: this computer starts with TX off.
                    self.revoke_execution();
                    let halt = transmit_stop::stop(&mut engine);
                    engine.poll_remote_transmit(now);
                    transmit_stop::halt_off_thread(halt);
                    c.lease = None;
                    c.windows.clear();
                    self.advance(&mut c)?;
                }
                if c.lease.is_none() {
                    c.lease = Some(Lease {
                        id: snapshot_id()?,
                        session: session.into(),
                        device: device.into(),
                        until: now + LEASE,
                        sequence: 0,
                        lan: lan_road(connection),
                    });
                    self.advance(&mut c)?;
                }
                self.apply_tx_hold(&c, &mut engine, now);
                self.state(&mut c, session, device, now, control)
            }
        }
    }
}

/// The local grant a request needs, read before Engine is locked. Stop and the
/// lease-only requests need none here; their arms keep their own rules.
fn permitted(c: &Core, version: u8, device: &str, request: &Request) -> bool {
    match request {
        Request::Acquire { .. } | Request::TakeControl { .. } => {
            version >= 2 && c.control_grants.contains(device)
        }
        _ => true,
    }
}

/// A LAN connection's id carries this bit, so no relay connection's id is ever a LAN one's.
/// The relay's ids count up from 1, with `u64::MAX` its exhausted mark, so neither ever has this
/// bit alone at the top.
pub const LAN_ROAD: u64 = 1 << 62;

/// The LAN connection `connection` names, or `None` for the relay's.
fn lan_road(connection: u64) -> Option<u64> {
    (connection >> 62 == 1).then_some(connection)
}

/// The LAN road's connections: the ones open now, and the count their ids are made from.
#[derive(Default)]
struct LanRoad {
    next: u64,
    open: BTreeSet<u64>,
}

/// The station's one stream slot, held by a stream from its offer until it has finished ending.
pub struct StreamClaim(std::sync::Arc<Authority>);
impl Drop for StreamClaim {
    fn drop(&mut self) {
        self.0.streaming.store(false, Ordering::SeqCst);
    }
}

/// One computer's connection on the LAN road. Retired when dropped, however it ends.
pub struct LanConnection {
    pub authority: std::sync::Arc<Authority>,
    pub id: u64,
}
impl LanConnection {
    pub fn new(authority: std::sync::Arc<Authority>) -> Self {
        let id = authority.start_lan_connection();
        Self { authority, id }
    }
}
impl Drop for LanConnection {
    fn drop(&mut self) {
        self.authority.retire_lan_connection(self.id)
    }
}

pub(crate) use transmit_stop::RoadStop;

/// Presence for a computer on Stop's second road (`lan::stop_road`): minted under that computer's
/// own lease, exactly as a stream's is ([`Authority::stream_presence`]), and handed to the engine,
/// which halts every transmission at the station when it lapses. Neither the authority nor the
/// Engine is waited for: the authority busy is a heartbeat that renewed nothing, and the next asks
/// again; the Engine busy leaves the minted permit with the road, which hands it over at its next
/// look, as a stream's waits for its next tick. The road's thread holds this, never the Engine
/// itself.
///
/// ⛔ IT GRANTS NOTHING, as a stream's presence grants nothing: holding it arms and keys nothing,
/// and its only power is to stop the station when it lapses.
pub(crate) struct RoadPresence {
    authority: std::sync::Arc<Authority>,
    engine: crate::SharedEngine,
}

impl RoadPresence {
    pub(crate) fn new(authority: std::sync::Arc<Authority>, engine: crate::SharedEngine) -> Self {
        Self { authority, engine }
    }

    /// Mint presence for `device`'s `lease` on `session`, now: the permit and its deadline, or
    /// `None` when it is not this computer's lease, or the authority was busy.
    pub(crate) fn mint(
        &self,
        session: &str,
        device: &str,
        lease: &str,
        now: Instant,
    ) -> Option<(tempo_app::remote_control::transmit::TransmitPermit, Instant)> {
        self.authority
            .presence_until(session, device, lease, now)
            .ok()
    }

    /// Hand a minted permit to the engine, whose lock is only tried: `None` while the Engine is
    /// busy, else whether the engine holds presence after.
    pub(crate) fn install(
        &self,
        permit: &tempo_app::remote_control::transmit::TransmitPermit,
        now: Instant,
    ) -> Option<bool> {
        let mut engine = match tempo_app::engine::engine_try_lock(&self.engine) {
            Ok(engine) => engine,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => return None,
        };
        Some(engine.hold_remote_presence(permit.clone(), now))
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
pub(crate) mod tests;
