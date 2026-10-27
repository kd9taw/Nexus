//! Local approval and the Remote background task. Engine access is restricted to
//! bounded observation, the stream, the lease and Stop.
/// The station's account: binding it to its owner by a sign-in at the shack, the issuer's keys,
/// and the checks a listed device's proof must pass.
pub(crate) mod account;
/// Receive audio for a listening browser. Gated with the radio feature because the
/// encoder lives in tempo-audio, which a build without it does not have at all.
#[cfg(feature = "radio")]
mod audio;
/// Remote over this network: the shack's own listener, for a paired computer on the same network.
pub(crate) mod lan;
/// The relay's older lanes, held to the browser's own key (security review S1-M1).
mod lanes;
#[cfg(test)]
pub(crate) mod log_tests;
pub(crate) mod operations;
/// The shack PC's two identifiers, hashed, sent when approving a station may start a trial.
mod pc_identity;
/// The station's own signing key, which signs its stream answers (security review S3-M1).
mod station_key;
#[cfg(test)]
pub(crate) mod stored_log_tests;
/// Remote as a stream: the `streamSignal` lane and the session thread (see its header).
pub(crate) mod stream;
#[cfg(test)]
mod tests;
mod transport;
/// The tunnel through the room: Nexus on another computer driving this station over the internet.
mod tunnel;
pub(crate) mod vault;

