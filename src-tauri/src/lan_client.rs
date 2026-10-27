//! Remote over this network, on the computer an operator works FROM: the other end of the shack's
//! own listener (`remote_service::lan`), carried in a Nexus window of its own (the operator's
//! rulings of 2026-10-04: "Window only in v1", so never a browser; "Both at once", beside the hosted
//! road; "Refuse, say which" for versions). No relay, no sign-in and no internet.
//!
//! ## What it holds, and what it never does
//!
//! - **Its own key for each station** ([`ComputerKey`]): P-256, made with ring when this computer
//!   pairs with that station, and kept with the station's record in the OS credential store, and
//!   nowhere else. This is the design's second key, approved on 2026-10-04 ("Approve as designed"),
//!   and the only secret this module keeps. It proves this computer twice over, in the TLS
//!   handshake and on every stream offer (A5), and the two signatures cover different labels, so
//!   neither passes for the other. The private half is never logged, shown or sent: the page never
//!   holds it, and nothing that carries it has `Debug`, `Clone` or `Serialize`.
//! - **Each station's record** ([`Stations`]): the station's key as it presented it while pairing,
//!   pinned from then on; its LAN station id; the device id it gave this computer; and the
//!   addresses it answered at, the last one that worked first. One credential entry for each
//!   station, within the smallest one Windows keeps (2560 bytes of UTF-16), and the list of them
//!   in one more.
//! - **What the operator typed for each station** ([`Stations::typed`]): the address or host name
//!   Operate tries first, once Operate opened there or the operator set it on the station's row.
//!   Not a secret, so it is kept in a file of its own ([`typed_path`]), never in the credential
//!   store: keeping, changing or clearing it reads and writes no entry that holds a key.
//! - **When this computer signed in for each station** ([`Stations::signed`]): for a station added
//!   with the account ([`account`]), the time its proof was signed, from which the station admits
//!   it on its network for thirty days. Not a secret either: a file of its own ([`signed_path`]).
//!   Pairing with a code drops it ([`Stations::drop_signed`]): the station then admits this
//!   computer until it is removed there.
//! - **Which stations were paired with a code** ([`Stations::code_paired`]): each one's id and its
//!   LAN key's fingerprint, in one credential entry of its own, written when a code pairing is kept
//!   ([`Stations::keep_coded`]) and dropped when the station is linked to the account or forgotten.
//!   Never read from the signed file, which is a plain file: a station it does not name is not
//!   taken as paired with a code.
//! - **The keys for the next station signed in to** ([`Stations::staged`]): made before that
//!   sign-in, and kept in the credential store until a station is kept with them.
//! - **A station signed in to from elsewhere** ([`Stations::keep_away`]): one this computer signed
//!   in to with the account but did not find on the network it was on (a laptop away from home, a
//!   guest who lives elsewhere). It is kept by the service's id for it, with its internet road, the
//!   station's LAN key as its own proof vouched for it and this computer's key for that network,
//!   until this computer first reaches it there ([`Stations::fold`]); then it is kept as any other.
//!   Until then Operate drives it over the internet road alone (`crate::client_mode`).
//! - **No station takes another's place.** The id a station says (pairing, enrolling) and the id
//!   the service lists a station by are their word, not a proof: a record is never written in place
//!   of one stored under the same id with another key, nor of a station kept by its service id, nor
//!   of another station's road ([`Stations::keep`], [`Stations::keep_away`], [`Stations::fold`]).
//! - **No engine, no radio and no operations authority.** This computer's own station, if it has
//!   one, is out of reach: nothing here names the engine, and the window's page reaches no Nexus
//!   command (the loopback origin, below).
//! - **This computer's headset, in client mode** ([`audio_road`]): the station's audio out and the
//!   operator's microphone in, on this computer's own devices, opened only while wanted. It plays
//!   and captures; nothing here keys anything.
//!
//! ## Signing in ([`account`])
//!
//! The way a station is added now: one sign-in with the station's account, and the station is on
//! this computer on its network and over the internet, with no code and nothing pressed at the
//! station. Code pairing stays beside it for a station whose owner has not signed in at the shack.
//!
//! ## Pairing ([`pairing`])
//!
//! The computer's half of the shack's ceremony, with the shack's own functions
//! (`remote_service::lan::pairing`). The operator types the code the shack shows: sixteen
//! hexadecimal characters, either case, spaces anywhere ([`code`]). This computer makes a key,
//! connects with no station key pinned (that one connection only), checks the station's proof
//! BEFORE it sends its own, and keeps the record when the station says `paired`. Each refusal
//! comes back as a code the page says in a sentence.
//!
//! ## The road ([`road`])
//!
//! TLS 1.3 with this computer's key and the station's key pinned: any other key is refused before
//! this computer sends its own. Then the hello: the versions must be the station's own, or the page
//! is told which side to update. After the welcome, the page's operation requests (state, acquire,
//! heartbeat, release and Stop) and its stream signals go to the station, and the station's
//! answers, stream state and status line come back. Two things happen here, in Rust, so the key
//! never enters the page: every offer is signed with this computer's key for this station and
//! session (A5), and every answer is checked against the station's pinned key before the page
//! sees it (S3-M1). The page checks the answer again with its own code.
//!
//! ## Found by name (the operator's ruling of 2026-10-04, "By name, or typed")
//!
//! The pairing dialog's address field offers the stations Windows' own DNS-SD finds on this
//! computer's networks (`tempo_stream::lan::dnssd::find`, through the page's socket), and a typed
//! address still works. A paired station is tried where it answered before, the last address that
//! worked first, and only when none of those answers, or another key answers there, where a look
//! by name finds it: an advert whose key tag starts the key pinned for it. A station found by name is a hint, never an
//! identity, since the road takes only the pinned key.
//!
//! ## The loopback origin ([`origin`])
//!
//! The window's page is served from `http://127.0.0.1:<port>`, which Tauri treats as a remote
//! origin: the page reaches NO Nexus command, as no hosted page does (`remote_window`'s tests hold
//! both). It talks to this module over one WebSocket on
//! the same origin. The listener binds the loopback address only, lives while the window does,
//! and answers a request only through all three of its gates ([`origin::Gates`]), each of which
//! refuses on its own:
//!
//! 1. **The launch secret**: 32 random bytes made each time the window opens, the first segment of
//!    every path. The window's address carries it; nothing else knows it.
//! 2. **The `Host` header**: exactly `127.0.0.1:<port>`, so a page that points a name of its own
//!    at this address (DNS rebinding) is refused.
//! 3. **The `Origin` header**: the WebSocket only from `http://127.0.0.1:<port>`, the window's own
//!    page, and any other request that names another origin is refused too.
//!
//! ## Control taken back by the hosted road
//!
//! The station ends this computer's lease as a side effect of decisions about the hosted road (Turn
//! Remote on or off, revoking a browser), and gives the grant back at once. The page acquires
//! again after `notController` or `leaseExpired`, when nobody else holds control; nothing at the
//! station changes for it.
/// Signing in with the station's account: adding a station, linking one paired with a code, and
/// signing in again before a station stops admitting this computer on its network.
pub(crate) mod account;
/// The audio road, beside the command link, and this computer's headset (the client core's).
pub(crate) use tempo_client::audio_road;
/// The command link, protocol 2, which client mode drives (the client core's).
pub(crate) use tempo_client::link;
/// The internet road: adding it to a paired station, and opening it again.
pub(crate) mod internet;
mod origin;
pub(crate) mod pairing;
/// The road to a paired station (the client core's).
pub(crate) use tempo_client::road;
/// Stop's second road, beside the command link (the client core's).
pub(crate) use tempo_client::stop_road;
#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
pub(crate) use origin::PREFERRED_PORT;
pub(crate) use origin::{Origin, Reach};

