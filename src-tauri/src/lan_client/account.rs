//! Signing in on this computer with the station's account: one sign-in, and a station is on this
//! computer on its network and over the internet, with no code and nothing pressed at the station.
//! The station's half is `remote_service::lan::enrol`; the service's, `client/session` and
//! `client/device` (`remote/src/index.ts`, THE NATIVE CLIENT).
//!
//! ## One sign-in ([`sign_in`])
//!
//! 1. **Two keys**, this computer's for the station it will add: one for the internet road (N3: a
//!    key of its own) and one for the station's network. They are kept before anything is sent
//!    ([`Stations::staged`]), so a sign-in that stops part-way and is tried again names the same
//!    keys: the service finds the device it registered, and nothing is added twice.
//! 2. **The sign-in** through the system browser to this build's own tenant and its Native
//!    application (`crate::account_sign_in`: the issuer and the application are built in, never the
//!    service's to name), its nonce the hash of both keys (`tempo_client::account::device_nonce`).
//!    The issuer's ID token then names exactly them: this computer's proof. It says who signed in
//!    and the address the issuer verified (`Scope::Device`), which is how a station knows a person
//!    it invited; the station and the service read it, and no one else.
//! 3. **The account's stations** (`client/session`), each trusted only once the station's own proof
//!    holds here, with the issuer's keys fetched from the issuer itself, for exactly the station key
//!    and LAN key listed beside it (`tempo_client::account::listed_station`). A station the account
//!    is invited to only listen at is refused (`listenOnly`: a computer holds station control on the
//!    station's network). The operator picks one when more than one can take this computer.
//! 4. **The internet road**, registered in the same step, and free: the road key as a device of that
//!    station, with the LAN key and the proof (`client/device`, signed by the road key). The service
//!    approves it on the proof, and the station checks the same proof itself; since the 2026-10-23 sign-in decision it
//!    grants control on that alone (`remote_service::account::GRANTS_ON_SIGN_IN`), so an approved
//!    road does not wait for Approve at the station ([`Signed`]) — it holds control at once, with a
//!    transmit hold the station enforces. A road the service has not approved still waits.
//! 5. **First contact on the station's network** ([`enrol`]): where a look by name finds it (an
//!    advert whose key tag starts its LAN key's fingerprint), else where the operator says it is.
//!    TLS with this computer's LAN key and the station's pinned, so no other machine ever hears the
//!    proof, and `enrol` with the proof as the first message. The station checks it itself, keeps
//!    this computer, and says its ids.
//! 6. **Kept**: the station's record for its network, its internet road, and when the proof was
//!    signed ([`Stations::keep_signed`]); then the staged keys go.
//!
//! ## Signed in from elsewhere
//!
//! A computer that has never been on the station's network (a laptop away from home, a guest who
//! lives elsewhere) finds the station nowhere at step 5, and the operator says it is not here. The
//! station is then kept by the service's id for it ([`Stations::keep_away`]): its road, already
//! registered at step 4, and this computer's two keys, with the station's LAN key exactly as its
//! own proof vouched for it at step 3, pinned from then on. Operate drives it over the internet road
//! alone (`crate::client_mode`). No sign-in time is kept for it: nothing on the station's network
//! rests on this sign-in.
//!
//! **First contact on its network** is a sign-in from its row there ([`arrive`]): the same two keys
//! proven again, the service asked for nothing but the sign-in (its device is the one registered
//! already: no second device, no second enrolment), and `enrol` at the station found by name, or
//! where the operator says, on TLS pinned to that LAN key. Then the station is kept as any other,
//! in its place ([`Stations::fold`]): its record by its network's ids, the same road, and the new
//! proof's time, from which the station counts its thirty days on its network. Not found, it stays
//! as it was.
//!
//! ## A station paired here with a code
//!
//! Signing in from its row links that pairing to the account in place: the same LAN key (the
//! station replaces its entry for it), and a new road key, registered with the LAN key and the
//! proof. Never the key of an internet road added before: the service keeps a key it has approved
//! as it was approved, with no LAN key, and a station drops from its network every computer kept
//! there on a proof whose LAN key the service does not list. So the old road's approval is ended at
//! the service once the new road is kept, as Forget ends it. A station whose owner has not signed in
//! at the shack is listed with no proof: then, as "Add the internet road" did, the internet road
//! alone is registered, for the station the operator picks, and waits for Approve at the station.
//!
//! ## The station's key, never on the service's word alone
//!
//! A station's own proof binds its key for the internet road to the person who signed in at the
//! shack, and this computer checks that person against its own sign-in only for the account's own
//! stations. So the key the service lists is pinned without more only there. For a station paired
//! here with a code whose owner has not signed in at the shack, the station is asked for its key
//! first, on this network's road, pinned by the code ([`hear`]): the same key, and the road is
//! registered and kept; another key, and nothing is registered (`stationKeyDisagrees`); found nowhere
//! here, the road is registered and kept HELD (`Stations::hold_road`): client mode opens nothing for
//! it until the operator compares the station's key with the station's own screen and says it
//! matches, or until the station names that same key on this network's road, at the next Operate
//! there. It doesn't match then drops the internet road alone: the code pairing stays. A road to a
//! station the account was invited to is kept held the same way, on its network or not: the proof
//! there is someone else's, which nothing here binds to anyone, and its network's key came from the
//! same listing, so only the operator's check frees it, and It doesn't match forgets the station.
//!
//! A station added with the account and then paired again here with a code is one of these from
//! then on: the pairing drops its sign-in time (`Stations::drop_signed`), since the station keeps
//! this computer until it is removed there, so signing in from its row links it the same way.
//!
//! ## Signing in again
//!
//! A station admits a computer on its network for `tempo_client::account::PROOF_MAX_AGE_S` from its
//! proof's signing, internet up or down (rulings 486, 490, 491, 528). Signing in for a station this
//! computer was added to with the account proves its same two keys again, and the new proof goes to
//! the station on a connection of its own (`enrol` again, which replaces its entry): an open road
//! is never closed for it, and nothing at the service changes. From `RENEW_BEFORE_S` before the end
//! the window offers it, and Operate's launch does it by itself (`crate::client_mode`).
//!
//! Nothing here writes a log line, and no token is kept: the access token lives for the requests
//! that need it, the proof for the station that checks it.
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use ring::digest::{digest, SHA256};
use serde::Deserialize;
use serde_json::{json, Value};
use tempo_client::account::{self as proofs, KeySet, Listed, Role, Tenant};
use tempo_client::id_token::Claims;
use tempo_client::native::Scope;
use tempo_stream::lan::{Lookup, Typed};
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;