use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use tokio::sync::{mpsc, oneshot, watch};
/// The one Remote origin: the service this station talks to.
pub(crate) use transport::REMOTE_ORIGIN;
use transport::{credential, empty, identifier, station_path, Client};
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
    /// `devices` is a list the service gave, with nothing it has heard of since but what this
    /// station removed: false before the first list, while unpaired, and after an approval until
    /// the list is heard again. A Remove at the shack takes nothing as unlisted from a list it does
    /// not have ([`Control::roads`]).
    #[serde(skip)]
    listed: bool,
    error: Option<&'static str>,
    observation_generation: Option<String>,
    station_permissions: Vec<String>,
    /// The device holding the station's lease, if any.
    controller: Option<String>,
    /// A5: the browsers whose listed device key is the one pinned at the radio, the ones that can
    /// stream. An approved browser listed with a key and missing here is approved again to stream.
    pinned_devices: Vec<String>,
    /// The devices added by their own sign-in that the station holds from transmit now, each shown
    /// at the shack with Allow to transmit: only those granted station control and not removed
    /// here, waiting for an Approve, or on a proof not confirmed ([`allows_transmit`]).
    held_devices: Vec<String>,
    /// S3-L1: SHA-256 of this station's own signing key, lowercase hex, worked out as a page works out
    /// the key the service lists: shown as "This station's key" for the operator to compare with the
    /// key the page kept. Its public half only. `None` while there is no pairing or no key.
    station_key: Option<String>,
    /// S3-M1: the service holds another signing key for this station, so browsers refuse its stream
    /// until it is paired again. Its own field, because it lasts past any one request's error.
    key_refused: bool,
    /// Remote over this network (`lan`): its switch, its paired computers and its pairing window.
    /// Absent only from a build of the service without it.
    #[serde(skip_serializing_if = "Option::is_none")]
    lan: Option<lan::LanStatus>,
    /// The station's account (`account`): signing in at the shack, and the devices it granted on
    /// their own proof.
    account: AccountStatus,
    /// DISPLAY ONLY, for the shack's one Devices list: each device once, with the ids it goes by on
    /// each road as a Remove there takes them, and whose it is ([`Control::device_rows`]). Nothing
    /// reads it to decide anything.
    device_rows: Vec<DeviceRow>,
    /// DISPLAY ONLY: whose each device added by a sign-in is, as its proof last held here
    /// ([`Controller::recall_whose`]). Never read to decide anything.
    #[serde(skip)]
    whose: BTreeMap<String, Whose>,
}
/// One device of the shack's Devices list. DISPLAY ONLY: every button on it runs the action the
/// shack ran before, by the ids here.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceRow {
    /// Its ids in `devices`.
    listed: Vec<String>,
    /// Its ids in the LAN book (`lan.devices`).
    lan: Vec<String>,
    /// The invited person its proof was checked for, by address; `None` for the owner's, or one
    /// added at the shack.
    guest: Option<String>,
    /// Added by a sign-in, and whose is not known here: no proof of it has held since Nexus started.
    unknown: bool,
    /// It holds the station's lease now.
    live: bool,
    /// Seconds left of its transmit window, only where the remote PC is shown them
    /// (`operations::Authority::hold_display`): the window alone holds it and its owner was
    /// notified.
    hold_left: Option<u64>,
}
/// Whose a device added by a sign-in is, as its proof last held here: display only.
#[derive(Clone, Debug, PartialEq)]
enum Whose {
    Owner,
    Guest(String),
    /// Its proof did not hold here, or could not be checked.
    Unknown,
}
impl Whose {
    fn of(vouched: &tempo_client::account::Vouched) -> Self {
        vouched.mailbox.clone().map_or(Whose::Owner, Whose::Guest)
    }
}
/// The station's account, as the shack shows it.
#[derive(Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct AccountStatus {
    /// A sign-in at the shack is waiting for the browser.
    signing_in: bool,
    /// The key that signed the station's own proof has left the issuer's set, so devices can no
    /// longer check it: the operator signs in again to keep adding devices.
    reproof: bool,
    /// The devices the station granted on their own proof since Nexus started, for the notice at
    /// the shack, each until the operator dismisses it.
    added: Vec<Added>,
    /// The devices the service approved on their own proof that the station granted nothing, each
    /// with why: a proof it could not confirm, or `approveAtStation`, a proof that holds while the
    /// station grants nothing on a sign-in (`account::GRANTS_ON_SIGN_IN`). Each is shown with the
    /// prompt a device added at the station gets.
    unconfirmed: BTreeMap<String, &'static str>,
    /// Devices removed here while the service could not be reached: out at the station already,
    /// and told to the service before the station's next list.
    removals: Vec<String>,
    /// The people invited to this station, as it keeps them: each address as the service matches
    /// it, and a role.
    people: Vec<tempo_client::account::Invite>,
    /// The computers the last invitation taken back took off this network beside that guest's own:
    /// guests' computers the station kept no proof of whose they were, by name, for the shack to
    /// tell their owners to sign in again ([`Controller::off_lan`]).
    also_uninvited: Vec<String>,
    /// What a Remove leaves behind does not read at all, so the station grants nothing on a proof
    /// and the LAN takes no paired computer, until Reset network identity writes it afresh.
    removals_unreadable: bool,
}
/// A device the station granted on its own proof: what the notice says of it.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct Added {
    device_id: String,
    name: String,
    kind: Kind,
    platform: Option<Platform>,
    /// Its pin's fingerprint, shown shortened.
    key: Option<String>,
    role: tempo_client::account::Role,
    /// Someone the station invited, not its owner.
    guest: bool,
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
    /// The browser's device key (A5): its SPKI as lowercase hex. Listed only to a Nexus that asks
    /// (`transport::Client::post` does), and never shown: the desktop is given `key`.
    #[serde(default, skip_serializing)]
    public_key: Option<String>,
    /// SHA-256 of that key as lowercase hex: shown, shortened, beside the browser's name, and what
    /// approving it pins. Worked out here from the listed key, never taken from the service.
    #[serde(skip_deserializing)]
    key: Option<String>,
    /// What it is. Listed only to a Nexus that asks (`transport::Client::post` does); a service
    /// without kinds lists browsers alone, so a device it names no kind for is a browser.
    #[serde(default)]
    kind: Kind,
    /// The platform a native device runs on, one its kind may name ([`Kind::fits`]); none for a
    /// browser.
    #[serde(default)]
    platform: Option<Platform>,
    /// 1 for an approval that lasts until it is removed (the operator's decision of 2026-10-10: every
    /// approval since), whose `expires_at` is then an end no clock reaches. Listed only to a Nexus that
    /// asks for kinds; 0 from a service before that rule, whose approvals keep their listed end. Shown,
    /// never decided on: every list is still held to `expires_at`.
    #[serde(default)]
    until_removed: u8,
    /// What the device may do: listed only to a Nexus that checks proofs itself
    /// (`transport::Client::post` asks), as are the three fields after it. None from a service
    /// before roles: every device it lists operates.
    #[serde(default)]
    role: Option<tempo_client::account::Role>,
    /// How it was added: at the station (`shack`), or on its own proof (`account`).
    #[serde(default)]
    added_by: Option<String>,
    /// The proof it registered with: the issuer's ID token for exactly its keys. Checked here
    /// (`account::vouch`) before anything is granted on it, and never shown.
    #[serde(default, skip_serializing)]
    proof: Option<String>,
    /// A computer's LAN key, SPKI as lowercase hex, which its proof names beside its own key.
    #[serde(default, skip_serializing)]
    lan_key: Option<String>,
    /// 1 when the service has a durable owner-notice record for this device (the 2026-10-23 sign-in
    /// decision): the precondition a sign-in device's transmit hold lifts against. Listed only to a
    /// Nexus that checks proofs itself. 0 (the default) from a service before notices, or for a
    /// device whose notice row is not yet written — which holds the device FAIL-CLOSED.
    #[serde(default)]
    notified: u8,
}
/// A device's kind, held to the service's own values: a browser, or Nexus itself on another
/// computer or a phone (the native client). Approving a native device grants nothing on the
/// relay's lanes: it pins its key, which the tunnel through the room checks, and that is all.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    #[default]
    Browser,
    Computer,
    Phone,
}
/// The platforms a native device may name, held to the service's own values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Platform {
    Windows,
    Macos,
    Linux,
    Ios,
    Android,
}
impl Kind {
    /// May a device of this kind name this platform? A browser names none, a computer Windows,
    /// macOS or Linux, and a phone iOS or Android, as the service registers them.
    fn fits(self, platform: Option<Platform>) -> bool {
        matches!(
            (self, platform),
            (Kind::Browser, None)
                | (
                    Kind::Computer,
                    Some(Platform::Windows | Platform::Macos | Platform::Linux)
                )
                | (Kind::Phone, Some(Platform::Ios | Platform::Android))
        )
    }
}
/// The station's key (S3-M1), and the same key as TLS presents it for the room's tunnel: both, or
/// neither, but for a key TLS would not take, which then signs answers alone.
type StationKeys = (
    Option<Arc<station_key::Signer>>,
    Option<Arc<lan::tls::Identity>>,
);
/// The station's key as the room's tunnel proves it, for the pairing's station.
fn tunnel_identity(pkcs8: &str, binding: &Binding) -> Option<Arc<lan::tls::Identity>> {
    lan::tls::Identity::new(pkcs8, binding.station_id.clone()).map(Arc::new)
}
/// SHA-256 of a device key's SPKI, lowercase hex in and out: its pin (A5). The station's own key is
/// shown the same way (S3-L1), and so is a computer's key for its internet road on that computer
/// (`lan_client::internet::shown_key`). `None` for anything that is not the shape of a P-256 key.
pub(crate) fn fingerprint(public_key: &str) -> Option<String> {
    if !tempo_stream::protocol::device_key(public_key) {
        return None;
    }
    let spki = tempo_stream::protocol::hex_bytes(public_key)?;
    Some(
        ring::digest::digest(&ring::digest::SHA256, &spki)
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
/// A pin as stored: 32 bytes of SHA-256 as lowercase hex.
fn pin_shape(key: &str) -> bool {
    key.len() == 64 && key.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}
/// What a computer's (or phone's) pin begins with.
const NATIVE_PIN_LABEL: &[u8] = b"nexus-native-pin/1";
/// The pin approving a computer or a phone makes: SHA-256 over [`NATIVE_PIN_LABEL`], the approval's
/// generation and its key's fingerprint (`key`, lowercase hex), as lowercase hex. Never a browser's
/// pin, which is the fingerprint itself, so a browser's approval admits nothing through the room's
/// tunnel and a computer's admits nothing on the relay's lanes; and never another approval's, so a
/// computer the service lists as approved again, under an approval the shack did not give, is not
/// taken on the pin made for the one before. A renewal on use keeps the generation, and the pin.
fn native_pin(key: &str, generation: u64) -> Option<String> {
    let fingerprint = tempo_stream::protocol::hex_bytes(key).filter(|f| f.len() == 32)?;
    let mut hash = ring::digest::Context::new(&ring::digest::SHA256);
    hash.update(NATIVE_PIN_LABEL);
    hash.update(&generation.to_be_bytes());
    hash.update(&fingerprint);
    Some(
        hash.finish()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    )
}
/// An id the service made for a device: the shape every id has, at version 4 (the service makes them
/// with `crypto.randomUUID`). The shack's own names for computers are version 8 (`lan::device_id`,
/// `lan::tunnel_device_id`), so no device the service lists can stand for one of them in the
/// authority they share, or take its grants.
fn hosted(id: &str) -> bool {
    identifier(id) && id.as_bytes()[14] == b'4'
}
impl Device {
    fn approval(&self) -> Approval {
        Approval {
            expires_at: self.expires_at,
            generation: self.generation,
        }
    }
    /// The pin approving it as it is listed makes: a browser's key's fingerprint, or a computer's or
    /// phone's [`native_pin`]. `None` with no key, and for a computer or phone with no approval
    /// generation, which pins nothing.
    /// What its proof is checked against (`account::vouch_listed`): its kind, its keys, and the
    /// proof itself.
    fn proof_row(&self) -> serde_json::Value {
        json!({"kind": self.kind, "publicKey": self.public_key, "lanKey": self.lan_key,
            "proof": self.proof})
    }
    fn pin(&self) -> Option<String> {
        let key = self.key.as_deref()?;
        match self.kind {
            Kind::Browser => Some(key.to_string()),
            Kind::Computer | Kind::Phone => native_pin(key, self.generation?),
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
    /// Approve a device the service lists, at the shack, or remove it (`approve: false`): removed,
    /// it is gone on every road the station can link it to ([`Control::roads`]).
    Device {
        #[serde(rename = "deviceId")]
        device_id: String,
        approve: bool,
        /// A5: the device key the operator was shown beside this browser (its SHA-256, the device
        /// list's `key`). Pinned only if the service still lists that key once it has approved.
        #[serde(default)]
        key: Option<String>,
        /// The kind the operator was shown it as: a browser, a computer or a phone. Approving grants
        /// and pins only for a device the service still lists as that kind once it has approved; with
        /// none shown, it grants and pins nothing. Ignored when revoking.
        #[serde(default)]
        kind: Option<Kind>,
    },
    /// Remote over this network on, at the shack (`lan`): on `address`, this computer's private
    /// IPv4 address the operator picked, or with none as `lan` chooses; on `port`, or as it was.
    LanOn {
        #[serde(default)]
        address: Option<String>,
        #[serde(default)]
        port: Option<u16>,
    },
    /// Remote over this network off, at the shack: the port closes and every LAN session ends.
    LanOff {},
    /// Where Remote over this network listens, picked at the shack: one of this computer's private
    /// IPv4 addresses, or with none as `lan` chooses. Kept, on or off.
    LanAddress {
        #[serde(default)]
        address: Option<String>,
    },
    /// Pair a computer over this network, at the shack: the pairing window opens with a new code,
    /// and this press is the approval (as ruled on 2026-10-04, "One press").
    LanPair {},
    /// Close the pairing window early.
    LanCancel {},
    /// Remove a paired computer, at the shack: out at once, its station control and its session
    /// with it, and gone on its internet road too where the station can link the two
    /// ([`Control::roads`]).
    LanRevoke {
        #[serde(rename = "deviceId")]
        device_id: String,
    },
    /// Reset this station's network identity, at the shack: a new LAN key, and every paired
    /// computer removed, each on every road as Remove takes it.
    LanReset {},
    /// Sign in to Nexus Remote, at the shack: the station named `name` is bound to the account the
    /// operator signs in to through the system browser, with no code (`account`). Remote turns on
    /// once it is bound, as approving a pairing turns it on.
    SignIn {
        name: String,
    },
    /// Sign in again, at the shack, for a station bound already: its proof is replaced at the
    /// service, so devices can check it again once the issuer has retired the key that signed the
    /// old one. Nothing else about the station changes.
    SignInAgain {},
    /// End signing in again while the browser is still open: the station stays as it was, on or
    /// off (Cancel is a stop, and turns Remote off).
    CancelSignInAgain {},
    /// Invite someone to this station by their address, to operate it or only to listen: their
    /// devices are then approved when they sign in with that address, verified.
    Invite {
        email: String,
        role: tempo_client::account::Role,
    },
    /// Take an invitation back: none of that person's proofs is taken here from now on, and the
    /// service removes their devices.
    Uninvite {
        email: String,
    },
    /// The operator has seen the notice of a device the station granted on its own proof.
    DismissAdded {
        #[serde(rename = "deviceId")]
        device_id: String,
    },
    /// Allow to transmit, pressed at the shack for a device added by its own sign-in and held from
    /// transmit: the shack's waiver, as an Approve here gives it. ⛔ The shack's own screen only:
    /// `remote_station_action` is refused from afar at both ends (`command_classes`), and through
    /// a stream of the shack's own screen ([`Origin`]), so a held device can never allow itself.
    AllowTransmit {
        #[serde(rename = "deviceId")]
        device_id: String,
    },
}
/// ⛔ Where a press on the shack's Remote settings came from. A streamed operator drives the
/// shack's own window: their clicks and keys arrive as DOM events that the page's stream-input
/// module dispatches (`ui/src/remote-native/stream-input.ts`), so every one of their presses
/// reaches `remote_station_action` as a local call. The station cannot see a DOM dispatch, and a
/// streamed operator runs no code of their own on the page: the page's one entry to this command
/// (`remoteStationAction` in `ui/src/api.ts`) stamps each press with `isStreamInput()`, read
/// before anything awaits, which only that module's own dispatch makes true. The stamp is a
/// required argument, so a caller that does not give it is refused outright.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Origin {
    /// A press at the shack: the shack operator's own mouse or keyboard.
    Shack,
    /// A press a streamed operator made over the picture.
    Stream,
}
impl Action {
    /// ⛔ Whether this press may come through a stream: only a read. Every other action grants,
    /// removes or changes who may use the station, its transmit hold or Remote itself, and a
    /// device driving the station through a stream must never decide that for itself ("only the
    /// shack decides"). No wildcard: a new action is refused through a stream until it is placed.
    fn through_a_stream(&self) -> bool {
        match self {
            Action::Refresh {} => true,
            Action::StationPermission { .. }
            | Action::TakeOverLogging {}
            | Action::Begin { .. }
            | Action::Cancel {}
            | Action::Approve { .. }
            | Action::Enable {}
            | Action::Disable {}
            | Action::Forget {}
            | Action::Device { .. }
            | Action::LanOn { .. }
            | Action::LanOff {}
            | Action::LanAddress { .. }
            | Action::LanPair {}
            | Action::LanCancel {}
            | Action::LanRevoke { .. }
            | Action::LanReset {}
            | Action::SignIn { .. }
            | Action::SignInAgain {}
            | Action::CancelSignInAgain {}
            | Action::Invite { .. }
            | Action::Uninvite { .. }
            | Action::DismissAdded { .. }
            | Action::AllowTransmit { .. } => false,
        }
    }
}
type Reply = oneshot::Sender<Result<(), &'static str>>;
/// The most browsers a restart remembers grants for. The service lists at most eight per station,
/// and the record has to fit the smallest OS credential blob (Windows).
const MAX_REMEMBERED: usize = 8;
/// A decision a restart must remember. Operator decisions 2026-09-13: Remote remembers being on;
/// approving a browser grants it station control, kept for as long as that approval stands, until
/// revoked.
///
/// Every decision is applied to `Control::remembered` under the `Control` lock at the moment it is
/// made, so the record always holds the operator's decisions in the order they were made, and Turn
/// on reads exactly what has been decided. Writing it happens on the vault writer's own thread,
/// never under a lock and never on the path that revokes transmission, because an OS credential
/// store can block (a locked keychain can prompt).
enum Persist {
    /// The controller loaded or created this pairing; later decisions apply to it.
    Bound(
        Binding,
        Option<vault::State>,
        Option<Box<vault::Pins>>,
        Option<Box<vault::Waivers>>,
    ),
    /// Remote was turned on and is connecting.
    Enabled,
    /// Remote went off for a reason that ends access: Revoke station access, a cancelled pairing,
    /// or the connection refusing itself. The permissions it cleared are remembered as cleared.
    Off,
    /// Turn off Remote. It only pauses (operator decision 2026-09-14): every browser is
    /// disconnected and every live permission cleared, but the remembered grants stay, so Turn on
    /// or a restart restores them for browsers still approved at the same approval.
    Paused,
    /// A local decision cleared every browser permission: a browser's removal that could not be
    /// applied to that browser alone (see `Control::revoke_browser`).
    ClearGrants,
    /// One browser's grants changed. `approval` is the approval the change was made against: a grant
    /// is only ever created against a known approval, and a grant against a different approval (not
    /// merely a renewed one) starts that browser afresh. A `None` field is left as it was.
    Browser {
        device_id: String,
        approval: Option<Approval>,
        control: Option<bool>,
    },
    /// The approvals the service lists right now. A grant remembered against any other approval
    /// (a browser revoked, re-approved, expired or gone) is forgotten, as is one without station
    /// control; the rest are rebound to the approval as listed. So is a pin for a browser no longer
    /// approved.
    Approvals(Vec<(String, Approval)>),
    /// The pairing was removed.
    Removed,
    /// A5: the operator approved this browser at the radio with its device key shown (`Some`: the
    /// key's SHA-256, now pinned; for a computer or phone, its [`native_pin`]), or the approval
    /// pinned nothing, or the browser was revoked (`None`: no pin).
    Pin {
        device_id: String,
        key: Option<String>,
    },
    /// The browsers the service lists as approved right now: a pin for any other is forgotten.
    Listed(Vec<String>),
    /// The operator waived this sign-in device's transmit hold at the shack (Approve, or Allow to
    /// transmit). Kept for a restart until the device is revoked, removed or no longer listed.
    Waive { device_id: String },
    /// The devices the service lists now as added by their own sign-in, approved and unexpired,
    /// less any removed here and not yet told to the service: a waiver for any other is forgotten.
    SignedIn(Vec<String>),
    /// This sign-in device was granted again on its own sign-in, held for its window
    /// (`Controller::grant_vouched`): a waiver kept for it is forgotten, as the one in memory is.
    Hold { device_id: String },
}
/// What the vault must hold next. Each write is a whole record, so only the latest of each matters.
enum Write {
    Save(vault::State),
    Remove,
    /// The pins' own entry (A5).
    SavePins(vault::Pins),
    RemovePins,
    /// The shack's waivers' own entry.
    SaveWaivers(vault::Waivers),
    RemoveWaivers,
}
/// The record for the current pairing, kept in step with every decision.
#[derive(Default)]
struct Remembered {
    binding: Option<Binding>,
    state: Option<vault::State>,
    /// A5: browser id to the SHA-256 of the device key the operator pinned for it (a computer's or
    /// phone's: its [`native_pin`]), kept in the vault's pins entry. A pin is an approval's, not a
    /// permission's: the switches, take over and Turn off leave it; approving at the radio sets it
    /// and revoking the browser removes it.
    pins: BTreeMap<String, String>,
    /// The sign-in devices whose transmit hold the operator waived at the shack, kept in the vault's
    /// waivers entry. Only an explicit waiver adds one; whatever ends the approval it came with takes
    /// it out: a revoke, a removal, a list that no longer names the device as a sign-in device
    /// (expired, replaced, removed at the service), a grant on its own sign-in again, a pairing
    /// removed or replaced, and an off that ends access. Turn off and take over only pause, and leave
    /// it, as they leave the grants. Kept only beside the station control remembered for its device,
    /// so whatever ends that grant ends it too.
    waivers: std::collections::BTreeSet<String>,
}
impl Remembered {
    /// Apply one decision. Returns the writes it calls for: at most one for each entry, and none
    /// when nothing changed.
    fn apply(&mut self, decision: Persist) -> Vec<Write> {
        let before = self.waivers.clone();
        // A load, or a pairing gone, writes the waivers as kept whatever the vault held: none that
        // was not restored is left there for a later launch to read.
        let rewrite = matches!(decision, Persist::Bound(..) | Persist::Removed);
        let pins = self.apply_pins(&decision);
        self.apply_waivers(&decision);
        let state = self.apply_state(decision);
        let waivers = self.keep_waivers(&before, rewrite);
        // The waivers first, as the writer writes them, whether or not it takes them in one batch.
        waivers.into_iter().chain(state).chain(pins).collect()
    }
    fn apply_waivers(&mut self, decision: &Persist) {
        match decision {
            Persist::Bound(bound, _, _, waivers) => {
                self.waivers = waivers
                    .as_ref()
                    .filter(|w| w.binding == *bound)
                    .map(|w| w.devices.clone())
                    .unwrap_or_default();
            }
            Persist::Removed | Persist::Off | Persist::ClearGrants => self.waivers.clear(),
            Persist::Browser {
                device_id,
                control: Some(false),
                ..
            }
            | Persist::Hold { device_id } => {
                self.waivers.remove(device_id);
            }
            // At most what the service lists for a station: past that, a waiver is not kept, and the
            // device is held after a restart.
            Persist::Waive { device_id } if self.waivers.len() < MAX_REMEMBERED => {
                self.waivers.insert(device_id.clone());
            }
            Persist::Listed(approved) | Persist::SignedIn(approved) => {
                self.waivers.retain(|id| approved.contains(id))
            }
            Persist::Approvals(approvals) => self
                .waivers
                .retain(|id| approvals.iter().any(|(listed, _)| listed == id)),
            _ => {}
        }
    }
    /// After the state: a waiver stays only beside a remembered grant of station control for its
    /// device. Returns the write the waivers call for: none when nothing changed, unless `rewrite`.
    fn keep_waivers(
        &mut self,
        before: &std::collections::BTreeSet<String>,
        rewrite: bool,
    ) -> Option<Write> {
        let grants = self.state.as_ref().map_or(&[][..], |s| s.grants.as_slice());
        self.waivers
            .retain(|id| grants.iter().any(|g| g.device_id == *id && g.control));
        if self.waivers == *before && !rewrite {
            return None;
        }
        // Needs no pairing: a pairing removed still removes its waivers.
        if self.waivers.is_empty() {
            return Some(Write::RemoveWaivers);
        }
        Some(Write::SaveWaivers(vault::Waivers {
            binding: self.binding.clone()?,
            devices: self.waivers.clone(),
        }))
    }
    fn apply_pins(&mut self, decision: &Persist) -> Option<Write> {
        let before = self.pins.clone();
        match decision {
            Persist::Bound(bound, _, pins, _) => {
                self.pins = pins
                    .as_ref()
                    .filter(|p| p.binding == *bound)
                    .map(|p| p.keys.clone())
                    .unwrap_or_default();
                return None;
            }
            Persist::Removed => {
                self.pins.clear();
                return Some(Write::RemovePins);
            }
            Persist::Off => self.pins.clear(),
            Persist::Pin { device_id, key } => match key {
                Some(key) => {
                    self.pins.insert(device_id.clone(), key.clone());
                }
                None => {
                    self.pins.remove(device_id);
                }
            },
            Persist::Listed(approved) => self.pins.retain(|id, _| approved.contains(id)),
            Persist::Approvals(approvals) => self
                .pins
                .retain(|id, _| approvals.iter().any(|(listed, _)| listed == id)),
            _ => return None,
        }
        if self.pins == before {
            return None;
        }
        let binding = self.binding.clone()?;
        Some(if self.pins.is_empty() {
            Write::RemovePins
        } else {
            Write::SavePins(vault::Pins {
                binding,
                keys: self.pins.clone(),
            })
        })
    }
    fn apply_state(&mut self, decision: Persist) -> Option<Write> {
        match decision {
            Persist::Bound(bound, state, ..) => {
                self.state = state.filter(|s| s.binding == bound);
                self.binding = Some(bound);
                return None;
            }
            Persist::Removed => {
                self.binding = None;
                self.state = None;
                return Some(Write::Remove);
            }
            Persist::Pin { .. }
            | Persist::Listed(_)
            | Persist::Waive { .. }
            | Persist::SignedIn(_)
            | Persist::Hold { .. } => return None,
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
                control,
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
                        control: false,
                        retired_logging: Default::default(),
                        retired_transmit: Default::default(),
                    },
                    (Some(grant), None) => grant,
                    // Nothing remembered for this browser, and no approval to bind a grant to.
                    (None, None) => return None,
                };
                grant.control = control.unwrap_or(grant.control);
                let keep = grant.control;
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
                // A grant holding nothing (an older build's remote logging alone) goes too.
                listed.is_some() && g.control
            }),
            Persist::Bound(..)
            | Persist::Removed
            | Persist::Pin { .. }
            | Persist::Listed(_)
            | Persist::Waive { .. }
            | Persist::SignedIn(_)
            | Persist::Hold { .. } => return None,
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
    /// The device key pinned for this browser (A5), as the SHA-256 a stream offer's key must have.
    fn pinned(&self, device_id: &str) -> Option<[u8; 32]> {
        tempo_stream::protocol::hex_bytes(self.pins.get(device_id)?)?
            .try_into()
            .ok()
    }
}
#[derive(Default)]
struct Control {
    operations: Arc<operations::Authority>,
    enabled: bool,
    cancel: Option<watch::Sender<bool>>,
    generation: u64,
    vault_writer: Option<mpsc::UnboundedSender<Write>>,
    remembered: Remembered,
    /// This pairing's signing key (S3-M1), read from the vault or made there once. `None` while there
    /// is no pairing, or when the credential store would not give one up: the station's answers then
    /// go unsigned, and pages refuse them.
    station_key: Option<Arc<station_key::Signer>>,
    /// The service holds another key for this station (`stationKeyPinned`): pages will refuse its
    /// answers until it is paired again. Said at the shack.
    key_refused: bool,
    /// Remote over this network (`lan`), beside the relay's road and sharing its authority.
    /// Taken out of the lock to be used. `None` in a test build of the service.
    lan: Option<Arc<lan::Lan>>,
    /// The same key as `station_key`, as TLS presents it: what the room's tunnel proves itself
    /// with. Made from the same vault entry at the same moments.
    station_identity: Option<Arc<lan::tls::Identity>>,
    /// The computers approved here for the room's tunnel, kept in step with the pins and the
    /// service's list ([`Control::sync_approved`]).
    approved: Arc<tunnel::Approved>,
    /// The computers (and phones) the service lists as approved and not yet ended, by device id:
    /// what a pin is compared with.
    natives: BTreeMap<String, Native>,
    /// What the room's tunnel serves its computers with: the LAN road's authority, feeds and
    /// command lane. `None` in a build of the service without the LAN road.
    tunnel: Option<lan::TunnelDeps>,
    /// Hosted Remote is on: `enabled`, mirrored where the operations authority can read it under its
    /// own lock, for the tunnel's computers' station control (`Authority::hold_tunnel_devices`).
    /// Cleared before every stop's invalidation, set only by [`Control::start`].
    hosted: Arc<std::sync::atomic::AtomicBool>,
    /// What a computer enrolling on the LAN by its own sign-in is checked against, armed by the
    /// controller while the station is bound to its owner ([`Controller::arm_enrolment`]).
    enrolment: Arc<lan::enrol::Enrolment>,
}
impl Control {
    fn stop(&mut self) {
        // Before the invalidation, so the clear it makes is never undone by the tunnel's holder.
        self.hosted
            .store(false, std::sync::atomic::Ordering::SeqCst);
        self.operations.invalidate();
        self.enabled = false;
        self.generation = self.generation.wrapping_add(1);
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(true);
        }
    }
    /// Turn hosted Remote on: a fresh authority (every permission in memory cleared), then on. The
    /// tunnel's approved computers hold station control again from here.
    fn start(&mut self) {
        self.stop();
        self.enabled = true;
        self.hosted.store(true, std::sync::atomic::Ordering::SeqCst);
    }
    /// Removing one device's approval at the shack ends that device's grants at once (a lease it
    /// holds with them), remembered and live, and its pin; every other device keeps its own
    /// (operator decision 2026-10-14). A revoke never waits for Core. Should one not be taken, the
    /// whole authority is cleared instead, as it was before: fail closed.
    fn revoke_browser(&mut self, device_id: &str) {
        let operations = self.operations.clone();
        let taken = operations.permit_station(device_id, false).is_ok();
        if taken {
            self.remember(Persist::Browser {
                device_id: device_id.to_string(),
                approval: None,
                control: Some(false),
            });
        } else {
            operations.invalidate();
            self.remember(Persist::ClearGrants);
        }
        // A5: revoking ends the approval, and its pin with it.
        self.remember(Persist::Pin {
            device_id: device_id.to_string(),
            key: None,
        });
    }
    /// Hand a decision to the vault writer. Never called from `stop`: `stop` also runs when Nexus
    /// exits, and an exit must not be remembered as the operator turning Remote off.
    fn remember(&mut self, decision: Persist) {
        for write in self.remembered.apply(decision) {
            if let Some(writer) = &self.vault_writer {
                let _ = writer.send(write);
            }
        }
        self.sync_approved();
    }
    /// The computers the service lists as approved and not yet ended (`devices`): who the room's
    /// tunnel takes, with the pins.
    fn listed(&mut self, devices: &[Device]) {
        let now = now_ms();
        self.natives = devices
            .iter()
            .filter(|d| d.kind != Kind::Browser && d.approved == 1 && d.expires_at > now)
            .filter_map(|d| {
                let native = Native {
                    pin: d.pin()?,
                    key: d.key.clone()?,
                    until: d.expires_at,
                };
                Some((d.id.clone(), native))
            })
            .collect();
        self.sync_approved();
    }
    /// Who the room's tunnel takes, until when: each computer the service lists as approved whose
    /// pin here is the one approving it as listed makes ([`Device::pin`]: its key, under this same
    /// approval, approved as a computer). Never a browser, whatever its pin. Each is named from the
    /// key pinned for it (`lan::tunnel_device_id`), never by the service's id, so it can stand for
    /// no other device in the authority it shares with the relay's road and the LAN's.
    ///
    /// The transmit hold is told the service's ids for each name first (`set_tunnel_owners`), so a
    /// name the tunnel takes is never one the hold cannot place; a name that leaves it is one the
    /// hold no longer places, and so held, until its revoke lands. Two listed devices that share a
    /// key share its name, and the hold keeps both ids for it.
    fn sync_approved(&self) {
        let (mut keys, mut owners) = (BTreeMap::new(), BTreeMap::new());
        for (id, pin, name, until) in self.on_tunnel() {
            owners
                .entry(name.clone())
                .or_insert_with(std::collections::BTreeSet::new)
                .insert(id.clone());
            keys.insert(pin, (name, until));
        }
        self.operations.set_tunnel_owners(owners);
        // A computer no longer on it loses station control at once, after it is off the holder.
        for name in self.approved.set(keys) {
            let _ = self.operations.permit_station(&name, false);
        }
    }
    /// Each computer the room's tunnel takes ([`Control::sync_approved`]), by the service's id: the
    /// key pinned for it, the name the tunnel takes it as, and when its approval ends.
    fn on_tunnel(&self) -> impl Iterator<Item = (&String, [u8; 32], String, u64)> + '_ {
        self.natives
            .iter()
            .filter(|(id, native)| self.remembered.pins.get(*id) == Some(&native.pin))
            .filter_map(|(id, native)| {
                let pin: [u8; 32] = tempo_stream::protocol::hex_bytes(&native.key)?
                    .try_into()
                    .ok()?;
                Some((id, pin, lan::tunnel_device_id(&pin), native.until))
            })
    }
    /// Whether `device_id` holds station control in `granted` (the authority's `controlDevices`):
    /// under the service's id, or, for a computer on the room's tunnel, under its tunnel name.
    fn holds_control(&self, device_id: &str, granted: &[String]) -> bool {
        granted.iter().any(|g| g == device_id)
            || self
                .on_tunnel()
                .any(|(id, _, name, _)| id == device_id && granted.contains(&name))
    }
}
/// Where a Remove at the shack was pressed: on a device the service lists, by its id, or on a
/// computer in the LAN book, by its LAN id.
#[derive(Clone, Copy)]
enum Road<'a> {
    Listed(&'a str),
    Lan(&'a str),
}
/// Every road one device takes to this station, as [`Control::roads`] links them: what a Remove at
/// the shack ends, all of them at once.
#[derive(Default)]
struct Roads {
    /// Its ids in the service's list: each its station control, its pin, its waiver and its place
    /// on the room's tunnel, and its listing at the service.
    listed: std::collections::BTreeSet<String>,
    /// Its ids in the LAN book.
    lan: std::collections::BTreeSet<String>,
    /// The keys it holds them by, as far as the station knows them (lowercase hex SHA-256: a LAN
    /// key's pin, an internet key's fingerprint): kept as removed before any road ends
    /// (`lan::enrol::Enrolment::remove`), so none comes back on a proof signed before.
    keys: std::collections::BTreeSet<String>,
    /// The other list may hold it under a link nothing the station keeps: a listed computer naming
    /// its LAN key with no link kept, one of the same name, or no list heard to tell. Left as it
    /// is, and the shack told to look there too.
    unconfirmed: bool,
}
impl Roads {
    fn extend(&mut self, other: Roads) {
        self.listed.extend(other.listed);
        self.lan.extend(other.lan);
        self.keys.extend(other.keys);
        self.unconfirmed |= other.unconfirmed;
    }
}
/// A computer in the LAN book: its id, its name and its key's pin (lowercase hex).
struct Paired {
    id: String,
    name: String,
    pin: String,
}
impl Control {
    /// The roads of the device a Remove was pressed on (`start`): linked only through what the
    /// station keeps. A listed computer and a computer in the LAN `book` are one when a link kept
    /// for the book's LAN key names the internet key it is listed with (`lan::enrol::linked`: kept
    /// when the station first checked the proof that named both, never a guest's claim alone); two
    /// listed ids are one computer when the room's tunnel takes both by one name
    /// ([`Control::on_tunnel`]: one key, pinned here for each). `devices` is the service's last
    /// list, `None` when there is none to go by; the computers it lists approved are known here too
    /// ([`Control::natives`]). A key the list names with no link kept links nothing.
    fn roads(&self, devices: Option<&[Device]>, book: &[Paired], start: Road) -> Roads {
        use lan::enrol::short;
        let listed = devices.unwrap_or_default();
        // Listed as approved now: a listing that reaches the station, the only kind worth a word.
        let now = now_ms();
        let live = |d: &Device| d.approved == 1 && d.expires_at > now;
        // Every listed id known here with its key's fingerprint: the list's, and the computers the
        // station holds as listed.
        // A browser's never reach the LAN, so none is kept as removed for it: its pin, its listing
        // and the removals waiting for the service end it.
        let mut keyed: Vec<(String, String)> = listed
            .iter()
            .filter(|d| d.kind != Kind::Browser)
            .filter_map(|d| Some((d.id.clone(), d.key.clone()?)))
            .collect();
        keyed.extend(
            self.natives
                .iter()
                .map(|(id, native)| (id.clone(), native.key.clone())),
        );
        // (listed id, LAN id), linked by a link kept or not.
        let ties = self.enrolment.ties();
        let mut proven = Vec::new();
        for computer in book {
            let lan = short(&computer.pin);
            for tie in ties
                .iter()
                .filter(|t| t.lan == lan && lan::enrol::linked(t))
            {
                for (id, _) in keyed.iter().filter(|(_, key)| short(key) == tie.key) {
                    proven.push((id.clone(), computer.id.clone()));
                }
            }
        }
        let mut unproven = Vec::new();
        for d in listed.iter().filter(|d| d.kind != Kind::Browser && live(d)) {
            let Some(lan_id) = d
                .lan_key
                .as_deref()
                .and_then(fingerprint)
                .and_then(|pin| book.iter().find(|c| c.pin == pin))
                .map(|c| c.id.clone())
            else {
                continue;
            };
            if !proven.contains(&(d.id.clone(), lan_id.clone())) {
                unproven.push((d.id.clone(), lan_id));
            }
        }
        let tunnel: Vec<(String, String)> = self
            .on_tunnel()
            .map(|(id, _, name, _)| (id.clone(), name))
            .collect();
        let mut roads = Roads::default();
        match start {
            Road::Listed(id) => roads.listed.insert(id.to_string()),
            Road::Lan(id) => roads.lan.insert(id.to_string()),
        };
        loop {
            let before = roads.listed.len() + roads.lan.len();
            for (listed, lan) in &proven {
                if roads.listed.contains(listed) || roads.lan.contains(lan) {
                    roads.listed.insert(listed.clone());
                    roads.lan.insert(lan.clone());
                }
            }
            for (id, name) in &tunnel {
                if tunnel
                    .iter()
                    .any(|(other, same)| same == name && roads.listed.contains(other))
                {
                    roads.listed.insert(id.clone());
                }
            }
            if roads.listed.len() + roads.lan.len() == before {
                break;
            }
        }
        roads.keys = keyed
            .iter()
            .filter(|(id, _)| roads.listed.contains(id))
            .map(|(_, key)| key.clone())
            .chain(
                book.iter()
                    .filter(|c| roads.lan.contains(&c.id))
                    .map(|c| c.pin.clone()),
            )
            .collect();
        // And the internet key each computer taken out of the book is linked to, listed or not: its
        // listing may be one this station has no list to find yet.
        for computer in book.iter().filter(|c| roads.lan.contains(&c.id)) {
            let lan = short(&computer.pin);
            roads.keys.extend(
                ties.iter()
                    .filter(|t| t.lan == lan && lan::enrol::linked(t))
                    .map(|t| t.key.clone()),
            );
        }
        // Said, never acted on: no list to go by, a link none was kept for, and a computer of the
        // same name in the other list.
        let names = |ids: &std::collections::BTreeSet<String>, lan: bool| -> Vec<String> {
            ids.iter()
                .filter_map(|id| match lan {
                    true => book.iter().find(|c| c.id == *id).map(|c| c.name.clone()),
                    false => listed
                        .iter()
                        .find(|d| d.id == *id && d.kind != Kind::Browser && live(d))
                        .map(|d| d.name.clone()),
                })
                .collect()
        };
        let (listed_names, lan_names) = (names(&roads.listed, false), names(&roads.lan, true));
        roads.unconfirmed = devices.is_none()
            || unproven
                .iter()
                .any(|(listed, lan)| roads.listed.contains(listed) != roads.lan.contains(lan))
            || listed.iter().any(|d| {
                d.kind != Kind::Browser
                    && live(d)
                    && !roads.listed.contains(&d.id)
                    && lan_names.contains(&d.name)
            })
            || book
                .iter()
                .any(|c| !roads.lan.contains(&c.id) && listed_names.contains(&c.name));
        roads
    }
    /// DISPLAY ONLY: the shack's Devices list, as `status` lists the devices and the LAN `book`
    /// holds them, at `now`. One row per device: the ids a Remove pressed on any of them would end
    /// together ([`Control::roads`], so the row is exactly what its Remove takes). Whose it is: an
    /// invited person's where a proof of theirs held here (`status.whose`) or their sign-in on this
    /// network enrolled it (`lan::enrol::Enrolment::guests`); not known for one added by a sign-in
    /// the station has not told; else the owner's (a device added at the shack, or paired by code).
    fn device_rows(
        &self,
        status: &Status,
        book: &[Paired],
        now: std::time::Instant,
    ) -> Vec<DeviceRow> {
        let devices = &status.devices;
        let people = &status.account.people;
        let theirs: Vec<(String, Vec<String>)> = people
            .iter()
            .map(|p| (p.mailbox.clone(), self.enrolment.guests(&p.mailbox).0))
            .collect();
        // The guests' computers kept with no link to say whose: the same whoever is asked about.
        let (_, nobodys) = self.enrolment.guests("");
        // The held devices' transmit windows, from one look at the grants for the whole list.
        let holds = self.operations.hold_display(&status.held_devices, now);
        let starts = devices
            .iter()
            .map(|d| Road::Listed(&d.id))
            .chain(book.iter().map(|c| Road::Lan(&c.id)));
        let mut rows: Vec<DeviceRow> = Vec::new();
        for start in starts {
            let placed = rows.iter().any(|row| match start {
                Road::Listed(id) => row.listed.iter().any(|l| l == id),
                Road::Lan(id) => row.lan.iter().any(|l| l == id),
            });
            if placed {
                continue;
            }
            let roads = self.roads(Some(devices), book, start);
            let listed: Vec<&Device> = devices
                .iter()
                .filter(|d| roads.listed.contains(&d.id))
                .collect();
            let lan: Vec<&Paired> = book.iter().filter(|c| roads.lan.contains(&c.id)).collect();
            let (mut guest, mut owner, mut unknown) = (None, false, false);
            for d in &listed {
                match status.whose.get(&d.id) {
                    // A guest's only while they are invited: once their invitation is taken back the
                    // device is no longer shown as theirs, though the proof that named them is kept.
                    Some(Whose::Guest(mailbox)) if people.iter().any(|p| p.mailbox == *mailbox) => {
                        guest = guest.or(Some(mailbox.clone()))
                    }
                    Some(Whose::Owner) => owner = true,
                    _ if d.added_by.as_deref() == Some("account") => unknown = true,
                    _ => {}
                }
            }
            for c in &lan {
                let pin = lan::enrol::short(&c.pin);
                match theirs.iter().find(|(_, lans)| lans.contains(&pin)) {
                    Some((mailbox, _)) => guest = guest.or(Some(mailbox.clone())),
                    None if nobodys.contains(&pin) => unknown = true,
                    None => {}
                }
            }
            let live = status.controller.as_deref().is_some_and(|holder| {
                lan.iter().any(|c| c.id == holder)
                    || listed
                        .iter()
                        .any(|d| self.holds_control(&d.id, &[holder.to_string()]))
            });
            let hold_left = listed
                .iter()
                .filter_map(|d| holds.get(&d.id).and_then(|hold| hold.left))
                .max()
                .map(|left| left.as_millis().div_ceil(1000) as u64);
            rows.push(DeviceRow {
                listed: listed.iter().map(|d| d.id.clone()).collect(),
                lan: lan.iter().map(|c| c.id.clone()).collect(),
                unknown: guest.is_none() && !owner && unknown,
                guest,
                live,
                hold_left,
            });
        }
        rows
    }
}
/// Whether `device`'s key is one of those `removed` at the shack.
fn removed_key(removed: &[vault::Removed], device: &Device) -> bool {
    device.key.as_deref().is_some_and(|key| {
        let key = lan::enrol::short(key);
        removed.iter().any(|r| r.key == key)
    })
}
/// A computer (or phone) the service lists as approved: the pin approving it as listed makes, the
/// fingerprint of the key listed for it, and when its approval ends.
struct Native {
    pin: String,
    key: String,
    until: u64,
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
/// The wall clock in seconds, as proofs are read.
fn now_s() -> u64 {
    now_ms() / 1000
}
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
/// An address one could invite: something before and after an `@`, no space, at most 254 characters.
fn invitable(address: &str) -> bool {
    address.len() <= 254
        && !address.chars().any(|c| c.is_whitespace() || c.is_control())
        && address
            .find('@')
            .is_some_and(|at| at > 0 && at + 1 < address.len())
}
fn valid_name(name: &str) -> bool {
    !name.trim().is_empty() && name.chars().count() <= 48 && !name.chars().any(char::is_control)
}
impl Service {
    pub fn new(
        engine: crate::SharedEngine,
        publisher: crate::remote_monitor::Publisher,
        // `audio`: the station's bounded receive-audio copy, for a listening browser.
        // `None` leaves the audio lane unadvertised, so nothing is ever offered it.
        #[cfg(feature = "radio")] audio: Option<Arc<tempo_audio::receive_audio::ReceiveAudioFeed>>,
        // What a streamed session needs from the application: the window its input goes to.
        stream: stream::Host,
        // How a paired computer's calls run at the shack, and the events that cross to it.
        commands: lan::commands::Commands,
        // Opens the system browser, for signing in to Nexus Remote at the shack.
        open: account::Opener,
    ) -> Self {
        tempo_app::engine::engine_lock(&engine)
            .configure_remote_settings_store(crate::settings_path());
        let feeds = transport::Feeds {
            monitor: publisher,
            // One encoder for the station, whoever listens: the Listen lane and every stream.
            #[cfg(feature = "radio")]
            audio: audio.map(audio::ReceiveFanout::new),
            stream,
        };
        let service = Self::start(
            REMOTE_ORIGIN.to_string(),
            Box::new(SystemVault),
            engine.clone(),
            feeds.clone(),
            Accounts::system(open),
        );
        // Its LAN key and paired computers are in the OS credential store, read on its own thread.
        service.with_lan(
            crate::settings_path().with_file_name("remote-lan.json"),
            engine,
            feeds,
            Arc::new(lan::Book::new(Arc::new(SystemVault))),
            Arc::new(tempo_stream::lan::look),
            commands,
        )
    }
    /// This station's key for the internet road as it stands when asked: the public half of the
    /// pairing's signing key, or none without a pairing. Only the public half leaves the lock.
    fn station_key_of(&self) -> lan::StationKeyOf {
        let control = self.control.clone();
        Arc::new(move || {
            let control = control.lock().ok()?;
            Some(control.station_key.as_ref()?.public_key().to_string())
        })
    }
    /// Remote over this network, beside the relay's road and sharing its one authority: its switch
    /// kept at `path`, its key and paired computers in `book`, and a paired computer's calls run
    /// with `commands`.
    fn with_lan(
        self,
        path: std::path::PathBuf,
        engine: crate::SharedEngine,
        feeds: transport::Feeds,
        book: Arc<lan::Book>,
        resolve: lan::Resolve,
        commands: lan::commands::Commands,
    ) -> Self {
        let held = self
            .control
            .lock()
            .ok()
            .map(|c| (c.operations.clone(), c.enrolment.clone()));
        if let Some((authority, enrolment)) = held {
            let dispatcher =
                crate::command_dispatch::Dispatcher::new(crate::command_dispatch::ShackStores {
                    engine: engine.clone(),
                });
            // The room's tunnel serves its computers with the same authority, feeds and command
            // lane: one lease for the station, and one revision of each store, whichever road a
            // call comes by. Its computers are named by their own keys (`Control::sync_approved`).
            let tunnel = lan::TunnelDeps {
                authority: authority.clone(),
                engine: engine.clone(),
                feeds: feeds.clone(),
                dispatcher: dispatcher.clone(),
                commands: commands.clone(),
            };
            let lan = Arc::new(lan::Lan::start(
                path,
                lan::Deps {
                    authority,
                    engine,
                    feeds,
                    book,
                    enrolment,
                    resolve,
                    advertise: lan::advertise(),
                    firewall: lan::firewall_says(),
                    dispatcher,
                    commands,
                    station_key: self.station_key_of(),
                },
            ));
            if let Ok(mut control) = self.control.lock() {
                control.lan = Some(lan);
                control.tunnel = Some(tunnel);
            }
        }
        self
    }
    #[cfg(test)]
    fn configured(
        origin: String,
        vault: Box<dyn Vault>,
        engine: crate::SharedEngine,
        publisher: crate::remote_monitor::Publisher,
    ) -> Self {
        Self::with_accounts(origin, vault, engine, publisher, Accounts::not_ready())
    }
    #[cfg(test)]
    fn with_accounts(
        origin: String,
        vault: Box<dyn Vault>,
        engine: crate::SharedEngine,
        publisher: crate::remote_monitor::Publisher,
        accounts: Accounts,
    ) -> Self {
        Self::start(
            origin,
            vault,
            engine,
            transport::Feeds {
                monitor: publisher,
                #[cfg(feature = "radio")]
                audio: None,
                stream: stream::Host::default(),
            },
            accounts,
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
        mut accounts: Accounts,
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
        let operations = Arc::new(operations::Authority::default());
        // A stand-down at the shack retires remote transmit authority. Handing the engine this
        // revocation is what makes `Engine::halt_tx` — the verb every local stop funnels through,
        // including WSJT-X's UDP HaltTx — move the generation a browser carries as its
        // `transmitEpoch`, its Stop's token. See `Engine::halt_tx`.
        tempo_app::engine::engine_lock(&engine)
            .set_remote_transmit_revocation(operations.transmit_revocation());
        // A streamed operator's held PTT: the stream records the holds, the engine arms and
        // releases the microphone over on its own radio-loop tick (`engine/remote_stream.rs`).
        tempo_app::engine::engine_lock(&engine).set_remote_ptt_hold(feeds.stream.ptt.clone());
        // …and the page's microphone, which the stream decodes and the engine's microphone over
        // plays, only while that press has armed it (`engine/remote_mic.rs`).
        tempo_app::engine::engine_lock(&engine).set_remote_mic_feed(feeds.stream.mic.clone());
        // The sign-in transmit hold (the 2026-10-23 sign-in decision) is re-derived from the live
        // lease on every radio-loop tick, so it tracks the CURRENT controller: it lifts the instant
        // a held controller stops controlling by any path — Remote turned off, a disconnect, a
        // retired or expired lease — and holds a still-controlling device. A request's own
        // `apply_tx_hold` still sets it immediately on the request path; this is the backstop for
        // the departures that bring no further request.
        let hold_source = operations.clone();
        tempo_app::engine::engine_lock(&engine)
            .set_remote_tx_hold_source(Arc::new(move |now| hold_source.current_transmit_hold(now)));
        let control = Arc::new(Mutex::new(Control {
            operations,
            vault_writer,
            enrolment: Arc::new(lan::enrol::Enrolment::kept_in(
                accounts.vault.clone(),
                accounts.now_s,
            )),
            ..Default::default()
        }));
        // The tunnel's approved computers hold station control while hosted Remote is on, as the
        // LAN's paired computers do while LAN is on: read from the tunnel's roster and the mirror of
        // "on", each behind a lock of its own (never Control's, which is held while Core is taken).
        if let Ok(control) = control.lock() {
            let (approved, hosted) = (control.approved.clone(), control.hosted.clone());
            control.operations.hold_tunnel_devices(Box::new(move || {
                if hosted.load(std::sync::atomic::Ordering::SeqCst) {
                    approved.names()
                } else {
                    Vec::new()
                }
            }));
        }
        // A binding names the LAN's own key, once the LAN has started beside this service.
        if accounts.lan_key.is_none() {
            let held = Arc::downgrade(&control);
            accounts.lan_key = Some(Arc::new(move || {
                let lan = held.upgrade()?.lock().ok()?.lan.clone()?;
                lan.public_key()
            }));
        }
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
                            accounts,
                            signing: None,
                            staged: None,
                            removals: Some(Vec::new()),
                            uninvites: Some(Vec::new()),
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
        status.controller = operations["controller"].as_str().map(str::to_string);
        status.station_permissions =
            serde_json::from_value(operations["controlDevices"].clone()).unwrap_or_default();
        status.pinned_devices = status
            .devices
            .iter()
            .filter(|d| {
                d.pin()
                    .is_some_and(|pin| control.remembered.pins.get(&d.id) == Some(&pin))
            })
            .map(|d| d.id.clone())
            .collect();
        status.held_devices = status
            .devices
            .iter()
            .filter(|d| {
                allows_transmit(
                    &control,
                    d,
                    control.holds_control(&d.id, &status.station_permissions),
                    status.account.removals.contains(&d.id),
                    status.account.unconfirmed.contains_key(&d.id),
                )
            })
            .map(|d| d.id.clone())
            .collect();
        status.station_key = control
            .station_key
            .as_ref()
            .and_then(|key| fingerprint(key.public_key()));
        status.observation_generation = enabled.then(|| control.generation.to_string());
        status.key_refused = control.key_refused;
        status.account.removals_unreadable = control.enrolment.unreadable();
        status.lan = control.lan.as_deref().map(lan::Lan::status);
        let book: Vec<Paired> = status.lan.as_ref().map_or_else(Vec::new, |lan| {
            lan.devices
                .iter()
                .map(|computer| Paired {
                    id: computer.id.clone(),
                    name: computer.name.clone(),
                    pin: computer.key.clone(),
                })
                .collect()
        });
        status.device_rows = control.device_rows(&status, &book, std::time::Instant::now());
        if !enabled && ["connected", "connecting", "reconnecting"].contains(&status.phase.as_str())
        {
            status.phase = "disabled".into();
        }
        Ok(status)
    }
    /// Remote over this network, out of the lock.
    fn lan(&self) -> Result<Option<Arc<lan::Lan>>, &'static str> {
        Ok(self
            .control
            .lock()
            .map_err(|_| "serviceUnavailable")?
            .lan
            .clone())
    }
    /// A press on the shack's Remote settings, from `origin`: through a stream, only a read is
    /// taken ([`Action::through_a_stream`]); everything else is only at the station's own screen.
    pub async fn press(&self, origin: Origin, action: Action) -> Result<Status, &'static str> {
        if origin == Origin::Stream && !action.through_a_stream() {
            return Err("atShackOnly");
        }
        self.action(action).await
    }
    /// ⛔ Private: [`Service::press`] is the only way in, so nothing can skip its origin check.
    async fn action(&self, action: Action) -> Result<Status, &'static str> {
        match &action {
            Action::StationPermission { device_id, allow } => {
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
                    // A browser's alone: a computer holds nothing on the relay's lanes to allow.
                    let Some(device) = status
                        .devices
                        .iter()
                        .find(|d| d.id == *device_id && d.approved == 1 && d.kind == Kind::Browser)
                        .filter(|_| control.enabled)
                    else {
                        return Err("accessDenied");
                    };
                    approval = Some(device.approval());
                }
                // The restrict switch: it allows or revokes station control for one browser, live
                // first, and the record follows only what took effect.
                control.operations.permit_station(device_id, *allow)?;
                control.remember(Persist::Browser {
                    device_id: device_id.clone(),
                    approval,
                    control: Some(*allow),
                });
                drop(control);
                return self.status();
            }
            Action::TakeOverLogging {} => {
                // "End remote control" (the operator's decision of 2026-10-10, one rule for every
                // device): it drops everyone and holds until Remote is turned on again. Hosted Remote
                // goes off as Turn off takes it, here and now, never behind the command queue: every
                // permission in memory cleared, the station's socket closed and with it every browser
                // and every tunnel session, and off remembered across a restart. What each approval
                // gave is remembered (`Paused`, not cleared), so turning Remote on again gives every
                // approved device back what it had, with no approval again.
                let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
                control.stop();
                control.remember(Persist::Paused);
                let lan = control.lan.clone();
                drop(control);
                // It ends Remote over this network too: its port closes and every LAN session ends,
                // and the shack says why until it is turned on again.
                if let Some(lan) = lan {
                    lan.end_at_shack();
                }
                return self.status();
            }
            Action::LanOn { address, port } => {
                let lan = self.lan()?.ok_or("serviceUnavailable")?;
                lan.turn_on(address.as_deref(), *port)?;
                return self.status();
            }
            Action::LanOff {} => {
                if let Some(lan) = self.lan()? {
                    lan.turn_off();
                }
                return self.status();
            }
            Action::LanAddress { address } => {
                self.lan()?
                    .ok_or("serviceUnavailable")?
                    .pick(address.as_deref())?;
                return self.status();
            }
            Action::LanPair {} => {
                self.lan()?.ok_or("serviceUnavailable")?.pair()?;
                return self.status();
            }
            Action::LanCancel {} => {
                if let Some(lan) = self.lan()? {
                    lan.cancel_pairing();
                }
                return self.status();
            }
            Action::LanRevoke { device_id } => {
                if !identifier(device_id) {
                    return Err("invalidRequest");
                }
                self.lan()?.ok_or("serviceUnavailable")?;
                // Gone on every road: its internet road too, where the station can link it.
                let roads = self.roads(Road::Lan(device_id))?;
                self.end_roads(&roads).await?;
                return self.status();
            }
            Action::LanReset {} => {
                let lan = self.lan()?.ok_or("serviceUnavailable")?;
                // What a Remove leaves behind, its floor set to now first (written afresh if it does
                // not read at all): it refuses every proof signed before, and the LAN admits again.
                // The reset goes on whatever it answers.
                let enrolment = self
                    .control
                    .lock()
                    .map_err(|_| "serviceUnavailable")?
                    .enrolment
                    .clone();
                let recovered = tokio::task::spawn_blocking(move || enrolment.recover())
                    .await
                    .map_err(|_| "serviceUnavailable")
                    .and_then(|recovered| recovered);
                // Each paired computer's other roads, read before the book forgets them.
                let mut roads = Roads::default();
                for computer in lan.status().devices {
                    roads.extend(self.roads(Road::Lan(&computer.id))?);
                }
                // The reset takes them out of the book; every key is kept as removed first.
                roads.lan.clear();
                let ended = self.end_roads(&roads).await;
                let reset = tokio::task::spawn_blocking(move || lan.reset())
                    .await
                    .map_err(|_| "serviceUnavailable")
                    .and_then(|reset| reset);
                reset?;
                recovered?;
                ended?;
                return self.status();
            }
            // Remove at the shack, wherever the device shows: gone on every road the station can
            // link it to. A request out of shape is refused by the controller, as before.
            Action::Device {
                device_id,
                approve: false,
                key,
                ..
            } if identifier(device_id) && key.as_deref().is_none_or(pin_shape) => {
                let roads = self.roads(Road::Listed(device_id))?;
                self.end_roads(&roads).await?;
                return self.status();
            }
            _ => {}
        }
        self.queued(action).await?;
        self.status()
    }
    /// The roads `start` links to ([`Control::roads`]), as the station knows them now: the service's
    /// last list, if there is one to go by, and who the LAN book holds.
    fn roads(&self, start: Road) -> Result<Roads, &'static str> {
        let devices = {
            let status = self.status.lock().map_err(|_| "serviceUnavailable")?;
            status.listed.then(|| status.devices.clone())
        };
        // What it keeps read first, outside the lock: the credential store can block.
        let enrolment = self
            .control
            .lock()
            .map_err(|_| "serviceUnavailable")?
            .enrolment
            .clone();
        enrolment.load();
        let control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        let book: Vec<Paired> = control.lan.as_deref().map_or_else(Vec::new, |lan| {
            lan.status()
                .devices
                .into_iter()
                .map(|computer| Paired {
                    id: computer.id,
                    name: computer.name,
                    pin: computer.key,
                })
                .collect()
        });
        Ok(control.roads(devices.as_deref(), &book, start))
    }
    /// Ends every road in `roads`. Its keys are kept as removed first, before any road ends
    /// (`lan::enrol::Enrolment::remove`): from then on nothing is granted again on a proof signed
    /// before, on either road, and a listing still approved at the service is taken out at its next
    /// list even if the service is never told here ([`Controller::end_removed`],
    /// [`Controller::vouch`]). Then out at the station:
    /// each listed id's station control, pin, waiver and place on the room's tunnel at once, then
    /// each LAN id out of the book. Then the service is told of each listed id, or the removal kept
    /// for it ([`Controller::queue_removal`]). FAIL CLOSED: every road is ended whatever another
    /// answers, so a failure leaves less access, never more, and the first failure is what the
    /// shack is shown; with none, `removeElsewhere` when the other list may hold it under a link
    /// the station does not keep.
    async fn end_roads(&self, roads: &Roads) -> Result<(), &'static str> {
        let enrolment = self
            .control
            .lock()
            .map(|control| control.enrolment.clone())
            .map_err(|_| "serviceUnavailable");
        let keys: Vec<String> = roads.keys.iter().cloned().collect();
        // Off this thread, since the credential store can block.
        let kept = match enrolment {
            Ok(enrolment) => tokio::task::spawn_blocking(move || enrolment.remove(&keys))
                .await
                .map_err(|_| "serviceUnavailable")
                .and_then(|kept| kept),
            Err(error) => Err(error),
        };
        let mut failed = kept.err();
        if let Ok(mut control) = self.control.lock() {
            for id in &roads.listed {
                control.revoke_browser(id);
            }
        }
        let lan = match roads.lan.is_empty() {
            true => None,
            false => self.lan().unwrap_or_else(|error| {
                failed = Some(error);
                None
            }),
        };
        if let Some(lan) = lan {
            let ids = roads.lan.clone();
            // Off this thread, since the credential store can block. Inside, each computer is out
            // and its control revoked before the store is written.
            let out = tokio::task::spawn_blocking(move || {
                let mut out = Ok(());
                for id in &ids {
                    out = out.and(lan.revoke(id));
                }
                out
            })
            .await
            .map_err(|_| "serviceUnavailable")
            .and_then(|out| out);
            failed = failed.or(out.err());
        }
        for id in &roads.listed {
            let told = self
                .queued(Action::Device {
                    device_id: id.clone(),
                    approve: false,
                    key: None,
                    kind: None,
                })
                .await;
            failed = failed.or(told.err());
        }
        match failed {
            Some(error) => Err(error),
            None if roads.unconfirmed => Err("removeElsewhere"),
            None => Ok(()),
        }
    }
    /// An action for the controller, behind its queue, with what must be decided at the press
    /// decided here first.
    async fn queued(&self, action: Action) -> Result<(), &'static str> {
        let (reply, response) = oneshot::channel();
        {
            let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
            if let Action::Device {
                approve: false,
                device_id,
                ..
            } = &action
            {
                control.revoke_browser(device_id);
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
                control.start();
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
        response.await.map_err(|_| "serviceUnavailable")?
    }
}
impl Drop for Service {
    fn drop(&mut self) {
        if let Ok(mut control) = self.control.lock() {
            control.stop();
        }
    }
}

