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
//! - **No engine, no radio and no operations authority.** This computer's own station, if it has
//!   one, is out of reach: nothing here names the engine, and the window's page reaches no Nexus
//!   command (the loopback origin, below).
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
//! origin: the page reaches NO Nexus command, as the hosted page in the Remote stations window
//! reaches none (`remote_window`'s tests hold both). It talks to this module over one WebSocket on
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
mod origin;
pub(crate) mod pairing;
pub(crate) mod road;
#[cfg(test)]
pub(crate) mod tests;

#[cfg(test)]
pub(crate) use origin::PREFERRED_PORT;
pub(crate) use origin::{Origin, Reach};

use std::net::SocketAddrV4;
use std::sync::{Arc, Mutex};

use ring::digest::{digest, SHA256};
use ring::rand::SystemRandom;
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use serde::Serialize;
use tempo_stream::protocol;

use crate::remote_service::lan::{valid_name, NAME_CHARS};
use crate::remote_service::vault::{PairedStation, PairedStations, StationVault};

/// The most stations this computer keeps a pairing with.
pub(crate) const MAX_STATIONS: usize = 8;
/// The most addresses one station's record keeps.
pub(crate) const MAX_ADDRESSES: usize = 4;
/// The smallest OS credential blob, Windows': 2560 bytes of UTF-16.
const CREDENTIAL_BYTES: usize = 2560;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// This computer's own key for one station. See the module header for what may never happen to it.
pub(crate) struct ComputerKey {
    pair: EcdsaKeyPair,
    /// The public half: SPKI DER, lowercase hex, the shape a device key has (182 characters).
    public_key: String,
}

impl ComputerKey {
    /// A new key, and its PKCS#8 document as lowercase hex for the station's record: the one copy
    /// of the private key that ever leaves this type, and it goes only there.
    pub(crate) fn generate() -> Option<(Self, String)> {
        let pkcs8 =
            EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &SystemRandom::new())
                .ok()?;
        let document = hex(pkcs8.as_ref());
        Some((Self::restore(&document)?, document))
    }

    /// The key a record holds, or `None` for anything that is not one.
    pub(crate) fn restore(pkcs8: &str) -> Option<Self> {
        let pair = EcdsaKeyPair::from_pkcs8(
            &ECDSA_P256_SHA256_FIXED_SIGNING,
            &protocol::hex_bytes(pkcs8)?,
            &SystemRandom::new(),
        )
        .ok()?;
        let public_key = format!(
            "{}{}",
            protocol::P256_SPKI_PREFIX_HEX,
            hex(pair.public_key().as_ref())
        );
        Some(Self { pair, public_key })
    }

    /// The public half, SPKI as lowercase hex: what the station pinned while pairing.
    pub(crate) fn public_key(&self) -> &str {
        &self.public_key
    }

    /// This computer's signature over an offer (A5): over SHA-256 of `sdp`'s DTLS fingerprint and
    /// the station's, device's and session's ids, as the station checks it (`stream::verify`).
    /// `None` for an offer with no single fingerprint, which no station would take.
    pub(crate) fn sign_offer(
        &self,
        sdp: &str,
        station: &str,
        device: &str,
        session: &str,
    ) -> Option<String> {
        let fingerprint: [u8; 32] = digest(&SHA256, &protocol::offer_fingerprint(sdp)?)
            .as_ref()
            .try_into()
            .ok()?;
        let signed = protocol::offer_binding(&fingerprint, station, device, session);
        let signature = self.pair.sign(&SystemRandom::new(), &signed).ok()?;
        Some(hex(signature.as_ref()))
    }
}

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

/// A paired station as the page shows it: never this computer's key, only the station's
/// fingerprint (SHA-256 of its SPKI, lowercase hex) and where it answered.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StationView {
    pub id: String,
    pub addresses: Vec<String>,
    pub key: String,
}

impl StationView {
    fn of(record: &PairedStation) -> Option<Self> {
        let spki = protocol::hex_bytes(&record.station_key)?;
        Some(Self {
            id: record.station_id.clone(),
            addresses: record.addresses.clone(),
            key: hex(digest(&SHA256, &spki).as_ref()),
        })
    }
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

/// The stations this computer is paired with, in the OS credential store. The store can block, so
/// a caller off any thread that must not wait makes these calls.
pub(crate) struct Stations {
    vault: Arc<dyn StationVault>,
    /// One writer at a time, so two presses never interleave their writes to the list.
    writing: Mutex<()>,
}

impl Stations {
    pub(crate) fn new(vault: Arc<dyn StationVault>) -> Self {
        Self {
            vault,
            writing: Mutex::new(()),
        }
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
        Ok(self.records()?.iter().filter_map(StationView::of).collect())
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

    /// Keep a station's record: a new pairing, or the same station paired again (its record is
    /// replaced). The record first, then the list, so a crash between them leaves a record nobody
    /// lists, never a listed station with no record. `stationsFull` with [`MAX_STATIONS`] paired
    /// already, or a record that would not fit one entry.
    pub(crate) fn keep(&self, record: &PairedStation) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if !sound(record) {
            return Err("notStation");
        }
        if !fits(record) {
            return Err("stationsFull");
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

    /// Forget station `id` on this computer: its record, with this computer's key for it, first,
    /// then the list. A list that still names it after a failure names a record that is gone,
    /// which reads as no pairing.
    pub(crate) fn forget(&self, id: &str) -> Result<(), &'static str> {
        let _writing = self.writing.lock().map_err(|_| "storeUnavailable")?;
        if !protocol::identifier(id) {
            return Err("unknownStation");
        }
        self.vault
            .remove_paired_station(id)
            .map_err(|_| "storeUnavailable")?;
        let mut ids = self.ids()?;
        ids.retain(|kept| kept != id);
        self.vault
            .save_paired_stations(&PairedStations { stations: ids })
            .map_err(|_| "storeUnavailable")
    }
}