use super::internet::{self, Choose};
use super::origin::{Find, FIND_FOR};
use super::road::{self, Failed, Opened, Socket};
use super::{ComputerKey, Stations, AWAY_NAME_CHARS, MAX_STATIONS};
use crate::account_sign_in::{SIGN_IN_PORT, SIGN_IN_WAIT};
use crate::remote_service::account::Service as Account;
use crate::remote_service::lan::{device_id, tls, VERSIONS};
use crate::remote_service::vault::{AwayStation, InternetRoad, PairedStation};

/// How long one enrolment at one address may take: the station's own handshake deadline, with
/// room to spare.
const ENROLLING: Duration = Duration::from_secs(15);
/// The most stations of an account list read: the service's two owned and sixteen invited.
const MOST_LISTED: usize = 18;

/// Where the station is, asked of the operator when it is found nowhere else: the station's name as
/// the account lists it (empty when it is one already on this computer), and why the last address
/// tried did not take this computer, if one was. The address or host name the operator typed, or
/// none (cancelled, or the window gone).
pub(crate) type Ask = Arc<
    dyn Fn(String, Option<&'static str>) -> Pin<Box<dyn Future<Output = Option<String>> + Send>>
        + Send
        + Sync,
>;

/// How a listed station's proof is checked: `tempo_client::account::listed_station` ([`Env::new`]).
pub(crate) type Trust = fn(&Value, &KeySet, &Tenant, &str, u64) -> Result<Listed, &'static str>;

/// What a station paired here says its key for the internet road is, asked on this network at each
/// of the addresses given in turn, on the road pinned to the key this computer holds for it
/// ([`hear`] in the app): the key, none from a station with no pairing with the service, or why
/// nothing was heard.
pub(crate) type Hear = for<'a> fn(&'a PairedStation, Vec<SocketAddrV4>) -> Heard<'a>;

/// What [`Hear`] gives, in time.
pub(crate) type Heard<'a> =
    Pin<Box<dyn Future<Output = Result<Option<String>, &'static str>> + Send + 'a>>;

/// What signing in reaches: this computer's stations, the account (this build's tenant, the issuer's
/// keys and the service's audience), the service's device routes, the station's network, and the
/// clock proofs are read by.
pub(crate) struct Env {
    pub(crate) stations: Arc<Stations>,
    pub(crate) account: Arc<Account>,
    pub(crate) service: internet::Service,
    pub(crate) find: Find,
    pub(crate) lookup: Lookup,
    /// This computer's name, as the station and the service show it.
    pub(crate) name: String,
    pub(crate) trust: Trust,
    pub(crate) hear: Hear,
    pub(crate) now_s: fn() -> u64,
}