use std::collections::BTreeMap;
use std::net::SocketAddrV4;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tempo_client::hex;
/// This computer's key for a station, and a paired station as the page shows it (the client
/// core's).
pub(crate) use tempo_client::{ComputerKey, StationView};
use tempo_stream::lan::Typed;
use tempo_stream::protocol;

use crate::remote_service::lan::{valid_name, NAME_CHARS};
use crate::remote_service::vault::{
    AwayStation, CodePaired, CodePairing, HeldRoad, HeldRoads, InternetRoad, PairedStation,
    PairedStations, StagedKeys, StationVault,
};

/// The most stations this computer keeps a pairing with, those kept by their service id included.
pub(crate) const MAX_STATIONS: usize = 8;
/// The longest name kept for a station kept by its service id, in characters: as long as the
/// account's stations are offered by name.
pub(crate) const AWAY_NAME_CHARS: usize = 48;
/// The most addresses one station's record keeps.
pub(crate) const MAX_ADDRESSES: usize = 4;

/// The most the typed file is read at: one line for each of [`MAX_STATIONS`], with room to spare.
const TYPED_FILE_BYTES: u64 = 16 * 1024;

/// Where the app keeps what the operator typed for each station: beside the default profile's
/// settings, as the paired stations are this computer's, not one profile's.
pub(crate) fn typed_path() -> PathBuf {
    crate::config_dir_for(None).join("lan-typed.json")
}

/// Where the app keeps when this computer signed in for each station it added with the account,
/// beside the typed file.
pub(crate) fn signed_path() -> PathBuf {
    crate::config_dir_for(None).join("lan-account.json")
}
/// The smallest OS credential blob, Windows': 2560 bytes of UTF-16.
const CREDENTIAL_BYTES: usize = 2560;

/// The code as the operator typed it: sixteen hexadecimal characters, either case, with spaces
/// anywhere (the shack shows it in groups of four). `None` for anything else.
pub(crate) fn code(typed: &str) -> Option<[u8; 8]> {
    let hex: String = typed
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase();
    if hex.len() != 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    protocol::hex_bytes(&hex)?.try_into().ok()
}

/// This computer's name as the operating system gives it, for the station to show beside its key:
/// at most [`NAME_CHARS`] characters, or empty when there is none to give, and the operator types
/// one.
pub(crate) fn computer_name() -> String {
    let given = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_default();
    let name: String = given
        .chars()
        .filter(|c| !c.is_control())
        .take(NAME_CHARS)
        .collect();
    if valid_name(&name) {
        name.trim().to_string()
    } else {
        String::new()
    }
}

