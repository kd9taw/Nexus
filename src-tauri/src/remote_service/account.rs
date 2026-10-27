//! The station's account, this side: binding the station to its owner by a sign-in at the shack, the
//! issuer's keys kept for checking proofs with the internet down, and the checks a device the
//! service lists must pass before the station would grant it anything (`tempo_client::account`).
//!
//! ## Binding ([`bind_on`])
//!
//! Settings ▸ Remote access ▸ Sign in to Nexus Remote. The station's two keys exist first (its
//! signing key and its LAN key); their hash is the sign-in's nonce. The operator signs in through
//! the system browser to the built-in tenant's Native application (`crate::account_sign_in`), and
//! the issuer's ID token comes back naming exactly those keys and who signed in, and nothing more
//! (`Scope::Station`): the service lists this proof to every computer of the account and of each
//! person the station invites, so it carries no address. The station checks that proof itself
//! with the issuer's key set ([`Owner::proven`]) before it sends it anywhere, then sends it with the
//! keys and the station credential to the service (`client/station`), which writes the station for
//! the token's account. Kept: the owner (no token). The access token and the proof are dropped when
//! the call ends.
//!
//! ## The issuer's keys ([`keys`])
//!
//! Fetched from the issuer's own address, never the service's, and kept for that issuer. Fetched
//! again when due, or once for a proof naming a key the set lacks (at most every
//! `KEY_SET_RETRY_S`). Each fetch says whether the key that signed the station's proof is still in
//! the set; when it is not, devices can no longer check that proof and the station must sign in
//! again ([`Keys::reproof`]), which replaces its proof at the service ([`reprove_on`]).
//!
//! ## A listed device ([`vouch_listed`], [`sweep`])
//!
//! A device the service lists with a proof (`x-nexus-device-proof`) is checked here against the
//! owner and the people invited, for exactly the keys the list names; while [`GRANTS_ON_SIGN_IN`]
//! is on it is granted CONTROL on that alone (the 2026-10-23 sign-in decision), with a 15-minute transmit hold
//! ([`TX_HOLD`]) unless the sign-in waives it, and an email to the owner. Each list the
//! station hears sweeps the devices it keeps for the LAN: one the service no longer lists as
//! approved is taken out, and its pin returned, before any LAN session is admitted.
//!
//! Nothing here writes a log line. What it returns to a caller is a name (`&'static str`) or a
//! value with no token in it: no access token, proof or credential leaves this module except in
//! the request that needs it.
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use tempo_client::account::{self, DeviceCache, KeySet, Owner, Tenant, Vouched};
use tempo_client::native::{Scope, SignIn};
use tokio::net::TcpListener;

use super::vault::{AccountKeys, AccountVault, DevicesHeld};

/// The most of a service or issuer answer read.
const ANSWER_LIMIT: usize = 16384;

/// ⛔ A SIGN-IN GRANTS CONTROL, NOT TRANSMIT YET (the 2026-10-23 sign-in decision). Whether the station grants a device
/// the service approved on its own sign-in (its owner's, or that of someone the station invited)
/// what its role says, with no press at the shack, once its proof holds here
/// (`Controller::grant_vouched`). On (since the 2026-10-23 sign-in decision built the full hold and email): a signed-in
/// device of the account is admitted for CONTROL — it may connect, operate the receiver and hold
/// station control — on both roads, LAN and internet, with no Approve press. Code pairing and an
/// explicit Approve at the shack still work exactly as before (this is additive, not a
/// replacement). The computer that signed in no longer waits for that Approve on its internet road
/// (`crate::lan_client::account`).
///
/// TRANSMIT, though, is held: a device whose control came from a sign-in carries a 15-minute
/// transmit hold ([`TX_HOLD`]) enforced at the engine's arming gate
/// (`tempo_app::engine::Engine::remote_transmit_held`)— it may hold control and operate the
/// receiver, but no transmit ARM starts until the window passes. Stop, and the off half of a
/// switch, are never held. The hold is WAIVED for one Approved at the shack
/// (`Controller::grant_approved`) and for one that enrols on the station's own network by its sign-in (`super::lan::enrol` records
/// no hold — standing at the shack's LAN is the shack tap's equal). Adding a device by sign-in also
/// notifies the station's owner by email, which the service sends (the station keeps no address);
/// the provider secret is a credential gate the operator wires (gate 5).
pub(crate) const GRANTS_ON_SIGN_IN: bool = true;

