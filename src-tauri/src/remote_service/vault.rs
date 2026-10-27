//! Isolated Remote vault. One atomic OS entry contains the pairing and its secret;
//! no connector settings, logbook passwords or fallback files are accessed.
//!
//! A second entry remembers what a restart must restore: whether Remote was on, and each approved
//! browser's station-control grant. It is a SEPARATE entry,
//! not a new field on the pairing record, because that record is `deny_unknown_fields`: an older
//! Nexus reading a pairing that had grown a field would lose its pairing, and then could not even
//! revoke it. It is bound to the pairing it was written for, so an orphan left behind by a failed
//! delete is ignored by any other pairing.
//!
//! An older build also remembered the old Remote page's FT8/FT4 transmit, as `transmit` on a grant,
//! written only when granted, and its remote logging, as `logging` on every grant. Both permissions
//! are gone: this build reads such a record as it reads any other and acts on neither field, so the
//! record still restores each browser's station control, and nothing else. A browser that held only
//! logging is restored nothing and forgotten at the next write. `transmit` is never written.
//! `logging` is written, always `false`: every earlier build requires it on a grant, and a record
//! without it would read there as unreadable and leave Remote off when the operator moves back.
//!
//! A third entry holds the device keys the operator pinned at the radio (security test A5): per
//! browser, the SHA-256 of the public key it signs its stream offers with. It is its OWN entry, not
//! a field on the state entry, because that record is `deny_unknown_fields` too: a pin there would
//! make every older Nexus read the whole record as unreadable and leave Remote off with nothing
//! restored, and the operator moves between builds. An older Nexus never reads this entry at all.
//! It is bound to its pairing like the state entry, so a leftover from another pairing pins nothing.
//!
//! A fourth entry holds the station's own signing key (security review S3-M1), which signs its
//! stream answers so a browser can tell the station from a relay standing in for it
//! (`station_key`). Its own entry for the same reason as the pins: an older Nexus never reads it,
//! and nothing it reads changes. Bound to its pairing like the others, so a key left behind by
//! another pairing signs nothing for this one. This entry and this process are the only places the
//! private key exists: it is never logged, shown or sent.
//!
//! A fifth entry and a sixth belong to Remote over this network (`lan::book`), and to no pairing
//! with the service: the two are paired and revoked separately. The fifth is the shack's LAN key,
//! handled as the station key is (never logged, shown or sent), with the LAN station id beside it.
//! The sixth lists the computers paired with that key: for each, only the SHA-256 of its key and
//! the name it gave, which is no secret. It is bound to the LAN station id, so a list left behind by
//! another key admits nobody, and eight records fit the smallest credential blob (Windows, 2560
//! bytes of UTF-16) in one entry. Unlike the records above, these two read past a field they do not
//! know, so a newer Nexus may add one and this build still reads them (a list it writes again
//! keeps only the fields it knows). One that does not read at all is never taken for none
//! ([`lan_entry`]): no key is made over it and the list is never written over or deleted on its
//! own; the station has no key, or admits nobody, until the operator resets its network identity.
//!
//! Four more belong to the station's account (`remote_service::account`), and are each their own
//! entry so no older Nexus ever reads them: who owns the station (the service's id for it, the
//! issuer and the owner's subject, and the key id and signing time of its proof: no token), the
//! issuer's key set, the devices kept for the LAN (each the SHA-256 of its key, its role and two
//! times), and the people invited (each an address as the service matches it, and a role). A fifth
//! holds the removals made at the shack that the service has not heard yet (device ids), so a
//! device removed offline is never granted again on its proof before the service hears of it; a
//! sixth, the invitations taken back at the shack that the service has not heard yet (each address
//! as it was typed), so the service still removes that person's devices once it is reached. None
//! holds a secret: a key set is public, and a pin is a hash. Like the LAN entries they read past a
//! field they do not know, and one that does not read at all is never taken for none: the station
//! then admits no device on a proof until it signs in again. The devices kept for the LAN have
//! several writers, and each holds that entry from its read to its save ([`DevicesHeld`]).
//!
//! One more ([`Ties`]) holds what links a computer's two roads and what a Remove at the shack
//! leaves behind: for each computer kept for the LAN, the LAN key and the internet key one proof
//! named together, and who signed in for it (a hash of a guest's address, never the address); and
//! each key removed at the shack, with when. Short hashes and no service id, so it fits the smallest
//! credential blob beside the devices kept for the LAN, and is held with them ([`DevicesHeld`]). It
//! grants nothing: a link only ever takes access away, and a removal refuses a proof signed before
//! it. Its own entry, so no older Nexus reads it; one that does not read admits no computer on the
//! LAN, as the devices kept for it do, until Reset network identity writes it afresh. When more
//! removals are made than it keeps, the oldest still in force leaves its time behind instead (its
//! floor): every proof signed by then is refused, whatever key it names.
//!
//! The rest belong to the other end of Remote over this network, the computer an operator works
//! from (`crate::lan_client`). One entry for each station it is paired with: its own key for that
//! station (the design's second key, approved on 2026-10-04 and handled as the station key is:
//! never logged, shown or sent), the station's key as it presented it while pairing, the two ids
//! and the addresses it answered at. Each fits the smallest credential blob on its own; a last
//! entry lists them, since a credential store cannot be asked what it holds. Neither is ever read
//! by the shack's side, nor the shack's entries by these. One more holds the two keys this computer
//! made for the next station it signs in to with the account (`crate::lan_client::account`), kept
//! until a station is kept with them so a sign-in tried again names the same keys: handled as the
//! others (never logged, shown or sent), its own entry so no older Nexus reads it, and read past a
//! field it does not know. And one for each station this computer signed in to with the account
//! but has not yet reached on the station's network (a laptop away from home, a guest who lives
//! elsewhere): kept by the service's id for it until it does, with its name, the station's LAN key
//! as the station's own proof vouched for it (pinned from then on) and this computer's own key for
//! that network, which its sign-in named. Handled as the others, its own entry so no older Nexus
//! reads it, and read past a field it does not know. Its internet road is the station's road entry
//! under the same id, and its id is on the list with the rest.
//!
//! A last entry holds the internet roads this computer keeps held: each road's station id and the
//! fingerprint of the station key it was pinned to, either until the operator confirms that key
//! against the station's own screen, or for good, the station having named another key on its own
//! network (`crate::lan_client::Stations::road_held`). It is its OWN entry, not a field on the
//! road's, because [`InternetRoad`] is `deny_unknown_fields`: a field there would make an older Nexus
//! read the road as none. It holds no secret. One that is missing or does not read holds nothing,
//! so an older Nexus, which never reads it, simply drives each road as it always did.
//!
//! And one more lists the stations this computer paired with a code, each by its id and the
//! fingerprint of the station's LAN key that pairing pinned (`crate::lan_client::Stations::code_paired`):
//! written when a code pairing is kept, and dropped when the station is linked to the account or
//! forgotten. Its own entry, not a field on the station's, for the same reason as the held roads'
//! ([`PairedStation`] is `deny_unknown_fields`). It holds no secret. One that is missing or does not
//! read names no station, so a station reads as paired with a code only while this entry says so.
//!
//! One more belongs to the pairing, as the pins do: the devices whose sign-in transmit hold the
//! operator waived at the shack (Approve, or Allow to transmit), so a restart does not hold them
//! again. Its own entry for the same reason as the pins, and bound to its pairing like them. It is
//! only ever a waiver: a held device is never written to it. One that is missing, locked or does
//! not read waives nothing, and every sign-in device is then held as it would be with no record.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const SERVICE: &str = "org.hamradiotools.nexus.remote";
const ACCOUNT: &str = "pairing";
const STATE: &str = "state";
const PINS: &str = "pins";
const STATION_KEY: &str = "station-key";
const WAIVERS: &str = "waivers";
const LAN_KEY: &str = "lan-key";
const LAN_DEVICES: &str = "lan-devices";
const PAIRED_STATIONS: &str = "lan-stations";
/// One paired station's entry: this, then its LAN station id.
const PAIRED_STATION: &str = "lan-station-";
/// One paired station's internet road: this, then its LAN station id.
const INTERNET_ROAD: &str = "lan-internet-";
/// The two keys made for the next station this computer signs in to with the account.
const STAGED_KEYS: &str = "lan-account-staged";
/// A station signed in to but not yet reached on its network: this, then the service's id for it.
const AWAY_STATION: &str = "lan-away-";
/// The internet roads this computer keeps held.
const HELD_ROADS: &str = "lan-held-roads";
/// The stations this computer paired with a code.
const CODE_PAIRED: &str = "lan-code-paired";
const ACCOUNT_OWNER: &str = "account-owner";
const ACCOUNT_KEYS: &str = "account-keys";
const ACCOUNT_DEVICES: &str = "account-devices";
const ACCOUNT_PEOPLE: &str = "account-people";
const ACCOUNT_REVOKES: &str = "account-revokes";
const ACCOUNT_UNINVITES: &str = "account-uninvites";
const ACCOUNT_NOTIFIED: &str = "account-notified";
const ACCOUNT_TIES: &str = "account-ties";

