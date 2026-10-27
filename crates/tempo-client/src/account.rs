//! The account, as a station checks it: the sign-in that binds a station to its owner, and the
//! proofs that let a device the owner (or someone they invited) signed in on be admitted with
//! nobody pressing Approve. The service in between stores and forwards proofs; it never mints one,
//! and nothing here takes its word for who signed in.
//!
//! - **Built in, never read from the service** ([`Tenant`]): the issuer and both client ids, the
//!   Native application's (a station, a computer, a phone) and the browser page's, for each
//!   service origin. A service that named them could point a station at an issuer of its choosing.
//! - **What a sign-in vouches for** ([`station_nonce`], [`device_nonce`]): SHA-256 of a label, the
//!   device's kind and the SPKI DER of each key it holds, put in the sign-in as its `nonce`, so the
//!   token the issuer signs names exactly those keys. The same bytes as the service's `nonceOf`.
//! - **A proof** ([`verify_proof`]) is such a token checked long after its sign-in: everything
//!   [`crate::id_token::verify`] checks except its life, which is bounded by when it was signed
//!   (`iat`) and [`PROOF_MAX_AGE_S`] instead, because the issuer's tokens last hours and a proof is
//!   read again days later.
//! - **The issuer's keys** ([`KeySet`]) are kept, so a proof is checked with the internet down, and
//!   fetched again every [`KEY_SET_REFRESH_S`] or when a token names a key the set lacks (at most
//!   every [`KEY_SET_RETRY_S`]). When the key that signed the station's own proof leaves the set,
//!   devices can no longer check it, and the station must sign in again ([`Owner::needs_reproof`]).
//! - **Who is admitted** ([`vouched`]): the owner's own devices, and the devices of a person the
//!   station invited by address, with the role the invitation gave. Kept for the LAN in a
//!   [`DeviceCache`], swept against the service's list whenever the station hears it.
//! - **What a device trusts** ([`listed_station`]): the stations its account's list names, each
//!   only once the station's own proof holds for the two keys listed beside it, so the LAN key it
//!   then pins is the one the owner signed in for at the shack. Its own sign-in's token it reads
//!   for when it signed and who ([`own_token`]), and it signs in again for each station before
//!   that station stops admitting it on its LAN ([`renew_due`]).
//!
//! Nothing here fetches or stores anything, and nothing keeps a token: the caller does both. A token
//! is never written to a log: it is no password, but it names a person.
use ring::digest::{digest, SHA256};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tempo_stream::protocol;

use crate::id_token::{self, Claims, Expected, LEEWAY_S};
use crate::native::base64url;

/// How long a device's own proof admits it, from its signing (`iat`): on a LAN whose internet is
/// down, a device the station has never heard the service confirm is admitted only this long after
/// the sign-in that made its proof, and then signs in again. Also the oldest proof a station takes
/// at all. Built in here, and only here: the service deliberately carries no copy of it.
pub const PROOF_MAX_AGE_S: u64 = 30 * 24 * 60 * 60;

/// How long before a station stops admitting a device on its LAN the device signs in again for it:
/// the last week of [`PROOF_MAX_AGE_S`]. Not a bound on anything (the station keeps that one), only
/// when the device starts to.
pub const RENEW_BEFORE_S: u64 = 7 * 24 * 60 * 60;

/// When a station stops admitting, on its LAN, a device whose proof was signed at `issued_at`.
pub fn admitted_until(issued_at: u64) -> u64 {
    issued_at.saturating_add(PROOF_MAX_AGE_S)
}

/// Whether a device whose proof was signed at `issued_at` should sign in again for its station now:
/// within [`RENEW_BEFORE_S`] of [`admitted_until`], or past it.
pub fn renew_due(issued_at: u64, now_s: u64) -> bool {
    now_s.saturating_add(RENEW_BEFORE_S) >= admitted_until(issued_at)
}

/// How often a kept key set is fetched again while the internet is up.
pub const KEY_SET_REFRESH_S: u64 = 24 * 60 * 60;
/// How soon after a fetch a token naming a key the set lacks may fetch it again: an unknown `kid`
/// is the issuer rotating, or a forger, and a forger must not make a station fetch at will.
pub const KEY_SET_RETRY_S: u64 = 5 * 60;
/// The most of a key set kept, as JSON: a credential store's smallest blob (Windows, 2560 bytes of
/// UTF-16) with room to spare. Two 2048-bit keys, the issuer's usual set, take about 820.
const KEY_SET_CHARS: usize = 1200;

/// The most devices a station keeps for its LAN: the service's own cap for a station.
pub const CACHED_DEVICES: usize = 8;

/// One service's sign-in, as this build trusts it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tenant {
    /// The service origin this sign-in belongs to, exactly (no trailing slash).
    pub origin: &'static str,
    /// The issuer, exactly as its tokens' `iss` names it.
    pub issuer: &'static str,
    /// The Native application's client id: a station's, a computer's and a phone's sign-in.
    pub native_client_id: &'static str,
    /// The browser page's (SPA) client id.
    pub spa_client_id: &'static str,
}

/// What a tenant not yet given its applications or its issuer names: no sign-in is offered for it.
const UNCONFIGURED: &str = "unconfigured";
const UNCONFIGURED_ISSUER: &str = "https://unconfigured.invalid/";

/// Every service this build signs in to. The client ids are public (every sign-in sends them), but
/// they are the operator's to create, so a tenant stays [`UNCONFIGURED`] until they exist. Staging's
/// exist; production's do not yet.
pub const TENANTS: [Tenant; 2] = [
    Tenant {
        origin: "https://remote-staging.hamradiotools.io",
        issuer: "https://dev-60ma72hxdqoohki1.us.auth0.com/",
        native_client_id: "JJKIvb7BorB4geDgMBxdylkPjsbTt9kw",
        spa_client_id: "GO8aSXhb5SmfNKajeCuvgTpvbCAYScKQ",
    },
    Tenant {
        origin: "https://remote.hamradiotools.io",
        issuer: "https://login.hamradiotools.io/",
        native_client_id: UNCONFIGURED,
        spa_client_id: UNCONFIGURED,
    },
];

