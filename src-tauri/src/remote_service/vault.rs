//! Isolated Remote vault. One atomic OS entry contains the pairing and its secret;
//! no connector settings, logbook passwords or fallback files are accessed.
use serde::{Deserialize, Serialize};

const SERVICE: &str = "org.hamradiotools.nexus.remote";
const ACCOUNT: &str = "pairing";

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

pub trait Vault: Send {
    fn binding(&self) -> Result<Option<Binding>, &'static str>;
    fn credential(&self, binding: &Binding) -> Result<String, &'static str>;
    fn stage(&self, binding: &Binding, credential: &str) -> Result<(), &'static str>;
    fn save(&self, binding: &Binding, credential: &str) -> Result<(), &'static str>;
    fn remove(&self, binding: &Binding) -> Result<(), &'static str>;
}

pub struct SystemVault;
fn entry() -> Result<keyring::Entry, &'static str> {
    keyring::Entry::new(SERVICE, ACCOUNT).map_err(|_| "credentialStoreUnavailable")
}
fn read() -> Result<Option<Record>, &'static str> {
    match entry()?.get_password() {
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
    entry()?
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
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err("credentialStoreUnavailable"),
        }
    }
}