#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Binding {
    pub origin: String,
    pub station_id: String,
    pub account_id: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    binding: Binding,
    credential: String,
    confirmed: bool,
}

/// One approved browser's grants as they stood when the operator gave them.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Grant {
    pub device_id: String,
    /// The browser's approval expiry as the service last listed it for this approval. Without a
    /// `generation` it identifies the approval: every approval and revocation rewrites it, and a
    /// service that reports no generation never renews one.
    pub expires_at: u64,
    /// The approval generation (browser approval lifetime, operator decision 2026-09-14). The service
    /// renews an approval on use by moving its expiry and never this; approving again or revoking
    /// moves it. So whenever the service reports one, a restart restores against it, and a renewed
    /// browser keeps its grants. Omitted when unknown, which keeps a record in 1.12.0's shape; one
    /// holding it reads as unreadable on 1.12.0, which leaves Remote off with nothing restored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation: Option<u64>,
    /// The old remote logging permission, as an older build wrote it on every grant: read past, so
    /// that record still loads under `deny_unknown_fields`, and written `false` for an earlier build
    /// to read. See the module note.
    #[serde(rename = "logging", default, serialize_with = "never_granted")]
    pub retired_logging: serde::de::IgnoredAny,
    pub control: bool,
    /// The old Remote page's FT8/FT4 transmit, as an older build wrote it: read past, so that record
    /// still loads under `deny_unknown_fields`, and never written. See the module note.
    #[serde(rename = "transmit", default, skip_serializing)]
    pub retired_transmit: serde::de::IgnoredAny,
}
/// A retired permission, as an earlier build reads it: never granted.
fn never_granted<S: serde::Serializer>(
    _: &serde::de::IgnoredAny,
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_bool(false)
}
/// What a restart restores, for exactly one pairing.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct State {
    pub binding: Binding,
    /// Remote was on when this was written. Turn off Remote, Revoke station access and the
    /// connection refusing itself all write `false`.
    pub enabled: bool,
    pub grants: Vec<Grant>,
}
/// The device keys pinned for exactly one pairing: browser id to the SHA-256 of its device key's
/// SPKI, as lowercase hex. A map, and short keys, because the record has to fit the smallest OS
/// credential blob (Windows, 2560 bytes of UTF-16) with a pin for every browser the service lists.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pins {
    pub binding: Binding,
    pub keys: BTreeMap<String, String>,
}