/// How long a device auto-granted control by a sign-in is held off transmit (the 2026-10-23 sign-in decision), from when
/// the station first grants it. An ADDITIVE transmit guard: the engine refuses every arm within it
/// and never a Stop or a disarm ([`GRANTS_ON_SIGN_IN`]).
pub(crate) const TX_HOLD: Duration = Duration::from_secs(15 * 60);

/// The account's side of the Remote service and its issuer.
pub(crate) struct Service {
    http: reqwest::Client,
    origin: String,
    tenant: &'static Tenant,
    /// Where the issuer is asked: its own address, the tenant's issuer. A test's stand-in only in
    /// a test build.
    issuer_at: String,
}

impl Service {
    /// The account at the service Nexus is built for: `accountNotReady` while this build has no
    /// configured sign-in for it.
    pub(crate) fn new(origin: &str) -> Result<Self, &'static str> {
        if origin != super::REMOTE_ORIGIN {
            return Err("originDenied");
        }
        let tenant = Tenant::of(origin).ok_or("accountNotReady")?;
        Self::with(origin, tenant, tenant.issuer)
    }

    pub(crate) fn with(
        origin: &str,
        tenant: &'static Tenant,
        issuer_at: &str,
    ) -> Result<Self, &'static str> {
        let http = reqwest::Client::builder()
            .https_only(!cfg!(test))
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|_| "serviceUnavailable")?;
        Ok(Self {
            http,
            origin: origin.to_string(),
            tenant,
            issuer_at: issuer_at.to_string(),
        })
    }

    /// The tenant this build signs in to.
    pub(crate) fn tenant(&self) -> &'static Tenant {
        self.tenant
    }

    /// The client the sign-in and the service's requests go out on.
    pub(crate) fn http(&self) -> &reqwest::Client {
        &self.http
    }

    /// A sign-in to this build's own tenant and its Native application, at the issuer's own address,
    /// for the service's API: the station's and a computer's alike.
    pub(crate) async fn native_sign_in(&self) -> Result<SignIn, &'static str> {
        Ok(SignIn {
            issuer: self.issuer_at.clone(),
            client_id: self.tenant.native_client_id.to_string(),
            audience: self.audience().await?,
        })
    }

    /// The service's API audience (`GET /api/remote/config`): only that. The issuer and the
    /// application are this build's own.
    async fn audience(&self) -> Result<String, &'static str> {
        let response = self
            .http
            .get(format!("{}/api/remote/config", self.origin))
            .send()
            .await
            .map_err(|_| "serviceUnavailable")?;
        let config = answer(response).await?;
        config["audience"]
            .as_str()
            .filter(|audience| !audience.is_empty() && audience.len() <= 512)
            .map(str::to_string)
            .ok_or("accountNotReady")
    }

    /// The issuer's key set, fetched now from the issuer itself.
    pub(crate) async fn fetch_keys(&self, now_s: u64) -> Result<KeySet, &'static str> {
        let response = self
            .http
            .get(format!("{}.well-known/jwks.json", self.issuer_at))
            .send()
            .await
            .map_err(|_| "issuerUnavailable")?;
        let body = answer(response).await.map_err(|_| "issuerUnavailable")?;
        KeySet::fetched(&body, now_s).ok_or("issuerUnavailable")
    }
}