impl Tenant {
    /// The sign-in for the service at `origin`, or `None` when this build has none for it, or has
    /// one not yet given its applications.
    pub fn of(origin: &str) -> Option<&'static Tenant> {
        TENANTS
            .iter()
            .find(|tenant| tenant.origin == origin)
            .filter(|tenant| tenant.configured())
    }

    /// Whether both applications and the issuer are named.
    pub fn configured(&self) -> bool {
        self.issuer != UNCONFIGURED_ISSUER
            && self.native_client_id != UNCONFIGURED
            && self.spa_client_id != UNCONFIGURED
    }

    /// The application a device of `kind` signs in to: a browser's is the page's, a computer's and a
    /// phone's the Native one. `None` for a kind there is no such device of.
    pub fn client_for(&self, kind: &str) -> Option<&'static str> {
        match kind {
            "browser" => Some(self.spa_client_id),
            "computer" | "phone" => Some(self.native_client_id),
            _ => None,
        }
    }
}

/// What a station's sign-in vouches for: its signing key and its LAN key, SPKI DER, in that order.
pub fn station_nonce(station_key: &[u8], lan_key: &[u8]) -> String {
    nonce_of("nexus-station/1", None, &[station_key, lan_key])
}

/// What a device's sign-in vouches for: its kind and each of its keys, SPKI DER, in order (a
/// computer's road key then its LAN key; a phone's and a browser's one key).
pub fn device_nonce(kind: &str, keys: &[&[u8]]) -> String {
    nonce_of("nexus-device/1", Some(kind), keys)
}

/// SHA-256, base64url, of the label, a zero byte, the kind and a zero byte when there is one, then
/// each key. DER is self-delimiting and the kinds a fixed set, so no two lists hash the same bytes.
fn nonce_of(label: &str, kind: Option<&str>, keys: &[&[u8]]) -> String {
    let mut bytes = Vec::with_capacity(256);
    bytes.extend_from_slice(label.as_bytes());
    bytes.push(0);
    if let Some(kind) = kind {
        bytes.extend_from_slice(kind.as_bytes());
        bytes.push(0);
    }
    for key in keys {
        bytes.extend_from_slice(key);
    }
    base64url(digest(&SHA256, &bytes).as_ref())
}

/// A proof's claims when it holds: the issuer's signature by a key in `keys`, the issuer and the
/// application `expected` names, its `nonce` exactly, and a signing time (`iat`) neither more than
/// [`LEEWAY_S`] ahead of `now_s` nor [`PROOF_MAX_AGE_S`] or more behind it. Its `exp` is not read:
/// a proof is evidence of a sign-in, not a credential that lapses. The first check that fails is
/// the answer, as [`id_token::verify`]'s, with `tokenIssuedAt` for the signing time.
pub fn verify_proof(
    token: &str,
    keys: &Value,
    expected: &Expected,
    now_s: u64,
) -> Result<Claims, &'static str> {
    let claims = id_token::signed(token, keys, expected.issuer, expected.client_id)?;
    let issued = claims["iat"].as_u64().ok_or("tokenIssuedAt")?;
    if issued > now_s.saturating_add(LEEWAY_S) || now_s >= issued.saturating_add(PROOF_MAX_AGE_S) {
        return Err("tokenIssuedAt");
    }
    if claims["nonce"] != expected.nonce {
        return Err("tokenNonce");
    }
    id_token::claims_of(&claims)
}

/// The `kid` a token's header names, read with no check at all: only to ask whether a key set has
/// that key, never to trust anything the token says.
pub fn kid_of(token: &str) -> Option<String> {
    let header = token.split('.').next()?;
    let header = crate::id_token::unbase64url(header)?;
    let header: Value = serde_json::from_slice(&header).ok()?;
    header["kid"]
        .as_str()
        .filter(|kid| !kid.is_empty() && kid.len() <= 128)
        .map(str::to_string)
}

/// The issuer's keys as a station keeps them: the RS256 signing keys of the set it fetched, each cut
/// to what a check reads (`kid`, `kty`, `n`, `e`), and when it was fetched.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeySet {
    keys: Vec<Value>,
    fetched_at: u64,
}

impl KeySet {
    /// The set the issuer answered (`{"keys":[…]}`), fetched at `now_s`, or `None` when it holds no
    /// key a proof could be signed by. Keys past [`KEY_SET_CHARS`] are left out, last first.
    pub fn fetched(body: &Value, now_s: u64) -> Option<Self> {
        let text = |key: &Value, name: &str| {
            key[name]
                .as_str()
                .filter(|v| !v.is_empty() && v.len() <= 1400)
                .map(str::to_string)
        };
        let mut keys: Vec<Value> = body["keys"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|key| {
                key["kty"] == "RSA"
                    && key.get("use").is_none_or(|v| v == "sig")
                    && key.get("alg").is_none_or(|v| v == "RS256")
            })
            .filter_map(|key| {
                Some(json!({
                    "kid": text(key, "kid").filter(|kid| kid.len() <= 128)?,
                    "kty": "RSA",
                    "n": text(key, "n")?,
                    "e": text(key, "e")?,
                }))
            })
            .collect();
        while !keys.is_empty() && Value::from(keys.clone()).to_string().len() > KEY_SET_CHARS {
            keys.pop();
        }
        (!keys.is_empty()).then_some(Self {
            keys,
            fetched_at: now_s,
        })
    }

    /// The set as [`verify_proof`] reads one.
    pub fn jwks(&self) -> Value {
        json!({ "keys": self.keys })
    }

    /// Whether the set has a key `kid`.
    pub fn knows(&self, kid: &str) -> bool {
        self.keys.iter().any(|key| key["kid"] == kid)
    }

    /// Whether it is time to fetch the set again: [`KEY_SET_REFRESH_S`] since the last fetch, or a
    /// clock now behind that fetch, which says nothing about its age.
    pub fn due(&self, now_s: u64) -> bool {
        now_s < self.fetched_at || now_s >= self.fetched_at.saturating_add(KEY_SET_REFRESH_S)
    }

    /// Whether a token naming `kid` may have the set fetched again now: the set lacks that key, and
    /// [`KEY_SET_RETRY_S`] has passed since the last fetch.
    pub fn may_fetch_for(&self, kid: &str, now_s: u64) -> bool {
        !self.knows(kid)
            && (now_s < self.fetched_at || now_s >= self.fetched_at.saturating_add(KEY_SET_RETRY_S))
    }

    /// When it was fetched, seconds since 1970.
    pub fn fetched_at(&self) -> u64 {
        self.fetched_at
    }
}