impl Env {
    /// Signing in to the service Nexus is built for: `accountNotReady` while this build has no
    /// sign-in for it.
    pub(crate) fn new(
        stations: Arc<Stations>,
        find: Find,
        lookup: Lookup,
        name: String,
    ) -> Result<Self, &'static str> {
        let origin = crate::remote_service::REMOTE_ORIGIN;
        Ok(Self {
            stations,
            account: Arc::new(Account::new(origin)?),
            service: internet::Service::new(origin)?,
            find,
            lookup,
            name,
            trust: proofs::listed_station,
            hear,
            now_s,
        })
    }
}

/// The station's key for the internet road as it says it on this network: the command link opened
/// to `record` at each of `order` in turn, as Operate opens it (TLS pinned to the station's key on
/// this network, the hello, the same build, the `open`), the key its `opened` names read, and the
/// link closed. Nothing is asked of the station but that.
pub(crate) fn hear<'a>(record: &'a PairedStation, order: Vec<SocketAddrV4>) -> Heard<'a> {
    Box::pin(async move {
        let run = super::link::run_id();
        let (link, _) = super::link::Link::connect_at(record, &order, run, None).await?;
        let said = (link.ids.station == record.station_id).then(|| link.station_key.clone());
        link.close();
        said.ok_or("notThisStation")
    })
}

/// Seconds since 1970, by this computer's clock.
fn now_s() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// What a sign-in did: the station's id on this computer, whether its internet road waits for
/// Approve at the station (one the service did not approve, and every road a sign-in adds while the
/// station grants nothing on a sign-in: [`waits`]),
/// whether it is kept by its service id until this computer first reaches it on its network, and
/// this computer's key for its internet road as the station shows it (`internet::shown_key`), which
/// the station asks the operator to compare before approving this computer.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Signed {
    pub(crate) station: String,
    pub(crate) waiting: bool,
    pub(crate) away: bool,
    pub(crate) key: String,
}

/// Whether a road this sign-in registered waits for Approve at the station: one the service did not
/// approve. Since the 2026-10-23 sign-in decision the station grants control on a sign-in
/// (`crate::remote_service::account::GRANTS_ON_SIGN_IN`), so an approved road no longer waits — it
/// is admitted for control, with a transmit hold the station enforces (see that constant); only one
/// the service has not approved waits. (A build with the switch off makes every road wait, as
/// before.)
fn waits(approved: bool) -> bool {
    !approved || !crate::remote_service::account::GRANTS_ON_SIGN_IN
}

/// Signs in through `open`'s browser, on the sign-in's own port: to add a station new to this
/// computer (`here` none), to link one paired here with a code, again for one added with the
/// account, or on its network for one kept by its service id (`here` its id). See the header.
pub(crate) async fn sign_in(
    env: &Env,
    here: Option<String>,
    open: impl FnOnce(&str) -> Result<(), String>,
    choose: Choose,
    ask: Ask,
) -> Result<Signed, &'static str> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, SIGN_IN_PORT))
        .await
        .map_err(|_| "signInPortBusy")?;
    sign_in_on(env, here, listener, SIGN_IN_WAIT, open, choose, ask).await
}

/// [`sign_in`] on a listener already bound, waiting at most `wait` for the browser.
pub(crate) async fn sign_in_on(
    env: &Env,
    here: Option<String>,
    listener: TcpListener,
    wait: Duration,
    open: impl FnOnce(&str) -> Result<(), String>,
    choose: Choose,
    ask: Ask,
) -> Result<Signed, &'static str> {
    let browser = Browser {
        listener,
        wait,
        open,
    };
    let Some(id) = here else {
        return add(env, browser, choose, ask).await;
    };
    let wanted = id.clone();
    let (record, road, signed, away) = store(env, move |stations| {
        Ok((
            stations.get(&wanted)?,
            stations.internet(&wanted)?,
            stations.signed(&wanted),
            stations.away(&wanted)?,
        ))
    })
    .await?;
    match (record, road, signed, away) {
        (None, Some(road), _, Some(away)) => arrive(env, away, road, browser, ask).await,
        (None, _, _, _) => Err("unknownStation"),
        (Some(record), Some(road), Some(_), _) => again(env, record, road, browser, ask).await,
        (Some(record), road, _, _) => link(env, record, road, browser, choose, ask).await,
    }
}