/// Does this record fit the smallest credential blob?
fn fits<T: Serialize>(record: &T) -> bool {
    serde_json::to_string(record)
        .is_ok_and(|value| value.encode_utf16().count() * 2 <= CREDENTIAL_BYTES)
}

/// Is this a record this module could have written: ids that are ids, a station key that is a
/// P-256 SPKI, a key that is one, and addresses that are private? A record that is not reads as
/// none: that station is paired again.
fn sound(record: &PairedStation) -> bool {
    protocol::identifier(&record.station_id)
        && protocol::identifier(&record.device_id)
        && protocol::device_key(&record.station_key)
        && !record.addresses.is_empty()
        && record.addresses.len() <= MAX_ADDRESSES
        && record
            .addresses
            .iter()
            .all(|a| tempo_stream::lan::typed(a).is_some())
        && ComputerKey::restore(&record.pkcs8).is_some()
}

/// Are these keys this module could have made: two P-256 keys? Ones that are not read as none: new
/// keys are made.
fn sound_staged(keys: &StagedKeys) -> bool {
    ComputerKey::restore(&keys.road).is_some() && ComputerKey::restore(&keys.lan).is_some()
}

/// Is this an internet road this module could have written: the service's ids, a station key that
/// is a P-256 SPKI, and a key that is one? One that is not reads as none: the road is added again.
fn sound_road(road: &InternetRoad) -> bool {
    protocol::identifier(&road.station_id)
        && protocol::identifier(&road.device_id)
        && protocol::device_key(&road.station_key)
        && ComputerKey::restore(&road.pkcs8).is_some()
}

/// Is this a station kept by its service id that this module could have written: an id that is
/// one, a name no longer than one is kept, the station's LAN key a P-256 SPKI, and a key that is
/// one? One that is not reads as none: that station is signed in to again.
fn sound_away(record: &AwayStation) -> bool {
    protocol::identifier(&record.station_id)
        && record.name.chars().count() <= AWAY_NAME_CHARS
        && protocol::device_key(&record.station_key)
        && ComputerKey::restore(&record.pkcs8).is_some()
}

/// What is stored under an id that a new record under it could replace, by the key each names: the
/// station's LAN key of the record on this network and of the record kept by the service's id, and
/// the station key the internet road was pinned to. An entry that reads as none here is none.
struct Held {
    lan: Option<String>,
    away: Option<String>,
    road: Option<String>,
}

/// A station kept by its service id, as the page shows it: that id, its name as the account listed
/// it, and its LAN key's fingerprint (SHA-256 of the key, lowercase hex), as a station on this
/// network is shown. Never this computer's key.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(crate) struct AwayView {
    pub id: String,
    pub name: String,
    pub key: String,
}

impl AwayView {
    fn of(record: &AwayStation) -> Option<Self> {
        let spki = protocol::hex_bytes(&record.station_key)?;
        let key = ring::digest::digest(&ring::digest::SHA256, &spki);
        Some(Self {
            id: record.station_id.clone(),
            name: record.name.clone(),
            key: hex(key.as_ref()),
        })
    }
}

/// The stations this computer is paired with, in the OS credential store. The store can block, so
/// a caller off any thread that must not wait makes these calls.
pub(crate) struct Stations {
    vault: Arc<dyn StationVault>,
    /// The file that keeps what the operator typed for each station ([`Stations::keep_typed`]),
    /// none in a test that keeps nothing typed.
    typed: Option<PathBuf>,
    /// The file that keeps when this computer signed in for each station ([`Stations::keep_signed`]),
    /// none in a test that keeps nothing of it.
    signed: Option<PathBuf>,
    /// One writer at a time, so two presses never interleave their writes to the list.
    writing: Mutex<()>,
}

impl Stations {
    pub(crate) fn new(vault: Arc<dyn StationVault>) -> Self {
        Self {
            vault,
            typed: None,
            signed: None,
            writing: Mutex::new(()),
        }
    }

    /// The same stations, with what the operator typed for each kept in the file at `path`.
    pub(crate) fn typed_in(mut self, path: PathBuf) -> Self {
        self.typed = Some(path);
        self
    }

    /// The same stations, with when this computer signed in for each kept in the file at `path`.
    pub(crate) fn signed_in(mut self, path: PathBuf) -> Self {
        self.signed = Some(path);
        self
    }