/// The devices whose sign-in transmit hold the operator waived at the shack, for exactly one
/// pairing, by device id.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Waivers {
    pub binding: Binding,
    pub devices: std::collections::BTreeSet<String>,
}

/// The station's signing key for exactly one pairing (S3-M1): its PKCS#8 document, lowercase hex.
/// Deliberately no `Debug` and no `Clone`: nothing may print it, and it is copied only from this
/// entry into the signer and back.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StationKey {
    pub binding: Binding,
    pub pkcs8: String,
}

/// The shack's LAN key (Remote over this network): its PKCS#8 document, lowercase hex, and the LAN
/// station id that goes with it. No `Debug` and no `Clone`, as [`StationKey`]. Read past a field it
/// does not know (see the header).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanKey {
    pub station_id: String,
    pub pkcs8: String,
}

/// The computers paired over this network with the LAN key whose station id this is. Read past a
/// field it does not know, as each [`LanDevice`] is (see the header).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LanDevices {
    pub station_id: String,
    pub devices: Vec<LanDevice>,
}

/// One paired computer: the SHA-256 of its key's SPKI as lowercase hex, which is all that admits
/// it, and the name it gave. Its device id is worked out from the pin, so the record does not
/// carry one. One-letter keys, so eight records fit one entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LanDevice {
    #[serde(rename = "p")]
    pub pin: String,
    #[serde(rename = "n")]
    pub name: String,
}

/// A station this computer is paired with over its network (`crate::lan_client`), as the client
/// core reads and keeps its record.
pub use tempo_client::PairedStation;

/// The stations this computer is paired with, by LAN station id, in the order they were paired.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PairedStations {
    pub stations: Vec<String>,
}

/// The internet road to a station this computer is paired with over its network (N3: a key of its
/// own for each road): the station as the Remote service knows it, the device id the service gave
/// this computer, the station key the service listed (pinned from then on), and this computer's
/// own key for that road, PKCS#8 as lowercase hex. Kept under the LAN station's id, in an entry of
/// its own beside that station's. No `Debug` and no `Clone`: it holds a private key.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InternetRoad {
    pub station_id: String,
    pub device_id: String,
    pub station_key: String,
    pub pkcs8: String,
}

/// The two keys this computer made for the next station it signs in to with the account
/// (`crate::lan_client::account`): one for the internet road and one for the station's network,
/// each PKCS#8 as lowercase hex. No `Debug` and no `Clone`: they are private keys. Read past a field
/// it does not know (see the header).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StagedKeys {
    pub road: String,
    pub lan: String,
}

/// A station this computer signed in to with the account and has not reached on its network yet
/// (`crate::lan_client::account`), kept by the service's id for it until it does: its name as the
/// account listed it; the station's LAN key, SPKI as lowercase hex, exactly as the station's own
/// proof vouched for it at that sign-in (pinned from then on); and this computer's own key for the
/// station's network, PKCS#8 as lowercase hex, which the sign-in's proof named. Its internet road is
/// kept under the same id ([`InternetRoad`]). No `Debug` and no `Clone`: it holds a private key.
/// Read past a field it does not know (see the header).
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwayStation {
    pub station_id: String,
    pub name: String,
    pub station_key: String,
    pub pkcs8: String,
}

/// The internet roads this computer keeps held (see the header). Read past a field it does not know.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct HeldRoads {
    pub roads: Vec<HeldRoad>,
}