/// A service or issuer answer: its JSON when it is a success, else the error the service names
/// when it is one of the account's, else `serviceRefused`.
async fn answer(mut response: reqwest::Response) -> Result<Value, &'static str> {
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| "serviceUnavailable")? {
        if bytes.len() + chunk.len() > ANSWER_LIMIT {
            return Err("invalidResponse");
        }
        bytes.extend_from_slice(&chunk);
    }
    let body = serde_json::from_slice::<Value>(&bytes).ok();
    if !status.is_success() {
        // An ALLOW-LIST: a code from the service never becomes text this side shows.
        return Err(match body.as_ref().and_then(|b| b["error"].as_str()) {
            Some("stationLimit") => "stationLimit",
            Some("proofInvalid") => "proofInvalid",
            Some("serviceNotConfigured") => "accountNotReady",
            Some("rateLimited") => "rateLimited",
            Some("trialDisabled") => "trialDisabled",
            _ => "serviceRefused",
        });
    }
    body.ok_or("invalidResponse")
}

/// The issuer's keys as the station holds them after [`keys`]: the set, and whether the station must
/// sign in again because the key that signed its proof has left it.
pub(crate) struct Keys {
    pub(crate) set: KeySet,
    pub(crate) reproof: bool,
}

/// The kept key set, fetched again first when it is due, when none is kept for this issuer, or when
/// `for_kid` names a key it lacks and a fetch for it is allowed now. A fetched set is kept. When a
/// fetch fails the kept set is used, if there is one: a proof is then checked with what the station
/// already had, and one it cannot check is refused.
pub(crate) async fn keys(
    service: &Service,
    vault: &dyn AccountVault,
    for_kid: Option<&str>,
    now_s: u64,
) -> Result<Keys, &'static str> {
    let kept = vault
        .account_keys()
        .ok()
        .flatten()
        .filter(|kept| kept.issuer == service.tenant.issuer)
        .map(|kept| kept.keys);
    let fetch = match &kept {
        None => true,
        Some(set) => set.due(now_s) || for_kid.is_some_and(|kid| set.may_fetch_for(kid, now_s)),
    };
    let set = if fetch {
        match service.fetch_keys(now_s).await {
            Ok(set) => {
                vault.save_account_keys(&AccountKeys {
                    issuer: service.tenant.issuer.to_string(),
                    keys: set.clone(),
                })?;
                set
            }
            Err(reason) => kept.ok_or(reason)?,
        }
    } else {
        kept.ok_or("issuerUnavailable")?
    };
    let reproof = match vault.owner() {
        Ok(Some(owner)) => owner.needs_reproof(&set),
        _ => false,
    };
    Ok(Keys { set, reproof })
}

/// What [`sign_in`] gives the caller, which turns Remote on as an approved pairing does: the service's
/// ids for the station and its account.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Bound {
    pub(crate) station_id: String,
    pub(crate) account_id: String,
}

/// The station's public keys, SPKI as lowercase hex, and its credential with the service: what it
/// binds with. No `Debug`: the credential is a secret.
pub(crate) struct Station<'a> {
    pub(crate) name: &'a str,
    pub(crate) station_key: &'a str,
    pub(crate) lan_key: &'a str,
    pub(crate) credential: &'a str,
}

/// Signs in at the shack through `open`'s browser, on the sign-in's own port, for `binds`: binds
/// the station ([`bind_on`]), or, for one bound already, proves it again ([`reprove_on`]) and gives
/// back its binding.
pub(crate) async fn sign_in(
    service: &Service,
    vault: &dyn AccountVault,
    binds: Binds,
    open: impl FnOnce(&str) -> Result<(), String>,
    now_s: u64,
) -> Result<Bound, &'static str> {
    let listener = TcpListener::bind((
        std::net::Ipv4Addr::LOCALHOST,
        crate::account_sign_in::SIGN_IN_PORT,
    ))
    .await
    .map_err(|_| "signInPortBusy")?;
    let wait = crate::account_sign_in::SIGN_IN_WAIT;
    sign_in_on(service, vault, binds, listener, wait, open, now_s).await
}