    fn ids(&self) -> Result<Vec<String>, &'static str> {
        Ok(self
            .vault
            .paired_stations()
            .map_err(|_| "storeUnavailable")?
            .map(|list| list.stations)
            .unwrap_or_default()
            .into_iter()
            .filter(|id| protocol::identifier(id))
            .collect())
    }

    /// Every paired station the store holds a sound record for, in the order they were paired.
    pub(crate) fn list(&self) -> Result<Vec<StationView>, &'static str> {
        let typed = self.typed_file();
        let mut views: Vec<StationView> =
            self.records()?.iter().filter_map(StationView::of).collect();
        for view in &mut views {
            view.typed_address = typed.get(&view.id).map(Typed::to_string);
        }
        Ok(views)
    }

    /// The same stations' records, this computer's key for each included: what pairing with one of
    /// them again takes that key from ([`pairing::pair`]).
    pub(crate) fn records(&self) -> Result<Vec<PairedStation>, &'static str> {
        let mut records = Vec::new();
        for id in self.ids()? {
            records.extend(self.get(&id)?);
        }
        Ok(records)
    }

    /// The record for station `id`, if the store holds a sound one.
    pub(crate) fn get(&self, id: &str) -> Result<Option<PairedStation>, &'static str> {
        if !protocol::identifier(id) {
            return Ok(None);
        }
        Ok(self
            .vault
            .paired_station(id)
            .map_err(|_| "storeUnavailable")?
            .filter(|record| record.station_id == id && sound(record)))
    }

    /// What is stored under `id`, by the key each entry names ([`Held`]).
    fn held(&self, id: &str) -> Result<Held, &'static str> {
        Ok(Held {
            lan: self.get(id)?.map(|record| record.station_key),
            away: self.away(id)?.map(|record| record.station_key),
            road: self.internet(id)?.map(|road| road.station_key),
        })
    }

    /// Keep a station's record: a new pairing, or the same station paired again (its record is
    /// replaced). The record first, then the list, so a crash between them leaves a record nobody
    /// lists, never a listed station with no record. `stationsFull` with [`MAX_STATIONS`] paired
    /// already, or a record that would not fit one entry. `notStation` for a record under the id of
    /// another station kept here (another key, or one kept by its service id): the id is the
    /// station's own word, and never replaces a station this computer holds.
    pub(crate) fn keep(&self, record: &PairedStation) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if !sound(record) {
            return Err("notStation");
        }
        if !fits(record) {
            return Err("stationsFull");
        }
        let held = self.held(&record.station_id)?;
        if held.lan.is_some_and(|key| key != record.station_key) || held.away.is_some() {
            return Err("notStation");
        }
        let mut ids = self.ids()?;
        ids.retain(|id| *id != record.station_id);
        if ids.len() >= MAX_STATIONS {
            return Err("stationsFull");
        }
        ids.push(record.station_id.clone());
        let list = PairedStations { stations: ids };
        if !fits(&list) {
            return Err("stationsFull");
        }
        self.vault
            .save_paired_station(record)
            .map_err(|_| "storeUnavailable")?;
        self.vault
            .save_paired_stations(&list)
            .map_err(|_| "storeUnavailable")
    }

    /// Station `id` answered at `at`: that address goes first, so it is the first tried next time.
    pub(crate) fn worked(&self, id: &str, at: SocketAddrV4) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        let Some(mut record) = self.get(id)? else {
            return Err("unknownStation");
        };
        let at = at.to_string();
        if record.addresses.first() == Some(&at) {
            return Ok(());
        }
        record.addresses.retain(|a| *a != at);
        record.addresses.insert(0, at);
        record.addresses.truncate(MAX_ADDRESSES);
        self.vault
            .save_paired_station(&record)
            .map_err(|_| "storeUnavailable")
    }

    /// Keep `typed`, an address or a host name, as the one Operate tries first for station `id`,
    /// or none, in the typed file. Only the list of paired stations, which holds ids alone, is
    /// asked whether `id` is paired here: no entry that holds a key is read or written. Lines for
    /// stations no longer paired here go with the write.
    pub(crate) fn keep_typed(&self, id: &str, typed: Option<&Typed>) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        let ids = self.ids()?;
        if !ids.iter().any(|kept| kept == id) {
            return Err("unknownStation");
        }
        let mut file = self.typed_file();
        if file.get(id) == typed {
            return Ok(());
        }
        match typed {
            Some(typed) => file.insert(id.to_string(), typed.clone()),
            None => file.remove(id),
        };
        file.retain(|kept, _| ids.contains(kept));
        self.write_typed(&file)
    }

    /// What the operator typed for station `id`, kept for Operate to try first, if any.
    pub(crate) fn typed(&self, id: &str) -> Option<Typed> {
        self.typed_file().remove(id)
    }

    /// The typed file, each station id with what was typed for it. A file that is missing, too
    /// big, not JSON, or not ids to addresses or host names is nothing typed; a line that is not
    /// one is skipped.
    fn typed_file(&self) -> BTreeMap<String, Typed> {
        let Some(path) = &self.typed else {
            return BTreeMap::new();
        };
        let text = match std::fs::metadata(path) {
            Ok(meta) if meta.len() <= TYPED_FILE_BYTES => std::fs::read_to_string(path).ok(),
            _ => None,
        };
        let lines: BTreeMap<String, serde_json::Value> = text
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        lines
            .into_iter()
            .filter_map(|(id, typed)| Some((id, Typed::read(typed.as_str()?)?)))
            .collect()
    }

    /// Write the typed file, through a temporary file the rename publishes, so a crash mid-write
    /// leaves the old file or the new one, never half of one.
    fn write_typed(&self, file: &BTreeMap<String, Typed>) -> Result<(), &'static str> {
        let Some(path) = &self.typed else {
            return Err("storeUnavailable");
        };
        let lines: BTreeMap<&str, String> = file
            .iter()
            .map(|(id, typed)| (id.as_str(), typed.to_string()))
            .collect();
        let text = serde_json::to_string(&lines).map_err(|_| "storeUnavailable")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|_| "storeUnavailable")?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|_| "storeUnavailable")?;
        std::fs::rename(&tmp, path).map_err(|_| "storeUnavailable")
    }

    /// The internet road kept for station `id`, if the store holds a sound one.
    pub(crate) fn internet(&self, id: &str) -> Result<Option<InternetRoad>, &'static str> {
        if !protocol::identifier(id) {
            return Ok(None);
        }
        Ok(self
            .vault
            .internet_road(id)
            .map_err(|_| "storeUnavailable")?
            .filter(sound_road))
    }

    /// Keep the internet road to station `id`, paired here: a new one replaces any before it.
    pub(crate) fn keep_internet(&self, id: &str, road: &InternetRoad) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if self.get(id)?.is_none() {
            return Err("unknownStation");
        }
        if !sound_road(road) {
            return Err("notStation");
        }
        if !fits(road) {
            return Err("stationsFull");
        }
        self.vault
            .save_internet_road(id, road)
            .map_err(|_| "storeUnavailable")
    }

    /// The time the proof of this computer's last sign-in for station `id` was signed, seconds
    /// since 1970, when it was added with the account; none for a station paired with a code.
    pub(crate) fn signed(&self, id: &str) -> Option<u64> {
        self.signed_file().remove(id)
    }

    /// Keep `issued_at`, the time the proof of this computer's sign-in for station `id` was signed,
    /// paired here. Lines for stations no longer paired here go with the write.
    pub(crate) fn keep_signed(&self, id: &str, issued_at: u64) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        let ids = self.ids()?;
        if !ids.iter().any(|kept| kept == id) {
            return Err("unknownStation");
        }
        // Admitted on a sign-in from now on, not on a code: it reads so before the line is written.
        self.unmark(id)?;
        let mut file = self.signed_file();
        file.insert(id.to_string(), issued_at);
        file.retain(|kept, _| ids.contains(kept));
        self.write_signed(&file)
    }

    /// Station `id` was paired with a code here: it admits this computer until it is removed there,
    /// whatever a sign-in said before, so its line goes from the signed file, and with it the day
    /// its row names and Operate's launch signing in again for it. No line, nothing written.
    pub(crate) fn drop_signed(&self, id: &str) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        let mut file = self.signed_file();
        if file.remove(id).is_none() {
            return Ok(());
        }
        self.write_signed(&file)
    }

    /// The signed file, each station id with the time its proof was signed. A file that is missing,
    /// too big, not JSON, or not ids to times is nothing signed; a line that is not one is skipped.
    fn signed_file(&self) -> BTreeMap<String, u64> {
        let Some(path) = &self.signed else {
            return BTreeMap::new();
        };
        let text = match std::fs::metadata(path) {
            Ok(meta) if meta.len() <= TYPED_FILE_BYTES => std::fs::read_to_string(path).ok(),
            _ => None,
        };
        let lines: BTreeMap<String, serde_json::Value> = text
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or_default();
        lines
            .into_iter()
            .filter(|(id, _)| protocol::identifier(id))
            .filter_map(|(id, at)| Some((id, at.as_u64()?)))
            .collect()
    }

    /// Write the signed file, through a temporary file the rename publishes, as the typed file.
    fn write_signed(&self, file: &BTreeMap<String, u64>) -> Result<(), &'static str> {
        let Some(path) = &self.signed else {
            return Err("storeUnavailable");
        };
        let text = serde_json::to_string(file).map_err(|_| "storeUnavailable")?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|_| "storeUnavailable")?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, text).map_err(|_| "storeUnavailable")?;
        std::fs::rename(&tmp, path).map_err(|_| "storeUnavailable")
    }

    /// This computer's two keys for the next station it signs in to with the account, made and kept
    /// now when none are: kept until a station is kept with them ([`Stations::drop_staged`]), so a
    /// sign-in that stopped part-way and is tried again names the same keys.
    pub(crate) fn staged(&self) -> Result<StagedKeys, &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        let kept = self
            .vault
            .staged_keys()
            .map_err(|_| "storeUnavailable")?
            .filter(sound_staged);
        if let Some(kept) = kept {
            return Ok(kept);
        }
        let (_, road) = ComputerKey::generate().ok_or("storeUnavailable")?;
        let (_, lan) = ComputerKey::generate().ok_or("storeUnavailable")?;
        let made = StagedKeys { road, lan };
        self.vault
            .save_staged_keys(&made)
            .map_err(|_| "storeUnavailable")?;
        Ok(made)
    }

    /// The staged keys, gone: a station is kept with them.
    pub(crate) fn drop_staged(&self) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        self.vault
            .remove_staged_keys()
            .map_err(|_| "storeUnavailable")
    }

    /// The station kept by the service's id `id` until this computer first reaches it on its network
    /// ([`Stations::keep_away`]), if the store holds a sound one.
    pub(crate) fn away(&self, id: &str) -> Result<Option<AwayStation>, &'static str> {
        if !protocol::identifier(id) {
            return Ok(None);
        }
        Ok(self
            .vault
            .away_station(id)
            .map_err(|_| "storeUnavailable")?
            .filter(|record| record.station_id == id && sound_away(record)))
    }

    /// Every station kept by its service id that the store holds a sound record for, in the order
    /// they were kept.
    pub(crate) fn away_records(&self) -> Result<Vec<AwayStation>, &'static str> {
        let mut records = Vec::new();
        for id in self.ids()? {
            records.extend(self.away(&id)?);
        }
        Ok(records)
    }

    /// The same stations, as the page shows them.
    pub(crate) fn list_away(&self) -> Result<Vec<AwayView>, &'static str> {
        Ok(self
            .away_records()?
            .iter()
            .filter_map(AwayView::of)
            .collect())
    }

    /// Keep `record`, a station signed in to but not reached on its network, by the service's id
    /// for it, with its internet road `road` under the same id, until this computer first reaches
    /// it there ([`Stations::fold`]). The record and the road first, then the list, as
    /// [`Stations::keep`]. `stationsFull` with [`MAX_STATIONS`] on this computer already, or a record
    /// that would not fit one entry; `notStation` for a record or a road this module could not have
    /// written, or a road to another station. The id is the service's word (a station's proof
    /// vouches for its keys, not for the id it is listed by), so it never replaces a station kept
    /// here: `notStation` under the id of one with another key or another road, `alreadyHere` under
    /// the id of the same station on this network.
    pub(crate) fn keep_away(
        &self,
        record: &AwayStation,
        road: &InternetRoad,
    ) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if !sound_away(record) || !sound_road(road) || road.station_id != record.station_id {
            return Err("notStation");
        }
        if !fits(record) || !fits(road) {
            return Err("stationsFull");
        }
        let held = self.held(&record.station_id)?;
        if let Some(key) = held.lan {
            return Err(if key == record.station_key {
                "alreadyHere"
            } else {
                "notStation"
            });
        }
        if held.away.is_some_and(|key| key != record.station_key)
            || held.road.is_some_and(|key| key != road.station_key)
        {
            return Err("notStation");
        }
        let mut ids = self.ids()?;
        ids.retain(|id| *id != record.station_id);
        if ids.len() >= MAX_STATIONS {
            return Err("stationsFull");
        }
        ids.push(record.station_id.clone());
        let list = PairedStations { stations: ids };
        if !fits(&list) {
            return Err("stationsFull");
        }
        self.vault
            .save_away_station(record)
            .map_err(|_| "storeUnavailable")?;
        self.vault
            .save_internet_road(&record.station_id, road)
            .map_err(|_| "storeUnavailable")?;
        self.vault
            .save_paired_stations(&list)
            .map_err(|_| "storeUnavailable")
    }

    /// The station kept by the service's id `away`, first reached on its network: kept from now on
    /// as `record`, by the id its network gave it, with the same internet road (the same device at
    /// the service), and `issued_at` as when this computer signed in for it, so its thirty days on
    /// that network are counted from then. The record, the road and the time first; then the list,
    /// where the network's id takes the away one's place (the station is kept the new way from that
    /// write on); then the away entries. A crash before the list leaves the station as it was; one
    /// after it, away entries that no list names.
    ///
    /// Only the station kept away, by the LAN key pinned when it was kept (`notThisStation`), and
    /// never over a station stored under the id its network gave, which is that station's own word:
    /// a record on this network (`alreadyHere` for its own: it is here already, paired with a code;
    /// `notStation` for another's), a station kept by its service id, or another station's road.
    pub(crate) fn fold(
        &self,
        away: &str,
        record: &PairedStation,
        issued_at: u64,
    ) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if !sound(record) {
            return Err("notStation");
        }
        if !fits(record) {
            return Err("stationsFull");
        }
        let kept = self.away(away)?.ok_or("unknownStation")?;
        if kept.station_key != record.station_key {
            return Err("notThisStation");
        }
        let road = self
            .vault
            .internet_road(away)
            .map_err(|_| "storeUnavailable")?
            .filter(sound_road)
            .ok_or("unknownStation")?;
        // Its own service id included: the station kept away is held there.
        let held = self.held(&record.station_id)?;
        if let Some(key) = held.lan {
            return Err(if key == record.station_key {
                "alreadyHere"
            } else {
                "notStation"
            });
        }
        if held.away.is_some() || held.road.is_some_and(|key| key != road.station_key) {
            return Err("notStation");
        }
        let mut ids = self.ids()?;
        ids.retain(|id| *id != record.station_id);
        let place = ids
            .iter()
            .position(|id| id == away)
            .ok_or("unknownStation")?;
        ids[place] = record.station_id.clone();
        let list = PairedStations { stations: ids };
        // A hold on the road moves with it, written first: the road is never kept unheld under
        // its new id.
        // Kept on a sign-in, never as one paired with a code, whatever an entry left behind says.
        self.unmark(&record.station_id)?;
        let mut held = self.held_list()?;
        if let Some(moved) = held.roads.iter().find(|h| h.station_id == away).cloned() {
            held.roads.retain(|h| h.station_id != record.station_id);
            held.roads.push(HeldRoad {
                station_id: record.station_id.clone(),
                ..moved
            });
            self.vault
                .save_held_roads(&held)
                .map_err(|_| "storeUnavailable")?;
        }
        self.vault
            .save_paired_station(record)
            .map_err(|_| "storeUnavailable")?;
        self.vault
            .save_internet_road(&record.station_id, &road)
            .map_err(|_| "storeUnavailable")?;
        let mut file = self.signed_file();
        file.insert(record.station_id.clone(), issued_at);
        file.retain(|kept, _| list.stations.contains(kept));
        self.write_signed(&file)?;
        self.vault
            .save_paired_stations(&list)
            .map_err(|_| "storeUnavailable")?;
        // Listed nowhere now: one the store will not let go of is read by nothing.
        let _ = self.vault.remove_away_station(away);
        let _ = self.vault.remove_internet_road(away);
        Ok(())
    }

    /// Why the internet road kept for station `id` is held, if it is: `stationKeyUnconfirmed` until
    /// the operator confirms the station's key against the station's own screen, or
    /// `stationKeyDisagrees` for good once the station named another key on its own network. A hold
    /// counts only for the station key it was written for: a road added again with another key is
    /// not held by it. No road, no hold.
    pub(crate) fn road_held(&self, id: &str) -> Result<Option<&'static str>, &'static str> {
        let Some(road) = self.internet(id)? else {
            return Ok(None);
        };
        let pinned = crate::remote_service::fingerprint(&road.station_key);
        Ok(self
            .held_list()?
            .roads
            .iter()
            .find(|held| held.station_id == id && Some(&held.shown) == pinned.as_ref())
            .map(|held| {
                if held.disowned {
                    "stationKeyDisagrees"
                } else {
                    "stationKeyUnconfirmed"
                }
            }))
    }

    /// The held roads as the store holds them: none when the entry is missing or does not read.
    fn held_list(&self) -> Result<HeldRoads, &'static str> {
        Ok(self
            .vault
            .held_roads()
            .map_err(|_| "storeUnavailable")?
            .unwrap_or_default())
    }

    /// Write the held roads with station `id`'s replaced by `held` (or dropped, with none), keeping
    /// only those of stations still on this computer besides it.
    fn write_held(&self, id: &str, held: Option<HeldRoad>) -> Result<(), &'static str> {
        let ids = self.ids()?;
        let mut list = self.held_list()?;
        list.roads
            .retain(|kept| kept.station_id != id && ids.contains(&kept.station_id));
        list.roads.extend(held);
        if !fits(&list) {
            return Err("stationsFull");
        }
        self.vault
            .save_held_roads(&list)
            .map_err(|_| "storeUnavailable")
    }

    /// Hold the internet road about to be kept for station `id`, pinned to `station_key`, until the
    /// operator confirms that key. Written BEFORE the road, so no road is ever kept unheld that
    /// should be held: a hold for a road not yet kept holds nothing.
    pub(crate) fn hold_road(&self, id: &str, station_key: &str) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        let shown = crate::remote_service::fingerprint(station_key).ok_or("notStation")?;
        self.write_held(
            id,
            Some(HeldRoad {
                station_id: id.to_string(),
                shown,
                disowned: false,
            }),
        )
    }

    /// Keep a station just paired here with a code: its record ([`Self::keep`]), then the entry
    /// that says it was paired with a code, bound to the station's key the code pinned
    /// ([`Self::code_paired`]). The pairing stands if the store will not take that entry: the
    /// station then reads as not paired with a code, which keeps a held road held until the
    /// operator checks it. Paired at the station, which takes this computer until it is removed
    /// there: no day to sign in again by stays with it. The pairing stands if the file will not
    /// take that; the row then keeps its day, as before.
    pub(crate) fn keep_coded(&self, record: &PairedStation) -> Result<(), &'static str> {
        self.keep(record)?;
        if let Some(shown) = crate::remote_service::fingerprint(&record.station_key) {
            let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
            let _ = self.write_coded(
                &record.station_id,
                Some(CodePairing {
                    station_id: record.station_id.clone(),
                    shown,
                }),
            );
        }
        let _ = self.drop_signed(&record.station_id);
        Ok(())
    }

    /// The stations paired here with a code as the store lists them: none when the entry is
    /// missing or does not read.
    fn coded_list(&self) -> Result<CodePaired, &'static str> {
        Ok(self
            .vault
            .code_paired()
            .map_err(|_| "storeUnavailable")?
            .unwrap_or_default())
    }

    /// Write the stations paired with a code with station `id`'s replaced by `paired` (or dropped,
    /// with none), keeping only those of stations still on this computer besides it.
    fn write_coded(&self, id: &str, paired: Option<CodePairing>) -> Result<(), &'static str> {
        let ids = self.ids()?;
        let mut list = self.coded_list()?;
        list.stations
            .retain(|kept| kept.station_id != id && ids.contains(&kept.station_id));
        list.stations.extend(paired);
        if !fits(&list) {
            return Err("stationsFull");
        }
        self.vault
            .save_code_paired(&list)
            .map_err(|_| "storeUnavailable")
    }

    /// Station `id` is no longer one paired here with a code: its entry goes. Nothing written when
    /// it has none.
    fn unmark(&self, id: &str) -> Result<(), &'static str> {
        if !self
            .coded_list()?
            .stations
            .iter()
            .any(|kept| kept.station_id == id)
        {
            return Ok(());
        }
        self.write_coded(id, None)
    }

    /// Station `id` named `said` as its key for the internet road, on its own network: a road kept
    /// for it pinned to another key is held for good (`stationKeyDisagrees`). Whether it was. The
    /// same key frees a road held until the operator checks it, when the station is paired here
    /// with a code ([`Self::code_paired`]): its network's road is pinned by the code, so the station
    /// said it itself. Never one held for good: only forgetting it and adding it again frees that.
    pub(crate) fn disown_road(&self, id: &str, said: &str) -> Result<bool, &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        let Some(road) = self.internet(id)? else {
            return Ok(false);
        };
        if road.station_key == said {
            let pinned = crate::remote_service::fingerprint(&road.station_key);
            let unconfirmed = self.held_list()?.roads.iter().any(|held| {
                held.station_id == id && Some(&held.shown) == pinned.as_ref() && !held.disowned
            });
            if unconfirmed && self.code_paired(id)? {
                self.write_held(id, None)?;
            }
            return Ok(false);
        }
        let shown = crate::remote_service::fingerprint(&road.station_key).ok_or("notStation")?;
        self.write_held(
            id,
            Some(HeldRoad {
                station_id: id.to_string(),
                shown,
                disowned: true,
            }),
        )?;
        Ok(true)
    }

    /// The operator compared the station's key and pressed It matches: the road kept for station
    /// `id` is no longer held. Never one the station disowned (`stationKeyDisagrees`).
    pub(crate) fn confirm_road(&self, id: &str) -> Result<(), &'static str> {
        if self.road_held(id)? == Some("stationKeyDisagrees") {
            return Err("stationKeyDisagrees");
        }
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if !self
            .held_list()?
            .roads
            .iter()
            .any(|held| held.station_id == id)
        {
            return Ok(());
        }
        self.write_held(id, None)
    }

    /// Whether station `id` is paired here with a code: its record on its network, the credential
    /// store's entry that says it was paired with a code ([`Self::keep_coded`]) for that record's
    /// key, and no sign-in time. Its network's road is then pinned by the code, not by a key the
    /// service listed. Never on the sign-in file's word alone, which is a plain file: a station with
    /// no such entry (a guest whose line is lost, or one paired before the entry was kept) is not
    /// paired with a code, so its held road stays held until the operator checks it.
    pub(crate) fn code_paired(&self, id: &str) -> Result<bool, &'static str> {
        let Some(record) = self.get(id)? else {
            return Ok(false);
        };
        let Some(pinned) = crate::remote_service::fingerprint(&record.station_key) else {
            return Ok(false);
        };
        Ok(self
            .coded_list()?
            .stations
            .iter()
            .any(|kept| kept.station_id == id && kept.shown == pinned)
            && self.signed(id).is_none())
    }

    /// Drop the internet road kept for station `id`, and its hold, leaving its pairing on its
    /// network: the operator said the station's key the road was pinned to is not the station's.
    /// The road can be added again from the station's row. Its approval at the service is ended
    /// first, by the caller, while this computer still holds the key that signs for it.
    pub(crate) fn drop_road(&self, id: &str) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if !protocol::identifier(id) {
            return Err("unknownStation");
        }
        self.vault
            .remove_internet_road(id)
            .map_err(|_| "storeUnavailable")?;
        // A hold the store will not let go of holds no road.
        let _ = self.write_held(id, None);
        Ok(())
    }

    /// Forget station `id` on this computer: its internet road and its record (or, for one kept by
    /// its service id, that record), with this computer's keys for it, first, then the list, then
    /// its lines in the typed and signed files. A list that still names it after a failure names a
    /// record that is gone, which reads as no pairing; a line a file would not let go of goes with
    /// its next write.
    pub(crate) fn forget(&self, id: &str) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if !protocol::identifier(id) {
            return Err("unknownStation");
        }
        self.vault
            .remove_internet_road(id)
            .map_err(|_| "storeUnavailable")?;
        self.vault
            .remove_paired_station(id)
            .map_err(|_| "storeUnavailable")?;
        self.vault
            .remove_away_station(id)
            .map_err(|_| "storeUnavailable")?;
        let mut ids = self.ids()?;
        ids.retain(|kept| kept != id);
        self.vault
            .save_paired_stations(&PairedStations { stations: ids })
            .map_err(|_| "storeUnavailable")?;
        // Its hold goes with it; one the store will not let go of holds no road.
        let _ = self.write_held(id, None);
        // As does its entry as paired with a code; one left behind is dropped when the station is
        // next kept on a sign-in.
        let _ = self.unmark(id);
        // Forgotten all the same when a file will not take it: the station is listed nowhere.
        let mut file = self.typed_file();
        if file.remove(id).is_some() {
            let _ = self.write_typed(&file);
        }
        let mut file = self.signed_file();
        if file.remove(id).is_some() {
            let _ = self.write_signed(&file);
        }
        Ok(())
    }
}