/// One held road: the id it is kept under on this computer, the fingerprint (SHA-256 of the SPKI,
/// lowercase hex) of the station key it was pinned to, which binds the hold to that pin alone, and
/// whether the station named another key on its own network (held for good, until the road is
/// added again) rather than waiting for the operator's check. One-letter names, so every station's
/// fits one entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HeldRoad {
    #[serde(rename = "s")]
    pub station_id: String,
    #[serde(rename = "k")]
    pub shown: String,
    #[serde(rename = "d", default, skip_serializing_if = "not")]
    pub disowned: bool,
}

/// `disowned` is written only when true, so a hold that is merely waiting stays two fields.
fn not(value: &bool) -> bool {
    !*value
}

/// The stations this computer paired with a code (see the header). Read past a field it does not
/// know.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct CodePaired {
    pub stations: Vec<CodePairing>,
}

/// One station paired with a code: the id it is kept under on this computer, and the fingerprint
/// (SHA-256 of the SPKI, lowercase hex) of the station's LAN key the code pinned, which binds the
/// entry to that key alone. One-letter names, as a held road's.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CodePairing {
    #[serde(rename = "s")]
    pub station_id: String,
    #[serde(rename = "k")]
    pub shown: String,
}

/// The computer's side of Remote over this network: its entries, kept apart from the shack's and
/// from [`Vault`]'s, so no test of either reaches them, nor they it.
pub trait StationVault: Send + Sync {
    fn paired_stations(&self) -> Result<Option<PairedStations>, &'static str>;
    fn save_paired_stations(&self, list: &PairedStations) -> Result<(), &'static str>;
    fn paired_station(&self, id: &str) -> Result<Option<PairedStation>, &'static str>;
    fn save_paired_station(&self, station: &PairedStation) -> Result<(), &'static str>;
    fn remove_paired_station(&self, id: &str) -> Result<(), &'static str>;
    /// The internet road kept for the station paired here as `id`, if any.
    fn internet_road(&self, id: &str) -> Result<Option<InternetRoad>, &'static str>;
    fn save_internet_road(&self, id: &str, road: &InternetRoad) -> Result<(), &'static str>;
    fn remove_internet_road(&self, id: &str) -> Result<(), &'static str>;
    /// The keys made for the next station this computer signs in to, if any are kept.
    fn staged_keys(&self) -> Result<Option<StagedKeys>, &'static str>;
    fn save_staged_keys(&self, keys: &StagedKeys) -> Result<(), &'static str>;
    fn remove_staged_keys(&self) -> Result<(), &'static str>;
    /// The station kept by the service's id `id` until this computer reaches it on its network, if
    /// any.
    fn away_station(&self, id: &str) -> Result<Option<AwayStation>, &'static str>;
    fn save_away_station(&self, station: &AwayStation) -> Result<(), &'static str>;
    fn remove_away_station(&self, id: &str) -> Result<(), &'static str>;
    /// The internet roads kept held, if any are.
    fn held_roads(&self) -> Result<Option<HeldRoads>, &'static str>;
    fn save_held_roads(&self, held: &HeldRoads) -> Result<(), &'static str>;
    /// The stations paired here with a code, if any are listed.
    fn code_paired(&self) -> Result<Option<CodePaired>, &'static str>;
    fn save_code_paired(&self, paired: &CodePaired) -> Result<(), &'static str>;
}

/// Remote over this network's two entries, kept apart from [`Vault`]'s: a test of the hosted
/// pairing never reaches them, nor they it.
pub trait LanVault: Send + Sync {
    fn lan_key(&self) -> Result<Option<LanKey>, &'static str>;
    fn save_lan_key(&self, key: &LanKey) -> Result<(), &'static str>;
    fn lan_devices(&self) -> Result<Option<LanDevices>, &'static str>;
    fn save_lan_devices(&self, devices: &LanDevices) -> Result<(), &'static str>;
    fn remove_lan_devices(&self) -> Result<(), &'static str>;
}

/// The issuer's key set as the station keeps it, for the issuer it was fetched from: a set kept for
/// another issuer checks nothing.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountKeys {
    pub issuer: String,
    pub keys: tempo_client::account::KeySet,
}

/// The people invited to the station the service knows as `station_id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct People {
    pub station_id: String,
    pub invites: Vec<tempo_client::account::Invite>,
}

/// The removals made at the station that the service known as `station_id` has not heard yet: the
/// ids it lists those devices under.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Revokes {
    pub station_id: String,
    pub devices: Vec<String>,
}

/// What links a computer's roads to this station, and the keys removed here (see the header): both
/// read past a field they do not know.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Ties {
    #[serde(default)]
    pub ties: Vec<Tie>,
    #[serde(default)]
    pub removed: Vec<Removed>,
    /// Unix seconds: a proof signed by then is refused for any key, since a removal it may name was
    /// dropped to keep within the blob (`lan::enrol`). 0 for none.
    #[serde(rename = "f", default, skip_serializing_if = "zero")]
    pub floor: u64,
}

fn zero(value: &u64) -> bool {
    *value == 0
}

