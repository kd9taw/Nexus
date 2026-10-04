//! Isolated Remote vault. One atomic OS entry contains the pairing and its secret;
//! no connector settings, logbook passwords or fallback files are accessed.
//!
//! A second entry remembers what a restart must restore: whether Remote was on, and each approved
//! browser's station-control, remote-logging and FT8/FT4 transmit grants. It is a SEPARATE entry,
//! not a new field on the pairing record, because that record is `deny_unknown_fields`: an older
//! Nexus reading a pairing that had grown a field would lose its pairing, and then could not even
//! revoke it. It is bound to the pairing it was written for, so an orphan left behind by a failed
//! delete is ignored by any other pairing.
//!
//! FT8/FT4 transmit is remembered (operator decision 2026-09-13, late: a checkbox on the approval,
//! kept until revoked). It is written only when granted, so a record without it stays readable by
//! the 1.12.0 build, whose `Grant` has no such field; a record that holds it reads as unreadable
//! there, which leaves Remote off. A remembered grant arms nothing: the TX-enable latch still starts
//! off, and the browser still has to press TX On.
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
//! bytes of UTF-16) in one entry.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const SERVICE: &str = "org.hamradiotools.nexus.remote";
const ACCOUNT: &str = "pairing";
const STATE: &str = "state";
const PINS: &str = "pins";
const STATION_KEY: &str = "station-key";
const LAN_KEY: &str = "lan-key";
const LAN_DEVICES: &str = "lan-devices";

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
    pub logging: bool,
    pub control: bool,
    /// FT8/FT4 transmit. Only ever true alongside `control`. Omitted when false; see the module note.
    #[serde(default, skip_serializing_if = "not")]
    pub transmit: bool,
}
fn not(value: &bool) -> bool {
    !*value
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
/// station id that goes with it. No `Debug` and no `Clone`, as [`StationKey`].
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LanKey {
    pub station_id: String,
    pub pkcs8: String,
}

/// The computers paired over this network with the LAN key whose station id this is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct LanDevices {
    pub station_id: String,
    pub devices: Vec<LanDevice>,
}

/// One paired computer: the SHA-256 of its key's SPKI as lowercase hex, which is all that admits
/// it, and the name it gave. Its device id is worked out from the pin, so the record does not
/// carry one. One-letter keys, so eight records fit one entry.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LanDevice {
    #[serde(rename = "p")]
    pub pin: String,
    #[serde(rename = "n")]
    pub name: String,
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
impl LanVault for SystemVault {
    fn lan_key(&self) -> Result<Option<LanKey>, &'static str> {
        match entry(LAN_KEY)?.get_password() {
            // Unreadable is no key, as the station key's is: a new one is made, which every paired
            // computer refuses until it is paired again.
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
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
            // An unreadable list admits nobody: every computer is paired again.
            Ok(value) => Ok(serde_json::from_str(&value).ok()),
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