/// Whether the shack offers Allow to transmit for `device`, and runs it: only for a device the
/// service lists as added by its own sign-in, approved and unexpired, that holds station control now
/// (`granted`: a computer on the room's tunnel under its tunnel name, [`Control::holds_control`])
/// and that the station holds from transmit now. Never for one removed here and the
/// service not told yet (`removed`), one whose proof the station could not confirm or that waits
/// for an Approve here (`unconfirmed`), or one whose key is not pinned here (it is offered the
/// Approve, which waives the hold itself). Read under the control lock.
fn allows_transmit(
    control: &Control,
    device: &Device,
    granted: bool,
    removed: bool,
    unconfirmed: bool,
) -> bool {
    let pinned = device.key.is_none()
        || device
            .pin()
            .is_some_and(|pin| control.remembered.pins.get(&device.id) == Some(&pin));
    device.added_by.as_deref() == Some("account")
        && device.approved == 1
        && device.expires_at > now_ms()
        && granted
        && !removed
        && !unconfirmed
        && pinned
        && control
            .operations
            .tx_held(&device.id, std::time::Instant::now())
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
    accounts: Accounts,
    signing: Option<Signing>,
    staged: Option<Staged>,
    /// The removals made here that the service has not heard yet ([`Controller::queue_removal`]).
    /// `None` when they could not be read: then no device is granted on its proof, since one of
    /// them may have been removed here.
    removals: Option<Vec<String>>,
    /// The invitations taken back here that the service has not heard yet, each address as it was
    /// typed ([`Controller::queue_uninvite`]). `None` when they could not be read.
    uninvites: Option<Vec<String>>,
}
/// The station's account, as the controller reaches it (`account`): its entries in the credential
/// store, the service's account side, how a sign-in binds, the LAN key a binding names, and the
/// clock proofs are read by.
pub(crate) struct Accounts {
    vault: Arc<dyn vault::AccountVault>,
    /// `accountNotReady` while this build has no configured sign-in for its service.
    service: Result<Arc<account::Service>, &'static str>,
    bind: account::Binder,
    /// The LAN key's public half, SPKI as lowercase hex (made now if there is none): `None` while
    /// the store will not give one up. `None` here: the LAN's own, once the service has started.
    lan_key: Option<Arc<dyn Fn() -> Option<String> + Send + Sync>>,
    now_s: fn() -> u64,
    /// Whether a device is granted anything on its own sign-in: [`account::GRANTS_ON_SIGN_IN`], in
    /// every build. Only a test of the grants a sign-in would make sets it otherwise.
    grants_on_sign_in: bool,
}
impl Accounts {
    /// The account of the service this build is made for, in the OS credential store, signing in
    /// through `open`'s browser.
    fn system(open: account::Opener) -> Self {
        let vault: Arc<dyn vault::AccountVault> = Arc::new(SystemVault);
        let service = account::Service::new(REMOTE_ORIGIN).map(Arc::new);
        Self {
            bind: account::binder(service.clone(), vault.clone(), open, now_s),
            vault,
            service,
            lan_key: None,
            now_s,
            grants_on_sign_in: account::GRANTS_ON_SIGN_IN,
        }
    }
    /// No account sign-in: this build's own tenant is not a test's service.
    #[cfg(test)]
    fn not_ready() -> Self {
        Self {
            vault: Arc::new(tests::MemoryVault::default()),
            service: Err("accountNotReady"),
            bind: Arc::new(|_| Box::pin(async { Err("accountNotReady") })),
            lan_key: None,
            now_s,
            grants_on_sign_in: account::GRANTS_ON_SIGN_IN,
        }
    }
}
/// A sign-in at the shack, running on a task of its own while the operator is in the browser, and
/// the Remote generation it was asked under: a later Turn off or Cancel wins.
struct Signing {
    task: tokio::task::JoinHandle<Result<account::Bound, &'static str>>,
    generation: u64,
    /// Proving a bound station again (`Action::SignInAgain`): nothing is installed when it ends.
    again: bool,
}
/// The keys and credential a sign-in binds with: made once and kept until the station is bound, so a
/// sign-in tried again names the same keys and the service finds the station it already made for
/// them. No `Debug`: the signing key and the credential are secrets.
struct Staged {
    pkcs8: String,
    public_key: String,
    credential: String,
}
/// What the controller waits on next.
enum Next {
    Command(Option<Command>),
    Retry,
    Signed(Result<Result<account::Bound, &'static str>, tokio::task::JoinError>),
}
/// Remote came on with grants remembered; they wait for the service's browser list.
struct Restore {
    /// The authority epoch when Remote came on. Any local decision since then (Turn off, take over)
    /// moves it, and that decision wins. A browser revoked since is read out of the record.
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
            let signing = self.signing.as_mut().map(|s| &mut s.task);
            let next = tokio::select! {
                next = receiver.recv() => Next::Command(next),
                _ = async {
                    match retry {
                        Some(at) => tokio::time::sleep_until(at).await,
                        None => std::future::pending().await,
                    }
                } => Next::Retry,
                done = async {
                    match signing {
                        Some(task) => task.await,
                        None => std::future::pending().await,
                    }
                } => Next::Signed(done),
            };
            match next {
                Next::Command(next) => {
                    let Some((action, generation, epoch, reply)) = next else {
                        break;
                    };
                    let result = self.handle(action, generation, epoch).await;
                    if let Err(error) = result {
                        if let Ok(mut status) = self.status.lock() {
                            status.error = Some(error);
                        }
                    }
                    let _ = reply.send(result);
                }
                Next::Retry => self.retry_restore().await,
                Next::Signed(done) => self.signed(done),
            }
        }
        if let Some(signing) = self.signing.take() {
            signing.task.abort();
        }
        if let Ok(mut control) = self.control.lock() {
            control.stop();
        }
    }
    /// Remote remembers being on. If it was on when Nexus last exited, turn it on again exactly as
    /// Turn on Remote does: a fresh local authority (every in-memory permission cleared), a fresh
    /// generation, and the remembered grants restored for still-approved
    /// browsers. A locked or unreadable store, a record written for another pairing, or a record
    /// that says off all leave it off.
    async fn resume(&mut self, binding: Binding) {
        self.arm_enrolment();
        self.removals = match self.accounts.vault.revokes() {
            Ok(Some(kept)) if kept.station_id == binding.station_id => Some(kept.devices),
            Ok(_) => Some(Vec::new()),
            Err(_) => None,
        };
        self.uninvites = match self.accounts.vault.uninvites() {
            Ok(Some(kept)) if kept.station_id == binding.station_id => Some(kept.people),
            Ok(_) => Some(Vec::new()),
            Err(_) => None,
        };
        self.reflect_removals();
        self.reflect_people();
        // The transmit windows kept for the sign-in devices the service had confirmed it notified
        // their owner about (`keep_notified`), so a restart does not hold them afresh: only this
        // station's, for this account. An entry that is locked, does not read, or names anything
        // but a device id restores nothing, and those devices are held as any sign-in device with
        // no record is. The next list still decides each one (`Authority::tx_held_for`).
        let kept = self.accounts.vault.notified().ok().flatten().filter(|k| {
            k.station_id == binding.station_id
                && k.account_id == binding.account_id
                && k.windows.keys().all(|id| identifier(id))
        });
        if let (Some(kept), Ok(control)) = (kept, self.control.lock()) {
            let now_s = now_ms() / 1000;
            control.operations.restore_transmit_windows(
                kept.windows.into_iter().map(|(id, ends)| {
                    (
                        id,
                        std::time::Duration::from_secs(ends.saturating_sub(now_s)),
                    )
                }),
                std::time::Instant::now(),
            );
        }
        let remembered = match self.vault.state() {
            Ok(state) => state.filter(|s| {
                s.binding == binding
                    && s.grants.len() <= MAX_REMEMBERED
                    && s.grants.iter().all(|g| identifier(&g.device_id))
            }),
            Err(error) => {
                if let Ok(mut control) = self.control.lock() {
                    control.remember(Persist::Bound(binding, None, None, None));
                }
                self.reflect("disabled");
                phase(&self.status, "disabled", Some(error));
                return;
            }
        };
        // A5: the pins, from their own entry. One that is locked, unreadable or out of shape pins
        // nothing: every browser is then approved again at the radio before it streams.
        let pins = self.vault.pins().ok().flatten().filter(|p| {
            p.binding == binding
                && p.keys.len() <= MAX_REMEMBERED
                && p.keys
                    .iter()
                    .all(|(id, key)| identifier(id) && pin_shape(key))
        });
        // The waivers given at the shack before the restart. One that is locked, unreadable, out of
        // shape or another pairing's waives nothing; nor does it for a device removed here and not
        // yet told to the service (or with the removals unreadable), nor for one whose station
        // control this station no longer remembers (`Remembered::keep_waivers`): a pin alone is
        // not enough. What is restored is written back, so the vault keeps no other. The list still
        // decides each one before it is free (`Authority::tx_held_for`).
        let waivers = self.vault.waivers().ok().flatten().filter(|w| {
            w.binding == binding
                && w.devices.len() <= MAX_REMEMBERED
                && w.devices.iter().all(|id| identifier(id))
        });
        let waivers = waivers.map(|mut w| {
            w.devices.retain(|id| {
                self.removals
                    .as_ref()
                    .is_some_and(|removals| !removals.contains(id))
            });
            w
        });
        // S3-M1: this pairing's signing key, made here once for a pairing from before it existed.
        let (key, identity) = self.station_key(&binding);
        // Decide under the lock BEFORE the pairing becomes visible, so a Turn off that arrives
        // afterwards is ordered after this decision and wins.
        let resumed = {
            let Ok(mut control) = self.control.lock() else {
                return;
            };
            let enabled = remembered.as_ref().is_some_and(|s| s.enabled);
            control.station_key = key;
            control.station_identity = identity;
            control.key_refused = false;
            control.remember(Persist::Bound(
                binding,
                remembered,
                pins.map(Box::new),
                waivers.map(Box::new),
            ));
            let restored = control.remembered.waivers.clone();
            control.operations.restore_transmit_waivers(restored);
            if enabled {
                control.start();
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
        match self.list().await {
            Ok(devices) => {
                self.apply_restore(&devices);
                self.listed(&devices);
                let at = self
                    .control
                    .lock()
                    .ok()
                    .map(|c| (c.generation, c.operations.epoch()));
                if let Some(at) = at {
                    self.vouch(&devices, at).await;
                }
                if let Ok(mut status) = self.status.lock() {
                    status.devices = devices;
                    status.listed = true;
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
    /// a revocation made while the list was on its way is honoured. Nothing restored arms anything.
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
            control: ids(|g| g.control),
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
    /// The service's list, as the room's tunnel reads it (`Control::listed`).
    fn listed(&self, devices: &[Device]) {
        if let Ok(mut control) = self.control.lock() {
            control.listed(devices);
        }
    }
    /// The service's list, read after it has been told of every removal made here offline; a
    /// removal it no longer lists as approved is one it has heard.
    async fn list(&mut self) -> Result<Vec<Device>, &'static str> {
        self.send_uninvites().await;
        self.send_removals().await;
        let devices = self.devices().await?;
        let now = now_ms();
        if let Some(removals) = &self.removals {
            let heard: Vec<String> = removals
                .iter()
                .filter(|id| {
                    !devices
                        .iter()
                        .any(|d| d.id == **id && d.approved == 1 && d.expires_at > now)
                })
                .cloned()
                .collect();
            if !heard.is_empty() {
                // A store that will not keep the shorter list leaves it held here for this run.
                let _ = self.keep_removals(|removals| removals.retain(|id| !heard.contains(id)));
            }
        }
        self.end_removed(&devices);
        Ok(devices)
    }
    /// Each device listed as approved whose key was removed at the shack (`lan::enrol::Enrolment::
    /// remove`) and that is still pinned here, or listed with no proof, ended at once and its
    /// removal kept for the service ([`Controller::queue_removal`]): a removal the service was never
    /// told of, or pressed before this station had a list to find it in, is not lost. Before the
    /// list is taken in, so nothing is granted on it meanwhile. One listed with a proof and not
    /// pinned is [`Controller::vouch`]'s: it comes back on a proof signed after its removal. With
    /// the removals not read, none is known to end: each list reads them again first, and
    /// [`Controller::vouch`] grants nothing meanwhile.
    fn end_removed(&mut self, devices: &[Device]) {
        let Some(enrolment) = self.control.lock().ok().map(|c| c.enrolment.clone()) else {
            return;
        };
        enrolment.load();
        let Some(removed) = enrolment.kept().map(|kept| kept.removed) else {
            return;
        };
        let now = now_ms();
        let gone: Vec<String> = match self.control.lock() {
            Ok(mut control) => {
                let gone: Vec<String> = devices
                    .iter()
                    .filter(|d| d.approved == 1 && d.expires_at > now && removed_key(&removed, d))
                    .filter(|d| d.proof.is_none() || control.remembered.pins.contains_key(&d.id))
                    .map(|d| d.id.clone())
                    .collect();
                for id in &gone {
                    control.revoke_browser(id);
                }
                gone
            }
            Err(_) => Vec::new(),
        };
        for id in gone {
            let _ = self.queue_removal(id);
        }
    }
    /// Tells the service of each removal made here while it could not be reached, in order, until
    /// one cannot be sent. Each it answers is no longer waiting.
    async fn send_removals(&mut self) {
        let Some(waiting) = self.removals.clone().filter(|w| !w.is_empty()) else {
            return;
        };
        let Ok((binding, token)) = self.bound() else {
            return;
        };
        for device_id in waiting {
            let sent = self
                .client
                .post(
                    &station_path(&binding, "revoke-device"),
                    Some(&token),
                    json!({ "deviceId": device_id }),
                )
                .await;
            if matches!(sent, Err("serviceUnavailable" | "tryLater")) {
                break;
            }
            let _ = self.keep_removals(|removals| removals.retain(|id| *id != device_id));
        }
    }
    /// Tells the service of each invitation taken back here while it could not be reached, in order,
    /// until one cannot be sent. Each it answers is no longer waiting.
    async fn send_uninvites(&mut self) {
        let Some(waiting) = self.uninvites.clone().filter(|w| !w.is_empty()) else {
            return;
        };
        let Ok((binding, token)) = self.bound() else {
            return;
        };
        for address in waiting {
            let sent = self
                .client
                .post(
                    &station_path(&binding, "uninvite"),
                    Some(&token),
                    json!({ "email": address }),
                )
                .await;
            if matches!(sent, Err("serviceUnavailable" | "tryLater")) {
                break;
            }
            let _ = self.keep_uninvites(|people| people.retain(|a| *a != address));
        }
    }
    /// Changes the invitations taken back and waiting, and keeps them in the store. One the store
    /// will not keep is still held here for this run.
    fn keep_uninvites(
        &mut self,
        change: impl FnOnce(&mut Vec<String>),
    ) -> Result<(), &'static str> {
        let (Some(people), Some(binding)) = (self.uninvites.as_mut(), self.binding.as_ref()) else {
            return Err("stationNotPaired");
        };
        change(people);
        self.accounts.vault.save_uninvites(&vault::Uninvites {
            station_id: binding.station_id.clone(),
            people: people.clone(),
        })
    }
    /// A removal made here that the service could not be told of: kept, so it is sent before the
    /// next list and no proof grants the device again meanwhile.
    fn queue_removal(&mut self, device_id: String) -> Result<(), &'static str> {
        if self.removals.is_none() {
            return Err("credentialStoreUnavailable");
        }
        self.keep_removals(|removals| {
            if !removals.contains(&device_id) {
                removals.push(device_id);
            }
        })
    }
    /// Takes the computers of the guest with `mailbox` (as `tempo_client::account::mailbox` gives
    /// it) out of the LAN book, each as a Remove at the shack takes it, its key kept as removed
    /// first (`lan::enrol::Enrolment::guests`, which names too every guest's computer kept with no
    /// link to say whose). Every one is taken out whatever another answers; the first failure. And
    /// the names of those taken with no link to say they were this guest's, for the shack
    /// (`AccountStatus::also_uninvited`). Blocks on the credential store.
    fn off_lan(&self, mailbox: &str) -> (Vec<String>, Result<(), &'static str>) {
        let Some((lan, enrolment)) = self
            .control
            .lock()
            .ok()
            .and_then(|c| Some((c.lan.clone()?, c.enrolment.clone())))
        else {
            return (Vec::new(), Ok(()));
        };
        let (theirs, unknown) = enrolment.guests(mailbox);
        let computers: Vec<(String, String, String)> = lan
            .status()
            .devices
            .into_iter()
            .filter(|c| {
                let pin = lan::enrol::short(&c.key);
                theirs.contains(&pin) || unknown.contains(&pin)
            })
            .map(|c| (c.id, c.key, c.name))
            .collect();
        let others = computers
            .iter()
            .filter(|(_, key, _)| !theirs.contains(&lan::enrol::short(key)))
            .map(|(_, _, name)| name.clone())
            .collect();
        if computers.is_empty() {
            return (others, Ok(()));
        }
        let keys: Vec<String> = computers.iter().map(|(_, key, _)| key.clone()).collect();
        let mut out = enrolment.remove(&keys);
        for (id, _, _) in &computers {
            out = out.and(lan.revoke(id));
        }
        (others, out)
    }
    /// Changes the removals waiting, and keeps them in the store. One the store will not keep is
    /// still held here for this run.
    fn keep_removals(&mut self, change: impl FnOnce(&mut Vec<String>)) -> Result<(), &'static str> {
        let (Some(removals), Some(binding)) = (self.removals.as_mut(), self.binding.as_ref())
        else {
            return Err("stationNotPaired");
        };
        change(removals);
        let kept = vault::Revokes {
            station_id: binding.station_id.clone(),
            devices: removals.clone(),
        };
        let saved = self.accounts.vault.save_revokes(&kept);
        self.reflect_removals();
        saved
    }
    /// Changes the people invited to this station, and keeps them in the store, before anything
    /// takes them in: an entry that does not read is never written over.
    fn keep_people(
        &self,
        change: impl FnOnce(&mut Vec<tempo_client::account::Invite>),
    ) -> Result<(), &'static str> {
        let binding = self.binding.as_ref().ok_or("stationNotPaired")?;
        let vault = &self.accounts.vault;
        let mut people = vault
            .people()?
            .filter(|people| people.station_id == binding.station_id)
            .unwrap_or_else(|| vault::People {
                station_id: binding.station_id.clone(),
                invites: Vec::new(),
            });
        change(&mut people.invites);
        let saved = vault.save_people(&people);
        self.reflect_people();
        self.arm_enrolment();
        saved
    }
    fn reflect_people(&self) {
        let people = self
            .binding
            .as_ref()
            .and_then(|binding| {
                self.accounts
                    .vault
                    .people()
                    .ok()
                    .flatten()
                    .filter(|people| people.station_id == binding.station_id)
            })
            .map(|people| people.invites)
            .unwrap_or_default();
        if let Ok(mut status) = self.status.lock() {
            status.account.people = people;
        }
    }
    fn reflect_removals(&self) {
        if let Ok(mut status) = self.status.lock() {
            status.account.removals = self.removals.clone().unwrap_or_default();
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
        let mut value: Devices = serde_json::from_value(value).map_err(|_| "invalidResponse")?;
        // One id names one device, and only ever an id the service made: a press, a pin and a grant
        // are each made for an id, so a second device under it, or one under a name the shack gives
        // its own computers, refuses the list as a malformed id does.
        let ids: std::collections::BTreeSet<&str> =
            value.devices.iter().map(|d| d.id.as_str()).collect();
        if value.devices.len() > 8
            || ids.len() != value.devices.len()
            || value.devices.iter().any(|d| {
                !hosted(&d.id) || !valid_name(&d.name) || d.approved > 1 || !d.kind.fits(d.platform)
            })
        {
            return Err("invalidResponse");
        }
        // A5: a key that is not the shape of a P-256 key is no key. It shows nothing and pins
        // nothing, and the rest of the list (approvals, grants) stands.
        for device in &mut value.devices {
            device.key = device.public_key.as_deref().and_then(fingerprint);
        }
        Ok(value.devices)
    }
    /// This pairing's signing key (S3-M1), made once: the one the vault holds for it, or a new one
    /// saved there first. A store that will not answer (locked) makes nothing, because the key may
    /// be in it: the station's answers go unsigned until it does.
    fn station_key(&self, binding: &Binding) -> StationKeys {
        match self.vault.station_key() {
            Ok(Some(kept)) if kept.binding == *binding => {
                if let Some(signer) = station_key::Signer::restore(&kept.pkcs8) {
                    return (
                        Some(Arc::new(signer)),
                        tunnel_identity(&kept.pkcs8, binding),
                    );
                }
            }
            Ok(_) => {}
            Err(_) => return (None, None),
        }
        self.new_station_key(binding)
    }
    /// A new signing key for `binding`, saved before it is used: a key the vault did not keep would
    /// be gone at the next start, and the service would refuse the one made then.
    fn new_station_key(&self, binding: &Binding) -> StationKeys {
        let Some((signer, pkcs8)) = station_key::Signer::generate() else {
            return (None, None);
        };
        let identity = tunnel_identity(&pkcs8, binding);
        if self
            .vault
            .save_station_key(&vault::StationKey {
                binding: binding.clone(),
                pkcs8,
            })
            .is_err()
        {
            return (None, None);
        }
        (Some(Arc::new(signer)), identity)
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
                status.listed = false;
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
    /// ONE APPROVAL (operator decision 2026-09-13): approving a browser grants it station control.
    /// With Remote on it takes
    /// effect at once; with Remote off it waits in the record for Turn on. A local decision made
    /// after the approval was asked for (Turn off or on, take over) wins, and nothing is granted;
    /// revoking this device does too (the revoke is taken again after this, `Controller::handle`).
    ///
    /// A5: the device key the operator was shown (`shown`) is pinned if the service still lists that
    /// key for the browser now that it has approved it. A key that changed in between pins nothing,
    /// and that browser is approved again before it streams.
    ///
    /// The approval is of the device as it was shown (`kind`): a device the service now lists as
    /// another kind is granted nothing and pinned for nothing, and is asked about again in its own
    /// words. A computer's pin is its own ([`Device::pin`]), made for this approval.
    fn grant_approved(
        &self,
        devices: &[Device],
        device_id: String,
        shown: Option<String>,
        kind: Option<Kind>,
        (generation, epoch): (u64, u64),
    ) -> Result<(), &'static str> {
        let now = now_ms();
        let device = devices
            .iter()
            .find(|d| d.id == device_id && d.approved == 1 && d.expires_at > now)
            .ok_or("invalidResponse")?;
        let approval = device.approval();
        let as_shown = kind == Some(device.kind);
        let pin = shown
            .filter(|shown| as_shown && device.key.as_ref() == Some(shown))
            .and_then(|_| device.pin());
        let approved: Vec<String> = devices
            .iter()
            .filter(|d| d.approved == 1 && d.expires_at > now)
            .map(|d| d.id.clone())
            .collect();
        let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        if control.generation != generation || control.operations.epoch() != epoch {
            return Ok(());
        }
        // A computer (the native client) is granted nothing on the relay's lanes: it never reaches
        // them. Its approval is its pin, below, which
        // the tunnel through the room checks. A browser is granted only if it was shown as one.
        let browser = as_shown && device.kind == Kind::Browser;
        if control.enabled && browser {
            control.operations.permit_station(&device_id, true)?;
        }
        if browser {
            control.remember(Persist::Browser {
                device_id: device_id.clone(),
                approval: Some(approval),
                control: Some(true),
            });
        }
        control.remember(Persist::Listed(approved));
        // An Approve at the shack WAIVES the sign-in transmit hold (the 2026-10-23 sign-in decision):
        // the operator stood at the radio and let this device in, which is the shack tap the hold is
        // waived by. A positive waiver, so a sign-in device the operator approved transmits at once.
        control.operations.waive_transmit(&device_id);
        // Kept for a restart, for a device the hold applies to: one added by its own sign-in.
        if device.added_by.as_deref() == Some("account") {
            control.remember(Persist::Waive {
                device_id: device_id.clone(),
            });
        }
        control.remember(Persist::Pin {
            device_id,
            key: pin,
        });
        Ok(())
    }
    /// Allow to transmit, pressed at the shack: waives the sign-in transmit hold of `device_id` (the
    /// 2026-10-23 sign-in decision), as an Approve here does ([`Controller::grant_approved`]), and
    /// grants nothing else. Only for a device the service lists now as added by its own sign-in,
    /// approved and unexpired, not removed here, and offered it at the shack now
    /// ([`allows_transmit`]): any other is refused (`notSignInDevice`) and nothing is waived. A local decision made after the press (Turn off or on, take over) wins,
    /// and nothing is waived. Reached only from the shack's own screen (`Action::AllowTransmit`).
    fn allow_transmit(
        &self,
        devices: &[Device],
        device_id: &str,
        (generation, epoch): (u64, u64),
    ) -> Result<(), &'static str> {
        let now = now_ms();
        // Removals unreadable: none can be ruled out, so nothing is waived.
        let removed = self
            .removals
            .as_ref()
            .is_none_or(|removals| removals.iter().any(|id| id == device_id));
        let signed_in = devices.iter().any(|d| {
            d.id == device_id
                && d.added_by.as_deref() == Some("account")
                && d.approved == 1
                && d.expires_at > now
        });
        if removed || !signed_in {
            return Err("notSignInDevice");
        }
        let unconfirmed = self
            .status
            .lock()
            .map_err(|_| "serviceUnavailable")?
            .account
            .unconfirmed
            .contains_key(device_id);
        let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        if control.generation != generation || control.operations.epoch() != epoch {
            return Ok(());
        }
        // Granted station control now, as the status reads it: unknown while the authority is busy.
        let operations = control.operations.local_status();
        let granted: Vec<String> = serde_json::from_value(
            operations
                .get("controlDevices")
                .ok_or("remoteBusy")?
                .clone(),
        )
        .unwrap_or_default();
        let granted = control.holds_control(device_id, &granted);
        // Exactly the devices the shack offers it for (`Status::held_devices`): any other collects
        // no waiver it could carry into a later grant.
        if !devices
            .iter()
            .find(|d| d.id == device_id)
            .is_some_and(|d| allows_transmit(&control, d, granted, removed, unconfirmed))
        {
            return Err("notSignInDevice");
        }
        control.operations.waive_transmit(device_id);
        control.remember(Persist::Waive {
            device_id: device_id.to_string(),
        });
        Ok(())
    }
    /// The keys and credential this station binds with: those staged for it already, or new ones,
    /// staged now. The credential store is asked to take the credential first, as for a pairing, so
    /// the service never makes a station the shack cannot keep.
    fn staged(&mut self) -> Result<&Staged, &'static str> {
        if self.staged.is_none() {
            let (signer, pkcs8) =
                station_key::Signer::generate().ok_or("credentialStoreUnavailable")?;
            let credential = transport::random_secret()?;
            let unbound = Binding {
                origin: self.client.origin().into(),
                station_id: String::new(),
                account_id: String::new(),
            };
            self.vault.stage(&unbound, &credential)?;
            self.staged = Some(Staged {
                public_key: signer.public_key().to_string(),
                pkcs8,
                credential,
            });
        }
        self.staged.as_ref().ok_or("credentialStoreUnavailable")
    }
    /// The sign-in at the shack has ended: bound, or why not. Kept staged on a failure, so trying
    /// again names the same keys.
    fn signed(
        &mut self,
        done: Result<Result<account::Bound, &'static str>, tokio::task::JoinError>,
    ) {
        let Some(signing) = self.signing.take() else {
            return;
        };
        // Said ended before the station is bound, so nothing shows it bound and still signing in.
        self.reflect_account();
        let result = done.unwrap_or(Err("signInFailed")).and_then(|bound| {
            if signing.again {
                self.proven_again();
                Ok(())
            } else {
                self.install(bound, signing.generation)
            }
        });
        if let Err(error) = result {
            if let Ok(mut status) = self.status.lock() {
                status.error = Some(error);
            }
        }
    }
    /// The station's proof was replaced: devices can check it again, with the owner kept now.
    fn proven_again(&self) {
        if let Ok(mut status) = self.status.lock() {
            status.account.reproof = false;
            status.error = None;
        }
        self.arm_enrolment();
    }
    /// The station is bound to the account: its credential and its signing key are kept under the
    /// binding the service gave (the key first: the service already holds its public half, so a key
    /// made later would be refused), and Remote turns on as approving a pairing turns it on: a
    /// fresh authority, the TX-enable latch untouched, nothing remembered. A Turn off or Cancel
    /// since the sign-in started moved the generation, and wins.
    fn install(&mut self, bound: account::Bound, generation: u64) -> Result<(), &'static str> {
        let staged = self.staged.as_ref().ok_or("signInFailed")?;
        let binding = Binding {
            origin: self.client.origin().into(),
            station_id: bound.station_id,
            account_id: bound.account_id,
        };
        self.vault.save_station_key(&vault::StationKey {
            binding: binding.clone(),
            pkcs8: staged.pkcs8.clone(),
        })?;
        self.vault.save(&binding, &staged.credential)?;
        let key = station_key::Signer::restore(&staged.pkcs8).map(Arc::new);
        let identity = tunnel_identity(&staged.pkcs8, &binding);
        self.staged = None;
        if let Ok(mut control) = self.control.lock() {
            control.station_key = key;
            control.station_identity = identity;
            control.key_refused = false;
            control.remember(Persist::Bound(binding.clone(), None, None, None));
        }
        self.binding = Some(binding);
        self.pending = None;
        self.removals = Some(Vec::new());
        self.uninvites = Some(Vec::new());
        self.reflect_removals();
        self.reflect_people();
        self.arm_enrolment();
        self.reflect("disabled");
        let turned_on = self.control.lock().ok().and_then(|mut control| {
            (control.generation == generation).then(|| {
                control.start();
                control.generation
            })
        });
        if let Some(generation) = turned_on {
            self.enable(generation)?;
        }
        Ok(())
    }
    /// Arms the LAN's enrolment with what the station keeps of its account (`lan::enrol`): its owner,
    /// the issuer's keys and the people invited, read now. Disarmed while the station is not bound
    /// to an owner of its own, while either is not kept, or while one of the three does not read.
    fn arm_enrolment(&self) {
        let vault = &self.accounts.vault;
        let ready = self.accounts.service.as_ref().ok().and_then(|service| {
            let station_id = &self.binding.as_ref()?.station_id;
            let owner = vault
                .owner()
                .ok()
                .flatten()
                .filter(|owner| owner.station_id == *station_id)?;
            let tenant = service.tenant();
            let keys = vault
                .account_keys()
                .ok()
                .flatten()
                .filter(|kept| kept.issuer == tenant.issuer)?
                .keys;
            let invites = match vault.people().ok()? {
                Some(people) if people.station_id == *station_id => people.invites,
                _ => Vec::new(),
            };
            Some(lan::enrol::Ready {
                tenant,
                owner,
                keys,
                invites,
                vault: vault.clone(),
                now_s: self.accounts.now_s,
            })
        });
        if let Ok(control) = self.control.lock() {
            control.enrolment.arm(ready);
        }
    }
    /// The service's list, heard: every device the station keeps for the LAN on a proof that the
    /// service no longer lists as approved is taken out of that keeping (`account::sweep`), and out
    /// of the LAN at once, as a removal at the shack takes it out.
    fn sweep(&self, devices: &[Device], now_s: u64) {
        let Some(binding) = &self.binding else {
            return;
        };
        let now = now_ms();
        let rows: Vec<serde_json::Value> = devices
            .iter()
            .map(|d| {
                let mut row = d.proof_row();
                row["approved"] = u8::from(d.approved == 1 && d.expires_at > now).into();
                row
            })
            .collect();
        let Ok(gone) = account::sweep(&rows, &*self.accounts.vault, &binding.station_id, now_s)
        else {
            return;
        };
        let lan = self.control.lock().ok().and_then(|c| c.lan.clone());
        for pin in gone {
            let pin = lan::pairing::hex32(&pin);
            if let (Some(lan), Some(pin)) = (&lan, pin) {
                // Not every kept device is in the LAN's book (a browser never is).
                let _ = lan.revoke(&lan::device_id(&pin));
            }
        }
    }
    fn reflect_account(&self) {
        if let Ok(mut status) = self.status.lock() {
            status.account.signing_in = self.signing.is_some();
        }
    }
    /// Each device the service lists as approved on its own proof, and not pinned here at that
    /// approval yet, is checked by the station itself (`account::vouch`) before anything is granted
    /// on it. One whose proof holds is granted as its role says, as an Approve press grants
    /// ([`Controller::grant_vouched`]), kept for the LAN, and named in the notice at the shack; while
    /// the station grants nothing on a sign-in (`account::GRANTS_ON_SIGN_IN`) it is granted nothing
    /// and waits for Approve (`approveAtStation`). One whose proof does not hold is granted nothing
    /// and shown with why. Either is shown beside the prompt a device added at the station gets.
    /// Once pinned, a device is left to the operator's own decisions.
    ///
    /// A device whose key, or the LAN key its proof names, was removed at the shack
    /// (`lan::enrol::Enrolment::remove`) is granted nothing on a proof signed by then, and its
    /// removal is kept for the service ([`Controller::queue_removal`]), as one still pinned here is
    /// ([`Controller::end_removed`]). One that comes back on a proof signed after is granted as any
    /// new sign-in, and its key is no longer removed. Each proof that holds keeps the link between
    /// its two keys (`lan::enrol::tie_vouched`), and so does a pinned device's whose link was never
    /// kept ([`Controller::tie_pinned`]).
    async fn vouch(&mut self, devices: &[Device], at: (u64, u64)) {
        let now = now_ms();
        // The sign-in PROVENANCE the transmit hold keys to (the 2026-10-23 sign-in decision): every
        // device the service lists as added by a sign-in, approved and unexpired. Pushed from the
        // durable list on every list, so a sign-in device is held FAIL-CLOSED even if its own grant
        // record is missing (`Authority::tx_held_for`).
        let mut confirmed = None;
        if let Ok(mut control) = self.control.lock() {
            let signed_in: Vec<&Device> = devices
                .iter()
                .filter(|d| {
                    d.added_by.as_deref() == Some("account")
                        && d.approved == 1
                        && d.expires_at > now
                })
                .collect();
            let signin = signed_in.iter().map(|d| d.id.clone()).collect();
            // The subset the service confirms it has a durable owner-notice record for (the
            // 2026-10-23 sign-in decision): a sign-in device's transmit hold lifts only once it is
            // here, so a device whose notice row is missing stays held even past its window.
            let notified: std::collections::BTreeSet<String> = signed_in
                .iter()
                .filter(|d| d.notified == 1)
                .map(|d| d.id.clone())
                .collect();
            // A waiver kept for a restart lasts only while its device is one of these, and not
            // removed here; with the removals unreadable, none does.
            let waivable = signed_in
                .iter()
                .filter(|d| {
                    self.removals
                        .as_ref()
                        .is_some_and(|removals| !removals.contains(&d.id))
                })
                .map(|d| d.id.clone())
                .collect();
            control.operations.set_signin_devices(signin);
            control.operations.set_notified_devices(notified.clone());
            control.remember(Persist::SignedIn(waivable));
            // A waiver restored at launch lasts only while it is kept.
            let kept = control.remembered.waivers.clone();
            control.operations.keep_restored(&kept);
            confirmed = Some(notified);
        }
        // Removed at the shack: as the station keeps them now, all through this list. `None` while
        // they do not read, and then nothing is granted on a proof.
        let enrolment = self.control.lock().ok().map(|c| c.enrolment.clone());
        let removed = enrolment.as_ref().and_then(|enrolment| {
            enrolment.load();
            enrolment.kept()
        });
        let removed_key = |d: &Device| {
            removed
                .as_ref()
                .is_some_and(|kept| removed_key(&kept.removed, d))
        };
        let waiting: Vec<&Device> = {
            let Ok(control) = self.control.lock() else {
                return;
            };
            devices
                .iter()
                .filter(|d| d.proof.is_some() && d.approved == 1 && d.expires_at > now)
                .filter(|d| {
                    d.pin()
                        .is_none_or(|pin| control.remembered.pins.get(&d.id) != Some(&pin))
                })
                // Removed here, and the service not told yet: never granted again meanwhile.
                .filter(|d| {
                    self.removals
                        .as_ref()
                        .is_none_or(|removals| !removals.contains(&d.id))
                })
                .collect()
        };
        let mut unconfirmed = BTreeMap::new();
        let mut reproof = None;
        let now_s = (self.accounts.now_s)();
        let mut whose = Vec::new();
        if !waiting.is_empty() {
            let rows: Vec<serde_json::Value> = waiting.iter().map(|d| d.proof_row()).collect();
            let station_id = self
                .binding
                .as_ref()
                .map(|b| b.station_id.clone())
                .unwrap_or_default();
            let results = match &self.accounts.service {
                _ if self.removals.is_none() || removed.is_none() => rows
                    .iter()
                    .map(|_| Err("credentialStoreUnavailable"))
                    .collect(),
                Ok(service) => {
                    let (results, again) =
                        account::vouch(service, &*self.accounts.vault, &station_id, &rows, now_s)
                            .await;
                    reproof = Some(again);
                    results
                }
                Err(reason) => rows.iter().map(|_| Err(*reason)).collect(),
            };
            let approved: Vec<String> = devices
                .iter()
                .filter(|d| d.approved == 1 && d.expires_at > now)
                .map(|d| d.id.clone())
                .collect();
            let mut back = Vec::new();
            for (device, result) in waiting.into_iter().zip(results) {
                let key = device.key.clone().unwrap_or_default();
                let result = match result {
                    // Removed at the shack since it was signed: never granted on it again.
                    Ok((vouched, lan))
                        if removed.as_ref().is_some_and(|kept| {
                            lan::enrol::removed_by(
                                &kept.removed,
                                &[&lan::enrol::short(&key), &lan::enrol::short(&lan)],
                                vouched.issued_at,
                            )
                        }) =>
                    {
                        let _ = self.queue_removal(device.id.clone());
                        continue;
                    }
                    // Signed by a removal no longer kept by key (`lan::enrol::floored`): refused,
                    // and not taken for a removal of this device.
                    Ok((vouched, _))
                        if removed
                            .as_ref()
                            .is_some_and(|kept| lan::enrol::floored(kept, vouched.issued_at)) =>
                    {
                        Err("tokenRemoved")
                    }
                    Ok((vouched, lan)) => {
                        // A link for a listing that names a LAN key, the only kind with two roads:
                        // kept before it is granted, or nothing is (a Remove would miss a road), and
                        // the next list tries again.
                        let tied = match device.lan_key.is_some() {
                            true => {
                                let vault = &*self.accounts.vault;
                                let mailbox = vouched.mailbox.as_deref();
                                lan::enrol::tie_vouched(vault, &lan, &key, mailbox, now_s)
                            }
                            false => Ok(()),
                        };
                        tied.map(|()| {
                            if removed_key(device) {
                                back.push(key.clone());
                            }
                            (vouched, lan)
                        })
                    }
                    Err(reason) => Err(reason),
                };
                // Display only: whose it is, for the shack's Devices list.
                if let Ok((vouched, _)) = &result {
                    whose.push((device.id.clone(), Whose::of(vouched)));
                }
                match result.and_then(|(vouched, lan)| {
                    let granted = self.grant_vouched(device, &vouched, &approved, at)?;
                    Ok(granted.then_some((vouched, lan)))
                }) {
                    Ok(Some((vouched, lan))) => {
                        // The LAN's entry for it. One the store will not take leaves it out of the
                        // LAN with the internet down, which is where a refusal belongs.
                        let vault = &*self.accounts.vault;
                        let in_book = self.control.lock().ok().is_some_and(|control| {
                            control.lan.as_deref().is_some_and(|book| {
                                book.status().devices.iter().any(|c| c.key == lan)
                            })
                        });
                        let _ = Self::keep_for_lan(
                            vault,
                            &station_id,
                            &vouched,
                            &key,
                            lan,
                            in_book,
                            now_s,
                        );
                        self.added(device, &vouched);
                    }
                    // A local decision since the list was asked for: the next list asks again.
                    Ok(None) => {}
                    Err(reason) => {
                        unconfirmed.insert(device.id.clone(), reason);
                    }
                }
            }
            if let (Some(enrolment), false) = (&enrolment, back.is_empty()) {
                let _ = enrolment.unremove(&back);
            }
        }
        if let Some(kept) = &removed {
            self.tie_pinned(devices, &kept.removed, now_s).await;
        }
        if let Some(enrolment) = &enrolment {
            enrolment.reread();
        }
        self.sweep(devices, now_s);
        // And every computer enrolled on the LAN on a proof past the offline age.
        if let Some(lan) = self.control.lock().ok().and_then(|c| c.lan.clone()) {
            lan.expire();
        }
        // The issuer's keys may be new, and the owner or the people may have changed.
        self.arm_enrolment();
        let recalled = self.recall_whose(devices, &whose, now_s);
        if let Ok(mut status) = self.status.lock() {
            status.whose.extend(whose.into_iter().chain(recalled));
            status.account.unconfirmed = unconfirmed;
            // Known only when the issuer's keys were read for this list.
            if let Some(reproof) = reproof {
                status.account.reproof = reproof;
            }
        }
        if let Some(confirmed) = confirmed {
            self.keep_notified(&confirmed);
        }
    }
    /// The link of each computer pinned here and paired in the LAN book whose link was never kept
    /// (granted before links were kept), from its proof checked once more while it holds
    /// ([`Controller::vouch`]). One whose proof no longer holds stays unlinked: a Remove then tells
    /// the shack to check the other list.
    async fn tie_pinned(&self, devices: &[Device], removed: &[vault::Removed], now_s: u64) {
        use lan::enrol::short;
        let (Ok(service), Some(binding)) = (&self.accounts.service, &self.binding) else {
            return;
        };
        let untied: Vec<&Device> = {
            let Ok(control) = self.control.lock() else {
                return;
            };
            let book: Vec<String> = control.lan.as_deref().map_or_else(Vec::new, |lan| {
                lan.status().devices.into_iter().map(|c| c.key).collect()
            });
            let ties = control.enrolment.ties();
            let now = now_ms();
            devices
                .iter()
                .filter(|d| d.kind != Kind::Browser && d.approved == 1 && d.expires_at > now)
                .filter(|d| d.proof.is_some() && control.remembered.pins.contains_key(&d.id))
                .filter(|d| {
                    let (Some(key), Some(lan)) =
                        (d.key.as_deref(), d.lan_key.as_deref().and_then(fingerprint))
                    else {
                        return false;
                    };
                    let (key, lan) = (short(key), short(&lan));
                    !removed.iter().any(|r| r.key == key)
                        && book.iter().any(|pin| short(pin) == lan)
                        && !ties
                            .iter()
                            .any(|t| t.lan == lan && t.key == key && t.vouched)
                })
                .collect()
        };
        if untied.is_empty() || self.removals.is_none() {
            return;
        }
        let rows: Vec<serde_json::Value> = untied.iter().map(|d| d.proof_row()).collect();
        let vault = &*self.accounts.vault;
        let (results, _) = account::vouch(service, vault, &binding.station_id, &rows, now_s).await;
        for (device, result) in untied.into_iter().zip(results) {
            if let (Ok((vouched, lan)), Some(key)) = (result, device.key.as_deref()) {
                let mailbox = vouched.mailbox.as_deref();
                let _ = lan::enrol::tie_vouched(vault, &lan, key, mailbox, now_s);
            }
        }
    }
    /// Keeps, for a restart, what is left of the transmit window of each sign-in device this list
    /// confirmed the service notified its owner about (`confirmed`), bound to this station and its
    /// account: the next launch restores those windows (`resume`) rather than holding the devices
    /// afresh. Each list replaces it, so a device the list no longer confirms, no longer names as
    /// added by a sign-in, or that was removed here and the service not told yet, is dropped at
    /// once; with the removals unreadable, every device is. Written only when it changed.
    fn keep_notified(&self, confirmed: &std::collections::BTreeSet<String>) {
        let Some(binding) = &self.binding else {
            return;
        };
        let kept = confirmed
            .iter()
            .filter(|d| self.removals.as_ref().is_some_and(|r| !r.contains(*d)))
            .cloned()
            .collect();
        let windows = match self.control.lock() {
            Ok(control) => control
                .operations
                .transmit_windows(&kept, std::time::Instant::now()),
            Err(_) => return,
        };
        // Its end in unix seconds, rounded up; 0 once it has elapsed.
        let now_s = now_ms() / 1000;
        let notified = vault::Notified {
            station_id: binding.station_id.clone(),
            account_id: binding.account_id.clone(),
            windows: windows
                .into_iter()
                .map(|(d, left)| {
                    (
                        d,
                        if left.is_zero() {
                            0
                        } else {
                            now_s + left.as_secs() + 1
                        },
                    )
                })
                .collect(),
        };
        let vault = &self.accounts.vault;
        if vault.notified().ok().flatten().as_ref() != Some(&notified) {
            let _ = vault.save_notified(&notified);
        }
    }
    /// What an Approve press grants a device the operator was shown, for one the station vouched
    /// for on its own proof, by its role: a browser that operates gets station control,
    /// one that listens does not, and every device gets its pin ([`Device::pin`]). Station
    /// control and the pin are all a stream needs (`Authority::stream_admitted`), and a streaming
    /// browser uses the station's own window, transmit included, as any browser approved here does.
    /// With Remote on the grants take effect at
    /// once; with it off they wait in the record for Turn on. `false`, granting nothing, when a
    /// local decision (Turn off or on, take over) was made after the list was asked for.
    ///
    /// A computer or phone that would only listen is refused (`listenNative`): its pin alone holds
    /// station control on the room's tunnel whenever Remote is on (`Control::sync_approved`), so it
    /// cannot be pinned as a listener. The operator can still approve it at the shack to operate.
    ///
    /// ⛔ Every other device is refused too (`approveAtStation`) while the station grants nothing on
    /// a sign-in (`account::GRANTS_ON_SIGN_IN`): it waits for Approve at the shack, which grants it
    /// through [`Controller::grant_approved`] as it grants any device.
    fn grant_vouched(
        &self,
        device: &Device,
        vouched: &tempo_client::account::Vouched,
        approved: &[String],
        (generation, epoch): (u64, u64),
    ) -> Result<bool, &'static str> {
        let pin = device.pin().ok_or("deviceKey")?;
        let operate = vouched.role == tempo_client::account::Role::Operate;
        let browser = device.kind == Kind::Browser;
        if !browser && !operate {
            return Err("listenNative");
        }
        if !self.accounts.grants_on_sign_in {
            return Err("approveAtStation");
        }
        let mut control = self.control.lock().map_err(|_| "serviceUnavailable")?;
        if control.generation != generation || control.operations.epoch() != epoch {
            return Ok(false);
        }
        // ⛔ A 15-MINUTE TRANSMIT HOLD (the 2026-10-23 sign-in decision): a device whose control came
        // from a sign-in may hold control and operate the receiver, but no transmit ARM starts for
        // `account::TX_HOLD` — the engine's gate refuses it while this device controls
        // (`Authority::apply_tx_hold`). This records a POSITIVE outcome; FAIL-CLOSED, a sign-in
        // device with no record stays held (`Authority::tx_held_for` keys on the durable service
        // list, not on this write). Only an Approve at the shack waives it (`grant_approved`); a LAN
        // enrolment never records one (`lan::enrol`). A second factor at sign-in does NOT waive it:
        // that exemption is held for later (operator, 2026-10-24), so an MFA sign-in is held like any
        // other. Stop and disarming are never held. Recorded before anything is granted or pinned,
        // so a waiver this device had before is gone before the room's tunnel takes it again.
        control
            .operations
            .hold_transmit_until(&device.id, std::time::Instant::now() + account::TX_HOLD);
        control.remember(Persist::Hold {
            device_id: device.id.clone(),
        });
        if control.enabled && browser {
            control.operations.permit_station(&device.id, operate)?;
        }
        if browser {
            control.remember(Persist::Browser {
                device_id: device.id.clone(),
                approval: Some(device.approval()),
                control: Some(operate),
            });
        }
        control.remember(Persist::Listed(approved.to_vec()));
        control.remember(Persist::Pin {
            device_id: device.id.clone(),
            key: Some(pin),
        });
        Ok(true)
    }
    /// Keeps a device the station vouched for in the LAN's entry in `vault`, by the pin the LAN
    /// admits it by, the entry held from its read to its save (`vault::DevicesHeld`). An entry that
    /// does not read is never written over. A computer the LAN book holds (`in_book`) and the entry
    /// does not was paired by code at the shack, and is held to no sign-in's age (`lan::enrol`): a
    /// guest's listing naming its LAN key, a claim the service does not check, never makes it that
    /// guest's (an invitation taken back would take it off the LAN) nor puts it under their age.
    /// Nor does it touch a computer the entry holds already, unless the links kept show that same
    /// guest enrolled it here for the internet key `key`: so it never makes the owner's computer a
    /// guest's, nor holds another guest's computer to its own sign-in's age.
    fn keep_for_lan(
        vault: &dyn vault::AccountVault,
        station_id: &str,
        vouched: &tempo_client::account::Vouched,
        key: &str,
        pin: String,
        in_book: bool,
        now_s: u64,
    ) -> Result<(), &'static str> {
        let held = vault::DevicesHeld::take(vault);
        let mut cache = vault
            .device_cache()?
            .filter(|cache| cache.station_id == station_id)
            .unwrap_or_else(|| tempo_client::account::DeviceCache::new(station_id));
        if vouched.guest {
            match cache.find(&pin) {
                None if in_book => return Ok(()),
                None => {}
                Some(_) => {
                    let (lan, key) = (lan::enrol::short(&pin), lan::enrol::short(key));
                    let mailbox = vouched.mailbox.as_deref().map(lan::enrol::mailbox_tag);
                    let theirs = vault.ties()?.unwrap_or_default().ties.iter().any(|t| {
                        t.enrolled && t.lan == lan && t.key == key && t.mailbox == mailbox
                    });
                    if !theirs {
                        return Ok(());
                    }
                }
            }
        }
        cache.keep(tempo_client::account::CachedDevice {
            pin,
            role: vouched.role,
            guest: vouched.guest,
            issued_at: vouched.issued_at,
            confirmed_at: now_s,
        })?;
        vault.save_device_cache(&held, &cache)
    }
    /// DISPLAY ONLY (the shack's Devices list): whose each device added by a sign-in is, for each
    /// listed with a proof that neither this list (`learned`) nor an earlier one since Nexus started
    /// has told. Its proof is checked as a waiting device's is, with the issuer's keys kept here and
    /// nothing fetched, once a run; one that does not hold, or cannot be checked, is shown as not
    /// known. The answer goes nowhere but the list: nothing is granted, pinned or kept on it.
    fn recall_whose(
        &self,
        devices: &[Device],
        learned: &[(String, Whose)],
        now_s: u64,
    ) -> Vec<(String, Whose)> {
        let known = |id: &String| {
            learned.iter().any(|(learned, _)| learned == id)
                || self
                    .status
                    .lock()
                    .is_ok_and(|status| status.whose.contains_key(id))
        };
        let asked: Vec<&Device> = devices
            .iter()
            .filter(|d| d.added_by.as_deref() == Some("account") && d.proof.is_some())
            .filter(|d| !known(&d.id))
            .collect();
        if asked.is_empty() {
            return Vec::new();
        }
        let vault = &*self.accounts.vault;
        // As `account::vouch` refuses: an owner kept from another binding vouches for nobody here.
        let bound = self.binding.as_ref().map(|b| b.station_id.as_str());
        let owned = vault
            .owner()
            .ok()
            .flatten()
            .is_some_and(|owner| Some(owner.station_id.as_str()) == bound);
        let checked = match &self.accounts.service {
            Ok(service) if owned => vault
                .account_keys()
                .ok()
                .flatten()
                .filter(|kept| kept.issuer == service.tenant().issuer)
                .map(|kept| (service.tenant(), kept.keys)),
            _ => None,
        };
        asked
            .into_iter()
            .map(|d| {
                let whose = checked
                    .as_ref()
                    .and_then(|(tenant, keys)| {
                        account::vouch_listed(&d.proof_row(), vault, tenant, keys, now_s).ok()
                    })
                    .map_or(Whose::Unknown, |(vouched, _)| Whose::of(&vouched));
                (d.id.clone(), whose)
            })
            .collect()
    }
    /// The notice of a device the station granted on its own proof, once.
    fn added(&self, device: &Device, vouched: &tempo_client::account::Vouched) {
        if let Ok(mut status) = self.status.lock() {
            if status
                .account
                .added
                .iter()
                .any(|a| a.device_id == device.id)
            {
                return;
            }
            status.account.added.push(Added {
                device_id: device.id.clone(),
                name: device.name.clone(),
                kind: device.kind,
                platform: device.platform,
                key: device.key.clone(),
                role: vouched.role,
                guest: vouched.guest,
            });
        }
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
        // the same approval. The epoch is read here, under the lock: a later take over or Turn off
        // moves it, and that decision wins; a later revocation is read out of the record.
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
            Action::StationPermission { .. }
            | Action::TakeOverLogging {}
            | Action::LanOn { .. }
            | Action::LanOff {}
            | Action::LanAddress { .. }
            | Action::LanPair {}
            | Action::LanCancel {}
            | Action::LanRevoke { .. }
            | Action::LanReset {} => return Err("invalidRequest"),
            Action::Begin { name } => {
                if self.binding.is_some()
                    || self.pending.is_some()
                    || self.signing.is_some()
                    || !valid_name(&name)
                {
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
            Action::CancelSignInAgain {} => {
                if let Some(signing) = self.signing.take_if(|signing| signing.again) {
                    signing.task.abort();
                }
                self.reflect_account();
            }
            Action::Cancel {} => {
                if let Some(signing) = self.signing.take() {
                    signing.task.abort();
                }
                self.reflect_account();
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
                    let devices = self.list().await?;
                    // A fresh list is also what a restore after a restart is waiting for.
                    self.apply_restore(&devices);
                    self.listed(&devices);
                    self.vouch(&devices, (generation, epoch)).await;
                    if let Ok(mut status) = self.status.lock() {
                        status.devices = devices;
                        status.listed = true;
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
                // One trial per callsign and per shack PC: the station callsign configured here (sent
                // as it is, blank included, for the service to judge) and the PC's two hashed
                // identifiers (`pc_identity`), read only now, when approving may start a trial.
                let callsign = tempo_app::engine::engine_lock(&self.engine)
                    .settings()
                    .mycall
                    .trim()
                    .to_string();
                let result = self
                    .client
                    .post(
                        "enroll/approve",
                        None,
                        json!({ "id": pending.id, "proof": pending.proof, "credential": token,
                            "callsign": callsign, "pc": pc_identity::pc() }),
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
                // S3-M1: a new pairing is a new station, with a signing key of its own.
                let (key, identity) = self.new_station_key(&binding);
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
                            // The device key the confirming browser brought (A5), if any.
                            d.get("publicKey")
                                .and_then(|k| k.as_str())
                                .and_then(fingerprint),
                        ))
                    })
                    .filter(|(id, approval, _)| identifier(id) && approval.expires_at > now_ms());
                if let Ok(mut control) = self.control.lock() {
                    control.station_key = key;
                    control.station_identity = identity;
                    control.key_refused = false;
                    // A new pairing starts with nothing remembered: off, no grants, no pins.
                    control.remember(Persist::Bound(binding.clone(), None, None, None));
                    if let Some((device_id, approval, key)) = paired {
                        control.remember(Persist::Browser {
                            device_id: device_id.clone(),
                            approval: Some(approval),
                            control: Some(true),
                        });
                        // A5, operator ruling D4 (2026-09-28): that browser's key is pinned on first
                        // use, the trust the pairing already places in the service. Both ends show
                        // its fingerprint straight after, and revoking the browser is one click.
                        control.remember(Persist::Pin { device_id, key });
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
                        control.start();
                        control.generation
                    })
                });
                if let Some(generation) = turned_on {
                    self.enable(generation)?;
                }
            }
            Action::SignIn { name } => {
                if self.binding.is_some()
                    || self.pending.is_some()
                    || self.signing.is_some()
                    || !valid_name(&name)
                {
                    return Err("invalidRequest");
                }
                // Not ready: no key is made and nothing is staged.
                self.accounts.service.as_ref().map_err(|reason| *reason)?;
                let lan_key = self
                    .accounts
                    .lan_key
                    .as_ref()
                    .and_then(|lan_key| lan_key())
                    .ok_or("noKey")?;
                let staged = self.staged()?;
                let binds = account::Binds {
                    name: name.trim().to_string(),
                    station_key: staged.public_key.clone(),
                    lan_key,
                    credential: staged.credential.clone(),
                    again: None,
                };
                self.signing = Some(Signing {
                    task: tokio::spawn((self.accounts.bind)(binds)),
                    generation,
                    again: false,
                });
                self.reflect_account();
            }
            Action::SignInAgain {} => {
                if self.pending.is_some() || self.signing.is_some() {
                    return Err("invalidRequest");
                }
                self.accounts.service.as_ref().map_err(|reason| *reason)?;
                let (binding, credential) = self.bound()?;
                let station_key = self
                    .control
                    .lock()
                    .map_err(|_| "serviceUnavailable")?
                    .station_key
                    .as_ref()
                    .map(|key| key.public_key().to_string())
                    .ok_or("noKey")?;
                // The LAN key as it is now: one reset since the station was bound is proven too.
                let lan_key = self
                    .accounts
                    .lan_key
                    .as_ref()
                    .and_then(|lan_key| lan_key())
                    .ok_or("noKey")?;
                let binds = account::Binds {
                    name: String::new(),
                    station_key,
                    lan_key,
                    credential,
                    again: Some(account::Bound {
                        station_id: binding.station_id,
                        account_id: binding.account_id,
                    }),
                };
                self.signing = Some(Signing {
                    task: tokio::spawn((self.accounts.bind)(binds)),
                    generation,
                    again: true,
                });
                self.reflect_account();
            }
            Action::Invite { email, role } => {
                let address = email.trim();
                if !invitable(address) {
                    return Err("invalidRequest");
                }
                let (binding, token) = self.bound()?;
                self.client
                    .post(
                        &station_path(&binding, "invite"),
                        Some(&token),
                        json!({ "email": address, "role": role }),
                    )
                    .await?;
                let mailbox = tempo_client::account::mailbox(address);
                // The service holds this invitation now: one taken back offline and not heard yet
                // must not take it back when it is sent.
                let same = |a: &String| tempo_client::account::mailbox(a) == mailbox;
                if self.uninvites.as_ref().is_some_and(|w| w.iter().any(same)) {
                    let _ = self.keep_uninvites(|people| people.retain(|a| !same(a)));
                }
                self.keep_people(|invites| {
                    invites.retain(|invite| invite.mailbox != mailbox);
                    invites.push(tempo_client::account::Invite { mailbox, role });
                })?;
            }
            Action::Uninvite { email } => {
                let address = email.trim();
                if !invitable(address) {
                    return Err("invalidRequest");
                }
                let (binding, token) = self.bound()?;
                // Out here first: from now on no proof of theirs is taken at this station, whether
                // or not the service can be told.
                let mailbox = tempo_client::account::mailbox(address);
                self.keep_people(|invites| invites.retain(|invite| invite.mailbox != mailbox))?;
                // Their computers on this network out at once, each as a Remove at the shack takes
                // it: every one holds station control while the LAN is on, and the service, whose
                // next list would sweep them, may not be reached for days.
                let (others, off) = self.off_lan(&mailbox);
                if let Ok(mut status) = self.status.lock() {
                    status.account.also_uninvited = others;
                }
                let sent = self
                    .client
                    .post(
                        &station_path(&binding, "uninvite"),
                        Some(&token),
                        json!({ "email": address }),
                    )
                    .await;
                match sent {
                    Ok(_) => {}
                    // Out here already; the service is told before the station's next list, and
                    // then removes their devices.
                    Err("serviceUnavailable" | "tryLater") => {
                        if self.uninvites.is_none() {
                            return Err("credentialStoreUnavailable");
                        }
                        let address = address.to_string();
                        self.keep_uninvites(|people| {
                            if !people.contains(&address) {
                                people.push(address);
                            }
                        })?;
                    }
                    Err(error) => return Err(error),
                }
                off?;
            }
            Action::DismissAdded { device_id } => {
                if let Ok(mut status) = self.status.lock() {
                    status
                        .account
                        .added
                        .retain(|added| added.device_id != device_id);
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
                // What it kept of its confirmed owner notices goes with it.
                let _ = self.accounts.vault.save_notified(&vault::Notified {
                    station_id: binding.station_id.clone(),
                    account_id: binding.account_id.clone(),
                    windows: BTreeMap::new(),
                });
                // The pairing's key goes with it: a new pairing makes its own.
                let _ = self.vault.remove_station_key();
                if let Ok(mut control) = self.control.lock() {
                    control.station_key = None;
                    control.station_identity = None;
                    control.key_refused = false;
                    control.remember(Persist::Removed);
                }
                self.binding = None;
                self.removals = Some(Vec::new());
                self.uninvites = Some(Vec::new());
                self.reflect_removals();
                self.reflect_people();
                self.arm_enrolment();
                self.pending = None;
                self.reflect("unpaired");
            }
            Action::Device {
                device_id,
                approve,
                key,
                kind,
            } => {
                if !identifier(&device_id) || key.as_deref().is_some_and(|key| !pin_shape(key)) {
                    return Err("invalidRequest");
                }
                if !approve {
                    // Taken when the press was made (`Service::action`); taken again here, after
                    // any approval of this device that was already on its way when it was made,
                    // so that approval cannot hand the grants back.
                    if let Ok(mut control) = self.control.lock() {
                        control.revoke_browser(&device_id);
                    }
                } else if let Some(shown) = &key {
                    // Approved at the shack: the key shown is the operator's again, if it was
                    // removed here before, so the list that grants it does not end it.
                    let enrolment = self.control.lock().ok().map(|c| c.enrolment.clone());
                    if let Some(enrolment) = enrolment {
                        let _ = enrolment.unremove(std::slice::from_ref(shown));
                    }
                }
                let (binding, token) = self.bound()?;
                let sent = self
                    .client
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
                    .await;
                match sent {
                    Ok(_) => {}
                    // Removed here already; the service is told before the station's next list.
                    Err("serviceUnavailable" | "tryLater") if !approve => {
                        return self.queue_removal(device_id);
                    }
                    Err(error) => return Err(error),
                }
                if let Ok(mut status) = self.status.lock() {
                    if approve {
                        // Not the list the service holds now, until it is heard again.
                        status.devices.clear();
                        status.listed = false;
                    } else {
                        // The list less the one removed: still the list to go by.
                        status.devices.retain(|d| d.id != device_id);
                    }
                }
                if approve {
                    // The list names the approval the service just wrote; the grant binds to it.
                    let devices = self.list().await?;
                    let granted =
                        self.grant_approved(&devices, device_id, key, kind, (generation, epoch));
                    self.listed(&devices);
                    self.vouch(&devices, (generation, epoch)).await;
                    if let Ok(mut status) = self.status.lock() {
                        status.devices = devices;
                        status.listed = true;
                    }
                    granted?;
                }
            }
            Action::AllowTransmit { device_id } => {
                if !identifier(&device_id) {
                    return Err("invalidRequest");
                }
                // The list as the service holds it now: the device is checked on it, and what the
                // station keeps for a restart is written from it after the waiver (`vouch`).
                let devices = self.list().await?;
                let allowed = self.allow_transmit(&devices, &device_id, (generation, epoch));
                self.listed(&devices);
                self.vouch(&devices, (generation, epoch)).await;
                if let Ok(mut status) = self.status.lock() {
                    status.devices = devices;
                    status.listed = true;
                }
                allowed?;
            }
        }
        Ok(())
    }
}