/// The browser's part of a sign-in: the loopback listener its redirect comes back to, how long it
/// has, and how it is opened.
struct Browser<O> {
    listener: TcpListener,
    wait: Duration,
    open: O,
}

/// The store can block: off the async threads.
async fn store<T: Send + 'static>(
    env: &Env,
    work: impl FnOnce(&Stations) -> Result<T, &'static str> + Send + 'static,
) -> Result<T, &'static str> {
    let stations = env.stations.clone();
    tokio::task::spawn_blocking(move || work(&stations))
        .await
        .unwrap_or(Err("storeUnavailable"))
}

fn restore(pkcs8: &str) -> Result<ComputerKey, &'static str> {
    ComputerKey::restore(pkcs8).ok_or("storeUnavailable")
}

/// What a sign-in gave: the access token for the service's API, kept for this sign-in's requests
/// only; the proof; and who signed in and when, as the proof says. No `Debug`: the token and the
/// proof are no one else's.
struct Proven {
    access: String,
    proof: String,
    own: Claims,
}

/// The sign-in itself, for this computer's `road` key and `lan` key: the issuer's ID token naming
/// exactly those two (see the header, step 2).
async fn signed(
    env: &Env,
    road: &ComputerKey,
    lan: &ComputerKey,
    browser: Browser<impl FnOnce(&str) -> Result<(), String>>,
) -> Result<Proven, &'static str> {
    let spki = |key: &ComputerKey| {
        tempo_stream::protocol::hex_bytes(key.public_key()).ok_or("storeUnavailable")
    };
    let nonce = proofs::device_nonce("computer", &[&spki(road)?, &spki(lan)?]);
    let sign = env.account.native_sign_in().await?;
    let given = crate::account_sign_in::signed_in(
        env.account.http(),
        &sign,
        Scope::Device,
        Some(&nonce),
        browser.listener,
        browser.wait,
        browser.open,
    )
    .await?;
    let proof = given.id_token.ok_or("signInFailed")?;
    // Not for this sign-in's keys, or not this tenant's: nothing goes on with it.
    let own =
        proofs::own_token(&proof, env.account.tenant(), &nonce).map_err(|_| "signInFailed")?;
    Ok(Proven {
        access: given.access,
        proof,
        own,
    })
}

/// The account's stations as the service lists them with the access token (step 3): those trusted
/// on their own proofs, why each other one was not, and the list as the service gave it.
async fn listed(
    env: &Env,
    proven: &Proven,
) -> Result<(Vec<Listed>, Vec<&'static str>, Value), &'static str> {
    let session = env.service.session(&proven.access).await?;
    let now = (env.now_s)();
    let keys = env.account.fetch_keys(now).await?;
    let (mut trusted, mut refused) = (Vec::new(), Vec::new());
    for row in session["stations"]
        .as_array()
        .into_iter()
        .flatten()
        .take(MOST_LISTED)
    {
        match (env.trust)(row, &keys, env.account.tenant(), &proven.own.subject, now) {
            Ok(listed) => trusted.push(listed),
            Err(why) => refused.push(why),
        }
    }
    Ok((trusted, refused, session))
}

/// The one of `stations` this computer is added to: the only one, or the one the operator picks
/// through `choose`. `stationNotChosen` when none is picked.
async fn pick(stations: Vec<Listed>, choose: Choose) -> Result<Listed, &'static str> {
    if stations.len() == 1 {
        return stations.into_iter().next().ok_or("stationNotChosen");
    }
    let offered = stations
        .iter()
        .map(|station| internet::Choice {
            id: station.id.clone(),
            name: station.name.chars().take(48).collect(),
        })
        .collect();
    let picked = choose(offered).await.ok_or("stationNotChosen")?;
    stations
        .into_iter()
        .find(|station| station.id == picked)
        .ok_or("stationNotChosen")
}

