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
use serde::{Deserialize, Serialize};

const SERVICE: &str = "org.hamradiotools.nexus.remote";
const ACCOUNT: &str = "pairing";
const STATE: &str = "state";

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
    /// The browser's approval expiry as the service listed it at grant time. The service writes a
    /// new expiry on EVERY approval (and an expiry of "now" on every revocation), so this is the
    /// approval generation: a revoked or re-approved browser no longer matches and gets nothing
    /// back. No wire change was needed to bind to it.
    pub expires_at: u64,
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

pub trait Vault: Send + Sync {
    fn binding(&self) -> Result<Option<Binding>, &'static str>;
    fn credential(&self, binding: &Binding) -> Result<String, &'static str>;
    fn stage(&self, binding: &Binding, credential: &str) -> Result<(), &'static str>;
    fn save(&self, binding: &Binding, credential: &str) -> Result<(), &'static str>;
    fn remove(&self, binding: &Binding) -> Result<(), &'static str>;
    fn state(&self) -> Result<Option<State>, &'static str>;
    fn save_state(&self, state: &State) -> Result<(), &'static str>;
    fn remove_state(&self) -> Result<(), &'static str>;
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
}