/// One computer's LAN key and the internet key a proof named beside it, each as the first sixteen
/// hex digits of its SHA-256 (`lan::enrol::short`), and the guest whose proof it was. `enrolled`:
/// the computer proved the LAN key itself, enrolling on this network; `vouched`: the station
/// checked the proof its listing carries. One-letter names, as for the devices kept for the LAN.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tie {
    #[serde(rename = "p")]
    pub lan: String,
    #[serde(rename = "k")]
    pub key: String,
    /// A guest's address as `tempo_client::account::mailbox` gives it, hashed
    /// (`lan::enrol::mailbox_tag`); `None` for the owner.
    #[serde(rename = "m", default, skip_serializing_if = "Option::is_none")]
    pub mailbox: Option<String>,
    #[serde(rename = "e", default, skip_serializing_if = "not")]
    pub enrolled: bool,
    #[serde(rename = "v", default, skip_serializing_if = "not")]
    pub vouched: bool,
}

/// A key removed at the shack (a LAN key or an internet key, as [`Tie`] shortens it) and when, in
/// unix seconds: a proof signed by then no longer admits it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Removed {
    #[serde(rename = "k")]
    pub key: String,
    #[serde(rename = "x")]
    pub at: u64,
}

/// The invitations taken back at the station that the service known as `station_id` has not heard
/// yet: each address as it was typed.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Uninvites {
    pub station_id: String,
    pub people: Vec<String>,
}

/// The sign-in devices of the station the service knows as `station_id`, bound for `account_id`,
/// whose owner notice the service had confirmed, each with the end of its transmit window in unix
/// seconds (0 once it had elapsed). Only a confirmation the service gave is kept, and each list
/// replaces it: a restart restores the windows, never a confirmation the next list does not give.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Notified {
    pub station_id: String,
    pub account_id: String,
    pub windows: std::collections::BTreeMap<String, u64>,
}

/// The station's account entries (see the header), kept apart from [`Vault`]'s and the LAN's.
pub trait AccountVault: Send + Sync {
    fn owner(&self) -> Result<Option<tempo_client::account::Owner>, &'static str>;
    fn save_owner(&self, owner: &tempo_client::account::Owner) -> Result<(), &'static str>;
    fn account_keys(&self) -> Result<Option<AccountKeys>, &'static str>;
    fn save_account_keys(&self, keys: &AccountKeys) -> Result<(), &'static str>;
    fn device_cache(&self) -> Result<Option<tempo_client::account::DeviceCache>, &'static str>;
    /// Only while the cache is held ([`DevicesHeld`]).
    fn save_device_cache(
        &self,
        held: &DevicesHeld<'_>,
        cache: &tempo_client::account::DeviceCache,
    ) -> Result<(), &'static str>;
    /// The lock [`DevicesHeld`] takes: one for each store, so every writer of its device cache waits
    /// for the others.
    fn devices_lock(&self) -> &std::sync::Mutex<()>;
    fn people(&self) -> Result<Option<People>, &'static str>;
    fn save_people(&self, people: &People) -> Result<(), &'static str>;
    fn revokes(&self) -> Result<Option<Revokes>, &'static str>;
    fn save_revokes(&self, revokes: &Revokes) -> Result<(), &'static str>;
    fn uninvites(&self) -> Result<Option<Uninvites>, &'static str>;
    fn save_uninvites(&self, uninvites: &Uninvites) -> Result<(), &'static str>;
    fn notified(&self) -> Result<Option<Notified>, &'static str>;
    fn save_notified(&self, notified: &Notified) -> Result<(), &'static str>;
    fn ties(&self) -> Result<Option<Ties>, &'static str>;
    /// Only while the devices kept for the LAN are held ([`DevicesHeld`]).
    fn save_ties(&self, held: &DevicesHeld<'_>, ties: &Ties) -> Result<(), &'static str>;
}

/// The LAN's device cache (`account-devices`) held by one writer from its read to its save. It has
/// several: an enrolment adding its computer, each list the station hears (its sweep, and its
/// keeping of a device it vouched for), an expiry, a code pairing, and the station's copy of the
/// cache in memory, which every admission on the LAN reads (`super::lan::enrol`). Two that each read
/// it and saved what they made of it lost one change to the other's save; when the change lost was
/// an enrolment's entry, its computer stayed in the book with no proof's age held to it. A save asks
/// for this, so no writer saves without it.
///
/// Taken before the book's own lock (`super::lan::book`): an enrolment takes the book's while it
/// holds this one, and nothing that holds the book's takes this one (an expiry and a code pairing
/// write the book first and let its lock go).
pub struct DevicesHeld<'a> {
    _held: std::sync::MutexGuard<'a, ()>,
}

impl<'a> DevicesHeld<'a> {
    /// Waits for the device cache in `vault`, then holds it until dropped.
    pub fn take(vault: &'a dyn AccountVault) -> Self {
        // It guards no value of its own (the store is what it guards, and each save is whole), so a
        // writer that panicked holding it left the cache as a crash there would: no reason to stop
        // every writer after it.
        let held = vault.devices_lock().lock();
        Self {
            _held: held.unwrap_or_else(std::sync::PoisonError::into_inner),
        }
    }
}