/// Who a station belongs to, as it checked its own sign-in: the service's id for it, the issuer and
/// subject of the owner, and the key that signed the station's proof and when it was signed. No
/// token is kept: the service holds the proof and lists it to devices, and the station needs only
/// to know when that proof can no longer be checked.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Owner {
    pub station_id: String,
    pub issuer: String,
    pub subject: String,
    pub proof_kid: String,
    pub proof_issued_at: u64,
}

impl Owner {
    /// The owner a station's own proof names, when it holds for the station's two keys under
    /// `tenant`. Checked by the station itself before it sends the proof anywhere.
    pub fn proven(
        station_id: &str,
        proof: &str,
        keys: &KeySet,
        tenant: &Tenant,
        station_key: &[u8],
        lan_key: &[u8],
        now_s: u64,
    ) -> Result<Self, &'static str> {
        let nonce = station_nonce(station_key, lan_key);
        let expected = Expected {
            issuer: tenant.issuer,
            client_id: tenant.native_client_id,
            nonce: &nonce,
        };
        let claims = verify_proof(proof, &keys.jwks(), &expected, now_s)?;
        Ok(Self {
            station_id: station_id.to_string(),
            issuer: tenant.issuer.to_string(),
            subject: claims.subject,
            proof_kid: kid_of(proof).ok_or("tokenKey")?,
            proof_issued_at: claims.issued_at,
        })
    }

    /// Whether the station must sign in again: the key that signed its proof has left the issuer's
    /// set, so a device can no longer check the proof, and so cannot learn this station's LAN key.
    pub fn needs_reproof(&self, keys: &KeySet) -> bool {
        !keys.knows(&self.proof_kid)
    }
}

/// What a device may do at the station: operate it (station control and logging, and transmit
/// under every rule the station already keeps), or only listen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Operate,
    Listen,
}

/// A person the station invited: their address as [`mailbox`] gives it, and their role.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub mailbox: String,
    pub role: Role,
}

/// An address the way the service matches it: trimmed and lowercase, a `+tag` dropped, and for
/// Gmail the dots of the local part too (`googlemail.com` is `gmail.com`).
pub fn mailbox(address: &str) -> String {
    let email = address.trim().to_lowercase();
    let Some(at) = email.rfind('@').filter(|&at| at >= 1) else {
        return email;
    };
    let (mut local, mut domain) = (email[..at].to_string(), email[at + 1..].to_string());
    if let Some(plus) = local.find('+').filter(|&plus| plus > 0) {
        local.truncate(plus);
    }
    if domain == "googlemail.com" {
        domain = "gmail.com".into();
    }
    if domain == "gmail.com" {
        let dotless = local.replace('.', "");
        if !dotless.is_empty() {
            local = dotless;
        }
    }
    format!("{local}@{domain}")
}

/// A device the station admits on its proof: who signed in, the role it is given, whether that is a
/// guest (anyone but the owner), and when the proof was signed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vouched {
    pub subject: String,
    pub role: Role,
    pub guest: bool,
    pub issued_at: u64,
    /// Whether the sign-in used a second factor: its signature-verified `amr` claim names `mfa`.
    /// Read from the proof `verify_proof` already checked, never from an unverified parse, so a
    /// forged or unsigned MFA claim can never set it. The station waives the new device's transmit
    /// hold on this (the 2026-10-23 sign-in decision): an MFA sign-in is trusted to transmit at once.
    pub mfa: bool,
    /// A guest's address as [`mailbox`] gives it, the invitation it matched; `None` for the owner.
    pub mailbox: Option<String>,
}

/// Whether a device's proof admits it at this station, and as what. The proof must hold
/// ([`verify_proof`]) for the application its `kind` signs in to and for exactly the keys it
/// registered (`device_keys`, SPKI DER, in its nonce's order), under the issuer the station's owner
/// was proven under. Then: the owner's own sign-in operates; a guest's, with an address the issuer
/// verified that matches an invitation, has the invitation's role. Anyone else is `proofNotYours`.
#[allow(clippy::too_many_arguments)]
pub fn vouched(
    proof: &str,
    kind: &str,
    device_keys: &[&[u8]],
    keys: &KeySet,
    tenant: &Tenant,
    owner: &Owner,
    invites: &[Invite],
    now_s: u64,
) -> Result<Vouched, &'static str> {
    if owner.issuer != tenant.issuer {
        return Err("ownerIssuer");
    }
    let client_id = tenant.client_for(kind).ok_or("deviceKind")?;
    let nonce = device_nonce(kind, device_keys);
    let expected = Expected {
        issuer: tenant.issuer,
        client_id,
        nonce: &nonce,
    };
    let claims = verify_proof(proof, &keys.jwks(), &expected, now_s)?;
    let mfa = claims.methods.iter().any(|method| method == "mfa");
    if claims.subject == owner.subject {
        return Ok(Vouched {
            subject: claims.subject,
            role: Role::Operate,
            guest: false,
            issued_at: claims.issued_at,
            mfa,
            mailbox: None,
        });
    }
    let invited = claims
        .email
        .as_deref()
        .filter(|_| claims.email_verified)
        .map(mailbox)
        .and_then(|address| invites.iter().find(|invite| invite.mailbox == address));
    match invited {
        Some(invite) => Ok(Vouched {
            subject: claims.subject,
            role: invite.role,
            guest: true,
            issued_at: claims.issued_at,
            mfa,
            mailbox: Some(invite.mailbox.clone()),
        }),
        None => Err("proofNotYours"),
    }
}

/// The claims of the ID token a device's own sign-in was given, once it is this tenant's issuer's,
/// for the tenant's Native application, and carries exactly the `nonce` the sign-in sent. Its
/// signature is not checked: the token came straight from the issuer's token endpoint, over the TLS
/// this build verifies, to the sign-in that asked for it with PKCE (OpenID Connect Core 1.0
/// §3.1.3.7, rule 6). It is read for the device's own use (who signed in, and when); the proof the
/// device sends on is checked by the station itself.
pub fn own_token(token: &str, tenant: &Tenant, nonce: &str) -> Result<Claims, &'static str> {
    let payload = token.split('.').nth(1).ok_or("tokenMalformed")?;
    let claims: Value = crate::id_token::unbase64url(payload)
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .ok_or("tokenMalformed")?;
    id_token::issued_for(&claims, tenant.issuer, tenant.native_client_id)?;
    if claims["nonce"] != nonce {
        return Err("tokenNonce");
    }
    id_token::claims_of(&claims)
}