/// The vault writer. Each write is a whole record for the current pairing, produced in the order
/// the decisions were made, so a backlog collapses to the latest write of each entry. A failed save
/// is followed by deleting the record: a record that could not be updated must not survive to be
/// restored. No state record means Remote stays off at the next launch; no pins record means every
/// browser is approved again at the radio before it streams.
///
/// A waivers write that did not land, its fallback removal included, is tried again ahead of every
/// later batch, and until it lands no state is saved, only removed: a grant of station control kept
/// beside a waiver the station meant to take back would bring that waiver back at the next launch.
/// The latest state removed so is kept, and saved in the batch the waivers write lands in, unless a
/// later state write came first.
fn write_remembered(vault: Arc<dyn Vault>, mut writes: mpsc::UnboundedReceiver<Write>) {
    // The waivers write that did not land, and the latest state removed in its place.
    let (mut unwritten, mut deferred) = (None, None);
    while let Some(first) = writes.blocking_recv() {
        let (mut state, mut pins, mut waivers) = (deferred.take(), None, unwritten.take());
        let mut next = Some(first);
        while let Some(write) = next {
            match write {
                Write::Save(_) | Write::Remove => state = Some(write),
                Write::SavePins(_) | Write::RemovePins => pins = Some(write),
                Write::SaveWaivers(_) | Write::RemoveWaivers => waivers = Some(write),
            }
            next = writes.try_recv().ok();
        }
        // The waivers first: a crash between the writes then leaves a device without its waiver,
        // never a waiver beside a grant or pin that was revoked.
        for write in [waivers, state, pins].into_iter().flatten() {
            match write {
                Write::Save(state) => {
                    if unwritten.is_some() {
                        let _ = vault.remove_state();
                        deferred = Some(Write::Save(state));
                    } else if vault.save_state(&state).is_err() {
                        let _ = vault.remove_state();
                    }
                }
                Write::Remove => {
                    let _ = vault.remove_state();
                }
                Write::SavePins(pins) => {
                    if vault.save_pins(&pins).is_err() {
                        let _ = vault.remove_pins();
                    }
                }
                Write::RemovePins => {
                    let _ = vault.remove_pins();
                }
                // A waiver that could not be kept must not survive to be restored.
                Write::SaveWaivers(waivers) => {
                    if vault.save_waivers(&waivers).is_err() && vault.remove_waivers().is_err() {
                        unwritten = Some(Write::SaveWaivers(waivers));
                    }
                }
                Write::RemoveWaivers => {
                    if vault.remove_waivers().is_err() {
                        unwritten = Some(Write::RemoveWaivers);
                    }
                }
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
    origin: Origin,
) -> Result<Status, &'static str> {
    service.press(origin, action).await
}