/// Adding a station new to this computer: the whole of the header's six steps.
async fn add(
    env: &Env,
    browser: Browser<impl FnOnce(&str) -> Result<(), String>>,
    choose: Choose,
    ask: Ask,
) -> Result<Signed, &'static str> {
    // Room here first: a station this computer could not keep is never added at the station.
    let (records, away, staged) = store(env, |stations| {
        Ok((
            stations.records()?,
            stations.away_records()?,
            stations.staged()?,
        ))
    })
    .await?;
    if records.len() + away.len() >= MAX_STATIONS {
        return Err("stationsFull");
    }
    let (road, lan) = (restore(&staged.road)?, restore(&staged.lan)?);
    let proven = signed(env, &road, &lan, browser).await?;
    let (trusted, refused, _) = listed(env, &proven).await?;
    // A station on this computer already, on its network or kept by its service id, is signed in
    // to from its own row.
    let (here, fresh): (Vec<Listed>, Vec<Listed>) = trusted.into_iter().partition(|station| {
        records.iter().any(|r| r.station_key == station.lan_key)
            || away.iter().any(|a| a.station_key == station.lan_key)
    });
    let (operate, listen): (Vec<Listed>, Vec<Listed>) = fresh
        .into_iter()
        .partition(|station| station.role == Role::Operate);
    if operate.is_empty() {
        return Err(if !here.is_empty() {
            "alreadyHere"
        } else if !listen.is_empty() {
            "listenOnly"
        } else if refused.contains(&"stationNotProven") {
            "stationNotProven"
        } else if !refused.is_empty() {
            "stationProofFailed"
        } else {
            "stationUnavailable"
        });
    }
    let station = pick(operate, choose).await?;
    let (device, approved) = env
        .service
        .register(
            &proven.access,
            &station.id,
            &road,
            Some((&lan, &proven.proof)),
            &env.name,
        )
        .await?;
    let reached = reach(
        env,
        &station.name,
        &station.lan_key,
        Vec::new(),
        (&staged.lan, road.public_key(), &proven.proof),
        &ask,
    )
    .await;
    let (lan_id, lan_device, at) = match reached {
        // Found nowhere on this network, and the operator did not say where (see the header).
        Err("notOnThisNetwork") => {
            let id = station.id.clone();
            let kept = AwayStation {
                station_id: station.id.clone(),
                name: station.name.chars().take(AWAY_NAME_CHARS).collect(),
                station_key: station.lan_key,
                pkcs8: staged.lan,
            };
            let internet = InternetRoad {
                station_id: station.id,
                device_id: device,
                station_key: station.station_key,
                pkcs8: staged.road,
            };
            let key = internet::shown_key(&internet).ok_or("notStation")?;
            let guest = !station.own;
            store(env, move |stations| {
                if guest {
                    stations.hold_road(&kept.station_id, &internet.station_key)?;
                }
                stations.keep_away(&kept, &internet)?;
                stations.drop_staged()
            })
            .await?;
            return Ok(Signed {
                station: id,
                waiting: waits(approved),
                away: true,
                key,
            });
        }
        reached => reached?,
    };
    let record = PairedStation {
        station_id: lan_id.clone(),
        device_id: lan_device,
        station_key: station.lan_key.clone(),
        pkcs8: staged.lan,
        addresses: vec![at.to_string()],
    };
    let internet = InternetRoad {
        station_id: station.id,
        device_id: device,
        station_key: station.station_key,
        pkcs8: staged.road,
    };
    let key = internet::shown_key(&internet).ok_or("notStation")?;
    let issued_at = proven.own.issued_at;
    let id = lan_id.clone();
    let guest = !station.own;
    store(env, move |stations| {
        stations.keep(&record)?;
        if guest {
            stations.hold_road(&id, &internet.station_key)?;
        }
        stations.keep_internet(&id, &internet)?;
        stations.keep_signed(&id, issued_at)?;
        stations.drop_staged()
    })
    .await?;
    Ok(Signed {
        station: lan_id,
        waiting: waits(approved),
        away: false,
        key,
    })
}