async fn sign_in_on(
    service: &Service,
    vault: &dyn AccountVault,
    binds: Binds,
    listener: TcpListener,
    wait: Duration,
    open: impl FnOnce(&str) -> Result<(), String>,
    now_s: u64,
) -> Result<Bound, &'static str> {
    let station = Station {
        name: &binds.name,
        station_key: &binds.station_key,
        lan_key: &binds.lan_key,
        credential: &binds.credential,
    };
    match binds.again {
        None => bind_on(service, vault, &station, listener, wait, open, now_s).await,
        Some(bound) => {
            let id = &bound.station_id;
            reprove_on(service, vault, &station, id, listener, wait, open, now_s).await?;
            Ok(bound)
        }
    }
}

/// Binds this station to the account the operator signs in to through `open`'s browser (see the
/// header), keeping the owner its own sign-in proves. `accountNotReady` while the tenant or the
/// service is not configured; the sign-in's own refusals; `tokenNonce` (and the other checks'
/// names) when the issuer's token is not for exactly this station's keys; the service's refusal.
async fn bind_on(
    service: &Service,
    vault: &dyn AccountVault,
    station: &Station<'_>,
    listener: TcpListener,
    wait: Duration,
    open: impl FnOnce(&str) -> Result<(), String>,
    now_s: u64,
) -> Result<Bound, &'static str> {
    let (access, proof, owner) =
        proven(service, vault, station, listener, wait, open, now_s).await?;
    let response = service
        .http
        .post(format!("{}/api/remote/client/station", service.origin))
        .header("authorization", format!("Bearer {access}"))
        .header("content-type", "application/json")
        .body(
            json!({
                "name": station.name,
                "credential": station.credential,
                "stationKey": station.station_key,
                "lanKey": station.lan_key,
                "proof": proof,
            })
            .to_string(),
        )
        .send()
        .await
        .map_err(|_| "serviceUnavailable")?;
    let given = answer(response).await?;
    let id = |name: &str| {
        given[name]
            .as_str()
            .filter(|id| tempo_stream::protocol::identifier(id))
            .map(str::to_string)
            .ok_or("invalidResponse")
    };
    let bound = Bound {
        station_id: id("stationId")?,
        account_id: id("accountId")?,
    };
    vault.save_owner(&Owner {
        station_id: bound.station_id.clone(),
        ..owner
    })?;
    Ok(bound)
}

/// The sign-in for exactly `station`'s two keys, its token checked here before anything is sent:
/// the access token, the proof, and the owner it proves (under no station id yet).
async fn proven(
    service: &Service,
    vault: &dyn AccountVault,
    station: &Station<'_>,
    listener: TcpListener,
    wait: Duration,
    open: impl FnOnce(&str) -> Result<(), String>,
    now_s: u64,
) -> Result<(String, String, Owner), &'static str> {
    let spki = |hex: &str| tempo_stream::protocol::hex_bytes(hex).ok_or("invalidRequest");
    let (station_key, lan_key) = (spki(station.station_key)?, spki(station.lan_key)?);
    let tenant = service.tenant;
    // The built-in issuer, asked at its own address (the same, outside a test).
    let sign = service.native_sign_in().await?;
    let nonce = account::station_nonce(&station_key, &lan_key);
    let signed = crate::account_sign_in::signed_in(
        &service.http,
        &sign,
        Scope::Station,
        Some(&nonce),
        listener,
        wait,
        open,
    )
    .await?;
    let proof = signed.id_token.ok_or("signInFailed")?;
    let held = keys(service, vault, account::kid_of(&proof).as_deref(), now_s).await?;
    let owner = Owner::proven("", &proof, &held.set, tenant, &station_key, &lan_key, now_s)?;
    Ok((signed.access, proof, owner))
}