pub trait Vault: Send + Sync {
    fn binding(&self) -> Result<Option<Binding>, &'static str>;
    fn credential(&self, binding: &Binding) -> Result<String, &'static str>;
    fn stage(&self, binding: &Binding, credential: &str) -> Result<(), &'static str>;
    fn save(&self, binding: &Binding, credential: &str) -> Result<(), &'static str>;
    fn remove(&self, binding: &Binding) -> Result<(), &'static str>;
    fn state(&self) -> Result<Option<State>, &'static str>;
    fn save_state(&self, state: &State) -> Result<(), &'static str>;
    fn remove_state(&self) -> Result<(), &'static str>;
    fn pins(&self) -> Result<Option<Pins>, &'static str>;
    fn save_pins(&self, pins: &Pins) -> Result<(), &'static str>;
    fn remove_pins(&self) -> Result<(), &'static str>;
    fn waivers(&self) -> Result<Option<Waivers>, &'static str>;
    fn save_waivers(&self, waivers: &Waivers) -> Result<(), &'static str>;
    fn remove_waivers(&self) -> Result<(), &'static str>;
    fn station_key(&self) -> Result<Option<StationKey>, &'static str>;
    fn save_station_key(&self, key: &StationKey) -> Result<(), &'static str>;
    fn remove_station_key(&self) -> Result<(), &'static str>;
}