/// Linking a station paired here with a code to the account (see the header).
async fn link(
    env: &Env,
    record: PairedStation,
    road: Option<InternetRoad>,
    browser: Browser<impl FnOnce(&str) -> Result<(), String>>,
    choose: Choose,
    ask: Ask,
) -> Result<Signed, &'static str> {
    let lan = restore(&record.pkcs8)?;
    // A road key of its own for the account, never the old road's (see the header).
    let road_pkcs8 = store(env, |stations| stations.staged()).await?.road;
    let road_key = restore(&road_pkcs8)?;
    let proven = signed(env, &road_key, &lan, browser).await?;
    let (trusted, _, session) = listed(env, &proven).await?;
    let id = record.station_id.clone();
    let station = trusted
        .into_iter()
        .find(|station| station.lan_key == record.station_key);
    // Whether the road is kept held until the operator checks the station's key (see the header).
    let (internet, approved, issued_at, held) = match station {
        Some(station) if station.role != Role::Operate => return Err("listenOnly"),
        Some(station) => {
            let (device, approved) = env
                .service
                .register(
                    &proven.access,
                    &station.id,
                    &road_key,
                    Some((&lan, &proven.proof)),
                    &env.name,
                )
                .await?;
            let order = order(env, &record).await;
            let (lan_id, lan_device, at) = reach(
                env,
                "",
                &record.station_key,
                order,
                (&record.pkcs8, road_key.public_key(), &proven.proof),
                &ask,
            )
            .await?;
            // The same station, by its ids as well as its key.
            if lan_id != record.station_id || lan_device != record.device_id {
                return Err("notThisStation");
            }
            let kept = id.clone();
            store(env, move |stations| stations.worked(&kept, at)).await?;
            let internet = InternetRoad {
                station_id: station.id,
                device_id: device,
                station_key: station.station_key,
                pkcs8: road_pkcs8,
            };
            (internet, approved, Some(proven.own.issued_at), !station.own)
        }
        // Its owner has not signed in at the shack: the internet road alone, as before.
        None if road.is_some() => return Err("stationNotProven"),
        None => {
            let station = internet::chosen(&session, choose).await?;
            let station_id = station["id"].as_str().ok_or("invalidResponse")?;
            // The station key the service lists: without one the station has not yet said it to
            // the service, and the road could prove nothing.
            let station_key = station["stationKey"]
                .as_str()
                .filter(|key| tempo_stream::protocol::device_key(key))
                .ok_or("stationKeyMissing")?;
            // Pinned only once the station itself says it, on this network's road (pinned by the
            // code), before anything is registered; found nowhere here, held until the operator
            // checks it against the station's screen.
            let order = order(env, &record).await;
            let held = match (env.hear)(&record, order).await {
                Ok(Some(said)) if said == station_key => false,
                Ok(Some(_)) => {
                    let _ = store(env, |stations| stations.drop_staged()).await;
                    return Err("stationKeyDisagrees");
                }
                Ok(None) => return Err("internetNotReady"),
                Err(_) => true,
            };
            let (device, approved) = env
                .service
                .register(&proven.access, station_id, &road_key, None, &env.name)
                .await?;
            let internet = InternetRoad {
                station_id: station_id.to_string(),
                device_id: device,
                station_key: station_key.to_string(),
                pkcs8: road_pkcs8,
            };
            (internet, approved, None, held)
        }
    };
    let key = internet::shown_key(&internet).ok_or("notStation")?;
    let kept = id.clone();
    store(env, move |stations| {
        if held {
            stations.hold_road(&kept, &internet.station_key)?;
        }
        stations.keep_internet(&kept, &internet)?;
        if let Some(issued_at) = issued_at {
            stations.keep_signed(&kept, issued_at)?;
        }
        stations.drop_staged()
    })
    .await?;
    // The old road's approval ended at the service, as Forget ends it, signed by its own key. One
    // the service cannot be told of now stays approved there until it is removed at the station.
    if let Some(old) = road {
        let _ = env.service.forget(&old).await;
    }
    Ok(Signed {
        station: id,
        waiting: waits(approved),
        away: false,
        key,
    })
}

/// Signing in again for a station added with the account (see the header): the same two keys, a
/// new proof, and `enrol` again on a connection of its own. Nothing at the service changes.
async fn again(
    env: &Env,
    record: PairedStation,
    road: InternetRoad,
    browser: Browser<impl FnOnce(&str) -> Result<(), String>>,
    ask: Ask,
) -> Result<Signed, &'static str> {
    let (lan, road_key) = (restore(&record.pkcs8)?, restore(&road.pkcs8)?);
    let key = internet::shown_key(&road).ok_or("notStation")?;
    let proven = signed(env, &road_key, &lan, browser).await?;
    let order = order(env, &record).await;
    let (lan_id, lan_device, at) = reach(
        env,
        "",
        &record.station_key,
        order,
        (&record.pkcs8, road_key.public_key(), &proven.proof),
        &ask,
    )
    .await?;
    if lan_id != record.station_id || lan_device != record.device_id {
        return Err("notThisStation");
    }
    let (id, issued_at) = (record.station_id.clone(), proven.own.issued_at);
    store(env, move |stations| {
        stations.worked(&id, at)?;
        stations.keep_signed(&id, issued_at)
    })
    .await?;
    Ok(Signed {
        station: record.station_id,
        waiting: false,
        away: false,
        key,
    })
}