/// A station a device's account lists (`client/session`), as the device trusts it once its proof
/// holds ([`listed_station`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    /// The service's id for it, and the name it was given.
    pub id: String,
    pub name: String,
    /// Its signing key and its LAN key, SPKI as lowercase hex: the two its proof names.
    pub station_key: String,
    pub lan_key: String,
    /// Whether it is the device's own account's station, or one the account was invited to.
    pub own: bool,
    /// What the device may do there.
    pub role: Role,
}

/// One station of a device's account list, trusted: the service lists it with the proof the
/// station's owner signed in with at the shack, and that proof holds here, signed by the issuer for
/// its Native application, for exactly the station's signing key and LAN key as listed beside it
/// ([`station_nonce`]). For a station listed as the account's own, the proof must also be that
/// account's own sign-in (`own_subject`, `proofNotYours`). Only then is its LAN key the one to pin:
/// neither the service nor a machine on the network can stand in for the station.
///
/// A station's proof is not held to [`PROOF_MAX_AGE_S`], which bounds a device's: it stays evidence
/// of the owner's sign-in for those keys until the station signs in again
/// ([`Owner::needs_reproof`]). One signed ahead of `now_s` (with [`LEEWAY_S`]) is refused.
/// `stationNotProven` for a station listed with no proof or no LAN key (one paired by code whose
/// owner has not signed in at the shack yet); `invalidResponse` for a row out of shape.
pub fn listed_station(
    row: &Value,
    keys: &KeySet,
    tenant: &Tenant,
    own_subject: &str,
    now_s: u64,
) -> Result<Listed, &'static str> {
    let text = |name: &str| row[name].as_str().ok_or("invalidResponse");
    let id = text("id").map(str::to_string)?;
    let (own, role) = match text("role")? {
        "owner" => (true, Role::Operate),
        "operate" => (false, Role::Operate),
        "listen" => (false, Role::Listen),
        _ => return Err("invalidResponse"),
    };
    let (Some(proof), Some(lan_key)) = (row["proof"].as_str(), row["lanKey"].as_str()) else {
        return Err("stationNotProven");
    };
    let station_key = text("stationKey")?;
    let spki = |hex: &str| {
        Some(hex)
            .filter(|hex| protocol::device_key(hex))
            .and_then(protocol::hex_bytes)
            .ok_or("invalidResponse")
    };
    let nonce = station_nonce(&spki(station_key)?, &spki(lan_key)?);
    let claims = id_token::signed(proof, &keys.jwks(), tenant.issuer, tenant.native_client_id)?;
    let issued = claims["iat"].as_u64().ok_or("tokenIssuedAt")?;
    if issued > now_s.saturating_add(LEEWAY_S) {
        return Err("tokenIssuedAt");
    }
    if claims["nonce"] != nonce.as_str() {
        return Err("tokenNonce");
    }
    let claims = id_token::claims_of(&claims)?;
    if own && claims.subject != own_subject {
        return Err("proofNotYours");
    }
    Ok(Listed {
        id,
        name: row["name"].as_str().unwrap_or_default().to_string(),
        station_key: station_key.to_string(),
        lan_key: lan_key.to_string(),
        own,
        role,
    })
}

/// A device the station admitted on its proof, kept so it is admitted on the LAN with the internet
/// down: the SHA-256 of the key the LAN admits it by (lowercase hex), its role, whether it is a
/// guest's, when its proof was signed, and when the service last listed it as approved. One-letter
/// names, so [`CACHED_DEVICES`] fit one credential-store entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CachedDevice {
    #[serde(rename = "p")]
    pub pin: String,
    #[serde(rename = "r")]
    pub role: Role,
    #[serde(rename = "g", default, skip_serializing_if = "not")]
    pub guest: bool,
    #[serde(rename = "i")]
    pub issued_at: u64,
    #[serde(rename = "c")]
    pub confirmed_at: u64,
}

fn not(value: &bool) -> bool {
    !*value
}

impl CachedDevice {
    /// Whether the LAN admits this device now. While the station hears the service
    /// (`service_heard`), the service's list decides, and [`DeviceCache::sweep`] has already taken
    /// out every device it no longer lists. With the internet down, the device's own proof decides:
    /// [`PROOF_MAX_AGE_S`] from its signing, and not before it.
    pub fn admits(&self, now_s: u64, service_heard: bool) -> bool {
        service_heard
            || (now_s.saturating_add(LEEWAY_S) >= self.issued_at
                && now_s < self.issued_at.saturating_add(PROOF_MAX_AGE_S))
    }
}

/// The devices a station keeps for its LAN, for the service's id of that station: a cache left by
/// another binding admits nobody.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCache {
    pub station_id: String,
    pub devices: Vec<CachedDevice>,
}

impl DeviceCache {
    /// An empty cache for the station the service knows as `station_id`.
    pub fn new(station_id: &str) -> Self {
        Self {
            station_id: station_id.to_string(),
            devices: Vec::new(),
        }
    }

    /// The device with `pin`, if kept.
    pub fn find(&self, pin: &str) -> Option<&CachedDevice> {
        self.devices.iter().find(|device| device.pin == pin)
    }