/// Proves the station bound as `station_id` again, by its owner's sign-in through `open`'s browser:
/// what keeps it adding devices once the issuer has retired the key that signed its proof
/// ([`Keys::reproof`]). The new proof names the station's keys as they are now (its LAN key may have
/// been reset), is checked here for the owner the station already keeps (`proofNotYours` for any
/// other account), and replaces the old at the service (`station-proof`), with the station's
/// credential; then the owner is kept with the new proof's key id and signing time.
#[allow(clippy::too_many_arguments)]
async fn reprove_on(
    service: &Service,
    vault: &dyn AccountVault,
    station: &Station<'_>,
    station_id: &str,
    listener: TcpListener,
    wait: Duration,
    open: impl FnOnce(&str) -> Result<(), String>,
    now_s: u64,
) -> Result<(), &'static str> {
    let kept = vault
        .owner()?
        .filter(|owner| owner.station_id == station_id)
        .ok_or("stationNotPaired")?;
    let (access, proof, owner) =
        proven(service, vault, station, listener, wait, open, now_s).await?;
    if owner.issuer != kept.issuer || owner.subject != kept.subject {
        return Err("proofNotYours");
    }
    let response = service
        .http
        .post(format!(
            "{}/api/remote/stations/{station_id}/client/station-proof",
            service.origin
        ))
        .header("authorization", format!("Bearer {access}"))
        .header("content-type", "application/json")
        .body(
            json!({
                "credential": station.credential,
                "stationKey": station.station_key,
                "lanKey": station.lan_key,
                "proof": proof,
            })
            .to_string(),
        )
        .send()
        .await
        .map_err(|_| "serviceUnavailable")?;
    answer(response).await?;
    vault.save_owner(&Owner {
        station_id: kept.station_id,
        ..owner
    })
}

/// Opens an address in the system browser: the sign-in's first step.
pub(crate) type Opener = Arc<dyn Fn(&str) -> Result<(), String> + Send + Sync>;

/// What binding is given, owned because it runs on a task of its own while the operator signs in.
/// No `Debug`: the credential is a secret.
pub(crate) struct Binds {
    pub(crate) name: String,
    pub(crate) station_key: String,
    pub(crate) lan_key: String,
    pub(crate) credential: String,
    /// The station's binding when it is bound already: then the sign-in proves it again
    /// ([`reprove_on`]) and binds nothing, and [`Bound`] is that binding.
    pub(crate) again: Option<Bound>,
}

/// How the station binds: [`sign_in`] through the system browser, outside a test.
pub(crate) type Binder = Arc<
    dyn Fn(Binds) -> Pin<Box<dyn Future<Output = Result<Bound, &'static str>> + Send>>
        + Send
        + Sync,
>;

/// [`sign_in`] on `service`, keeping the owner in `vault`, signing in through `open`'s browser, at the
/// time `now_s` gives when the sign-in starts.
pub(crate) fn binder(
    service: Result<Arc<Service>, &'static str>,
    vault: Arc<dyn AccountVault>,
    open: Opener,
    now_s: fn() -> u64,
) -> Binder {
    Arc::new(move |binds: Binds| {
        let (service, vault, open) = (service.clone(), vault.clone(), open.clone());
        Box::pin(async move { sign_in(&*service?, &*vault, binds, |url| open(url), now_s()).await })
    })
}

/// The keys a listed device's proof must name, SPKI DER in its nonce's order: its key, then a
/// computer's LAN key; and the key the LAN admits it by (a computer's LAN key, else its one key).
fn listed_keys(row: &Value) -> Option<(Vec<Vec<u8>>, Vec<u8>)> {
    let bytes = |name: &str| {
        row[name]
            .as_str()
            .and_then(tempo_stream::protocol::hex_bytes)
    };
    let key = bytes("publicKey")?;
    match row.get("lanKey").filter(|v| !v.is_null()) {
        Some(_) => {
            let lan = bytes("lanKey")?;
            Some((vec![key, lan.clone()], lan))
        }
        None => Some((vec![key.clone()], key)),
    }
}

/// The SHA-256 of a key, lowercase hex: a pin.
fn pin(spki: &[u8]) -> String {
    tempo_client::hex(ring::digest::digest(&ring::digest::SHA256, spki).as_ref())
}