/// First contact on its network for a station kept by its service id (see the header): its same two
/// keys proven by a new sign-in, nothing asked of the service but that, and `enrol` at the station
/// on TLS pinned to the LAN key its proof vouched for when it was kept; then it is kept in its
/// place as a station on its network, with the new proof's time. Found nowhere, and not said to be
/// anywhere, it stays as it was (`notOnThisNetwork`).
async fn arrive(
    env: &Env,
    away: AwayStation,
    road: InternetRoad,
    browser: Browser<impl FnOnce(&str) -> Result<(), String>>,
    ask: Ask,
) -> Result<Signed, &'static str> {
    let (lan, road_key) = (restore(&away.pkcs8)?, restore(&road.pkcs8)?);
    let key = internet::shown_key(&road).ok_or("notStation")?;
    let proven = signed(env, &road_key, &lan, browser).await?;
    let (lan_id, lan_device, at) = reach(
        env,
        &away.name,
        &away.station_key,
        Vec::new(),
        (&away.pkcs8, road_key.public_key(), &proven.proof),
        &ask,
    )
    .await?;
    let record = PairedStation {
        station_id: lan_id.clone(),
        device_id: lan_device,
        station_key: away.station_key,
        pkcs8: away.pkcs8,
        addresses: vec![at.to_string()],
    };
    let (id, issued_at) = (away.station_id, proven.own.issued_at);
    store(env, move |stations| stations.fold(&id, &record, issued_at)).await?;
    Ok(Signed {
        station: lan_id,
        waiting: false,
        away: false,
        key,
    })
}

/// Where a station on this computer is tried, in turn: the address or host name the operator typed
/// for it, then each remembered one, the last that worked first.
async fn order(env: &Env, record: &PairedStation) -> Vec<SocketAddrV4> {
    let (id, lookup) = (record.station_id.clone(), env.lookup.clone());
    let typed = store(env, move |stations| {
        Ok(stations.typed(&id).and_then(|typed| typed.resolve(&lookup)))
    })
    .await
    .ok()
    .flatten();
    road::order(record, typed)
}

/// First contact with the station on its network, its LAN key `lan_key` pinned (step 5): at each of
/// `tried`, then where a look by name finds it, then where the operator says it is (`name` is how
/// the account lists it), for as long as they keep saying. `with` is what this computer enrols
/// with: its LAN key's PKCS#8, its road key and its proof. The station's id for its network, the
/// device id it gave this computer, and where it answered; or the station's refusal, at once.
async fn reach(
    env: &Env,
    name: &str,
    lan_key: &str,
    tried: Vec<SocketAddrV4>,
    with: (&str, &str, &str),
    ask: &Ask,
) -> Result<(String, String, SocketAddrV4), &'static str> {
    let mut why = None;
    if let Some(taken) = at_each(env, &tried, lan_key, with, &mut why).await? {
        return Ok(taken);
    }
    // A look by name: an advert whose key tag starts the station's LAN key's fingerprint.
    let find = env.find.clone();
    let looked = tokio::task::spawn_blocking(move || find(FIND_FOR)).await;
    let probe = PairedStation {
        station_id: String::new(),
        device_id: String::new(),
        station_key: lan_key.to_string(),
        pkcs8: String::new(),
        addresses: Vec::new(),
    };
    let found = road::found_for(
        &probe,
        &tried,
        looked.ok().and_then(Result::ok).unwrap_or_default(),
    );
    if let Some(taken) = at_each(env, &found, lan_key, with, &mut why).await? {
        return Ok(taken);
    }
    // Where the operator says it is, for as long as they say.
    loop {
        let typed = ask(name.to_string(), why).await.ok_or("notOnThisNetwork")?;
        let Some(typed) = Typed::read(&typed) else {
            why = Some("badAddress");
            continue;
        };
        let lookup = env.lookup.clone();
        let at = tokio::task::spawn_blocking(move || typed.resolve(&lookup))
            .await
            .ok()
            .flatten();
        let Some(at) = at else {
            why = Some("nameUnresolved");
            continue;
        };
        why = None;
        if let Some(taken) = at_each(env, &[at], lan_key, with, &mut why).await? {
            return Ok(taken);
        }
    }
}

/// Enrolment at each of `order` in turn: the first that takes this computer, or none, with `why`
/// the most telling reason nothing answered or another key did; a station's refusal is the answer
/// at once.
async fn at_each(
    env: &Env,
    order: &[SocketAddrV4],
    lan_key: &str,
    (lan_pkcs8, road_key, proof): (&str, &str, &str),
    why: &mut Option<&'static str>,
) -> Result<Option<(String, String, SocketAddrV4)>, &'static str> {
    for &at in order {
        match enrol(at, lan_pkcs8, lan_key, &env.name, road_key, proof).await {
            Ok((station, device)) => return Ok(Some((station, device, at))),
            Err(reason) if road::look_further(reason) => {
                if why.is_none_or(|said| road::weight(reason) > road::weight(said)) {
                    *why = Some(reason);
                }
            }
            Err(reason) => return Err(reason),
        }
    }
    Ok(None)
}