    /// Keeps `device`, in place of any kept with its pin. `deviceCacheFull` when [`CACHED_DEVICES`]
    /// others are kept: the service lists no more, so a ninth is a list the station should not take.
    pub fn keep(&mut self, device: CachedDevice) -> Result<(), &'static str> {
        if let Some(kept) = self.devices.iter_mut().find(|d| d.pin == device.pin) {
            *kept = device;
            return Ok(());
        }
        if self.devices.len() >= CACHED_DEVICES {
            return Err("deviceCacheFull");
        }
        self.devices.push(device);
        Ok(())
    }

    /// Takes out the device with `pin`; whether one was kept.
    pub fn forget(&mut self, pin: &str) -> bool {
        let before = self.devices.len();
        self.devices.retain(|device| device.pin != pin);
        self.devices.len() != before
    }

    /// The service's list, heard at `now_s`: the pins of every device it lists as approved. Each
    /// kept device it lists is confirmed now; every one it does not is taken out, and returned, so
    /// the caller takes it out of the LAN and its pins too, before any LAN session is admitted.
    pub fn sweep(&mut self, listed: &[String], now_s: u64) -> Vec<CachedDevice> {
        let (kept, gone): (Vec<_>, Vec<_>) = self
            .devices
            .drain(..)
            .partition(|device| listed.contains(&device.pin));
        self.devices = kept
            .into_iter()
            .map(|device| CachedDevice {
                confirmed_at: now_s,
                ..device
            })
            .collect();
        gone
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hex;

    // Minted once by throwaway RSA keys that never left a scratch process: only their public halves
    // are in the fixture's key set, so these tokens prove nothing to anyone. The P-256 keys are
    // public keys only. The nonces were also computed by the service's own `nonceOf`.
    const CASES: &str = include_str!("../tests/fixtures/account-proof-cases.json");

    const TEST: Tenant = Tenant {
        origin: "https://service.test.invalid",
        issuer: "https://issuer.test.invalid/",
        native_client_id: "test-native-client",
        spa_client_id: "test-spa-client",
    };

    struct Case {
        all: Value,
    }

    impl Case {
        fn load() -> Self {
            Self {
                all: serde_json::from_str(CASES).unwrap(),
            }
        }
        fn token(&self, name: &str) -> &str {
            self.all["tokens"][name].as_str().unwrap()
        }
        fn key(&self, name: &str) -> Vec<u8> {
            let text = self.all["keys"][name].as_str().unwrap();
            (0..text.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
                .collect()
        }
        fn nonce(&self, name: &str) -> &str {
            self.all["nonces"][name].as_str().unwrap()
        }
        fn now(&self) -> u64 {
            self.all["now"].as_u64().unwrap()
        }
        fn issued(&self) -> u64 {
            self.all["issuedAt"].as_u64().unwrap()
        }
        fn keys(&self) -> KeySet {
            KeySet::fetched(&self.all["jwks"], self.now()).unwrap()
        }
        fn owner(&self) -> Owner {
            Owner::proven(
                "station-1",
                self.token("station"),
                &self.keys(),
                &TEST,
                &self.key("station"),
                &self.key("stationLan"),
                self.now(),
            )
            .unwrap()
        }
        fn computer(&self) -> [Vec<u8>; 2] {
            [self.key("road"), self.key("computerLan")]
        }
        fn vouch(&self, name: &str, invites: &[Invite], now: u64) -> Result<Vouched, &'static str> {
            let [road, lan] = self.computer();
            vouched(
                self.token(name),
                "computer",
                &[&road, &lan],
                &self.keys(),
                &TEST,
                &self.owner(),
                invites,
                now,
            )
        }
    }

    #[test]
    fn the_offline_proof_age_is_thirty_days() {
        assert_eq!(PROOF_MAX_AGE_S, 2_592_000);
    }

    #[test]
    fn nonces_are_the_services_own() {
        let case = Case::load();
        assert_eq!(
            station_nonce(&case.key("station"), &case.key("stationLan")),
            case.nonce("station")
        );
        let [road, lan] = case.computer();
        assert_eq!(
            device_nonce("computer", &[&road, &lan]),
            case.nonce("computer")
        );
        assert_eq!(
            device_nonce("browser", &[&case.key("browser")]),
            case.nonce("browser")
        );
        // The order of the keys, and the kind, are part of what is vouched for.
        assert_ne!(
            device_nonce("computer", &[&lan, &road]),
            case.nonce("computer")
        );
        assert_ne!(
            device_nonce("phone", &[&road, &lan]),
            case.nonce("computer")
        );
    }

    #[test]
    fn a_proof_is_bounded_by_its_signing_not_its_life() {
        let case = Case::load();
        let issued = case.issued();
        let expected = Expected {
            issuer: TEST.issuer,
            client_id: TEST.native_client_id,
            nonce: case.nonce("computer"),
        };
        let check = |now| verify_proof(case.token("computer"), &case.all["jwks"], &expected, now);
        // Its `exp` (ten hours) long past, and still a proof.
        assert_eq!(check(issued + 20 * 86_400).unwrap().issued_at, issued);
        assert!(check(issued + PROOF_MAX_AGE_S - 1).is_ok());
        assert_eq!(check(issued + PROOF_MAX_AGE_S), Err("tokenIssuedAt"));
        assert_eq!(check(issued + 400 * 86_400), Err("tokenIssuedAt"));
        // Signed ahead of this clock by more than two clocks may disagree.
        assert!(check(issued - LEEWAY_S).is_ok());
        assert_eq!(check(issued - LEEWAY_S - 1), Err("tokenIssuedAt"));
        assert_eq!(
            verify_proof(
                case.token("noIat"),
                &case.all["jwks"],
                &expected,
                case.now()
            ),
            Err("tokenIssuedAt")
        );
    }

    #[test]
    fn a_proof_keeps_every_check_a_token_has() {
        let case = Case::load();
        let expected = Expected {
            issuer: TEST.issuer,
            client_id: TEST.native_client_id,
            nonce: case.nonce("computer"),
        };
        let check = |name| verify_proof(case.token(name), &case.all["jwks"], &expected, case.now());
        assert_eq!(check("forged"), Err("tokenSignature"));
        assert_eq!(check("wrongIss"), Err("tokenIssuer"));
        assert_eq!(check("browser"), Err("tokenAudience"));
        let other = Expected {
            nonce: case.nonce("station"),
            ..expected
        };
        assert_eq!(
            verify_proof(
                case.token("computer"),
                &case.all["jwks"],
                &other,
                case.now()
            ),
            Err("tokenNonce")
        );
    }

    #[test]
    fn the_owner_is_proven_by_the_stations_own_sign_in_for_its_keys() {
        let case = Case::load();
        assert_eq!(
            case.owner(),
            Owner {
                station_id: "station-1".into(),
                issuer: TEST.issuer.into(),
                subject: case.all["owner"].as_str().unwrap().into(),
                proof_kid: "test-p".into(),
                proof_issued_at: case.issued(),
            }
        );
        // Another LAN key than the one signed in for.
        let proven = Owner::proven(
            "station-1",
            case.token("station"),
            &case.keys(),
            &TEST,
            &case.key("station"),
            &case.key("computerLan"),
            case.now(),
        );
        assert_eq!(proven, Err("tokenNonce"));
        // A device's sign-in is no station's.
        let proven = Owner::proven(
            "station-1",
            case.token("computer"),
            &case.keys(),
            &TEST,
            &case.key("station"),
            &case.key("stationLan"),
            case.now(),
        );
        assert_eq!(proven, Err("tokenNonce"));
    }

    #[test]
    fn the_owners_devices_operate() {
        let case = Case::load();
        assert_eq!(
            case.vouch("computer", &[], case.now()),
            Ok(Vouched {
                subject: case.all["owner"].as_str().unwrap().into(),
                role: Role::Operate,
                guest: false,
                issued_at: case.issued(),
                mfa: false,
                mailbox: None,
            })
        );
        // A browser's proof is for the page's application.
        let owner = case.owner();
        let browser = case.key("browser");
        let vouch = |name| {
            vouched(
                case.token(name),
                "browser",
                &[&browser],
                &case.keys(),
                &TEST,
                &owner,
                &[],
                case.now(),
            )
        };
        assert_eq!(vouch("browser").map(|v| v.role), Ok(Role::Operate));
        assert_eq!(vouch("browserNativeAud"), Err("tokenAudience"));
    }

    #[test]
    fn a_device_proof_names_exactly_its_keys_and_kind() {
        let case = Case::load();
        let [road, lan] = case.computer();
        let keys = case.keys();
        let owner = case.owner();
        let vouch = |kind, device: &[&[u8]]| {
            vouched(
                case.token("computer"),
                kind,
                device,
                &keys,
                &TEST,
                &owner,
                &[],
                case.now(),
            )
        };
        assert!(vouch("computer", &[&road, &lan]).is_ok());
        assert_eq!(vouch("computer", &[&road]), Err("tokenNonce"));
        assert_eq!(vouch("computer", &[&road, &road]), Err("tokenNonce"));
        assert_eq!(vouch("phone", &[&road, &lan]), Err("tokenNonce"));
        assert_eq!(vouch("station", &[&road, &lan]), Err("deviceKind"));
    }

    #[test]
    fn a_guest_needs_an_invitation_and_a_verified_address() {
        let case = Case::load();
        let invited = [Invite {
            mailbox: "k9.guest@example.test".into(),
            role: Role::Listen,
        }];
        let guest = case.vouch("guest", &invited, case.now()).unwrap();
        assert_eq!((guest.role, guest.guest), (Role::Listen, true));
        assert_eq!(guest.subject, "auth0|test-guest");
        let operate = [Invite {
            role: Role::Operate,
            ..invited[0].clone()
        }];
        assert_eq!(
            case.vouch("guest", &operate, case.now()).map(|v| v.role),
            Ok(Role::Operate)
        );
        assert_eq!(case.vouch("guest", &[], case.now()), Err("proofNotYours"));
        // The address it carries is invited, but the issuer never verified it.
        let unverified = [Invite {
            mailbox: "k9guest@example.test".into(),
            role: Role::Operate,
        }];
        assert_eq!(
            case.vouch("guestUnverified", &unverified, case.now()),
            Err("proofNotYours")
        );
        assert_eq!(
            case.vouch("stranger", &invited, case.now()),
            Err("proofNotYours")
        );
    }

    #[test]
    fn an_owner_proven_under_another_issuer_admits_nobody() {
        let case = Case::load();
        let owner = Owner {
            issuer: "https://other.test.invalid/".into(),
            ..case.owner()
        };
        let [road, lan] = case.computer();
        let vouch = vouched(
            case.token("computer"),
            "computer",
            &[&road, &lan],
            &case.keys(),
            &TEST,
            &owner,
            &[],
            case.now(),
        );
        assert_eq!(vouch, Err("ownerIssuer"));
    }

    #[test]
    fn addresses_match_as_the_service_matches_them() {
        assert_eq!(
            mailbox(" K9.Guest+radio@Example.test "),
            "k9.guest@example.test"
        );
        assert_eq!(mailbox("A.B+x@GoogleMail.com"), "ab@gmail.com");
        assert_eq!(mailbox("+tag@example.test"), "+tag@example.test");
        assert_eq!(mailbox("...@gmail.com"), "...@gmail.com");
        assert_eq!(mailbox("nobody"), "nobody");
    }

    #[test]
    fn the_key_set_keeps_only_signing_keys_cut_to_what_a_check_reads() {
        let case = Case::load();
        let keys = case.keys();
        assert!(keys.knows("test-p") && keys.knows("test-q"));
        assert!(!keys.knows("test-enc"));
        for key in keys.jwks()["keys"].as_array().unwrap() {
            let mut names: Vec<_> = key.as_object().unwrap().keys().cloned().collect();
            names.sort();
            assert_eq!(names, ["e", "kid", "kty", "n"]);
        }
        // What is kept still checks a proof.
        let [road, lan] = case.computer();
        let proof = vouched(
            case.token("rotated"),
            "computer",
            &[&road, &lan],
            &keys,
            &TEST,
            &case.owner(),
            &[],
            case.now(),
        );
        assert!(proof.is_ok());
        assert!(serde_json::to_string(&keys).unwrap().len() <= KEY_SET_CHARS + 40);
        assert_eq!(KeySet::fetched(&json!({ "keys": [] }), 1), None);
        assert_eq!(KeySet::fetched(&json!({}), 1), None);
        // A set too big for the store keeps what fits, first keys first.
        let many: Vec<Value> = (0..6)
            .map(|i| {
                let mut key = case.all["jwks"]["keys"][0].clone();
                key["kid"] = format!("k{i}").into();
                key
            })
            .collect();
        let cut = KeySet::fetched(&json!({ "keys": many }), 1).unwrap();
        assert!(cut.knows("k0") && cut.knows("k1") && !cut.knows("k5"));
        assert!(cut.jwks()["keys"].to_string().len() <= KEY_SET_CHARS);
    }

    #[test]
    fn the_key_set_is_fetched_again_daily_and_for_an_unknown_key_at_most_every_five_minutes() {
        let case = Case::load();
        let keys = KeySet::fetched(&case.all["jwks"], 1000).unwrap();
        assert!(!keys.due(1000 + KEY_SET_REFRESH_S - 1));
        assert!(keys.due(1000 + KEY_SET_REFRESH_S));
        assert!(keys.due(999));
        assert!(!keys.may_fetch_for("test-p", 1000 + KEY_SET_RETRY_S));
        assert!(!keys.may_fetch_for("new", 1000 + KEY_SET_RETRY_S - 1));
        assert!(keys.may_fetch_for("new", 1000 + KEY_SET_RETRY_S));
        assert_eq!(keys.fetched_at(), 1000);
    }

    #[test]
    fn the_station_signs_in_again_when_its_proofs_key_leaves_the_set() {
        let case = Case::load();
        let owner = case.owner();
        assert!(!owner.needs_reproof(&case.keys()));
        let mut rotated = case.all["jwks"].clone();
        rotated["keys"].as_array_mut().unwrap().remove(0);
        let rotated = KeySet::fetched(&rotated, case.now()).unwrap();
        assert!(owner.needs_reproof(&rotated));
        assert_eq!(kid_of(case.token("rotated")).as_deref(), Some("test-q"));
        assert_eq!(kid_of("not-a-token"), None);
    }

    fn cached(pin: &str, issued_at: u64) -> CachedDevice {
        CachedDevice {
            pin: pin.into(),
            role: Role::Operate,
            guest: false,
            issued_at,
            confirmed_at: issued_at,
        }
    }

    #[test]
    fn offline_a_device_is_admitted_thirty_days_from_its_proofs_signing() {
        let device = cached("aa", 1_000_000);
        assert!(device.admits(1_000_000, false));
        assert!(device.admits(1_000_000 + PROOF_MAX_AGE_S - 1, false));
        assert!(!device.admits(1_000_000 + PROOF_MAX_AGE_S, false));
        assert!(!device.admits(1_000_000 - LEEWAY_S - 1, false));
        // While the service is heard, its list decides.
        assert!(device.admits(1_000_000 + PROOF_MAX_AGE_S, true));
    }

    #[test]
    fn the_sweep_takes_out_what_the_service_no_longer_lists_and_confirms_the_rest() {
        let mut cache = DeviceCache::new("station-1");
        for pin in ["aa", "bb", "cc"] {
            cache.keep(cached(pin, 10)).unwrap();
        }
        let gone = cache.sweep(&["aa".into(), "cc".into(), "zz".into()], 50);
        assert_eq!(gone, vec![cached("bb", 10)]);
        assert_eq!(cache.find("aa").map(|d| d.confirmed_at), Some(50));
        assert_eq!(cache.find("cc").map(|d| d.issued_at), Some(10));
        assert_eq!(cache.find("bb"), None);
        assert_eq!(cache.find("zz"), None);
        assert_eq!(cache.sweep(&[], 60).len(), 2);
        assert!(cache.devices.is_empty());
    }

    #[test]
    fn the_cache_holds_the_services_eight_and_replaces_by_pin() {
        let mut cache = DeviceCache::new("station-1");
        for i in 0..CACHED_DEVICES {
            cache.keep(cached(&hex(&[i as u8; 32]), 10)).unwrap();
        }
        assert_eq!(cache.keep(cached("ff", 10)), Err("deviceCacheFull"));
        let first = hex(&[0u8; 32]);
        cache.keep(cached(&first, 99)).unwrap();
        assert_eq!(cache.find(&first).map(|d| d.issued_at), Some(99));
        assert!(cache.forget(&first) && !cache.forget(&first));
        // Eight full records fit the smallest store entry.
        let mut full = DeviceCache::new(&"s".repeat(36));
        for i in 0..CACHED_DEVICES {
            full.keep(CachedDevice {
                guest: true,
                role: Role::Listen,
                ..cached(&hex(&[i as u8; 32]), 1_800_000_000)
            })
            .unwrap();
        }
        assert!(serde_json::to_string(&full).unwrap().len() <= 1280);
    }

    /// The fixture's station as the service lists it to a device of `role` there.
    fn station_row(case: &Case, role: &str) -> Value {
        json!({"id":"20000000-0000-4000-8000-000000000001","name":"Shack",
            "stationKey":case.all["keys"]["station"],"lanKey":case.all["keys"]["stationLan"],
            "proof":case.token("station"),"role":role})
    }

    /// ★ A listed station is trusted only when its own proof names exactly the two keys listed
    /// beside it: the LAN key a device then pins is the one the owner signed in for at the shack.
    /// CONTROL: the same proof beside another LAN key, or another station key, or a device's proof
    /// beside the station's keys, is `tokenNonce`.
    #[test]
    fn a_listed_station_holds_for_exactly_its_two_keys() {
        let case = Case::load();
        let owner = case.all["owner"].as_str().unwrap();
        let listed = listed_station(
            &station_row(&case, "owner"),
            &case.keys(),
            &TEST,
            owner,
            case.now(),
        );
        assert_eq!(
            listed,
            Ok(Listed {
                id: "20000000-0000-4000-8000-000000000001".into(),
                name: "Shack".into(),
                station_key: case.all["keys"]["station"].as_str().unwrap().into(),
                lan_key: case.all["keys"]["stationLan"].as_str().unwrap().into(),
                own: true,
                role: Role::Operate,
            })
        );
        let check = |row: Value| listed_station(&row, &case.keys(), &TEST, owner, case.now());
        let mut other_lan = station_row(&case, "owner");
        other_lan["lanKey"] = case.all["keys"]["computerLan"].clone();
        assert_eq!(check(other_lan), Err("tokenNonce"));
        let mut other_station = station_row(&case, "owner");
        other_station["stationKey"] = case.all["keys"]["road"].clone();
        assert_eq!(check(other_station), Err("tokenNonce"));
        let mut device_proof = station_row(&case, "owner");
        device_proof["proof"] = case.token("computer").into();
        assert_eq!(check(device_proof), Err("tokenNonce"));
    }

    /// ★ A listed station's proof keeps every check a token has: the issuer's signature, its
    /// issuer, and the Native application (a browser's sign-in is no station's).
    #[test]
    fn a_listed_stations_proof_keeps_every_check_a_token_has() {
        let case = Case::load();
        let owner = case.all["owner"].as_str().unwrap();
        for (name, why) in [
            ("forged", "tokenSignature"),
            ("wrongIss", "tokenIssuer"),
            ("browser", "tokenAudience"),
        ] {
            let mut row = station_row(&case, "owner");
            row["proof"] = case.token(name).into();
            assert_eq!(
                listed_station(&row, &case.keys(), &TEST, owner, case.now()),
                Err(why),
                "{name}"
            );
        }
    }

    /// ★ A station's proof is evidence of its owner's sign-in at the shack, and is not held to a
    /// device's thirty days: a year on, it still vouches for the station's keys. CONTROL: one
    /// signed ahead of this clock by more than two clocks may disagree is refused.
    #[test]
    fn a_stations_proof_is_not_held_to_a_devices_age() {
        let case = Case::load();
        let owner = case.all["owner"].as_str().unwrap();
        let row = station_row(&case, "owner");
        let at = |now| listed_station(&row, &case.keys(), &TEST, owner, now).map(|l| l.own);
        assert_eq!(at(case.issued() + 400 * 86_400), Ok(true));
        assert_eq!(at(case.issued() - LEEWAY_S), Ok(true));
        assert_eq!(at(case.issued() - LEEWAY_S - 1), Err("tokenIssuedAt"));
    }

    /// ★ A station listed as the account's own must be proven by that account's own sign-in, so a
    /// service cannot list another account's station as one's own. A station the account was
    /// invited to is proven by its owner, whoever that is, and carries the invitation's role.
    #[test]
    fn a_station_listed_as_ones_own_is_proven_by_ones_own_sign_in() {
        let case = Case::load();
        let check = |role: &str, subject: &str| {
            listed_station(
                &station_row(&case, role),
                &case.keys(),
                &TEST,
                subject,
                case.now(),
            )
            .map(|l| (l.own, l.role))
        };
        assert_eq!(check("owner", "auth0|test-guest"), Err("proofNotYours"));
        assert_eq!(
            check("operate", "auth0|test-guest"),
            Ok((false, Role::Operate))
        );
        assert_eq!(
            check("listen", "auth0|test-guest"),
            Ok((false, Role::Listen))
        );
        assert_eq!(check("admin", "auth0|test-guest"), Err("invalidResponse"));
    }

    /// ★ A station listed with no proof, or no LAN key (paired by code, its owner never signed in
    /// at the shack), is not proven, and nothing of it is trusted.
    #[test]
    fn a_station_listed_with_no_proof_is_not_proven() {
        let case = Case::load();
        let owner = case.all["owner"].as_str().unwrap();
        let check = |row: Value| listed_station(&row, &case.keys(), &TEST, owner, case.now());
        for gone in ["proof", "lanKey"] {
            let mut row = station_row(&case, "owner");
            row[gone] = Value::Null;
            assert_eq!(check(row), Err("stationNotProven"), "{gone}");
        }
        let mut row = station_row(&case, "owner");
        row["lanKey"] = "not-a-key".into();
        assert_eq!(check(row), Err("invalidResponse"));
        let mut row = station_row(&case, "owner");
        row.as_object_mut().unwrap().remove("id");
        assert_eq!(check(row), Err("invalidResponse"));
    }

    /// ★ A device's own token is read for its issuer, its application and its nonce: the one its
    /// sign-in sent. Its signature is not this check's (the token came straight from the issuer;
    /// the station checks what the device sends on): a forged signature still reads.
    #[test]
    fn a_devices_own_token_is_read_for_its_issuer_application_and_nonce() {
        let case = Case::load();
        let nonce = case.nonce("computer");
        let own = own_token(case.token("computer"), &TEST, nonce).unwrap();
        assert_eq!(
            (own.subject.as_str(), own.issued_at),
            (case.all["owner"].as_str().unwrap(), case.issued())
        );
        assert_eq!(
            own_token(case.token("computer"), &TEST, case.nonce("station")),
            Err("tokenNonce")
        );
        assert_eq!(
            own_token(case.token("wrongIss"), &TEST, nonce),
            Err("tokenIssuer")
        );
        assert_eq!(
            own_token(case.token("browser"), &TEST, case.nonce("browser")),
            Err("tokenAudience")
        );
        assert_eq!(
            own_token("not-a-token", &TEST, nonce),
            Err("tokenMalformed")
        );
        assert!(own_token(case.token("forged"), &TEST, nonce).is_ok());
    }

    /// A device signs in again in the last week before its station stops admitting it on the LAN,
    /// and not before.
    #[test]
    fn a_device_signs_in_again_in_the_last_week_of_its_proof() {
        let issued = 1_000_000;
        assert_eq!(admitted_until(issued), issued + PROOF_MAX_AGE_S);
        assert_eq!(RENEW_BEFORE_S, 7 * 86_400);
        let start = issued + PROOF_MAX_AGE_S - RENEW_BEFORE_S;
        assert!(!renew_due(issued, start - 1));
        assert!(renew_due(issued, start));
        assert!(renew_due(issued, issued + PROOF_MAX_AGE_S + 1));
    }

    /// Staging signs in to its own tenant, with exactly its two applications; production has none
    /// yet, so no sign-in is offered there.
    #[test]
    fn staging_carries_its_tenant_and_production_none_yet() {
        assert_eq!(
            Tenant::of("https://remote-staging.hamradiotools.io"),
            Some(&Tenant {
                origin: "https://remote-staging.hamradiotools.io",
                issuer: "https://dev-60ma72hxdqoohki1.us.auth0.com/",
                native_client_id: "JJKIvb7BorB4geDgMBxdylkPjsbTt9kw",
                spa_client_id: "GO8aSXhb5SmfNKajeCuvgTpvbCAYScKQ",
            })
        );
        assert_eq!(Tenant::of("https://remote.hamradiotools.io"), None);
        assert!(!TENANTS[1].configured());
    }

    #[test]
    fn only_a_configured_tenant_signs_in() {
        assert!(TENANTS.iter().all(|t| !t.issuer.is_empty()));
        assert_eq!(Tenant::of("https://elsewhere.test"), None);
        assert!(TEST.configured());
        for unset in [
            Tenant {
                issuer: UNCONFIGURED_ISSUER,
                ..TEST
            },
            Tenant {
                native_client_id: UNCONFIGURED,
                ..TEST
            },
            Tenant {
                spa_client_id: UNCONFIGURED,
                ..TEST
            },
        ] {
            assert!(!unset.configured());
        }
        assert_eq!(TEST.client_for("browser"), Some("test-spa-client"));
        assert_eq!(TEST.client_for("phone"), Some("test-native-client"));
        assert_eq!(TEST.client_for("computer"), Some("test-native-client"));
        assert_eq!(TEST.client_for("station"), None);
    }
}