/// Whether a device the service lists (a row of `native/devices` asked with
/// `x-nexus-device-proof`) is vouched for by its own proof: the owner's or an invited person's
/// sign-in, for exactly the keys the row names, checked with `keys`. Its pin beside it, for the LAN.
/// `stationNotBound` before the station has an owner; `proofMissing` for a row with none (a device
/// added at the station, which keeps today's prompt).
pub(crate) fn vouch_listed(
    row: &Value,
    vault: &dyn AccountVault,
    tenant: &Tenant,
    keys: &KeySet,
    now_s: u64,
) -> Result<(Vouched, String), &'static str> {
    let owner = vault.owner()?.ok_or("stationNotBound")?;
    let proof = row["proof"].as_str().ok_or("proofMissing")?;
    let kind = row["kind"].as_str().ok_or("deviceKind")?;
    let (device_keys, lan) = listed_keys(row).ok_or("invalidRequest")?;
    let device_keys: Vec<&[u8]> = device_keys.iter().map(Vec::as_slice).collect();
    let invites = vault
        .people()?
        .filter(|people| people.station_id == owner.station_id)
        .map(|people| people.invites)
        .unwrap_or_default();
    let vouched = account::vouched(
        proof,
        kind,
        &device_keys,
        keys,
        tenant,
        &owner,
        &invites,
        now_s,
    )?;
    Ok((vouched, pin(&lan)))
}

/// [`vouch_listed`] for each of `rows`, with the issuer's keys fetched at most once for them all
/// ([`keys`]: for a proof naming a key the kept set lacks, if any does), and whether the station must
/// sign in again ([`Keys::reproof`]). Every row is refused with `stationNotBound` unless the owner
/// kept is this station's (`station_id`, the service's id for it): an owner left by another binding
/// vouches for nobody here.
pub(crate) async fn vouch(
    service: &Service,
    vault: &dyn AccountVault,
    station_id: &str,
    rows: &[Value],
    now_s: u64,
) -> (Vec<Result<(Vouched, String), &'static str>>, bool) {
    let refused = |reason| (rows.iter().map(|_| Err(reason)).collect(), false);
    match vault.owner() {
        Ok(Some(owner)) if owner.station_id == station_id => {}
        Ok(_) => return refused("stationNotBound"),
        Err(reason) => return refused(reason),
    }
    let kept = vault
        .account_keys()
        .ok()
        .flatten()
        .filter(|kept| kept.issuer == service.tenant.issuer);
    let unknown = rows
        .iter()
        .filter_map(|row| account::kid_of(row["proof"].as_str()?))
        .find(|kid| kept.as_ref().is_none_or(|kept| !kept.keys.knows(kid)));
    let held = match keys(service, vault, unknown.as_deref(), now_s).await {
        Ok(held) => held,
        Err(reason) => return refused(reason),
    };
    let vouched = rows
        .iter()
        .map(|row| vouch_listed(row, vault, service.tenant, &held.set, now_s))
        .collect();
    (vouched, held.reproof)
}

/// The service's device list for the station, heard at `now_s`: every kept device it no longer
/// lists as approved is taken out of the LAN's cache, and its pin returned for the caller to take
/// out of the LAN and the pins before a LAN session is admitted again; every one it does list is
/// confirmed. A cache kept for another station is dropped whole. The cache held from its read to its
/// save ([`DevicesHeld`]).
pub(crate) fn sweep(
    rows: &[Value],
    vault: &dyn AccountVault,
    station_id: &str,
    now_s: u64,
) -> Result<Vec<String>, &'static str> {
    let held = DevicesHeld::take(vault);
    let Some(mut cache) = vault.device_cache()? else {
        return Ok(Vec::new());
    };
    if cache.station_id != station_id {
        let gone = cache.devices.into_iter().map(|d| d.pin).collect();
        vault.save_device_cache(&held, &DeviceCache::new(station_id))?;
        return Ok(gone);
    }
    let listed: Vec<String> = rows
        .iter()
        .filter(|row| row["approved"] == 1)
        .filter_map(|row| listed_keys(row).map(|(_, lan)| pin(&lan)))
        .collect();
    let gone = cache.sweep(&listed, now_s);
    vault.save_device_cache(&held, &cache)?;
    Ok(gone.into_iter().map(|device| device.pin).collect())
}

#[cfg(test)]
mod tests;