/// What the station says on an enrol-only connection. Parsed whole and exactly.
#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
enum FromStation {
    Enrolled {
        #[serde(rename = "deviceId")]
        device_id: String,
        #[serde(rename = "stationId")]
        station_id: String,
    },
    Refused {
        reason: String,
    },
}

/// A station's refusal of an enrolment, as the page names it. Anything the station never says means
/// it is not one.
fn refusal(reason: &str) -> &'static str {
    match reason {
        "updateStation" => "updateStation",
        "updateComputer" => "updateComputer",
        // Nothing to check a sign-in against: the station's owner has not signed in there.
        "enrolClosed" => "stationNotSignedIn",
        "proofNotYours" => "proofNotYours",
        "listenNative" => "listenOnly",
        "pairingFull" | "deviceCacheFull" => "pairingFull",
        "unavailable" | "credentialStoreUnavailable" => "stationCouldNotKeep",
        _ if reason.starts_with("token") => "proofRefused",
        "ownerIssuer" | "deviceKind" => "proofRefused",
        _ => "notStation",
    }
}

/// The station's next word on an enrol-only connection. Gone without one is `unreachable`.
async fn heard(socket: &mut Socket) -> Result<FromStation, &'static str> {
    loop {
        match socket.next().await {
            Some(Ok(Message::Text(text))) => {
                return serde_json::from_str(&text).map_err(|_| "notStation");
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => continue,
            _ => return Err("unreachable"),
        }
    }
}

/// One enrolment at `at` (see the header, step 5): TLS with this computer's LAN key (`lan_pkcs8`)
/// and the station's (`station_key`) pinned, so nothing else hears what follows; then `enrol` as the
/// first message, with this computer's `name`, its road key and its `proof`. The station's id for
/// its network and the device id it gave this computer, which must be the one it works out from
/// this computer's LAN key; or why not, by name.
pub(crate) async fn enrol(
    at: SocketAddrV4,
    lan_pkcs8: &str,
    station_key: &str,
    name: &str,
    road_key: &str,
    proof: &str,
) -> Result<(String, String), &'static str> {
    let enrolling = enrolled(at, lan_pkcs8, station_key, name, road_key, proof);
    tokio::time::timeout(ENROLLING, enrolling)
        .await
        .unwrap_or(Err("unreachable"))
}

async fn enrolled(
    at: SocketAddrV4,
    lan_pkcs8: &str,
    station_key: &str,
    name: &str,
    road_key: &str,
    proof: &str,
) -> Result<(String, String), &'static str> {
    let key = restore(lan_pkcs8)?;
    let config = tls::client::config(lan_pkcs8, station_key).ok_or("storeUnavailable")?;
    let Opened { mut socket, .. } =
        road::open(at, config)
            .await
            .map_err(|failed| match failed {
                Failed::Unreachable => "unreachable",
                // Nothing answered at the address: why, in the words the window uses for it.
                Failed::NotConnected(how) => {
                    tempo_stream::lan::unreached(&how.into(), tempo_stream::lan::here(*at.ip()))
                }
                // A station takes a key it does not know only while it can check a sign-in.
                Failed::Refused => "stationNotSignedIn",
                // Another key at that address: not the station the account vouched for.
                Failed::KeyChanged => "keyChanged",
            })?;
    let (protocol, stream, operation) = VERSIONS;
    let asked = json!({"type":"enrol","protocol":protocol,"stream":stream,"operation":operation,
        "name":name,"kind":"computer","publicKey":road_key,"proof":proof});
    road::say(&mut socket, asked.to_string())
        .await
        .map_err(|_| "unreachable")?;
    let answer = heard(&mut socket).await;
    let _ = tokio::time::timeout(Duration::from_secs(1), socket.close(None)).await;
    match answer? {
        FromStation::Enrolled {
            device_id: device,
            station_id,
        } => {
            // The station works the device id out of this computer's key, as it does for every
            // computer it admits: an id it did not work out so is not a station's answer.
            let spki =
                tempo_stream::protocol::hex_bytes(key.public_key()).ok_or("storeUnavailable")?;
            let pin: [u8; 32] = digest(&SHA256, &spki)
                .as_ref()
                .try_into()
                .map_err(|_| "storeUnavailable")?;
            if device != device_id(&pin) || !tempo_stream::protocol::identifier(&station_id) {
                return Err("notStation");
            }
            Ok((station_id, device))
        }
        FromStation::Refused { reason } => Err(refusal(&reason)),
    }
}

#[cfg(test)]
pub(crate) mod tests;