pub struct SystemVault;
fn entry(account: &str) -> Result<keyring::Entry, &'static str> {
    keyring::Entry::new(SERVICE, account).map_err(|_| "credentialStoreUnavailable")
}
fn read() -> Result<Option<Record>, &'static str> {
    match entry(ACCOUNT)?.get_password() {
        Ok(value) => serde_json::from_str(&value)
            .map(Some)
            .map_err(|_| "credentialStoreUnavailable"),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("credentialStoreUnavailable"),
    }
}
fn write(binding: &Binding, credential: &str, confirmed: bool) -> Result<(), &'static str> {
    let previous = read()?;
    if previous.is_some_and(|record| record.confirmed && record.binding != *binding) {
        return Err("credentialStoreUnavailable");
    }
    let record = Record {
        binding: binding.clone(),
        credential: credential.into(),
        confirmed,
    };
    let value = serde_json::to_string(&record).map_err(|_| "credentialStoreUnavailable")?;
    entry(ACCOUNT)?
        .set_password(&value)
        .map_err(|_| "credentialStoreUnavailable")
}
impl Vault for SystemVault {
    fn binding(&self) -> Result<Option<Binding>, &'static str> {
        Ok(read()?
            .filter(|record| record.confirmed)
            .map(|record| record.binding))
    }
    fn credential(&self, binding: &Binding) -> Result<String, &'static str> {
        read()?
            .filter(|record| record.confirmed && record.binding == *binding)
            .map(|record| record.credential)
            .ok_or("credentialStoreUnavailable")
    }
    fn stage(&self, binding: &Binding, credential: &str) -> Result<(), &'static str> {
        // A restart cannot restore a locally staged, unconfirmed enrollment.
        write(binding, credential, false)
    }
    fn save(&self, binding: &Binding, credential: &str) -> Result<(), &'static str> {
        write(binding, credential, true)
    }
    fn remove(&self, binding: &Binding) -> Result<(), &'static str> {
        if read()?.is_some_and(|record| record.binding != *binding) {
            return Err("credentialStoreUnavailable");
        }
        // Called after cloud revocation. A failed delete leaves the entire entry
        // available for retry, including after a restart; no orphan locator.
        match entry(ACCOUNT)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn state(&self) -> Result<Option<State>, &'static str> {
        match entry(STATE)?.get_password() {
            // An unreadable record is not an error to surface: it restores nothing, so Remote
            // stays off, which is the answer a corrupt record must get.
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_state(&self, state: &State) -> Result<(), &'static str> {
        let value = serde_json::to_string(state).map_err(|_| "credentialStoreUnavailable")?;
        entry(STATE)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn remove_state(&self) -> Result<(), &'static str> {
        match entry(STATE)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn pins(&self) -> Result<Option<Pins>, &'static str> {
        match entry(PINS)?.get_password() {
            // Unreadable pins pin nothing: every browser is approved again before it streams.
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_pins(&self, pins: &Pins) -> Result<(), &'static str> {
        let value = serde_json::to_string(pins).map_err(|_| "credentialStoreUnavailable")?;
        entry(PINS)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn remove_pins(&self) -> Result<(), &'static str> {
        match entry(PINS)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn waivers(&self) -> Result<Option<Waivers>, &'static str> {
        match entry(WAIVERS)?.get_password() {
            // Unreadable waivers waive nothing: every sign-in device is held.
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_waivers(&self, waivers: &Waivers) -> Result<(), &'static str> {
        let value = serde_json::to_string(waivers).map_err(|_| "credentialStoreUnavailable")?;
        entry(WAIVERS)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn remove_waivers(&self) -> Result<(), &'static str> {
        match entry(WAIVERS)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn station_key(&self) -> Result<Option<StationKey>, &'static str> {
        match entry(STATION_KEY)?.get_password() {
            // Unreadable is no key: the caller makes a new one, and the service, which already
            // holds the old one, refuses it by name. Pairing again is the way back.
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_station_key(&self, key: &StationKey) -> Result<(), &'static str> {
        let value = serde_json::to_string(key).map_err(|_| "credentialStoreUnavailable")?;
        entry(STATION_KEY)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn remove_station_key(&self) -> Result<(), &'static str> {
        match entry(STATION_KEY)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
}
/// One of Remote over this network's two entries, as the store gave it: read past any field it does
/// not know, and `Err` for one that does not read at all, as for a store that would not answer, so
/// it is never taken for no entry and nothing is made or written over it (see the header).
pub(crate) fn lan_entry<T: serde::de::DeserializeOwned>(
    value: &str,
) -> Result<Option<T>, &'static str> {
    serde_json::from_str(value)
        .map(Some)
        .map_err(|_| "unreadable")
}

impl LanVault for SystemVault {
    fn lan_key(&self) -> Result<Option<LanKey>, &'static str> {
        match entry(LAN_KEY)?.get_password() {
            // One that does not read is not none: no key is made over it.
            Ok(value) => lan_entry(&value),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_lan_key(&self, key: &LanKey) -> Result<(), &'static str> {
        let value = serde_json::to_string(key).map_err(|_| "credentialStoreUnavailable")?;
        entry(LAN_KEY)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn lan_devices(&self) -> Result<Option<LanDevices>, &'static str> {
        match entry(LAN_DEVICES)?.get_password() {
            // One that does not read admits nobody, and is not written over.
            Ok(value) => lan_entry(&value),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_lan_devices(&self, devices: &LanDevices) -> Result<(), &'static str> {
        let value = serde_json::to_string(devices).map_err(|_| "credentialStoreUnavailable")?;
        entry(LAN_DEVICES)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn remove_lan_devices(&self) -> Result<(), &'static str> {
        match entry(LAN_DEVICES)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
}
/// One paired station's entry, by its LAN station id: only an id names an entry, so nothing else a
/// station says can reach another account in the store.
fn paired_entry(id: &str) -> Result<keyring::Entry, &'static str> {
    if !tempo_stream::protocol::identifier(id) {
        return Err("credentialStoreUnavailable");
    }
    entry(&format!("{PAIRED_STATION}{id}"))
}
impl StationVault for SystemVault {
    fn paired_stations(&self) -> Result<Option<PairedStations>, &'static str> {
        match entry(PAIRED_STATIONS)?.get_password() {
            // An unreadable list names no station: each is paired again.
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_paired_stations(&self, list: &PairedStations) -> Result<(), &'static str> {
        let value = serde_json::to_string(list).map_err(|_| "credentialStoreUnavailable")?;
        entry(PAIRED_STATIONS)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn paired_station(&self, id: &str) -> Result<Option<PairedStation>, &'static str> {
        match paired_entry(id)?.get_password() {
            // Unreadable is no pairing, as the station key's is: pair that station again.
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_paired_station(&self, station: &PairedStation) -> Result<(), &'static str> {
        let value = serde_json::to_string(station).map_err(|_| "credentialStoreUnavailable")?;
        paired_entry(&station.station_id)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn remove_paired_station(&self, id: &str) -> Result<(), &'static str> {
        match paired_entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn internet_road(&self, id: &str) -> Result<Option<InternetRoad>, &'static str> {
        match internet_entry(id)?.get_password() {
            // Unreadable is no road: the operator adds it again.
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_internet_road(&self, id: &str, road: &InternetRoad) -> Result<(), &'static str> {
        let value = serde_json::to_string(road).map_err(|_| "credentialStoreUnavailable")?;
        internet_entry(id)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn remove_internet_road(&self, id: &str) -> Result<(), &'static str> {
        match internet_entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn staged_keys(&self) -> Result<Option<StagedKeys>, &'static str> {
        match entry(STAGED_KEYS)?.get_password() {
            // Unreadable is none: new keys are made, and nothing was kept with those.
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_staged_keys(&self, keys: &StagedKeys) -> Result<(), &'static str> {
        let value = serde_json::to_string(keys).map_err(|_| "credentialStoreUnavailable")?;
        entry(STAGED_KEYS)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn remove_staged_keys(&self) -> Result<(), &'static str> {
        match entry(STAGED_KEYS)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn away_station(&self, id: &str) -> Result<Option<AwayStation>, &'static str> {
        match away_entry(id)?.get_password() {
            // Unreadable is none, as a paired station's is: that station is signed in to again.
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_away_station(&self, station: &AwayStation) -> Result<(), &'static str> {
        let value = serde_json::to_string(station).map_err(|_| "credentialStoreUnavailable")?;
        away_entry(&station.station_id)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn remove_away_station(&self, id: &str) -> Result<(), &'static str> {
        match away_entry(id)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn held_roads(&self) -> Result<Option<HeldRoads>, &'static str> {
        match entry(HELD_ROADS)?.get_password() {
            // Unreadable holds nothing (see the header).
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_held_roads(&self, held: &HeldRoads) -> Result<(), &'static str> {
        let value = serde_json::to_string(held).map_err(|_| "credentialStoreUnavailable")?;
        entry(HELD_ROADS)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
    fn code_paired(&self) -> Result<Option<CodePaired>, &'static str> {
        match entry(CODE_PAIRED)?.get_password() {
            // Unreadable names no station (see the header).
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
    fn save_code_paired(&self, paired: &CodePaired) -> Result<(), &'static str> {
        let value = serde_json::to_string(paired).map_err(|_| "credentialStoreUnavailable")?;
        entry(CODE_PAIRED)?
            .set_password(&value)
            .map_err(|_| "credentialStoreUnavailable")
    }
}

/// A station's entry while it is kept by the service's id ([`AwayStation`]): only an id names one,
/// as for a paired station.
fn away_entry(id: &str) -> Result<keyring::Entry, &'static str> {
    if !tempo_stream::protocol::identifier(id) {
        return Err("credentialStoreUnavailable");
    }
    entry(&format!("{AWAY_STATION}{id}"))
}

/// One of the account's entries, as the store gave it: read as the LAN's are ([`lan_entry`]).
fn account_entry<T: serde::de::DeserializeOwned>(name: &str) -> Result<Option<T>, &'static str> {
    match entry(name)?.get_password() {
        Ok(value) => lan_entry(&value),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(_) => Err("credentialStoreUnavailable"),
    }
}
fn save_account_entry<T: Serialize>(name: &str, value: &T) -> Result<(), &'static str> {
    let value = serde_json::to_string(value).map_err(|_| "credentialStoreUnavailable")?;
    entry(name)?
        .set_password(&value)
        .map_err(|_| "credentialStoreUnavailable")
}
impl AccountVault for SystemVault {
    fn owner(&self) -> Result<Option<tempo_client::account::Owner>, &'static str> {
        account_entry(ACCOUNT_OWNER)
    }
    fn save_owner(&self, owner: &tempo_client::account::Owner) -> Result<(), &'static str> {
        save_account_entry(ACCOUNT_OWNER, owner)
    }
    fn account_keys(&self) -> Result<Option<AccountKeys>, &'static str> {
        account_entry(ACCOUNT_KEYS)
    }
    fn save_account_keys(&self, keys: &AccountKeys) -> Result<(), &'static str> {
        save_account_entry(ACCOUNT_KEYS, keys)
    }
    fn device_cache(&self) -> Result<Option<tempo_client::account::DeviceCache>, &'static str> {
        account_entry(ACCOUNT_DEVICES)
    }
    fn save_device_cache(
        &self,
        _held: &DevicesHeld<'_>,
        cache: &tempo_client::account::DeviceCache,
    ) -> Result<(), &'static str> {
        save_account_entry(ACCOUNT_DEVICES, cache)
    }
    fn devices_lock(&self) -> &std::sync::Mutex<()> {
        // One for the process: every `SystemVault` is the same credential store.
        static DEVICES: std::sync::Mutex<()> = std::sync::Mutex::new(());
        &DEVICES
    }
    fn people(&self) -> Result<Option<People>, &'static str> {
        account_entry(ACCOUNT_PEOPLE)
    }
    fn save_people(&self, people: &People) -> Result<(), &'static str> {
        save_account_entry(ACCOUNT_PEOPLE, people)
    }
    fn revokes(&self) -> Result<Option<Revokes>, &'static str> {
        account_entry(ACCOUNT_REVOKES)
    }
    fn save_revokes(&self, revokes: &Revokes) -> Result<(), &'static str> {
        save_account_entry(ACCOUNT_REVOKES, revokes)
    }
    fn uninvites(&self) -> Result<Option<Uninvites>, &'static str> {
        account_entry(ACCOUNT_UNINVITES)
    }
    fn save_uninvites(&self, uninvites: &Uninvites) -> Result<(), &'static str> {
        save_account_entry(ACCOUNT_UNINVITES, uninvites)
    }
    fn notified(&self) -> Result<Option<Notified>, &'static str> {
        account_entry(ACCOUNT_NOTIFIED)
    }
    fn save_notified(&self, notified: &Notified) -> Result<(), &'static str> {
        save_account_entry(ACCOUNT_NOTIFIED, notified)
    }
    fn ties(&self) -> Result<Option<Ties>, &'static str> {
        account_entry(ACCOUNT_TIES)
    }
    fn save_ties(&self, _held: &DevicesHeld<'_>, ties: &Ties) -> Result<(), &'static str> {
        save_account_entry(ACCOUNT_TIES, ties)
    }
}

fn internet_entry(id: &str) -> Result<keyring::Entry, &'static str> {
    if !tempo_stream::protocol::identifier(id) {
        return Err("credentialStoreUnavailable");
    }
    entry(&format!("{INTERNET_ROAD}{id}"))
}
