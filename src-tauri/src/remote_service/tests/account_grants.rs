//! The station's account, wired into the shack: signing in there binds the station with no code, and
//! a device the service approved on its own proof is granted only once the station has checked that
//! proof itself, then only as its role says. Since the 2026-10-23 sign-in decision the station grants control on a
//! sign-in (`account::GRANTS_ON_SIGN_IN`, on in every build) with a transmit hold; the off-switch
//! branch — a device waiting for Approve at the shack — is kept tested with it explicitly off
//! ([`launch_not_granting_on_sign_in`]).
//!
//! The proofs are tempo-client's account fixture (tokens minted once by throwaway keys that never
//! left a scratch process), read at the fixture's own time. The service is the fake cloud on
//! loopback; the issuer is never asked, because the station already holds its keys, fetched at
//! that same time.
use super::*;
use crate::remote_service::lan::enrol::{mailbox_tag, short};
use crate::remote_service::vault::{AccountKeys, AccountVault, DevicesHeld, People, Tie, Ties};
use tempo_client::account::{mailbox, CachedDevice, Invite, KeySet, Owner, Role, Tenant};

const CASES: &str =
    include_str!("../../../../crates/tempo-client/tests/fixtures/account-proof-cases.json");
const COMPUTER_ID: &str = "10000000-0000-4000-8000-00000000000d";

fn cases() -> serde_json::Value {
    serde_json::from_str(CASES).unwrap()
}
/// The fixture's own time: its proofs were signed 100 s before it.
fn fixture_now() -> u64 {
    1_800_000_000
}
fn key(name: &str) -> String {
    cases()["keys"][name].as_str().unwrap().to_string()
}
fn token(name: &str) -> String {
    cases()["tokens"][name].as_str().unwrap().to_string()
}
fn sha256(hex: &str) -> String {
    let spki = tempo_stream::protocol::hex_bytes(hex).unwrap();
    tempo_client::hex(ring::digest::digest(&ring::digest::SHA256, &spki).as_ref())
}

/// The fixture's tenant, its issuer never asked (the station holds its keys).
fn test_service() -> Arc<account::Service> {
    let tenant: &'static Tenant = Box::leak(Box::new(Tenant {
        origin: "http://127.0.0.1:9",
        issuer: "https://issuer.test.invalid/",
        native_client_id: "test-native-client",
        spa_client_id: "test-spa-client",
    }));
    Arc::new(account::Service::with("http://127.0.0.1:9", tenant, "http://127.0.0.1:9/").unwrap())
}

/// What the stand-in binder was given, call by call: name, station key, LAN key, credential, and
/// the station it was to prove again.
type Asked = Arc<Mutex<Vec<(String, String, String, String, Option<String>)>>>;

/// The account on `vault`, binding through `bind`, naming the fixture's LAN key, at the fixture's
/// time.
fn accounts(vault: &MemoryVault, bind: account::Binder) -> Accounts {
    Accounts {
        vault: Arc::new(vault.clone()),
        service: Ok(test_service()),
        bind,
        lan_key: Some(Arc::new(|| Some(key("stationLan")))),
        now_s: fixture_now,
        grants_on_sign_in: account::GRANTS_ON_SIGN_IN,
    }
}
fn launch_with(cloud: &FakeCloud, vault: &MemoryVault, accounts: Accounts) -> Service {
    Service::with_accounts(
        cloud.origin.clone(),
        Box::new(vault.clone()),
        engine(),
        crate::remote_monitor::Publisher::default(),
        accounts,
    )
}
/// A binder that answers each sign-in in turn from `answers`, recording what it was given; one
/// past the last never answers, as a sign-in left open in the browser.
fn binder(answers: Vec<Result<account::Bound, &'static str>>) -> (account::Binder, Asked) {
    let asked: Asked = Arc::default();
    let held = asked.clone();
    let answers = Arc::new(Mutex::new(answers.into_iter()));
    let bind: account::Binder = Arc::new(move |binds: account::Binds| {
        held.lock().unwrap().push((
            binds.name,
            binds.station_key,
            binds.lan_key,
            binds.credential,
            binds.again.map(|bound| bound.station_id),
        ));
        let answer = answers.lock().unwrap().next();
        Box::pin(async move {
            match answer {
                Some(answer) => answer,
                None => std::future::pending().await,
            }
        })
    });
    (bind, asked)
}
fn bound() -> account::Bound {
    account::Bound {
        station_id: STATION.into(),
        account_id: ACCOUNT.into(),
    }
}

// ---- Signing in at the shack ---------------------------------------------------------------------

/// ★ Sign in to Nexus Remote: the station's keys and credential are made and staged before the
/// browser opens, a sign-in tried again names the very same ones (so the service finds the station
/// it already made for them), and once bound the credential and the signing key are kept under the
/// binding the service gave, and Remote turns on as approving a pairing turns it on.
#[tokio::test(flavor = "multi_thread")]
async fn signing_in_binds_the_station_with_the_keys_it_staged_and_turns_remote_on() {
    let cloud = fake_cloud().await;
    let vault = MemoryVault::default();
    let (bind, asked) = binder(vec![Err("signInFailed"), Ok(bound())]);
    let service = launch_with(&cloud, &vault, accounts(&vault, bind));

    service
        .action(Action::SignIn {
            name: " Shack ".into(),
        })
        .await
        .unwrap();
    let failed = eventually(&service, "the first sign-in ended", |s| {
        !s.account.signing_in && s.error.is_some()
    })
    .await;
    assert_eq!(failed.error, Some("signInFailed"));
    assert_eq!(
        failed.station_id, None,
        "nothing is bound on a failed sign-in"
    );
    assert!(vault.binding().unwrap().is_none());

    service
        .action(Action::SignIn {
            name: "Shack".into(),
        })
        .await
        .unwrap();
    let status = eventually(&service, "bound and on", |s| {
        s.station_id.as_deref() == Some(STATION) && on(s)
    })
    .await;
    assert!(!status.account.signing_in);

    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2);
    let (name, station_key, lan_key, credential, again) = asked[0].clone();
    assert_eq!(again, None, "a station bound by this sign-in");
    assert_eq!(name, "Shack", "the name, trimmed");
    assert_eq!(lan_key, key("stationLan"), "the LAN's own key");
    assert!(
        tempo_stream::protocol::device_key(&station_key),
        "a P-256 SPKI"
    );
    assert!(credential.len() == 64 && credential.bytes().all(|b| b.is_ascii_hexdigit()));
    assert_eq!(
        asked[1], asked[0],
        "the second sign-in names the same keys and credential"
    );
    // Nothing the screen is shown holds a secret: no credential, no signing key.
    let kept_key = vault.station_key().unwrap().unwrap().pkcs8;
    let shown = serde_json::to_string(&service.status().unwrap()).unwrap();
    assert!(!shown.contains(&credential) && !shown.contains(&kept_key));

    let binding = Binding {
        origin: cloud.origin.clone(),
        station_id: STATION.into(),
        account_id: ACCOUNT.into(),
    };
    assert!(vault.binding().unwrap() == Some(binding.clone()));
    assert_eq!(vault.credential(&binding).unwrap(), credential);
    let kept = vault.station_key().unwrap().unwrap();
    assert!(
        kept.binding == binding,
        "the signing key is kept under the binding"
    );
    assert_eq!(
        station_key::Signer::restore(&kept.pkcs8)
            .unwrap()
            .public_key(),
        station_key,
        "the key kept is the one the sign-in named"
    );
    assert_eq!(
        status.station_key,
        fingerprint(&station_key),
        "and the station signs with it"
    );
}

/// Signing in is refused while the station is bound, and while this build has no sign-in for its
/// service: then no key is made and nothing is staged.
#[tokio::test(flavor = "multi_thread")]
async fn signing_in_is_refused_when_bound_or_not_ready() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let (bind, asked) = binder(vec![Ok(bound())]);
    let service = launch_with(&cloud, &vault, accounts(&vault, bind));
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let sign_in = || Action::SignIn {
        name: "Shack".into(),
    };
    assert!(matches!(
        service.action(sign_in()).await,
        Err("invalidRequest")
    ));
    assert!(asked.lock().unwrap().is_empty());

    let vault = MemoryVault::default();
    let (bind, asked) = binder(vec![Ok(bound())]);
    let service = launch_with(
        &cloud,
        &vault,
        Accounts {
            service: Err("accountNotReady"),
            ..accounts(&vault, bind)
        },
    );
    assert!(matches!(
        service.action(sign_in()).await,
        Err("accountNotReady")
    ));
    assert!(asked.lock().unwrap().is_empty());
    assert!(
        vault.values.lock().unwrap().is_empty(),
        "nothing staged: {:?}",
        vault.values.lock().unwrap().keys().collect::<Vec<_>>()
    );
}

/// Cancel while the browser is still open ends the sign-in: the station stays unbound, and a
/// pairing code can be asked for again.
#[tokio::test(flavor = "multi_thread")]
async fn cancel_ends_a_sign_in_left_open() {
    let cloud = fake_cloud().await;
    let vault = MemoryVault::default();
    let (bind, _) = binder(Vec::new());
    let service = launch_with(&cloud, &vault, accounts(&vault, bind));
    let signing = service
        .action(Action::SignIn {
            name: "Shack".into(),
        })
        .await
        .unwrap();
    assert!(signing.account.signing_in);
    let begin = || Action::Begin {
        name: "Shack".into(),
    };
    assert!(matches!(
        service.action(begin()).await,
        Err("invalidRequest")
    ));
    // Ending a sign-in again is not this: a first sign-in goes on.
    let still = service.action(Action::CancelSignInAgain {}).await.unwrap();
    assert!(still.account.signing_in);
    let cancelled = service.action(Action::Cancel {}).await.unwrap();
    assert!(!cancelled.account.signing_in);
    assert_eq!(cancelled.phase, "unpaired");
    service.action(begin()).await.unwrap();
}

/// ★ Sign in again, for a station bound already (its proof's key retired): the sign-in names the
/// station's own signing key, the LAN's key as it is now and the station's credential, and proves
/// that station again. Once it holds, the prompt goes, and nothing else about the station changes:
/// the same binding, the same key, and Remote as it was. Refused while another sign-in or a pairing
/// is open, and for a station not bound.
#[tokio::test(flavor = "multi_thread")]
async fn signing_in_again_proves_the_bound_station_and_changes_nothing_else() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let (bind, asked) = binder(vec![Err("proofNotYours"), Ok(bound())]);
    let service = launch_with(&cloud, &vault, accounts(&vault, bind));
    let before = eventually(&service, "the pairing loaded", |s| {
        s.station_id.is_some() && s.station_key.is_some()
    })
    .await;
    service.status.lock().unwrap().account.reproof = true;
    let kept = vault.station_key().unwrap().unwrap();

    service.action(Action::SignInAgain {}).await.unwrap();
    let failed = eventually(&service, "the first sign-in ended", |s| {
        !s.account.signing_in && s.error.is_some()
    })
    .await;
    assert_eq!(failed.error, Some("proofNotYours"));
    assert!(failed.account.reproof, "still to be signed in again");

    service.action(Action::SignInAgain {}).await.unwrap();
    let proven = eventually(&service, "the second sign-in ended", |s| {
        !s.account.signing_in
    })
    .await;
    assert!(!proven.account.reproof);
    assert_eq!(proven.error, None);
    assert_eq!(proven.phase, before.phase, "Remote as it was");
    assert_eq!(proven.station_id, before.station_id);
    assert_eq!(proven.station_key, before.station_key);

    let binding = Binding {
        origin: cloud.origin.clone(),
        station_id: STATION.into(),
        account_id: ACCOUNT.into(),
    };
    let asked = asked.lock().unwrap().clone();
    assert_eq!(asked.len(), 2);
    assert_eq!(
        asked[1],
        (
            String::new(),
            station_key::Signer::restore(&kept.pkcs8)
                .unwrap()
                .public_key()
                .to_string(),
            key("stationLan"),
            vault.credential(&binding).unwrap(),
            Some(STATION.to_string()),
        )
    );
    assert!(vault.binding().unwrap() == Some(binding));
    assert_eq!(vault.station_key().unwrap().unwrap().pkcs8, kept.pkcs8);

    // Not bound: nothing to prove.
    let vault = MemoryVault::default();
    let (bind, asked) = binder(vec![Ok(bound())]);
    let service = launch_with(&cloud, &vault, accounts(&vault, bind));
    assert!(matches!(
        service.action(Action::SignInAgain {}).await,
        Err("stationNotPaired")
    ));
    assert!(asked.lock().unwrap().is_empty());
}

/// While signing in again, no second sign-in starts; ending it leaves Remote on, under the same
/// generation (Cancel, a stop, would turn it off).
#[tokio::test(flavor = "multi_thread")]
async fn cancel_ends_signing_in_again_and_leaves_remote_as_it_was() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let (bind, _) = binder(Vec::new());
    let service = launch_with(&cloud, &vault, accounts(&vault, bind));
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    let on = eventually(&service, "on", on).await;
    assert!(
        service
            .action(Action::SignInAgain {})
            .await
            .unwrap()
            .account
            .signing_in
    );
    assert!(matches!(
        service.action(Action::SignInAgain {}).await,
        Err("invalidRequest")
    ));
    let cancelled = service.action(Action::CancelSignInAgain {}).await.unwrap();
    assert!(!cancelled.account.signing_in);
    assert!(
        cancelled.observation_generation.is_some()
            && cancelled.observation_generation == on.observation_generation,
        "Remote still on, as it was: {:?} {:?}",
        cancelled.observation_generation,
        on.observation_generation
    );
}

// ---- A device approved on its own proof ----------------------------------------------------------

/// The station bound to the fixture's owner, holding the issuer's keys as fetched at the fixture's
/// time, with `invites` invited.
fn owned_vault(origin: &str, invites: Vec<Invite>) -> MemoryVault {
    let vault = paired_vault(origin);
    vault
        .save_owner(&Owner {
            station_id: STATION.into(),
            issuer: "https://issuer.test.invalid/".into(),
            subject: cases()["owner"].as_str().unwrap().into(),
            proof_kid: "test-p".into(),
            proof_issued_at: cases()["issuedAt"].as_u64().unwrap(),
        })
        .unwrap();
    vault
        .save_account_keys(&AccountKeys {
            issuer: "https://issuer.test.invalid/".into(),
            keys: KeySet::fetched(&cases()["jwks"], fixture_now()).unwrap(),
        })
        .unwrap();
    vault
        .save_account_entry(
            "account-people",
            &People {
                station_id: STATION.into(),
                invites,
            },
        )
        .unwrap();
    vault
}
fn browser_row(proof: &str) -> serde_json::Value {
    json!({"id": BROWSER, "name": "Owner's browser", "approved": 1, "expiresAt": APPROVED_UNTIL,
        "generation": 2, "renewsUntil": null, "publicKey": key("browser"), "kind": "browser",
        "platform": null, "untilRemoved": 1, "role": "operate", "addedBy": "account",
        "proof": proof, "lanKey": null})
}
fn computer_row(proof: &str, role: &str) -> serde_json::Value {
    json!({"id": COMPUTER_ID, "name": "DESKTOP-7Q", "approved": 1, "expiresAt": APPROVED_UNTIL,
        "generation": 3, "renewsUntil": null, "publicKey": key("road"), "kind": "computer",
        "platform": "windows", "untilRemoved": 1, "role": role, "addedBy": "account",
        "proof": proof, "lanKey": key("computerLan")})
}
/// Remote on, the service listing `rows`, and the station's list read.
async fn listed(service: &Service, cloud: &FakeCloud, rows: Vec<serde_json::Value>) -> Status {
    *cloud.devices.lock().unwrap() = json!({ "devices": rows }).to_string();
    eventually(service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service.action(Action::Enable {}).await.unwrap();
    service.action(Action::Refresh {}).await.unwrap()
}
fn launch_owned(cloud: &FakeCloud, vault: &MemoryVault) -> Service {
    launch_owned_at(&cloud.origin, vault)
}
fn launch_owned_at(origin: &str, vault: &MemoryVault) -> Service {
    let (bind, _) = binder(Vec::new());
    Service::with_accounts(
        origin.to_string(),
        Box::new(vault.clone()),
        engine(),
        crate::remote_monitor::Publisher::default(),
        accounts(vault, bind),
    )
}
/// [`launch_owned`] in a build whose station grants a device what its role says on its own
/// sign-in (`account::GRANTS_ON_SIGN_IN` on): what such a grant is, and what still holds one back.
/// Since the 2026-10-23 sign-in decision every build has it on, so this is the default launcher's behaviour too; it is
/// kept as its own helper for the tests that name the grant.
fn launch_granting_on_sign_in(cloud: &FakeCloud, vault: &MemoryVault) -> Service {
    let (bind, _) = binder(Vec::new());
    launch_with(
        cloud,
        vault,
        Accounts {
            grants_on_sign_in: true,
            ..accounts(vault, bind)
        },
    )
}

/// [`launch_owned`] with the sign-in grant switched OFF (`grants_on_sign_in: false`): a device the
/// service approved on its own sign-in is granted nothing and waits for Approve at the shack. The
/// 2026-10-23 sign-in decision made every shipping build grant on a sign-in, so this is no longer a
/// build that ships; it keeps the off-switch branch tested, as [`launch_granting_on_sign_in`] kept
/// the on branch tested
/// while every build was off.
fn launch_not_granting_on_sign_in(cloud: &FakeCloud, vault: &MemoryVault) -> Service {
    let (bind, _) = binder(Vec::new());
    launch_with(
        cloud,
        vault,
        Accounts {
            grants_on_sign_in: false,
            ..accounts(vault, bind)
        },
    )
}

/// The pin the room's tunnel takes a computer by: its listed road key's.
fn road_pin() -> [u8; 32] {
    tempo_stream::protocol::hex_bytes(&sha256(&key("road")))
        .unwrap()
        .try_into()
        .unwrap()
}

/// ⛔ WITH THE SIGN-IN GRANT SWITCHED OFF (`account::GRANTS_ON_SIGN_IN`). A browser signed in as the
/// station's owner, its proof checked here and holding: granted nothing and pinned for nothing,
/// nothing kept for the LAN on it, and no notice that it was added; it is shown waiting for Approve
/// at the station, through the next list too. Approve here then grants it what Approve grants any
/// browser, and pins it. (Every shipping build has the switch on since the 2026-10-23 sign-in decision — see
/// [`granting_on_sign_in_a_browser_signed_in_as_the_owner_is_granted_with_no_press`].)
#[tokio::test(flavor = "multi_thread")]
async fn a_browser_signed_in_as_the_owner_waits_for_approve_at_the_station() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_not_granting_on_sign_in(&cloud, &vault);
    let status = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert_eq!(
        status.account.unconfirmed,
        BTreeMap::from([(BROWSER.to_string(), "approveAtStation")]),
        "shown waiting for Approve"
    );
    assert_eq!(status.station_permissions, Vec::<String>::new(), "control");
    assert_eq!(status.pinned_devices, Vec::<String>::new(), "pinned");
    assert!(status.account.added.is_empty(), "a notice of it");
    assert_eq!(vault.device_cache().unwrap(), None, "kept for the LAN");
    let again = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(again.station_permissions, Vec::<String>::new(), "next list");
    assert_eq!(again.pinned_devices, Vec::<String>::new(), "next list");

    let approved = service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: true,
            key: Some(sha256(&key("browser"))),
            kind: Some(Kind::Browser),
        })
        .await
        .unwrap();
    assert_eq!(
        (approved.station_permissions, approved.pinned_devices),
        (vec![BROWSER.to_string()], vec![BROWSER.to_string()]),
        "CONTROL: Approve at the station"
    );
    assert_eq!(approved.account.unconfirmed, BTreeMap::new());
}

/// ⛔ WITH THE SIGN-IN GRANT SWITCHED OFF: a computer signed in by someone the station invited to
/// operate: its proof and its invitation hold here, but it is pinned for nothing, so the room's
/// tunnel does not take it, and nothing is kept for the LAN on it; it waits for Approve at the
/// station. Approve here pins it and the tunnel takes it. CONTROL: invited only to listen it is
/// refused as it was, and someone not invited gets nothing. (The on-switch default is
/// [`granting_on_sign_in_an_invited_guests_computer_is_pinned_by_the_invitations_role`].)
#[tokio::test(flavor = "multi_thread")]
async fn an_invited_guests_computer_waits_for_approve_at_the_station() {
    let invited = |role| {
        vec![Invite {
            mailbox: mailbox("K9.Guest@example.test"),
            role,
        }]
    };
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, invited(Role::Operate));
    let service = launch_not_granting_on_sign_in(&cloud, &vault);
    let tunnel = || service.control.lock().unwrap().approved.paired(&road_pin());
    let status = listed(
        &service,
        &cloud,
        vec![computer_row(&token("guest"), "operate")],
    )
    .await;
    assert_eq!(
        status.account.unconfirmed,
        BTreeMap::from([(COMPUTER_ID.to_string(), "approveAtStation")]),
        "shown waiting for Approve"
    );
    assert_eq!(status.pinned_devices, Vec::<String>::new(), "pinned");
    assert_eq!(tunnel(), None, "the tunnel takes it");
    assert!(status.account.added.is_empty(), "a notice of it");
    assert_eq!(vault.device_cache().unwrap(), None, "kept for the LAN");

    let approved = service
        .action(Action::Device {
            device_id: COMPUTER_ID.into(),
            approve: true,
            key: Some(sha256(&key("road"))),
            kind: Some(Kind::Computer),
        })
        .await
        .unwrap();
    assert_eq!(
        approved.pinned_devices,
        vec![COMPUTER_ID.to_string()],
        "CONTROL: Approve at the station"
    );
    assert!(tunnel().is_some(), "CONTROL: the tunnel takes it approved");

    for (invites, proof, why) in [
        (invited(Role::Listen), "guest", "listenNative"),
        (Vec::new(), "stranger", "proofNotYours"),
    ] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, invites);
        let service = launch_owned(&cloud, &vault);
        let status = listed(
            &service,
            &cloud,
            vec![computer_row(&token(proof), "operate")],
        )
        .await;
        assert_eq!(
            status.account.unconfirmed,
            BTreeMap::from([(COMPUTER_ID.to_string(), why)]),
            "{proof}"
        );
        assert!(status.pinned_devices.is_empty(), "{proof}");
    }
}

/// ★ With sign-in grants on ([`launch_granting_on_sign_in`]), a browser signed in as the station's
/// owner: the service approved it, and the station, having checked its proof itself, grants it what
/// an Approve press grants and pins its key, with no press; the notice names it, and the LAN keeps
/// it under its key's pin. What it grants is station control: station control and the
/// pin are what a stream needs, and a streaming browser uses the station's own window, transmit
/// included.
#[tokio::test(flavor = "multi_thread")]
async fn granting_on_sign_in_a_browser_signed_in_as_the_owner_is_granted_with_no_press() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let status = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert_eq!(status.account.unconfirmed, BTreeMap::new());
    assert_eq!(status.station_permissions, vec![BROWSER.to_string()]);
    // The proof was listed to the station, and the screen is never shown it.
    let shown = serde_json::to_string(&status).unwrap();
    assert!(!shown.contains(&token("browser")[..40]), "{shown}");
    assert!(
        shown.contains(r#""addedBy":"account""#),
        "CONTROL: the row itself is shown"
    );
    assert_eq!(
        status.pinned_devices,
        vec![BROWSER.to_string()],
        "with station control, all a stream needs"
    );
    // What a restart restores is the same: control.
    let mut remembered = None;
    for _ in 0..100 {
        remembered = vault.state().unwrap().map(|state| state.grants);
        if remembered.as_ref().is_some_and(|g| !g.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let remembered: Vec<_> = remembered
        .unwrap()
        .iter()
        .map(|g| (g.device_id.clone(), g.control))
        .collect();
    assert_eq!(remembered, vec![(BROWSER.to_string(), true)], "control");
    let added: Vec<_> = status
        .account
        .added
        .iter()
        .map(|a| (a.device_id.as_str(), a.role, a.guest, a.key.clone()))
        .collect();
    assert_eq!(
        added,
        vec![(BROWSER, Role::Operate, false, Some(sha256(&key("browser"))))]
    );
    let cache = vault.device_cache().unwrap().unwrap();
    assert_eq!(cache.station_id, STATION);
    assert_eq!(
        cache.devices,
        vec![CachedDevice {
            pin: sha256(&key("browser")),
            role: Role::Operate,
            guest: false,
            issued_at: cases()["issuedAt"].as_u64().unwrap(),
            confirmed_at: fixture_now(),
        }]
    );
    // The notice goes when the operator has seen it; the grants stay.
    let dismissed = service
        .action(Action::DismissAdded {
            device_id: BROWSER.into(),
        })
        .await
        .unwrap();
    assert!(dismissed.account.added.is_empty());
    assert_eq!(dismissed.station_permissions, vec![BROWSER.to_string()]);
}

/// ★ THE 2026-10-23 SIGN-IN DECISION: a device granted control on its sign-in carries a 15-minute
/// transmit hold. The grant records a `Held` against the device, and the service lists it as
/// sign-in-added, so the engine's arm gate holds it while it controls (proved end to end in the
/// operations transmit tests). A sign-in never waives it; only an Approve at the shack does.
#[tokio::test(flavor = "multi_thread")]
async fn granting_on_sign_in_holds_the_devices_transmit() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let _ = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert!(
        service
            .control
            .lock()
            .unwrap()
            .operations
            .tx_held(BROWSER, std::time::Instant::now()),
        "a sign-in-granted device is held from transmit"
    );
}

/// ★ A proof the station cannot confirm grants nothing, whatever the service says: no permission,
/// no pin, nothing kept for the LAN, and the device is shown with why.
#[tokio::test(flavor = "multi_thread")]
async fn a_proof_the_station_cannot_confirm_grants_nothing() {
    let mut other_keys = computer_row(&token("computer"), "operate");
    other_keys["lanKey"] = key("stationLan").into();
    for (row, id, why) in [
        // A browser's keys, signed in to the wrong application.
        (
            browser_row(&token("browserNativeAud")),
            BROWSER,
            "tokenAudience",
        ),
        // Signed with a key the issuer never had.
        (
            computer_row(&token("forged"), "operate"),
            COMPUTER_ID,
            "tokenSignature",
        ),
        // The owner's sign-in, for another LAN key than the one listed.
        (other_keys, COMPUTER_ID, "tokenNonce"),
    ] {
        let proof = why;
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        let service = launch_owned(&cloud, &vault);
        let status = listed(&service, &cloud, vec![row]).await;
        assert_eq!(
            status.account.unconfirmed,
            BTreeMap::from([(id.to_string(), why)]),
            "{proof}"
        );
        assert!(status.station_permissions.is_empty(), "{proof}");
        assert!(status.pinned_devices.is_empty(), "{proof}");
        assert!(status.account.added.is_empty(), "{proof}");
        assert_eq!(vault.device_cache().unwrap(), None, "{proof}");
    }
}

/// With sign-in grants on ([`launch_granting_on_sign_in`]), a computer signed in by someone the
/// station invited to operate: pinned (its own pin, which the room's tunnel holds station control
/// by), granted nothing on the relay's lanes, and kept for the LAN under its LAN key as a guest's. The service's role is not the station's to take: the
/// invitation decides. Invited to listen, a computer is refused, since its pin alone would hold
/// station control; and someone not invited gets nothing.
#[tokio::test(flavor = "multi_thread")]
async fn granting_on_sign_in_an_invited_guests_computer_is_pinned_by_the_invitations_role() {
    let invited = |role| {
        vec![Invite {
            mailbox: mailbox("K9.Guest@example.test"),
            role,
        }]
    };
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, invited(Role::Operate));
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let status = listed(
        &service,
        &cloud,
        vec![computer_row(&token("guest"), "listen")],
    )
    .await;
    assert_eq!(status.account.unconfirmed, BTreeMap::new());
    assert_eq!(status.pinned_devices, vec![COMPUTER_ID.to_string()]);
    let added: Vec<_> = status
        .account
        .added
        .iter()
        .map(|a| (a.device_id.as_str(), a.role, a.guest))
        .collect();
    assert_eq!(added, vec![(COMPUTER_ID, Role::Operate, true)]);
    let cache = vault.device_cache().unwrap().unwrap();
    assert_eq!(
        cache
            .devices
            .iter()
            .map(|d| (d.pin.clone(), d.role, d.guest))
            .collect::<Vec<_>>(),
        vec![(sha256(&key("computerLan")), Role::Operate, true)]
    );

    for (invites, proof, why) in [
        (invited(Role::Listen), "guest", "listenNative"),
        (Vec::new(), "stranger", "proofNotYours"),
    ] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, invites);
        let service = launch_granting_on_sign_in(&cloud, &vault);
        let status = listed(
            &service,
            &cloud,
            vec![computer_row(&token(proof), "operate")],
        )
        .await;
        assert_eq!(
            status.account.unconfirmed,
            BTreeMap::from([(COMPUTER_ID.to_string(), why)]),
            "{proof}"
        );
        assert!(status.pinned_devices.is_empty(), "{proof}");
        assert!(status.station_permissions.is_empty(), "{proof}");
        assert_eq!(vault.device_cache().unwrap(), None, "{proof}");
    }
}

/// ★ A SIGN-IN COMPUTER ON THE ROOM'S TUNNEL IS HELD AS ITS SERVICE ID IS: granted on its own sign-in,
/// it holds the tunnel's lease under the shack's name for its key, never the service's id, and the
/// station holds that name from transmit once the list is heard, as it holds the service id: held
/// with no end while its owner notice is not confirmed, shown held at the shack under its service
/// id, and freed under both names by Allow to transmit there.
#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_computer_on_the_tunnel_is_held_under_its_tunnel_name() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let mut row = computer_row(&token("computer"), "operate");
    row["notified"] = 0.into();
    let status = listed(&service, &cloud, vec![row]).await;
    assert_eq!(
        status.pinned_devices,
        vec![COMPUTER_ID.to_string()],
        "premise: granted on its sign-in"
    );
    let named = lan::tunnel_device_id(&road_pin());
    let operations = service.control.lock().unwrap().operations.clone();
    // Its lease, held by a connection kept open through the test.
    let engine = engine();
    let connection = operations::LanConnection::new(operations.clone());
    let ask = |request: operations::Request| loop {
        match operations.handle_version(
            (connection.id, 4),
            "50000000-0000-4000-8000-0000000000c1",
            &named,
            &request,
            &engine,
            std::time::Instant::now(),
        ) {
            Err("stationBusy" | "remoteBusy") => std::thread::sleep(Duration::from_millis(20)),
            answer => return answer.unwrap(),
        }
    };
    let boot = ask(operations::Request::State {
        request_id: super::lan_grants::fresh_id(),
    })["stationBootId"]
        .as_str()
        .unwrap()
        .to_string();
    let acquired = ask(operations::Request::Acquire {
        request_id: super::lan_grants::fresh_id(),
        station_boot_id: boot,
    });
    assert_eq!(
        acquired["phase"], "controlling",
        "premise: it holds the tunnel's lease"
    );
    assert!(
        tempo_app::engine::engine_lock(&engine).remote_transmit_held(),
        "the engine's gate lets it start a transmission"
    );
    assert!(
        operations.tx_held(COMPUTER_ID, past_the_window()),
        "premise: its service id is held"
    );
    assert!(
        operations.tx_held(&named, std::time::Instant::now()),
        "its tunnel name is free"
    );
    assert!(
        operations.tx_held(&named, past_the_window()),
        "its tunnel name is free past the window"
    );
    assert_eq!(
        operations
            .current_transmit_hold(std::time::Instant::now())
            .map(|hold| hold.held),
        Some(true),
        "the radio loop's tick lets it start a transmission"
    );
    let status = service.status().unwrap();
    assert_eq!(
        status.held_devices,
        vec![COMPUTER_ID.to_string()],
        "not shown held at the shack"
    );

    service.action(allow(COMPUTER_ID)).await.unwrap();
    assert!(
        !operations.tx_held(&named, past_the_window()),
        "Allow to transmit left its tunnel name held"
    );
    assert!(!operations.tx_held(COMPUTER_ID, past_the_window()));
    assert_eq!(
        service.status().unwrap().held_devices,
        Vec::<String>::new(),
        "still shown held"
    );
}

/// ★ GRANTED AGAIN ON ITS OWN SIGN-IN, A COMPUTER IS HELD BEFORE THE TUNNEL TAKES IT AGAIN: one the
/// shack allowed to transmit, then approved again at the service, is held at the moment its name
/// joins the tunnel's list again, so the waiver it had in this run never frees it there.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_granted_again_on_its_sign_in_is_held_before_the_tunnel_takes_it() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let row = computer_row(&token("computer"), "operate");
    let _ = listed(&service, &cloud, vec![row.clone()]).await;
    service.action(allow(COMPUTER_ID)).await.unwrap();
    let named = lan::tunnel_device_id(&road_pin());
    let (operations, approved) = {
        let control = service.control.lock().unwrap();
        (control.operations.clone(), control.approved.clone())
    };
    assert!(
        !operations.tx_held(&named, past_the_window()),
        "premise: allowed to transmit"
    );
    let seen = Arc::new(Mutex::new(Vec::new()));
    {
        let (seen, named) = (seen.clone(), named.clone());
        *approved.on_set.lock().unwrap() = Some(Box::new(move |names: &[String]| {
            if names.contains(&named) {
                let held = operations.tx_held(&named, std::time::Instant::now());
                seen.lock().unwrap().push(held);
            }
        }));
    }
    let mut again = row;
    again["generation"] = 4.into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [again] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    let seen = seen.lock().unwrap().clone();
    assert!(!seen.is_empty(), "premise: it joined the tunnel again");
    assert!(
        seen.iter().all(|held| *held),
        "free on the tunnel the moment it joined again"
    );
}

/// ★ PAIRING ON THE STATION'S OWN NETWORK FREES THAT ROAD ONLY: a computer added by its own sign-in
/// and paired by code at the shack (in the LAN book, and not kept there by a sign-in) is still held
/// when it drives the station over the internet. Nothing the station can check proves that the key
/// it holds for the tunnel and the LAN key the shack paired are one computer's: the proof that
/// names both does not show the LAN key's private half. Allow to transmit frees it, as for any
/// other sign-in computer.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_paired_by_code_at_the_shack_and_signed_in_is_held_on_the_tunnel() {
    use crate::remote_service::lan::tests::{LanStore, Scratch};
    const LAN_STATION: &str = "60000000-0000-4000-8000-0000000000aa";
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let lan_pin = sha256(&key("computerLan"));
    let pin: [u8; 32] = tempo_stream::protocol::hex_bytes(&lan_pin)
        .unwrap()
        .try_into()
        .unwrap();
    let (_, station) = station_key::Signer::generate().unwrap();
    let store = LanStore::holding(&station, LAN_STATION, &[(&pin, "DESKTOP-7Q")]);
    let scratch = Scratch::new();
    let engine = engine();
    let service = super::lan_grants::with_lan(
        launch_granting_on_sign_in(&cloud, &vault),
        &engine,
        &store,
        &scratch,
    );
    let mut row = computer_row(&token("computer"), "operate");
    row["notified"] = 0.into();
    let status = listed(&service, &cloud, vec![row]).await;
    assert_eq!(
        status.pinned_devices,
        vec![COMPUTER_ID.to_string()],
        "premise: granted on its sign-in"
    );
    let named = lan::tunnel_device_id(&road_pin());
    let (operations, enrolment, lan) = {
        let control = service.control.lock().unwrap();
        (
            control.operations.clone(),
            control.enrolment.clone(),
            control.lan.clone().unwrap(),
        )
    };
    // Paired by code at the shack since: out of the LAN's device cache.
    {
        let held = DevicesHeld::take(&vault);
        let mut cache = vault
            .device_cache()
            .unwrap()
            .unwrap_or_else(|| tempo_client::account::DeviceCache::new(STATION));
        cache.forget(&lan_pin);
        vault.save_device_cache(&held, &cache).unwrap();
    }
    enrolment.reread();
    assert!(
        vault
            .device_cache()
            .unwrap()
            .is_none_or(|cache| cache.find(&lan_pin).is_none()),
        "premise: not kept on the LAN by a sign-in"
    );
    assert!(
        operations.tx_held(&named, past_the_window()),
        "paired by code at the shack: free on the tunnel"
    );

    service.action(allow(COMPUTER_ID)).await.unwrap();
    assert!(
        !operations.tx_held(&named, past_the_window()),
        "CONTROL: Allow to transmit frees it"
    );
    assert_eq!(
        lan.revoke(&lan::device_id(&pin)),
        Ok(()),
        "premise: the LAN book held it"
    );
}

/// A station whose owner is not known here (paired by code, or bound as another station) vouches
/// for nobody: the owner's own browser waits for a press, as it does today.
#[tokio::test(flavor = "multi_thread")]
async fn a_station_with_no_owner_of_its_own_vouches_for_nobody() {
    for other in [false, true] {
        let cloud = fake_cloud().await;
        let vault = if other {
            let vault = owned_vault(&cloud.origin, Vec::new());
            let mut owner = vault.owner().unwrap().unwrap();
            owner.station_id = "20000000-0000-4000-8000-0000000000ff".into();
            vault.save_owner(&owner).unwrap();
            vault
        } else {
            paired_vault(&cloud.origin)
        };
        let service = launch_owned(&cloud, &vault);
        let status = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
        assert_eq!(
            status.account.unconfirmed,
            BTreeMap::from([(BROWSER.to_string(), "stationNotBound")]),
            "another station's owner: {other}"
        );
        assert!(status.station_permissions.is_empty());
        assert!(status.pinned_devices.is_empty());
    }
}

/// Once granted, a device is the operator's: station control taken away at the shack stays away
/// through the next list, which does not grant it again. With sign-in grants on
/// ([`launch_granting_on_sign_in`]), where that next list would.
#[tokio::test(flavor = "multi_thread")]
async fn the_operators_own_decision_outlasts_the_next_list() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let status = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert_eq!(status.station_permissions, vec![BROWSER.to_string()]);
    service
        .action(Action::StationPermission {
            device_id: BROWSER.into(),
            allow: false,
        })
        .await
        .unwrap();
    let again = service.action(Action::Refresh {}).await.unwrap();
    assert!(
        again.station_permissions.is_empty(),
        "{again:?}",
        again = again.station_permissions
    );
    assert_eq!(again.pinned_devices, vec![BROWSER.to_string()]);
}

// ---- The LAN's side ------------------------------------------------------------------------------

/// The LAN takes enrolments while the station is bound to an owner of its own, with the issuer's keys
/// kept: from the launch on, and no longer once the station is removed.
#[tokio::test(flavor = "multi_thread")]
async fn the_lan_takes_enrolments_only_while_the_station_has_its_owner() {
    let cloud = fake_cloud().await;
    let vault = paired_vault(&cloud.origin);
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let open = || service.control.lock().unwrap().enrolment.open();
    assert!(!open(), "no owner kept: closed");

    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let open = || service.control.lock().unwrap().enrolment.open();
    assert!(open(), "bound to its owner: open");
    service.action(Action::Forget {}).await.unwrap();
    assert!(!open(), "removed: closed");

    let vault = owned_vault(&cloud.origin, Vec::new());
    let mut owner = vault.owner().unwrap().unwrap();
    owner.station_id = "20000000-0000-4000-8000-0000000000ff".into();
    vault.save_owner(&owner).unwrap();
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let open = || service.control.lock().unwrap().enrolment.open();
    assert!(!open(), "another station's owner: closed");
}

/// ★ A computer kept for the LAN on its proof stays while the service lists it as approved, and
/// leaves the LAN's book and its keeping at the first list that no longer does. A computer paired
/// by code, which the station keeps on no proof, is left alone.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_the_service_stops_listing_leaves_the_lan() {
    use crate::remote_service::lan::tests::{LanStore, Scratch};
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let enrolled: [u8; 32] = tempo_stream::protocol::hex_bytes(&sha256(&key("computerLan")))
        .unwrap()
        .try_into()
        .unwrap();
    let paired: [u8; 32] = [7; 32];
    let (_, station) = station_key::Signer::generate().unwrap();
    let store = LanStore::holding(
        &station,
        "60000000-0000-4000-8000-0000000000aa",
        &[(&paired, "Den PC"), (&enrolled, "DESKTOP-7Q")],
    );
    let mut cache = tempo_client::account::DeviceCache::new(STATION);
    cache
        .keep(CachedDevice {
            pin: sha256(&key("computerLan")),
            role: Role::Operate,
            guest: false,
            issued_at: cases()["issuedAt"].as_u64().unwrap(),
            confirmed_at: 0,
        })
        .unwrap();
    vault
        .save_device_cache(&DevicesHeld::take(&vault), &cache)
        .unwrap();
    let scratch = Scratch::new();
    let engine = engine();
    let service =
        super::lan_grants::with_lan(launch_owned(&cloud, &vault), &engine, &store, &scratch);
    let names = |status: &Status| -> Vec<String> {
        let mut names: Vec<String> = status
            .lan
            .as_ref()
            .map(|lan| lan.devices.iter().map(|d| d.name.clone()).collect())
            .unwrap_or_default();
        names.sort();
        names
    };
    eventually(&service, "the book read", |s| names(s).len() == 2).await;

    let status = listed(
        &service,
        &cloud,
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    assert_eq!(names(&status), vec!["DESKTOP-7Q", "Den PC"]);
    let kept = vault.device_cache().unwrap().unwrap().devices;
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].confirmed_at, fixture_now(), "confirmed by the list");

    // Removed on the web: listed, no longer approved.
    let mut removed = computer_row(&token("computer"), "operate");
    removed["approved"] = 0.into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [removed] }).to_string();
    let status = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(names(&status), vec!["Den PC"]);
    assert_eq!(vault.device_cache().unwrap().unwrap().devices, Vec::new());
}

/// ★ A computer enrolled on the LAN whose proof has passed its age leaves the book and the LAN's
/// keeping at the next list, though the service still lists it as approved (it signs in again to
/// come back). A computer paired by code is left alone.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_past_its_proofs_age_leaves_the_lan_at_the_next_list() {
    use crate::remote_service::lan::tests::{LanStore, Scratch};
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let enrolled: [u8; 32] = tempo_stream::protocol::hex_bytes(&sha256(&key("computerLan")))
        .unwrap()
        .try_into()
        .unwrap();
    let paired: [u8; 32] = [7; 32];
    let (_, station) = station_key::Signer::generate().unwrap();
    let store = LanStore::holding(
        &station,
        "60000000-0000-4000-8000-0000000000aa",
        &[(&paired, "Den PC"), (&enrolled, "DESKTOP-7Q")],
    );
    let scratch = Scratch::new();
    let engine = engine();
    let service =
        super::lan_grants::with_lan(launch_owned(&cloud, &vault), &engine, &store, &scratch);
    let names = |status: &Status| -> Vec<String> {
        let mut names: Vec<String> = status
            .lan
            .as_ref()
            .map(|lan| lan.devices.iter().map(|d| d.name.clone()).collect())
            .unwrap_or_default();
        names.sort();
        names
    };
    eventually(&service, "the book read", |s| names(s).len() == 2).await;
    // Kept since launch, its proof now exactly thirty days old: listed, approved, with no proof of
    // its own to vouch for again.
    let mut cache = tempo_client::account::DeviceCache::new(STATION);
    cache
        .keep(CachedDevice {
            pin: sha256(&key("computerLan")),
            role: Role::Operate,
            guest: false,
            issued_at: fixture_now() - tempo_client::account::PROOF_MAX_AGE_S,
            confirmed_at: 0,
        })
        .unwrap();
    vault
        .save_device_cache(&DevicesHeld::take(&vault), &cache)
        .unwrap();
    let mut row = computer_row(&token("computer"), "operate");
    row["proof"] = serde_json::Value::Null;
    let status = listed(&service, &cloud, vec![row]).await;
    assert_eq!(names(&status), vec!["Den PC"]);
    assert_eq!(vault.device_cache().unwrap().unwrap().devices, Vec::new());
}

/// ★ A computer kept for the LAN on its proof, then paired again with a code at the shack, is the
/// shack's to remove from then on, as every computer paired by code is: a removal on the web (the
/// service lists it, no longer approved) leaves it on the LAN and still welcomed there, and Remove
/// at the shack takes it out. CONTROL: a computer kept on its proof and never paired by code leaves
/// the LAN at that same list, as before. Skipped, saying so, on a box with no private address of
/// its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_paired_again_with_a_code_is_removed_at_the_shack_not_on_the_web() {
    use crate::lan_client::{code, pairing, road, ComputerKey};
    use crate::remote_service::lan::tests::{only, own_private_address, LanStore, Scratch};
    use crate::remote_service::vault::PairedStation;
    const LAN_STATION: &str = "60000000-0000-4000-8000-0000000000aa";
    const LAPTOP: &str = "10000000-0000-4000-8000-00000000000e";
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return;
    };
    let port = std::net::TcpListener::bind((address, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let network = tempo_stream::lan::Network::new(address, 32).unwrap();
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let pin_of = |spki: &str| -> [u8; 32] {
        tempo_stream::protocol::hex_bytes(&sha256(spki))
            .unwrap()
            .try_into()
            .unwrap()
    };
    // The fixture's computer, kept on its proof and never paired by code.
    let enrolled = pin_of(&key("computerLan"));
    // A laptop with keys made for this run, kept on its proof and then paired again with a code.
    let (laptop, laptop_pkcs8) = ComputerKey::generate().unwrap();
    let (laptop_road, _) = ComputerKey::generate().unwrap();
    let laptop_pin = pin_of(laptop.public_key());
    let paired: [u8; 32] = [7; 32];
    let (_, station) = station_key::Signer::generate().unwrap();
    let store = LanStore::holding(
        &station,
        LAN_STATION,
        &[
            (&paired, "Den PC"),
            (&enrolled, "DESKTOP-7Q"),
            (&laptop_pin, "Laptop"),
        ],
    );
    let mut cache = tempo_client::account::DeviceCache::new(STATION);
    for pin in [sha256(&key("computerLan")), sha256(laptop.public_key())] {
        cache
            .keep(CachedDevice {
                pin,
                role: Role::Operate,
                guest: false,
                issued_at: cases()["issuedAt"].as_u64().unwrap(),
                confirmed_at: 0,
            })
            .unwrap();
    }
    vault
        .save_device_cache(&DevicesHeld::take(&vault), &cache)
        .unwrap();
    let scratch = Scratch::new();
    let engine = engine();
    let service = launch_owned(&cloud, &vault).with_lan(
        scratch.path(),
        engine.clone(),
        transport::Feeds {
            monitor: crate::remote_monitor::Publisher::default(),
            #[cfg(feature = "radio")]
            audio: None,
            stream: stream::Host::default(),
        },
        Arc::new(lan::Book::new(Arc::new(store.clone()))),
        Arc::new(move |_| only(Ok(network))),
        lan::commands::Commands::default(),
    );
    let names = |status: &Status| -> Vec<String> {
        let mut names: Vec<String> = status
            .lan
            .as_ref()
            .map(|lan| lan.devices.iter().map(|d| d.name.clone()).collect())
            .unwrap_or_default();
        names.sort();
        names
    };
    eventually(&service, "the book read", |s| names(s).len() == 3).await;
    service
        .action(Action::LanOn {
            address: None,
            port: Some(port),
        })
        .await
        .unwrap();
    let at = std::net::SocketAddrV4::new(address, port);
    eventually(&service, "never listened", |s| {
        s.lan.as_ref().and_then(|lan| lan.listening.clone()) == Some(at.to_string())
    })
    .await;
    let laptop_row = json!({"id": LAPTOP, "name": "Laptop", "approved": 1,
        "expiresAt": APPROVED_UNTIL, "generation": 4, "renewsUntil": null,
        "publicKey": laptop_road.public_key(), "kind": "computer", "platform": "windows",
        "untilRemoved": 1, "role": "operate", "addedBy": "account", "proof": null,
        "lanKey": laptop.public_key()});
    let rows = vec![computer_row(&token("computer"), "operate"), laptop_row];
    let status = listed(&service, &cloud, rows.clone()).await;
    assert_eq!(names(&status), vec!["DESKTOP-7Q", "Den PC", "Laptop"]);

    // Paired again at the shack, with the code it shows, by the laptop's own key.
    let status = service.action(Action::LanPair {}).await.unwrap();
    let shown = status.lan.unwrap().pairing.unwrap().code;
    let known = PairedStation {
        station_id: LAN_STATION.into(),
        device_id: lan::device_id(&laptop_pin),
        station_key: lan::tls::Identity::new(&station, LAN_STATION.into())
            .unwrap()
            .public_key()
            .to_string(),
        pkcs8: laptop_pkcs8.clone(),
        addresses: vec![at.to_string()],
    };
    let again = pairing::pair(at, code(&shown).unwrap(), "Laptop", &[known])
        .await
        .unwrap();
    assert_eq!(again.pkcs8, laptop_pkcs8, "paired again with its own key");

    // Removed on the web: both listed, neither approved now.
    let removed: Vec<serde_json::Value> = rows
        .into_iter()
        .map(|mut row| {
            row["approved"] = 0.into();
            row
        })
        .collect();
    *cloud.devices.lock().unwrap() = json!({ "devices": removed }).to_string();
    let status = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        names(&status),
        vec!["Den PC", "Laptop"],
        "removed on the web"
    );
    let (welcomed, _) = road::connect(&again, Some(at))
        .await
        .expect("still welcomed on the LAN");
    assert_eq!(welcomed.ids.device, again.device_id);
    drop(welcomed);

    // Removed at the shack: out.
    let status = service
        .action(Action::LanRevoke {
            device_id: again.device_id.clone(),
        })
        .await
        .unwrap();
    assert_eq!(names(&status), vec!["Den PC"]);
    // The station's own refusal of its key, which nothing in front of the station gives.
    assert_eq!(
        road::connect(&again, Some(at)).await.err(),
        Some("notPaired"),
        "removed at the shack"
    );
}

// ---- Removals made offline -----------------------------------------------------------------------

/// ★ A device removed at the shack while the service cannot be told: out at the station at once, as
/// ever, and kept waiting (in the store too) instead of failing. While it waits, its proof grants
/// it nothing, though the service still lists it as approved; the service is told before the next
/// list once it answers, and it is no longer waiting. With sign-in grants on
/// ([`launch_granting_on_sign_in`]), where that proof would grant it again.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_made_offline_is_sent_before_the_next_list_and_grants_nothing_meanwhile() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let status = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert_eq!(status.pinned_devices, vec![BROWSER.to_string()], "premise");

    cloud.refusing_removals.store(true, Ordering::SeqCst);
    let removed = service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert_eq!(removed.account.removals, vec![BROWSER.to_string()]);
    assert!(removed.station_permissions.is_empty());
    assert!(removed.pinned_devices.is_empty());
    assert_eq!(
        vault.revokes().unwrap(),
        Some(vault::Revokes {
            station_id: STATION.into(),
            devices: vec![BROWSER.into()]
        })
    );

    // The service still lists it as approved, with its proof: nothing is granted.
    let waiting = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(waiting.account.removals, vec![BROWSER.to_string()]);
    assert!(waiting.station_permissions.is_empty());
    assert!(waiting.pinned_devices.is_empty());

    // A restart reads them back, and still grants nothing.
    drop(service);
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let restarted = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert_eq!(restarted.account.removals, vec![BROWSER.to_string()]);
    assert!(restarted.pinned_devices.is_empty());

    cloud.refusing_removals.store(false, Ordering::SeqCst);
    let heard = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(heard.account.removals, Vec::<String>::new());
    assert_eq!(heard.devices.len(), 0, "the service took it off its list");
    assert_eq!(
        vault.revokes().unwrap().unwrap().devices,
        Vec::<String>::new()
    );
}

/// A removal the service has meanwhile taken off its list (removed on the web too) is no longer
/// waiting, though the service still will not answer the removal itself.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_the_service_no_longer_lists_is_not_waiting() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_owned(&cloud, &vault);
    listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    cloud.refusing_removals.store(true, Ordering::SeqCst);
    let removed = service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert_eq!(
        removed.account.removals,
        vec![BROWSER.to_string()],
        "premise"
    );
    *cloud.devices.lock().unwrap() = json!({ "devices": [] }).to_string();
    let heard = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(heard.account.removals, Vec::<String>::new());
}

/// Removals that cannot be read may hide a device removed here: then no proof grants anything.
#[tokio::test(flavor = "multi_thread")]
async fn unreadable_removals_grant_nothing_on_a_proof() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    vault
        .values
        .lock()
        .unwrap()
        .insert("account-revokes".into(), "{not json".into());
    let service = launch_owned(&cloud, &vault);
    let status = listed(&service, &cloud, vec![browser_row(&token("browser"))]).await;
    assert_eq!(
        status.account.unconfirmed,
        BTreeMap::from([(BROWSER.to_string(), "credentialStoreUnavailable")])
    );
    assert!(status.pinned_devices.is_empty());
}

/// A removal the service refuses by name (not for being out of reach) is that refusal, said at the
/// shack, and nothing waits to be sent.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_the_service_refuses_by_name_does_not_wait() {
    let origin = refusing_cloud("403 Forbidden", r#"{"error":"stationRevoked"}"#).await;
    let vault = paired_vault(&origin);
    let service = Service::with_accounts(
        origin,
        Box::new(vault.clone()),
        engine(),
        crate::remote_monitor::Publisher::default(),
        accounts(&vault, binder(Vec::new()).0),
    );
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let refused = service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await;
    assert_eq!(refused.err(), Some("stationRevoked"));
    assert_eq!(
        service.status().unwrap().account.removals,
        Vec::<String>::new()
    );
    assert_eq!(vault.revokes().unwrap(), None);
}

// ---- People --------------------------------------------------------------------------------------

/// ★ Inviting someone tells the service (the address as typed, and the role), then keeps them here
/// by their address as the service matches it; with sign-in grants on
/// ([`launch_granting_on_sign_in`]), their computer is then approved on their own proof. Taking the
/// invitation back takes them out here before the service is told.
#[tokio::test(flavor = "multi_thread")]
async fn an_invitation_is_kept_here_and_its_guest_is_approved_on_their_proof() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    // Invited again with another role: one invitation, the later role.
    for role in [Role::Listen, Role::Operate] {
        service
            .action(Action::Invite {
                email: " K9.Guest+radio@Example.test ".into(),
                role,
            })
            .await
            .unwrap();
    }
    let invited = service.status().unwrap();
    let kept = vec![Invite {
        mailbox: "k9.guest@example.test".into(),
        role: Role::Operate,
    }];
    assert_eq!(invited.account.people, kept);
    let enrolment = service.control.lock().unwrap().enrolment.clone();
    assert_eq!(
        enrolment.invites(),
        Some(kept.clone()),
        "the LAN takes them too"
    );
    assert_eq!(
        vault.people().unwrap(),
        Some(People {
            station_id: STATION.into(),
            invites: kept
        })
    );
    assert_eq!(
        cloud.invites.lock().unwrap().last().cloned(),
        Some((
            "invite".to_string(),
            json!({"email": "K9.Guest+radio@Example.test", "role": "operate"})
        ))
    );
    let status = listed(
        &service,
        &cloud,
        vec![computer_row(&token("guest"), "operate")],
    )
    .await;
    assert_eq!(status.pinned_devices, vec![COMPUTER_ID.to_string()]);

    let taken_back = service
        .action(Action::Uninvite {
            email: "k9.guest@example.test".into(),
        })
        .await
        .unwrap();
    assert_eq!(taken_back.account.people, Vec::new());
    assert_eq!(vault.people().unwrap().unwrap().invites, Vec::new());
    assert_eq!(
        cloud.invites.lock().unwrap().last().cloned(),
        Some((
            "uninvite".to_string(),
            json!({"email": "k9.guest@example.test"})
        ))
    );
}

/// An invitation the service did not take is not kept; an address out of shape is sent nowhere; and
/// an invitation taken back while the service cannot be told is out here all the same (and waits to
/// be sent).
#[tokio::test(flavor = "multi_thread")]
async fn invitations_fail_closed() {
    let cloud = fake_cloud().await;
    let invite = Invite {
        mailbox: "k9.guest@example.test".into(),
        role: Role::Operate,
    };
    let vault = owned_vault(&cloud.origin, vec![invite]);
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    for email in [
        "no-at-sign",
        "@example.test",
        "k9@",
        "two words@example.test",
    ] {
        assert!(
            matches!(
                service
                    .action(Action::Invite {
                        email: email.into(),
                        role: Role::Listen,
                    })
                    .await,
                Err("invalidRequest")
            ),
            "{email}"
        );
    }
    assert!(cloud.invites.lock().unwrap().is_empty());

    cloud.refusing.store(true, Ordering::SeqCst);
    assert!(matches!(
        service
            .action(Action::Invite {
                email: "w1aw@example.test".into(),
                role: Role::Listen,
            })
            .await,
        Err("serviceUnavailable")
    ));
    assert_eq!(vault.people().unwrap().unwrap().invites.len(), 1);
    service
        .action(Action::Uninvite {
            email: "K9.Guest@example.test".into(),
        })
        .await
        .unwrap();
    assert_eq!(vault.people().unwrap().unwrap().invites, Vec::new());
}

/// The invitations taken back offline, as the store keeps them.
fn waiting_uninvites(vault: &MemoryVault) -> Vec<String> {
    vault
        .uninvites()
        .unwrap()
        .map(|u| u.people)
        .unwrap_or_default()
}

/// ★ An invitation taken back while the service cannot be told: out here at once, as ever, and kept
/// waiting (in the store too, read back after a restart) instead of failing. It is sent before the
/// next list once the service answers, and no longer waits; the service then removes that person's
/// devices.
#[tokio::test(flavor = "multi_thread")]
async fn an_invitation_taken_back_offline_is_sent_before_the_next_list() {
    let cloud = fake_cloud().await;
    let invite = Invite {
        mailbox: "k9.guest@example.test".into(),
        role: Role::Operate,
    };
    let vault = owned_vault(&cloud.origin, vec![invite]);
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    cloud.refusing.store(true, Ordering::SeqCst);
    let taken_back = service
        .action(Action::Uninvite {
            email: " K9.Guest@example.test ".into(),
        })
        .await
        .unwrap();
    assert_eq!(taken_back.account.people, Vec::new());
    assert_eq!(
        vault.uninvites().unwrap(),
        Some(vault::Uninvites {
            station_id: STATION.into(),
            people: vec!["K9.Guest@example.test".into()]
        })
    );
    // Still out of reach at the next list: still waiting.
    assert!(matches!(
        service.action(Action::Refresh {}).await,
        Err("serviceUnavailable")
    ));
    assert_eq!(waiting_uninvites(&vault), vec!["K9.Guest@example.test"]);
    assert!(cloud.invites.lock().unwrap().is_empty());

    drop(service);
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    cloud.refusing.store(false, Ordering::SeqCst);
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        cloud.invites.lock().unwrap().clone(),
        vec![(
            "uninvite".to_string(),
            json!({"email": "K9.Guest@example.test"})
        )]
    );
    assert_eq!(waiting_uninvites(&vault), Vec::<String>::new());
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(cloud.invites.lock().unwrap().len(), 1, "sent once");
}

/// Inviting the same person again, once the service takes it, drops the invitation taken back
/// offline: sent later, it would take the new one back. CONTROL: another person's stays waiting.
#[tokio::test(flavor = "multi_thread")]
async fn inviting_again_drops_the_same_persons_waiting_uninvite() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    cloud.refusing.store(true, Ordering::SeqCst);
    for email in ["K9.Guest@example.test", "w1aw@example.test"] {
        service
            .action(Action::Uninvite {
                email: email.into(),
            })
            .await
            .unwrap();
    }
    // Not taken by the service: the uninvite still waits.
    assert!(service
        .action(Action::Invite {
            email: "k9.guest+radio@example.test".into(),
            role: Role::Listen,
        })
        .await
        .is_err());
    assert_eq!(waiting_uninvites(&vault).len(), 2);
    cloud.refusing.store(false, Ordering::SeqCst);
    service
        .action(Action::Invite {
            email: "k9.guest+radio@example.test".into(),
            role: Role::Listen,
        })
        .await
        .unwrap();
    assert_eq!(waiting_uninvites(&vault), vec!["w1aw@example.test"]);
    service.action(Action::Refresh {}).await.unwrap();
    let sent = cloud.invites.lock().unwrap().clone();
    assert_eq!(
        sent.last().cloned(),
        Some((
            "uninvite".to_string(),
            json!({"email": "w1aw@example.test"})
        ))
    );
    assert_eq!(
        sent.iter().filter(|(verb, _)| verb == "uninvite").count(),
        1
    );
}

/// An invitation taken back that the service refuses by name is that refusal, and nothing waits;
/// taken back offline with the waiting ones unreadable, it is out here, and the shack says the store
/// failed rather than losing it silently.
#[tokio::test(flavor = "multi_thread")]
async fn an_uninvite_waits_only_for_an_unreachable_service() {
    let origin = refusing_cloud("403 Forbidden", r#"{"error":"stationRevoked"}"#).await;
    let vault = owned_vault(&origin, Vec::new());
    let service = launch_owned_at(&origin, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    let refused = service
        .action(Action::Uninvite {
            email: "k9.guest@example.test".into(),
        })
        .await;
    assert_eq!(refused.err(), Some("stationRevoked"));
    assert_eq!(vault.uninvites().unwrap(), None);

    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    vault
        .values
        .lock()
        .unwrap()
        .insert("account-uninvites".into(), "{not json".into());
    let service = launch_owned(&cloud, &vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    cloud.refusing.store(true, Ordering::SeqCst);
    let unkept = service
        .action(Action::Uninvite {
            email: "k9.guest@example.test".into(),
        })
        .await;
    assert_eq!(unkept.err(), Some("credentialStoreUnavailable"));
}

// ---- A confirmed owner notice across a restart ---------------------------------------------------

/// The owner's browser as the service lists it with its owner notice `notified` (1) or not (0).
fn notified_row(notified: u8) -> serde_json::Value {
    let mut row = browser_row(&token("browser"));
    row["notified"] = notified.into();
    row
}
/// Whether the station holds the owner's browser from transmit at `at`.
fn held_at(service: &Service, at: std::time::Instant) -> bool {
    service
        .control
        .lock()
        .unwrap()
        .operations
        .tx_held(BROWSER, at)
}
/// A moment past any transmit window the station could have recorded since this test began.
fn past_the_window() -> std::time::Instant {
    std::time::Instant::now() + account::TX_HOLD + Duration::from_secs(60)
}
/// The confirmed-notice entry a run kept in `vault`, once it is there.
async fn kept_notified(vault: &MemoryVault) -> vault::Notified {
    for _ in 0..100 {
        if let Ok(Some(kept)) = vault.notified() {
            return kept;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no confirmed-notice entry was kept");
}
/// The station's own confirmed-notice entry: the owner's browser, its window `ended_s` (unix
/// seconds, 0 when it had elapsed), for `station_id` and `account_id`.
fn notified_entry(station_id: &str, account_id: &str, ended_s: u64) -> vault::Notified {
    vault::Notified {
        station_id: station_id.into(),
        account_id: account_id.into(),
        windows: BTreeMap::from([(BROWSER.to_string(), ended_s)]),
    }
}
/// A browser granted on its sign-in, then restarted from `vault` holding `entry`, and listed by the
/// service with its notice `notified`.
async fn restarted_with(
    cloud: &FakeCloud,
    vault: &MemoryVault,
    entry: Option<&str>,
    notified: u8,
) -> Service {
    let first = launch_granting_on_sign_in(cloud, vault);
    let _ = listed(&first, cloud, vec![notified_row(1)]).await;
    let _ = kept_notified(vault).await;
    drop(first);
    if let Some(entry) = entry {
        vault
            .values
            .lock()
            .unwrap()
            .insert("account-notified".into(), entry.into());
    }
    let service = launch_granting_on_sign_in(cloud, vault);
    let _ = listed(&service, cloud, vec![notified_row(notified)]).await;
    service
}

/// ★ A sign-in device the service confirmed its owner was told about is not held afresh by a
/// restart: the station keeps that confirmation with the end of its transmit window, so after the
/// restart it is held until that window ends, as before, and then transmits, once the list
/// confirms the notice again. Before, a restart left it with no record and held it for good.
#[tokio::test(flavor = "multi_thread")]
async fn a_confirmed_device_is_held_only_for_its_window_after_a_restart() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let first = launch_granting_on_sign_in(&cloud, &vault);
    let _ = listed(&first, &cloud, vec![notified_row(1)]).await;
    // CONTROL: before the restart it is held in its window and free past it.
    assert!(
        held_at(&first, std::time::Instant::now()),
        "premise: in its window"
    );
    assert!(!held_at(&first, past_the_window()), "premise: free past it");
    let kept = kept_notified(&vault).await;
    assert_eq!(kept.station_id, STATION);
    assert_eq!(kept.account_id, ACCOUNT);
    assert_eq!(kept.windows.keys().collect::<Vec<_>>(), vec![BROWSER]);
    let ends = kept.windows[BROWSER];
    let now_s = now_ms() / 1000;
    assert!(
        ends > now_s && ends <= now_s + account::TX_HOLD.as_secs() + 1,
        "the window's end, {ends}, kept as it is: about 15 minutes from {now_s}"
    );
    drop(first);

    let service = launch_granting_on_sign_in(&cloud, &vault);
    let _ = listed(&service, &cloud, vec![notified_row(1)]).await;
    assert!(
        held_at(&service, std::time::Instant::now()),
        "still in its window after the restart: held"
    );
    assert!(
        !held_at(&service, past_the_window()),
        "past its window and confirmed: free, not held afresh"
    );
}

/// ★ FAIL-CLOSED: a confirmed-notice entry that does not read restores nothing, and the device is
/// held; the same entry, readable, frees it (the control).
#[tokio::test(flavor = "multi_thread")]
async fn an_unreadable_confirmed_notice_entry_holds_the_device() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let readable = serde_json::to_string(&notified_entry(STATION, ACCOUNT, 0)).unwrap();
    let control = restarted_with(&cloud, &vault, Some(&readable), 1).await;
    assert!(
        !held_at(&control, past_the_window()),
        "CONTROL: readable, free"
    );
    drop(control);

    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = restarted_with(&cloud, &vault, Some("{\"stationId\":"), 1).await;
    assert!(held_at(&service, past_the_window()), "unreadable: held");
}

/// ★ THE LIVE LIST WINS: a device the next list no longer confirms was notified is held, whatever
/// the station kept, and is dropped from what it keeps at once.
#[tokio::test(flavor = "multi_thread")]
async fn a_list_that_no_longer_confirms_the_notice_holds_and_drops_it() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let readable = serde_json::to_string(&notified_entry(STATION, ACCOUNT, 0)).unwrap();
    let service = restarted_with(&cloud, &vault, Some(&readable), 0).await;
    assert!(held_at(&service, past_the_window()), "not confirmed: held");
    let mut kept = None;
    for _ in 0..100 {
        kept = vault.notified().unwrap();
        if kept.as_ref().is_some_and(|k| k.windows.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(kept.unwrap().windows, BTreeMap::new(), "dropped at once");

    // And a device the list stops naming as added by a sign-in is dropped too.
    *cloud.devices.lock().unwrap() = json!({ "devices": [notified_row(1)] }).to_string();
    let _ = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        kept_notified(&vault).await.windows.len(),
        1,
        "CONTROL: confirmed, kept"
    );
    let mut shack = notified_row(1);
    shack["addedBy"] = "shack".into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [shack] }).to_string();
    let _ = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        vault.notified().unwrap().unwrap().windows,
        BTreeMap::new(),
        "no longer a sign-in device: dropped"
    );
}

/// ★ BOUND TO THIS STATION'S ACCOUNT: an entry kept for another station, or for another account,
/// restores nothing, and the device is held; the station's own frees it (the control).
#[tokio::test(flavor = "multi_thread")]
async fn a_confirmed_notice_entry_for_another_station_or_account_is_ignored() {
    for (station_id, account_id, held) in [
        (STATION, ACCOUNT, false),
        ("20000000-0000-4000-8000-000000000001", ACCOUNT, true),
        (STATION, "30000000-0000-4000-8000-000000000001", true),
    ] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        let entry = serde_json::to_string(&notified_entry(station_id, account_id, 0)).unwrap();
        let service = restarted_with(&cloud, &vault, Some(&entry), 1).await;
        assert_eq!(
            held_at(&service, past_the_window()),
            held,
            "{station_id} / {account_id}"
        );
    }
}

/// Forgetting the pairing forgets what the station kept of its confirmed notices.
#[tokio::test(flavor = "multi_thread")]
async fn forgetting_the_pairing_forgets_the_confirmed_notices() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let _ = listed(&service, &cloud, vec![notified_row(1)]).await;
    assert_eq!(kept_notified(&vault).await.windows.len(), 1, "premise");
    service.action(Action::Forget {}).await.unwrap();
    assert_eq!(vault.notified().unwrap().unwrap().windows, BTreeMap::new());
}

/// ★ A restored window is never longer than the hold itself, however far ahead the kept end says:
/// an end of `u64::MAX` (a corrupt but readable entry) restores at most one 15-minute window, and
/// starting the station with it does not fail.
#[tokio::test(flavor = "multi_thread")]
async fn a_restored_window_is_capped_at_the_hold() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let far = serde_json::to_string(&notified_entry(STATION, ACCOUNT, u64::MAX)).unwrap();
    let service = restarted_with(&cloud, &vault, Some(&far), 1).await;
    assert!(
        held_at(&service, std::time::Instant::now()),
        "inside the capped window: held"
    );
    assert!(
        !held_at(&service, past_the_window()),
        "past one whole window: free, not held for the far end"
    );
}

/// ★ A device removed at the shack, whose removal is still waiting for the service, is dropped from
/// what the station keeps of confirmed notices, though the service still lists it confirmed.
#[tokio::test(flavor = "multi_thread")]
async fn a_device_whose_removal_is_waiting_is_not_kept() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let _ = listed(&service, &cloud, vec![notified_row(1)]).await;
    assert_eq!(kept_notified(&vault).await.windows.len(), 1, "premise");

    cloud.refusing_removals.store(true, Ordering::SeqCst);
    let removed = service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert_eq!(
        removed.account.removals,
        vec![BROWSER.to_string()],
        "premise: waiting"
    );
    *cloud.devices.lock().unwrap() = json!({ "devices": [notified_row(1)] }).to_string();
    let _ = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        vault.notified().unwrap().unwrap().windows,
        BTreeMap::new(),
        "a removal waiting here: not kept"
    );
}

// ---- Allow to transmit, at the shack -------------------------------------------------------------

fn allow(device_id: &str) -> Action {
    Action::AllowTransmit {
        device_id: device_id.into(),
    }
}

/// ★ ALLOW TO TRANSMIT LIFTS THE HOLD: a device added by its own sign-in whose owner notice the
/// service never confirmed is held past its window, with no end; the shack lists it as held. Allow
/// to transmit, pressed at the shack, waives it: free now and past the window, and listed held no
/// more. Its station control is as it was.
#[tokio::test(flavor = "multi_thread")]
async fn allow_to_transmit_at_the_shack_lifts_a_held_devices_hold() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let before = listed(&service, &cloud, vec![notified_row(0)]).await;
    assert!(
        held_at(&service, past_the_window()),
        "premise: held with no end"
    );
    assert_eq!(before.held_devices, vec![BROWSER.to_string()], "shown held");

    let after = service.action(allow(BROWSER)).await.unwrap();
    assert!(!held_at(&service, std::time::Instant::now()), "free now");
    assert!(!held_at(&service, past_the_window()), "and after");
    assert_eq!(
        after.held_devices,
        Vec::<String>::new(),
        "shown held no more"
    );
    assert_eq!(after.station_permissions, before.station_permissions);
}

/// ★ A HELD DEVICE CANNOT ALLOW ITSELF THROUGH A STREAM: Allow to transmit pressed over the stream's
/// picture of the shack's own Settings is refused at the station with "only at the station's own
/// screen", and the device is still held, now and past its window. CONTROL: the same press made at
/// the shack lifts the hold.
#[tokio::test(flavor = "multi_thread")]
async fn allow_to_transmit_through_a_stream_is_refused_and_the_device_stays_held() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let before = listed(&service, &cloud, vec![notified_row(0)]).await;
    assert_eq!(before.held_devices, vec![BROWSER.to_string()], "premise");

    let streamed = service.press(Origin::Stream, allow(BROWSER)).await;
    assert_eq!(streamed.err(), Some("atShackOnly"));
    assert!(held_at(&service, past_the_window()), "still held");
    let status = service.status().unwrap();
    assert_eq!(status.held_devices, vec![BROWSER.to_string()], "shown held");

    let local = service.press(Origin::Shack, allow(BROWSER)).await.unwrap();
    assert!(
        !held_at(&service, past_the_window()),
        "the shack's press: free"
    );
    assert_eq!(local.held_devices, Vec::<String>::new());
}

/// Every press the shack's Remote settings can send that is not a read, as the page sends it.
fn every_press_but_a_read() -> Vec<serde_json::Value> {
    vec![
        json!({"type":"stationPermission","deviceId":BROWSER,"allow":false}),
        json!({"type":"stationPermission","deviceId":BROWSER,"allow":true}),
        json!({"type":"takeOverLogging"}),
        json!({"type":"begin","name":"K9XYZ"}),
        json!({"type":"cancel"}),
        json!({"type":"approve","enrollmentId":"e","accountId":"a"}),
        json!({"type":"enable"}),
        json!({"type":"disable"}),
        json!({"type":"forget"}),
        json!({"type":"device","deviceId":BROWSER,"approve":true,"kind":"browser"}),
        json!({"type":"device","deviceId":BROWSER,"approve":false}),
        json!({"type":"lanOn"}),
        json!({"type":"lanOff"}),
        json!({"type":"lanAddress"}),
        json!({"type":"lanPair"}),
        json!({"type":"lanCancel"}),
        json!({"type":"lanRevoke","deviceId":BROWSER}),
        json!({"type":"lanReset"}),
        json!({"type":"signIn","name":"K9XYZ"}),
        json!({"type":"signInAgain"}),
        json!({"type":"cancelSignInAgain"}),
        json!({"type":"invite","email":"k9.new@example.test","role":"operate"}),
        json!({"type":"uninvite","email":"k9.guest@example.test"}),
        json!({"type":"dismissAdded","deviceId":BROWSER}),
        json!({"type":"allowTransmit","deviceId":BROWSER}),
    ]
}
/// What a streamed press must leave as it was: who may use the station, its holds, and Remote on
/// or off (the phase alone moves between connecting and reconnecting by itself).
fn trust(status: &Status) -> serde_json::Value {
    let shown = serde_json::to_value(status).unwrap();
    json!([
        shown["phase"] == "disabled",
        shown["phase"] == "unpaired",
        shown["devices"],
        shown["stationPermissions"],
        shown["heldDevices"],
        shown["pinnedDevices"],
        shown["account"]["people"],
        shown["lan"]
    ])
}

/// ★ ONLY A READ COMES THROUGH A STREAM: every other press the shack's Remote settings can send —
/// approve, remove, allow to transmit, station control, invite and uninvite, the network's pairing
/// and reset, signing in, Remote on and off — made over the stream's picture is refused at the
/// station with "only at the station's own screen", and changes nothing. CONTROL: a read through
/// the stream is answered.
#[tokio::test(flavor = "multi_thread")]
async fn every_press_but_a_read_through_a_stream_is_refused_and_changes_nothing() {
    let guest = mailbox("K9.Guest@example.test");
    let cloud = fake_cloud().await;
    let vault = owned_vault(
        &cloud.origin,
        vec![Invite {
            mailbox: guest,
            role: Role::Operate,
        }],
    );
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let before = listed(&service, &cloud, vec![notified_row(0)]).await;
    assert_eq!(before.held_devices, vec![BROWSER.to_string()], "premise");
    for value in every_press_but_a_read() {
        let action: Action = serde_json::from_value(value.clone()).unwrap();
        assert_eq!(
            service.press(Origin::Stream, action).await.err(),
            Some("atShackOnly"),
            "{value}"
        );
        assert_eq!(trust(&service.status().unwrap()), trust(&before), "{value}");
    }
    assert!(held_at(&service, past_the_window()), "still held");
    let read = service.press(Origin::Stream, Action::Refresh {}).await;
    assert_eq!(trust(&read.unwrap()), trust(&before), "a read is answered");
}

/// The press's origin is one of the two the page stamps, by name; anything else is no press.
#[test]
fn a_presss_origin_is_the_shack_or_a_stream_and_nothing_else() {
    let origin = |v: serde_json::Value| serde_json::from_value::<Origin>(v).ok();
    assert_eq!(origin(json!("shack")), Some(Origin::Shack));
    assert_eq!(origin(json!("stream")), Some(Origin::Stream));
    for other in [json!("Stream"), json!(""), json!(true), json!(null)] {
        assert_eq!(origin(other.clone()), None, "{other}");
    }
}

/// ★ THE COMMAND TAKES NO PRESS WITHOUT ITS ORIGIN: `remote_station_action` called through Tauri's
/// own dispatch with no `origin`, or one the page never stamps, is refused before the station sees
/// it. CONTROL: the same call with `origin` set is answered.
#[tokio::test(flavor = "multi_thread")]
async fn the_command_refuses_a_press_with_no_origin() {
    use tauri::ipc::{CallbackFn, InvokeBody};
    use tauri::Manager;
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let app = tauri::test::mock_builder()
        .invoke_handler(tauri::generate_handler![remote_station_action])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("a mock app");
    app.manage(launch_granting_on_sign_in(&cloud, &vault));
    let window = tauri::WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .expect("a main window");
    let call = |body: serde_json::Value| {
        let request = tauri::webview::InvokeRequest {
            cmd: "remote_station_action".into(),
            callback: CallbackFn(0),
            error: CallbackFn(1),
            url: "tauri://localhost".parse().unwrap(),
            body: InvokeBody::Json(body),
            headers: Default::default(),
            invoke_key: tauri::test::INVOKE_KEY.to_string(),
        };
        tokio::task::block_in_place(|| tauri::test::get_ipc_response(&window, request)).is_ok()
    };
    let refresh = json!({"type":"refresh"});
    assert!(!call(json!({ "action": refresh })), "no origin");
    for other in [json!("Shack"), json!(""), json!(null)] {
        assert!(
            !call(json!({ "action": refresh, "origin": other })),
            "{other}"
        );
    }
    assert!(
        call(json!({ "action": refresh, "origin": "shack" })),
        "the shack's"
    );
    assert!(
        call(json!({ "action": refresh, "origin": "stream" })),
        "a stream's read"
    );
}

/// ★ KEPT LIKE AN APPROVE: for a device whose notice the service confirmed, what the station keeps
/// for a restart is its window ended (0) once it is allowed, as an Approve at the shack keeps it.
#[tokio::test(flavor = "multi_thread")]
async fn allow_to_transmit_is_kept_for_a_restart_as_an_approve_is() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let _ = listed(&service, &cloud, vec![notified_row(1)]).await;
    assert_ne!(
        kept_notified(&vault).await.windows.get(BROWSER),
        Some(&0),
        "premise: a window still running"
    );
    service.action(allow(BROWSER)).await.unwrap();
    assert_eq!(
        vault.notified().unwrap().unwrap().windows.get(BROWSER),
        Some(&0)
    );
}

/// ★ ONLY A SIGN-IN DEVICE THIS STATION LISTS: a device the service lists as added at the shack, a
/// device it does not list at all, and one removed here whose removal still waits for the service
/// are each refused, and nothing is waived: the held sign-in device beside them stays held.
#[tokio::test(flavor = "multi_thread")]
async fn allow_to_transmit_is_refused_for_any_other_device() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let mut shack = computer_row(&token("computer"), "operate");
    shack["addedBy"] = "shack".into();
    let rows = vec![notified_row(0), shack];
    let _ = listed(&service, &cloud, rows.clone()).await;
    assert!(held_at(&service, past_the_window()), "premise: held");
    for (device, why, what) in [
        (COMPUTER_ID, "notSignInDevice", "added at the shack"),
        (
            "10000000-0000-4000-8000-0000000000ff",
            "notSignInDevice",
            "not listed",
        ),
        ("not an id", "invalidRequest", "not an id"),
    ] {
        let refused = service.action(allow(device)).await;
        assert_eq!(refused.err(), Some(why), "{what}: refused");
        assert!(held_at(&service, past_the_window()), "{what}: still held");
    }

    cloud.refusing_removals.store(true, Ordering::SeqCst);
    let removed = service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert_eq!(
        removed.account.removals,
        vec![BROWSER.to_string()],
        "premise"
    );
    assert_eq!(
        removed.held_devices,
        Vec::<String>::new(),
        "a removal waiting here: not offered"
    );
    *cloud.devices.lock().unwrap() = json!({ "devices": rows }).to_string();
    assert_eq!(
        service.action(allow(BROWSER)).await.err(),
        Some("notSignInDevice"),
        "a removal waiting here"
    );
    assert!(held_at(&service, past_the_window()), "removed: still held");
}

/// ★ NEVER A WAIVER FOR A DEVICE THE STATION GRANTED NOTHING: a sign-in device whose proof the
/// station could not confirm, and one waiting for an Approve here, are held from transmit but hold no
/// station control. Neither is offered Allow to transmit, and a press for either is refused and
/// waives nothing: still held past any window. Granted later on a proof that holds, the device is
/// held for its window as any sign-in grant is.
#[tokio::test(flavor = "multi_thread")]
async fn allow_to_transmit_is_refused_for_a_device_granted_nothing() {
    for (granting, proof, why) in [
        (true, "browserNativeAud", "tokenAudience"),
        (false, "browser", "approveAtStation"),
    ] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        let service = if granting {
            launch_granting_on_sign_in(&cloud, &vault)
        } else {
            launch_not_granting_on_sign_in(&cloud, &vault)
        };
        let mut row = browser_row(&token(proof));
        row["notified"] = 1.into();
        let status = listed(&service, &cloud, vec![row]).await;
        assert_eq!(
            status.account.unconfirmed,
            BTreeMap::from([(BROWSER.to_string(), why)]),
            "{why}: premise"
        );
        assert_eq!(status.station_permissions, Vec::<String>::new(), "{why}");
        assert!(held_at(&service, past_the_window()), "{why}: premise: held");
        assert_eq!(
            status.held_devices,
            Vec::<String>::new(),
            "{why}: not offered"
        );
        assert_eq!(
            service.action(allow(BROWSER)).await.err(),
            Some("notSignInDevice"),
            "{why}: refused"
        );
        assert!(
            held_at(&service, past_the_window()),
            "{why}: nothing waived"
        );
    }

    // Granted later, on its own proof: held for its window, then free.
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let mut row = browser_row(&token("browserNativeAud"));
    row["notified"] = 1.into();
    let _ = listed(&service, &cloud, vec![row]).await;
    assert!(service.action(allow(BROWSER)).await.is_err(), "premise");
    let granted = listed(&service, &cloud, vec![notified_row(1)]).await;
    assert_eq!(granted.station_permissions, vec![BROWSER.to_string()]);
    assert!(
        held_at(&service, std::time::Instant::now()),
        "granted later: held for its window"
    );
    assert_eq!(granted.held_devices, vec![BROWSER.to_string()], "offered");
    assert!(!held_at(&service, past_the_window()), "then free");
}

/// ★ A LOCAL DECISION AFTER THE PRESS WINS: Allow to transmit is pressed, and Remote is taken over or
/// turned off at the shack before the station has run the press (it waits on the service's list).
/// The press is answered and waives nothing: the device is still held, now and past its window.
#[tokio::test(flavor = "multi_thread")]
async fn allow_to_transmit_overtaken_by_a_take_over_or_turn_off_leaves_the_device_held() {
    for (what, decision) in [
        ("take over", Action::TakeOverLogging {}),
        ("Turn off", Action::Disable {}),
    ] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        let service = launch_granting_on_sign_in(&cloud, &vault);
        let before = listed(&service, &cloud, vec![notified_row(0)]).await;
        assert_eq!(
            before.held_devices,
            vec![BROWSER.to_string()],
            "{what}: premise"
        );

        cloud.holding_lists.store(true, Ordering::SeqCst);
        let mut press = std::pin::pin!(service.action(allow(BROWSER)));
        assert!(
            tokio::time::timeout(Duration::from_millis(200), &mut press)
                .await
                .is_err(),
            "{what}: premise: the press waits on the service's list"
        );
        // Taken at once; a Turn off is in force at once and waits for its reply behind the press.
        let mut decided = std::pin::pin!(service.action(decision));
        let at_once = tokio::time::timeout(Duration::from_millis(200), &mut decided).await;
        cloud.holding_lists.store(false, Ordering::SeqCst);
        assert_eq!(press.await.err(), None, "{what}: the press is answered");
        if at_once.is_err() {
            let _ = decided.await;
        }
        assert!(
            held_at(&service, std::time::Instant::now()),
            "{what}: still held now"
        );
        assert!(
            held_at(&service, past_the_window()),
            "{what}: and past its window"
        );
    }
}

// ---- A shack waiver across a restart -------------------------------------------------------------

/// The owner's browser, its notice never confirmed, beside a computer added at the shack: the
/// service then answers an Approve with the list as it was, the device approved.
fn rows() -> Vec<serde_json::Value> {
    let mut shack = computer_row(&token("computer"), "operate");
    shack["addedBy"] = "shack".into();
    vec![notified_row(0), shack]
}
/// The devices the shack's waivers entry in `vault` names, once the vault writer (its own thread)
/// has written `want`; what it names after a while otherwise.
async fn waivers_become(vault: &MemoryVault, want: &[&str]) -> Vec<String> {
    let names = || -> Vec<String> {
        Vault::waivers(vault)
            .unwrap()
            .map(|w| w.devices.into_iter().collect())
            .unwrap_or_default()
    };
    for _ in 0..100 {
        if names() == want {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    names()
}
/// Launch again from `vault`, granting on a sign-in, and wait until the pairing has loaded.
async fn restarted(cloud: &FakeCloud, vault: &MemoryVault) -> Service {
    let service = launch_granting_on_sign_in(cloud, vault);
    eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
    service
}

/// ★ A WAIVER AT THE SHACK OUTLASTS A RESTART: a sign-in device whose owner notice the service never
/// confirmed is held with no end. Allow to transmit, or Approve, at the shack waives that hold, and
/// after the station restarts the device is still free once the service lists it as before.
/// CONTROL: with no waiver it is held after the restart.
#[tokio::test(flavor = "multi_thread")]
async fn a_waiver_given_at_the_shack_outlasts_a_restart() {
    for how in ["none", "allow", "approve"] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        let first = if how == "approve" {
            launch_not_granting_on_sign_in(&cloud, &vault)
        } else {
            launch_granting_on_sign_in(&cloud, &vault)
        };
        let _ = listed(&first, &cloud, rows()).await;
        assert!(held_at(&first, past_the_window()), "{how}: premise: held");
        match how {
            "allow" => {
                first.action(allow(BROWSER)).await.unwrap();
            }
            "approve" => {
                first
                    .action(Action::Device {
                        device_id: BROWSER.into(),
                        approve: true,
                        key: Some(sha256(&key("browser"))),
                        kind: Some(Kind::Browser),
                    })
                    .await
                    .unwrap();
            }
            _ => {}
        }
        let waived = how != "none";
        assert_eq!(!held_at(&first, past_the_window()), waived, "{how}: before");
        // Only a waiver is kept: a device held, here in no window at all, never is.
        let want: &[&str] = if waived { &[BROWSER] } else { &[] };
        assert_eq!(waivers_become(&vault, want).await, want, "{how}: kept");
        drop(first);

        let service = restarted(&cloud, &vault).await;
        let _ = listed(&service, &cloud, rows()).await;
        assert_eq!(
            !held_at(&service, past_the_window()),
            waived,
            "{how}: after the restart"
        );
    }
}

/// A sign-in browser allowed to transmit at the shack, its waiver kept in `vault`.
async fn allowed(cloud: &FakeCloud, vault: &MemoryVault) -> Service {
    let service = launch_granting_on_sign_in(cloud, vault);
    let _ = listed(&service, cloud, rows()).await;
    service.action(allow(BROWSER)).await.unwrap();
    assert_eq!(
        waivers_become(vault, &[BROWSER]).await,
        [BROWSER],
        "premise"
    );
    service
}

/// ★ FAIL-CLOSED ON WHAT WAS KEPT: a waivers entry that does not read, is out of shape, belongs to
/// another station, account or service, or names another device, restores no waiver, and the
/// device is held after the restart; nothing panics. CONTROL: the entry as the station kept it
/// frees the device.
#[tokio::test(flavor = "multi_thread")]
async fn a_waivers_entry_that_does_not_hold_restores_nothing() {
    type Entry = fn(&mut serde_json::Value) -> String;
    let cases: [(&str, Entry); 9] = [
        ("as kept", |w| w.to_string()),
        ("unreadable", |_| "{\"binding\":".into()),
        ("not JSON", |_| "waived".into()),
        ("devices not a list", |w| {
            w["devices"] = "all".into();
            w.to_string()
        }),
        ("a field it does not know", |w| {
            w["held"] = json!([]);
            w.to_string()
        }),
        ("another station", |w| {
            w["binding"]["stationId"] = "20000000-0000-4000-8000-000000000001".into();
            w.to_string()
        }),
        ("another account", |w| {
            w["binding"]["accountId"] = "30000000-0000-4000-8000-000000000001".into();
            w.to_string()
        }),
        ("another device", |w| {
            w["devices"] = json!(["10000000-0000-4000-8000-0000000000ff"]);
            w.to_string()
        }),
        ("not a device id", |w| {
            w["devices"] = json!(["10000000-0000-4000-8000-00000000000b", "not an id"]);
            w.to_string()
        }),
    ];
    for (what, entry) in cases {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        drop(allowed(&cloud, &vault).await);
        let mut kept: serde_json::Value =
            serde_json::from_str(&vault.values.lock().unwrap()["waivers"]).unwrap();
        let entry = entry(&mut kept);
        vault.values.lock().unwrap().insert("waivers".into(), entry);
        let service = restarted(&cloud, &vault).await;
        let _ = listed(&service, &cloud, rows()).await;
        assert_eq!(
            held_at(&service, past_the_window()),
            what != "as kept",
            "{what}"
        );
        // Nor is it left in the vault for a later restart to read.
        if what != "as kept" {
            assert!(waivers_entry_removed(&vault).await, "{what}: entry removed");
        }
    }
}

/// Whether `vault` holds no waivers entry at all, once the vault writer has had a while to remove it.
async fn waivers_entry_removed(vault: &MemoryVault) -> bool {
    for _ in 0..100 {
        if !vault.values.lock().unwrap().contains_key("waivers") {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}
/// Whether the station holds any transmit record for the owner's browser: before a list, with no
/// window kept for it, only a waiver restored at launch.
fn recorded(service: &Service) -> bool {
    service
        .control
        .lock()
        .unwrap()
        .operations
        .transmit_windows(
            &std::collections::BTreeSet::from([BROWSER.to_string()]),
            std::time::Instant::now(),
        )
        .contains_key(BROWSER)
}
/// Change the state entry kept in `vault`.
fn edit_state(vault: &MemoryVault, edit: impl FnOnce(&mut serde_json::Value)) {
    let mut values = vault.values.lock().unwrap();
    let mut state: serde_json::Value = serde_json::from_str(&values["state"]).unwrap();
    edit(&mut state);
    values.insert("state".into(), state.to_string());
}

/// ★ CHECKED AGAIN AT LOAD AGAINST WHAT THE STATION REMEMBERS: a kept waiver restores nothing for a
/// device whose removal here still waits for the service, nor for one whose station control the
/// station no longer remembers, its pin kept or not. Nothing is restored at launch, the entry is
/// rewritten to what was restored, and the device is held after the list. Remote is off at the
/// restart and nothing is granted on a sign-in, so no list or grant can stand in for the check.
#[tokio::test(flavor = "multi_thread")]
async fn a_kept_waiver_is_checked_against_the_device_at_load() {
    for what in [
        "as kept",
        "removal waiting",
        "nothing granted",
        "only a pin",
    ] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        drop(allowed(&cloud, &vault).await);
        match what {
            "removal waiting" => {
                vault
                    .save_revokes(&vault::Revokes {
                        station_id: STATION.into(),
                        devices: vec![BROWSER.into()],
                    })
                    .unwrap();
                cloud.refusing_removals.store(true, Ordering::SeqCst);
            }
            "nothing granted" => {
                vault.values.lock().unwrap().remove("pins");
                edit_state(&vault, |state| state["grants"] = json!([]));
            }
            "only a pin" => edit_state(&vault, |state| state["grants"] = json!([])),
            _ => {}
        }
        edit_state(&vault, |state| state["enabled"] = false.into());
        let kept = what == "as kept";
        let service = launch_not_granting_on_sign_in(&cloud, &vault);
        eventually(&service, "the pairing loaded", |s| s.station_id.is_some()).await;
        assert_eq!(recorded(&service), kept, "{what}: restored at launch");
        if kept {
            assert_eq!(waivers_become(&vault, &[BROWSER]).await, [BROWSER]);
        } else {
            assert!(waivers_entry_removed(&vault).await, "{what}: entry removed");
        }
        let _ = listed(&service, &cloud, rows()).await;
        assert_eq!(held_at(&service, past_the_window()), !kept, "{what}");
    }
}

/// ★ WHAT ENDS AN APPROVAL ENDS ITS KEPT WAIVER: revoking the browser's station control, removing
/// it, a list that no longer names it as a sign-in device (added at the shack, expired, or not
/// approved), Cancel, and Revoke station access each take the waiver out of what the station keeps,
/// so a restart cannot bring it back; where the pairing remains, the device is held after the
/// restart. Turn off only pauses, as it pauses the grants: the waiver is kept (the control).
#[tokio::test(flavor = "multi_thread")]
async fn whatever_ends_an_approval_ends_its_kept_waiver() {
    let relisted = |field: &'static str, value: serde_json::Value| {
        move || {
            let mut rows = rows();
            rows[0][field] = value.clone();
            rows
        }
    };
    for what in [
        "Turn off",
        "revoke station control",
        "remove",
        "remove, the service unreachable",
        "added at the shack",
        "expired",
        "not approved",
        "Cancel",
        "Revoke station access",
    ] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        let service = allowed(&cloud, &vault).await;
        let remove = Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            key: None,
            kind: None,
        };
        match what {
            "Turn off" => {
                service.action(Action::Disable {}).await.unwrap();
            }
            "revoke station control" => {
                service
                    .action(Action::StationPermission {
                        device_id: BROWSER.into(),
                        allow: false,
                    })
                    .await
                    .unwrap();
            }
            "remove" => {
                service.action(remove).await.unwrap();
            }
            "remove, the service unreachable" => {
                cloud.refusing_removals.store(true, Ordering::SeqCst);
                service.action(remove).await.unwrap();
            }
            "Cancel" => {
                let _ = service.action(Action::Cancel {}).await;
            }
            "Revoke station access" => {
                service.action(Action::Forget {}).await.unwrap();
            }
            listed_as => {
                let rows = match listed_as {
                    "added at the shack" => relisted("addedBy", "shack".into())(),
                    "expired" => relisted("expiresAt", 1.into())(),
                    _ => relisted("approved", 0.into())(),
                };
                *cloud.devices.lock().unwrap() = json!({ "devices": rows }).to_string();
                service.action(Action::Refresh {}).await.unwrap();
            }
        }
        let kept = what == "Turn off";
        let want: &[&str] = if kept { &[BROWSER] } else { &[] };
        assert_eq!(waivers_become(&vault, want).await, want, "{what}: kept");
        drop(service);
        if what == "Revoke station access" {
            continue;
        }
        let service = restarted(&cloud, &vault).await;
        let _ = listed(&service, &cloud, rows()).await;
        assert_eq!(
            held_at(&service, past_the_window()),
            !kept,
            "{what}: after the restart"
        );
    }
}

/// ★ A WAIVER LEFT BEHIND IS NEVER RESTORED BY A GRANT GIVEN WITHOUT THE SHACK: the station kept a
/// waiver, but not the grant or the pin it came with (a crash between the vault's writes, a write
/// the store refused, an older build). After a restart the list grants the device again on its own
/// sign-in, held for its window, and the waiver is gone: held after this restart and the next.
/// CONTROL: kept as it was, the waiver frees the device after both.
#[tokio::test(flavor = "multi_thread")]
async fn a_waiver_left_behind_is_never_restored_by_a_grant_given_without_the_shack() {
    for what in ["as kept", "grant and pin lost", "pin lost"] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        drop(allowed(&cloud, &vault).await);
        match what {
            "grant and pin lost" => {
                vault.values.lock().unwrap().remove("pins");
                edit_state(&vault, |state| state["grants"] = json!([]));
            }
            "pin lost" => {
                vault.values.lock().unwrap().remove("pins");
            }
            _ => {}
        }
        let kept = what == "as kept";
        for restart in ["first", "second"] {
            let service = restarted(&cloud, &vault).await;
            let _ = listed(&service, &cloud, rows()).await;
            assert_eq!(
                held_at(&service, past_the_window()),
                !kept,
                "{what}: {restart} restart"
            );
            if !kept {
                assert!(
                    waivers_entry_removed(&vault).await,
                    "{what}: {restart} restart: entry removed"
                );
            }
        }
    }
}

/// ★ A RESTRICTED DEVICE IS NOT FREED BY A WAIVER LEFT BEHIND: restricting a sign-in device's station
/// control ends its waiver but keeps its pin. Should the waiver outlive the restrict (a crash
/// between the vault's writes, or the store refusing to remove it), the restart restores nothing
/// from it, allowing station control again frees nothing, and the next restart is held too.
#[tokio::test(flavor = "multi_thread")]
async fn a_restricted_device_is_not_freed_by_a_waiver_left_behind() {
    for what in ["crash", "the store refusing"] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        let service = allowed(&cloud, &vault).await;
        let entry = vault.values.lock().unwrap()["waivers"].clone();
        if what == "the store refusing" {
            vault.refusing.lock().unwrap().insert("waivers");
        }
        service
            .action(Action::StationPermission {
                device_id: BROWSER.into(),
                allow: false,
            })
            .await
            .unwrap();
        // Kept as no grant remembered: the state saved without it, or, while the store refuses to
        // take the waiver back, removed.
        let kept = || vault.state().unwrap().is_none_or(|s| s.grants.is_empty());
        for _ in 0..100 {
            if kept() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(kept(), "{what}: premise: the restrict was kept");
        drop(service);
        if what == "crash" {
            vault.values.lock().unwrap().insert("waivers".into(), entry);
        } else {
            assert_eq!(
                vault.values.lock().unwrap()["waivers"],
                entry,
                "premise: the waiver outlived the restrict"
            );
            vault.refusing.lock().unwrap().clear();
        }
        assert!(
            vault.pins().unwrap().unwrap().keys.contains_key(BROWSER),
            "{what}: premise: the pin is kept"
        );

        let service = restarted(&cloud, &vault).await;
        let _ = listed(&service, &cloud, rows()).await;
        assert!(held_at(&service, past_the_window()), "{what}: restarted");
        service
            .action(Action::StationPermission {
                device_id: BROWSER.into(),
                allow: true,
            })
            .await
            .unwrap();
        assert!(
            held_at(&service, past_the_window()),
            "{what}: allowed station control again"
        );
        drop(service);
        let service = restarted(&cloud, &vault).await;
        let _ = listed(&service, &cloud, rows()).await;
        assert!(
            held_at(&service, past_the_window()),
            "{what}: the next restart"
        );
    }
}

/// ★ A RESTRICT WHOSE WAIVER THE STORE REFUSED TO TAKE BACK, ALLOWED AGAIN IN THE SAME RUN, FREES
/// NOTHING AT THE NEXT LAUNCH: the waiver stays in the store, so the station control allowed again
/// is never saved beside it, and after a restart, with the store taking the entry again, the device
/// is held.
#[tokio::test(flavor = "multi_thread")]
async fn a_restrict_the_store_refused_then_allowed_again_frees_nothing_after_a_restart() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = allowed(&cloud, &vault).await;
    vault.refusing.lock().unwrap().insert("waivers");
    for allow in [false, true] {
        service
            .action(Action::StationPermission {
                device_id: BROWSER.into(),
                allow,
            })
            .await
            .unwrap();
    }
    assert!(held_at(&service, past_the_window()), "premise: held now");
    // Until the writer has written the allow: the state removed, or (as it was) saved with control.
    for _ in 0..100 {
        if vault
            .state()
            .unwrap()
            .is_none_or(|s| s.grants.iter().any(|g| g.control))
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    drop(service);
    vault.refusing.lock().unwrap().clear();
    assert!(
        vault.values.lock().unwrap().contains_key("waivers"),
        "premise: the waiver outlived the restrict"
    );

    let service = restarted(&cloud, &vault).await;
    let _ = listed(&service, &cloud, rows()).await;
    assert!(
        held_at(&service, past_the_window()),
        "freed after the restart"
    );
}

/// ★ A DEVICE APPROVED AGAIN LOSES ITS KEPT WAIVER WITH ITS GRANT: after a restart the service lists
/// the device at another approval, so the grant kept for it is not given back, and the waiver kept
/// beside that grant goes with it: held, also once allowed station control again. CONTROL: listed
/// at the same approval, the waiver is kept.
#[tokio::test(flavor = "multi_thread")]
async fn a_device_approved_again_loses_its_kept_waiver_with_its_grant() {
    for generation in [2, 3] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        drop(allowed(&cloud, &vault).await);
        let mut rows = rows();
        rows[0]["generation"] = generation.into();
        *cloud.devices.lock().unwrap() = json!({ "devices": rows.clone() }).to_string();
        let service = restarted(&cloud, &vault).await;
        let _ = listed(&service, &cloud, rows).await;
        let kept = generation == 2;
        let want: &[&str] = if kept { &[BROWSER] } else { &[] };
        assert_eq!(
            waivers_become(&vault, want).await,
            want,
            "generation {generation}: kept"
        );
        if !kept {
            service
                .action(Action::StationPermission {
                    device_id: BROWSER.into(),
                    allow: true,
                })
                .await
                .unwrap();
        }
        assert_eq!(
            held_at(&service, past_the_window()),
            !kept,
            "generation {generation}"
        );
    }
}

/// ★ A STATE THAT WILL NOT READ AT LOAD RESTORES NO WAIVER AND KEEPS NONE: Remote stays off, and the
/// waivers entry is removed, so a later launch that reads the state cannot restore it either.
#[tokio::test(flavor = "multi_thread")]
async fn a_state_that_will_not_read_removes_the_kept_waivers() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    drop(allowed(&cloud, &vault).await);
    vault.refusing.lock().unwrap().insert("state");
    let service = launch_granting_on_sign_in(&cloud, &vault);
    assert!(waivers_entry_removed(&vault).await, "entry removed");
    assert!(!recorded(&service), "nothing restored");
}

// ---- Remove at the shack ends every road -------------------------------------------------------

/// The fixture's computer in the LAN book as well (`DESKTOP-7Q`, by the LAN key its proof names),
/// with `others` paired beside it, and the service granting it on its own sign-in and listing
/// `rows`: the station, its LAN store, and the computer's LAN id.
async fn on_both_roads(
    cloud: &FakeCloud,
    vault: &MemoryVault,
    others: &[(&[u8; 32], &str)],
    rows: Vec<serde_json::Value>,
) -> (
    Service,
    crate::remote_service::lan::tests::LanStore,
    crate::remote_service::lan::tests::Scratch,
    String,
) {
    on_both_roads_at(cloud, vault, others, rows, fixture_now).await
}
/// [`on_both_roads`], the station's clock `now_s`.
async fn on_both_roads_at(
    cloud: &FakeCloud,
    vault: &MemoryVault,
    others: &[(&[u8; 32], &str)],
    rows: Vec<serde_json::Value>,
    now_s: fn() -> u64,
) -> (
    Service,
    crate::remote_service::lan::tests::LanStore,
    crate::remote_service::lan::tests::Scratch,
    String,
) {
    use crate::remote_service::lan::tests::{LanStore, Scratch};
    let pin = computer_lan_pin();
    let (_, station) = station_key::Signer::generate().unwrap();
    let mut paired = vec![(&pin, "DESKTOP-7Q")];
    paired.extend_from_slice(others);
    let store = LanStore::holding(&station, "60000000-0000-4000-8000-0000000000aa", &paired);
    let scratch = Scratch::new();
    let service = relaunched(cloud, vault, &store, &scratch, now_s);
    eventually(&service, "the book read", |s| {
        s.lan
            .as_ref()
            .is_some_and(|lan| lan.devices.len() == paired.len())
    })
    .await;
    let _ = listed(&service, cloud, rows).await;
    (service, store, scratch, lan::device_id(&pin))
}
/// The station granting on a sign-in, on `vault` and the LAN `store`, its clock `now_s`: launched,
/// or launched again as after a restart.
fn relaunched(
    cloud: &FakeCloud,
    vault: &MemoryVault,
    store: &crate::remote_service::lan::tests::LanStore,
    scratch: &crate::remote_service::lan::tests::Scratch,
    now_s: fn() -> u64,
) -> Service {
    let (bind, _) = binder(Vec::new());
    let accounts = Accounts {
        grants_on_sign_in: true,
        now_s,
        ..accounts(vault, bind)
    };
    super::lan_grants::with_lan(
        launch_with(cloud, vault, accounts),
        &engine(),
        store,
        scratch,
    )
}
/// The fixture's computer's LAN key's pin.
fn computer_lan_pin() -> [u8; 32] {
    tempo_stream::protocol::hex_bytes(&sha256(&key("computerLan")))
        .unwrap()
        .try_into()
        .unwrap()
}
/// What the station keeps of the roads' links and of each removal (`lan::enrol`).
fn ties_kept(vault: &MemoryVault) -> Ties {
    vault.ties().unwrap().unwrap_or_default()
}
/// Keeps `ties` as the station's links, and its copy in memory follows.
fn keep_ties(service: &Service, vault: &MemoryVault, ties: Vec<Tie>) {
    let mut kept = ties_kept(vault);
    kept.ties.extend(ties);
    vault.save_account_entry("account-ties", &kept).unwrap();
    service.control.lock().unwrap().enrolment.reread();
}
/// The LAN ids the book holds now.
fn in_book(status: &Status) -> Vec<String> {
    status
        .lan
        .as_ref()
        .map(|lan| lan.devices.iter().map(|d| d.id.clone()).collect())
        .unwrap_or_default()
}
/// The service ids the fake service lists now.
fn listed_ids(cloud: &FakeCloud) -> Vec<String> {
    let listed: serde_json::Value = serde_json::from_str(&cloud.devices.lock().unwrap()).unwrap();
    listed["devices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["id"].as_str().unwrap().to_string())
        .collect()
}
/// The devices the station keeps a pin for now: what a list read after a removal cannot show.
fn pinned(service: &Service) -> Vec<String> {
    let control = service.control.lock().unwrap();
    control.remembered.pins.keys().cloned().collect()
}
/// The names the room's tunnel takes now.
fn on_tunnel(service: &Service) -> Vec<String> {
    service.control.lock().unwrap().approved.names()
}

/// ★ REMOVE IN PAIRED COMPUTERS ENDS ITS INTERNET ROAD TOO: a computer on the LAN and, by its own
/// sign-in, on the internet (its proof names both keys), allowed to transmit at the shack. Remove
/// at the shack takes it out of the LAN book, out of the room's tunnel, its pin and its waiver, and
/// tells the service, which no longer lists it.
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_paired_computer_ends_its_internet_road_too() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let (service, _store, _scratch, lan_id) = on_both_roads(
        &cloud,
        &vault,
        &[],
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    service.action(allow(COMPUTER_ID)).await.unwrap();
    let named = lan::tunnel_device_id(&road_pin());
    let status = service.status().unwrap();
    assert_eq!(
        status.pinned_devices,
        vec![COMPUTER_ID.to_string()],
        "premise"
    );
    assert_eq!(on_tunnel(&service), vec![named.clone()], "premise");
    let operations = service.control.lock().unwrap().operations.clone();
    assert!(
        !operations.tx_held(COMPUTER_ID, past_the_window()),
        "premise: its waiver"
    );

    let status = service
        .action(Action::LanRevoke { device_id: lan_id })
        .await
        .unwrap();
    assert!(in_book(&status).is_empty(), "still on the LAN");
    assert!(pinned(&service).is_empty(), "still pinned");
    assert!(on_tunnel(&service).is_empty(), "still on the tunnel");
    // Read before any list could grant it again: its waiver ended with the Remove.
    assert!(
        operations.tx_held(COMPUTER_ID, past_the_window()),
        "its waiver kept"
    );
    assert!(listed_ids(&cloud).is_empty(), "the service was not told");
}

/// ★ REMOVE IN YOUR COMPUTERS ENDS ITS ROAD ON THIS NETWORK TOO: the same computer, removed from the
/// account's list at the shack, leaves the LAN book at once, in memory and in the store. CONTROL:
/// another computer paired by code stays.
#[tokio::test(flavor = "multi_thread")]
async fn removing_a_listed_computer_ends_its_road_on_this_network_too() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let other: [u8; 32] = [7; 32];
    let (service, store, _scratch, lan_id) = on_both_roads(
        &cloud,
        &vault,
        &[(&other, "Den PC")],
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    let removed = service
        .action(Action::Device {
            device_id: COMPUTER_ID.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await;
    let status = service.status().unwrap();
    assert_eq!(
        in_book(&status),
        vec![lan::device_id(&other)],
        "still on the LAN, or the other one gone"
    );
    let kept = store.devices.lock().unwrap().clone().unwrap();
    assert!(!kept.contains(&sha256(&key("computerLan"))), "still kept");
    assert!(!in_book(&status).contains(&lan_id));
    assert!(listed_ids(&cloud).is_empty());
    assert_eq!(removed.err(), None);
}

/// ★ A COMPUTER REMOVED AT THE SHACK COMES BACK HELD: allowed to transmit, then removed in Paired
/// computers, it signs in again and the service lists it again. It is granted again on that
/// sign-in and held from transmit, under its service id and its tunnel name, and it is not back in
/// the LAN book: nothing the shack waived before follows it back. The station's clock reads the
/// removal as made before the fixture's proof was signed, past the clocks' leeway (a removal reads
/// it on the side that refuses), so the proof is the new sign-in's.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_removed_at_the_shack_comes_back_held_when_it_signs_in_again() {
    static CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let before_signing =
        cases()["issuedAt"].as_u64().unwrap() - tempo_client::id_token::LEEWAY_S - 1;
    // Listed again under the approval it had, or under a new one.
    for generation in [3, 4] {
        CLOCK.store(fixture_now(), Ordering::SeqCst);
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        let row = computer_row(&token("computer"), "operate");
        let (service, _store, _scratch, lan_id) =
            on_both_roads_at(&cloud, &vault, &[], vec![row.clone()], clock).await;
        service.action(allow(COMPUTER_ID)).await.unwrap();
        let operations = service.control.lock().unwrap().operations.clone();
        let named = lan::tunnel_device_id(&road_pin());
        assert!(!operations.tx_held(&named, past_the_window()), "premise");
        CLOCK.store(before_signing, Ordering::SeqCst);
        service
            .action(Action::LanRevoke { device_id: lan_id })
            .await
            .unwrap();
        CLOCK.store(fixture_now(), Ordering::SeqCst);

        // Signed in again: approved on its proof.
        let mut again = row;
        again["generation"] = generation.into();
        *cloud.devices.lock().unwrap() = json!({ "devices": [again] }).to_string();
        let status = service.action(Action::Refresh {}).await.unwrap();
        assert_eq!(
            status.pinned_devices,
            vec![COMPUTER_ID.to_string()],
            "generation {generation}: granted again on its sign-in"
        );
        assert!(
            in_book(&status).is_empty(),
            "generation {generation}: back on the LAN"
        );
        let now = std::time::Instant::now();
        assert!(
            operations.tx_held(COMPUTER_ID, now),
            "generation {generation}: free under its id"
        );
        assert!(
            operations.tx_held(&named, now),
            "generation {generation}: free on the tunnel"
        );
    }
}

/// ★ A ROAD THAT CANNOT BE ENDED NOW LEAVES LESS ACCESS, NEVER MORE: with the service down for
/// removals, Remove in Paired computers still takes the computer off the LAN and off the tunnel at
/// once and keeps its removal waiting, so the next list grants nothing on its proof. With the LAN's
/// store locked, Remove in Your computers still takes it out of the LAN in memory and tells the
/// service, and the shack is shown the store's failure.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_that_cannot_end_every_road_now_leaves_less_access() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let row = computer_row(&token("computer"), "operate");
    let (service, _store, _scratch, lan_id) =
        on_both_roads(&cloud, &vault, &[], vec![row.clone()]).await;
    cloud.refusing_removals.store(true, Ordering::SeqCst);
    let status = service
        .action(Action::LanRevoke { device_id: lan_id })
        .await
        .unwrap();
    assert!(in_book(&status).is_empty());
    assert_eq!(status.account.removals, vec![COMPUTER_ID.to_string()]);
    assert!(pinned(&service).is_empty());
    assert!(on_tunnel(&service).is_empty());
    service.action(Action::Refresh {}).await.unwrap();
    assert!(pinned(&service).is_empty(), "granted again meanwhile");
    assert!(on_tunnel(&service).is_empty());

    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let (service, store, _scratch, lan_id) = on_both_roads(&cloud, &vault, &[], vec![row]).await;
    store.locked.store(true, Ordering::SeqCst);
    let removed = service
        .action(Action::Device {
            device_id: COMPUTER_ID.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await;
    assert_eq!(removed.err(), Some("credentialStoreUnavailable"));
    let status = service.status().unwrap();
    assert!(!in_book(&status).contains(&lan_id), "still on the LAN");
    assert!(pinned(&service).is_empty());
    assert!(listed_ids(&cloud).is_empty(), "the service was not told");
}

/// ★ ONLY A LINK THE STATION KEPT LINKS THE ROADS: a computer the service lists with the LAN key of
/// one in the book, but no proof that names both, is not taken to be that computer. Remove in Paired
/// computers takes the LAN computer out and leaves the listed one as it is, and the shack is told to
/// look there too: by its key alone, since the two are named otherwise.
#[tokio::test(flavor = "multi_thread")]
async fn a_listing_no_proof_links_is_left_and_the_shack_told() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    // Named otherwise than in the book: only the key it is listed with says it may be the same.
    let mut row = computer_row(&token("browser"), "operate");
    row["name"] = "LAPTOP-3".into();
    let (service, _store, _scratch, lan_id) = on_both_roads(&cloud, &vault, &[], vec![row]).await;
    let removed = service
        .action(Action::LanRevoke { device_id: lan_id })
        .await;
    assert_eq!(listed_ids(&cloud), vec![COMPUTER_ID.to_string()], "guessed");
    assert!(in_book(&service.status().unwrap()).is_empty());
    assert_eq!(removed.err(), Some("removeElsewhere"));
}

/// ★ ONE COMPUTER LISTED TWICE GOES AT ONCE: two listings the room's tunnel takes by one key (one
/// name) are one computer's, and Remove of either ends both.
#[tokio::test(flavor = "multi_thread")]
async fn one_computer_listed_twice_is_removed_from_both_listings() {
    const TWICE: &str = "10000000-0000-4000-8000-00000000000f";
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let mut second = computer_row(&token("computer"), "operate");
    second["id"] = TWICE.into();
    let status = listed(
        &service,
        &cloud,
        vec![computer_row(&token("computer"), "operate"), second],
    )
    .await;
    assert_eq!(status.pinned_devices.len(), 2, "premise");
    service
        .action(Action::Device {
            device_id: COMPUTER_ID.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert!(pinned(&service).is_empty(), "{:?}", pinned(&service));
    assert!(on_tunnel(&service).is_empty());
    assert!(listed_ids(&cloud).is_empty(), "{:?}", listed_ids(&cloud));
}

/// Reset network identity removes every paired computer, and with each its internet road.
#[tokio::test(flavor = "multi_thread")]
async fn resetting_the_network_identity_ends_each_computers_internet_road() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let (service, _store, _scratch, _) = on_both_roads(
        &cloud,
        &vault,
        &[],
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    let status = service.action(Action::LanReset {}).await.unwrap();
    assert!(in_book(&status).is_empty());
    assert!(pinned(&service).is_empty());
    assert!(on_tunnel(&service).is_empty());
    assert!(listed_ids(&cloud).is_empty());
}

/// ★ AN OLD PROOF BRINGS A REMOVED COMPUTER BACK ON NO ROAD: removed in Paired computers, the
/// computer's listing comes back with the proof it had, signed before the removal. Nothing is
/// granted on it, and its removal is kept for the service, which no longer lists it after the next
/// list. Only a sign-in after the removal brings it back
/// (`a_computer_removed_at_the_shack_comes_back_held_when_it_signs_in_again`).
#[tokio::test(flavor = "multi_thread")]
async fn an_old_proof_brings_a_removed_computer_back_on_no_road() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let row = computer_row(&token("computer"), "operate");
    let (service, _store, _scratch, lan_id) =
        on_both_roads(&cloud, &vault, &[], vec![row.clone()]).await;
    service
        .action(Action::LanRevoke { device_id: lan_id })
        .await
        .unwrap();
    let mut again = row;
    again["generation"] = 4.into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [again] }).to_string();
    let status = service.action(Action::Refresh {}).await.unwrap();
    assert!(
        pinned(&service).is_empty(),
        "granted again on its old proof"
    );
    assert!(on_tunnel(&service).is_empty());
    assert_eq!(status.account.removals, vec![COMPUTER_ID.to_string()]);
    service.action(Action::Refresh {}).await.unwrap();
    assert!(listed_ids(&cloud).is_empty(), "the service was not told");
}

/// ★ WITH NO LIST HEARD, REMOVE STILL ENDS BOTH ROADS AND SAYS SO: after a restart with the service
/// unreachable, the station has no list. Remove in Paired computers takes the computer off the LAN
/// and tells the shack to check the other list. Its internet road, still pinned from before the
/// restart, ends at the first list the station hears, before the tunnel takes it, and the service
/// is told at the next.
#[tokio::test(flavor = "multi_thread")]
async fn with_no_list_heard_a_paired_computer_is_removed_on_both_roads_and_the_shack_told() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let (service, store, scratch, lan_id) = on_both_roads(
        &cloud,
        &vault,
        &[],
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    assert_eq!(pinned(&service), vec![COMPUTER_ID.to_string()], "premise");
    drop(service);
    cloud.refusing.store(true, Ordering::SeqCst);
    let service = relaunched(&cloud, &vault, &store, &scratch, fixture_now);
    eventually(&service, "the book read", |s| {
        s.lan.as_ref().is_some_and(|lan| lan.devices.len() == 1)
    })
    .await;
    assert_eq!(pinned(&service), vec![COMPUTER_ID.to_string()], "premise");

    let removed = service
        .action(Action::LanRevoke { device_id: lan_id })
        .await;
    assert_eq!(removed.err(), Some("removeElsewhere"), "no list, and Ok");
    assert!(in_book(&service.status().unwrap()).is_empty());

    cloud.refusing.store(false, Ordering::SeqCst);
    service.action(Action::Refresh {}).await.unwrap();
    assert!(pinned(&service).is_empty(), "still pinned");
    assert!(on_tunnel(&service).is_empty(), "on the tunnel");
    service.action(Action::Refresh {}).await.unwrap();
    assert!(listed_ids(&cloud).is_empty(), "the service was not told");
}

/// ★ A REMOVAL THE CONTROLLER COULD NOT TAKE IS NOT LOST: with the controller busy on a list and
/// another command waiting, Remove in Paired computers is answered `remoteBusy`, but the computer is
/// off the LAN and unpinned at once, and the list that was under way grants nothing on its proof:
/// its removal reaches the service at the next list.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_the_controller_could_not_take_is_not_lost() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let (service, _store, _scratch, lan_id) = on_both_roads(
        &cloud,
        &vault,
        &[],
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    cloud.holding_lists.store(true, Ordering::SeqCst);
    let mut busy = std::pin::pin!(service.action(Action::Refresh {}));
    assert!(
        tokio::time::timeout(Duration::from_millis(200), &mut busy)
            .await
            .is_err(),
        "premise: the controller waits on the service's list"
    );
    let mut waiting = std::pin::pin!(service.action(Action::Refresh {}));
    let _ = tokio::time::timeout(Duration::from_millis(100), &mut waiting).await;
    let removed = service
        .action(Action::LanRevoke { device_id: lan_id })
        .await;
    assert_eq!(removed.err(), Some("remoteBusy"), "premise");
    assert!(in_book(&service.status().unwrap()).is_empty());
    assert!(pinned(&service).is_empty());
    cloud.holding_lists.store(false, Ordering::SeqCst);
    let _ = busy.await;
    let _ = waiting.await;
    assert!(pinned(&service).is_empty(), "granted again on its proof");
    assert!(on_tunnel(&service).is_empty());
    service.action(Action::Refresh {}).await.unwrap();
    assert!(pinned(&service).is_empty());
    assert!(listed_ids(&cloud).is_empty(), "the service was never told");
}

/// ★ BOTH LISTINGS OF ONE COMPUTER END AT ONCE: two listings the room's tunnel takes by one key.
/// While the service is still being told of the first, the second is already unpinned and off the
/// tunnel: no road waits on the service.
#[tokio::test(flavor = "multi_thread")]
async fn both_listings_of_one_computer_end_before_the_service_is_told() {
    const TWICE: &str = "10000000-0000-4000-8000-00000000000f";
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let mut second = computer_row(&token("computer"), "operate");
    second["id"] = TWICE.into();
    let status = listed(
        &service,
        &cloud,
        vec![computer_row(&token("computer"), "operate"), second],
    )
    .await;
    assert_eq!(status.pinned_devices.len(), 2, "premise");
    cloud.holding_lists.store(true, Ordering::SeqCst);
    let mut busy = std::pin::pin!(service.action(Action::Refresh {}));
    assert!(
        tokio::time::timeout(Duration::from_millis(200), &mut busy)
            .await
            .is_err(),
        "premise: the controller waits on the service's list"
    );
    let mut press = std::pin::pin!(service.action(Action::Device {
        device_id: COMPUTER_ID.into(),
        approve: false,
        key: None,
        kind: None,
    }));
    assert!(
        tokio::time::timeout(Duration::from_millis(200), &mut press)
            .await
            .is_err(),
        "premise: the press waits on the service"
    );
    let (pins, tunnel) = (pinned(&service), on_tunnel(&service));
    cloud.holding_lists.store(false, Ordering::SeqCst);
    let _ = busy.await;
    let _ = press.await;
    assert!(pins.is_empty(), "{pins:?}");
    assert!(tunnel.is_empty(), "{tunnel:?}");
}

/// ★ A LINK OF A LINK IS FOLLOWED: two computers in the LAN book whose proofs named one internet
/// key, and that key's listing. Remove of the second in Paired computers ends the listing, and
/// through it the first.
#[tokio::test(flavor = "multi_thread")]
async fn a_remove_follows_a_link_of_a_link() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let other: [u8; 32] = [7; 32];
    let (service, _store, _scratch, first) = on_both_roads(
        &cloud,
        &vault,
        &[(&other, "Den PC")],
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    keep_ties(
        &service,
        &vault,
        vec![Tie {
            lan: short(&tempo_client::hex(&other)),
            key: short(&sha256(&key("road"))),
            mailbox: None,
            enrolled: true,
            vouched: false,
        }],
    );
    let status = service
        .action(Action::LanRevoke {
            device_id: lan::device_id(&other),
        })
        .await
        .unwrap();
    assert!(
        !in_book(&status).contains(&first),
        "the first, linked through it, stays"
    );
    assert!(in_book(&status).is_empty());
    assert!(pinned(&service).is_empty());
    assert!(listed_ids(&cloud).is_empty());
}

/// ★ A LISTING WHOSE LAN KEY THE BOOK DOES NOT HOLD IS NOTHING TO CHECK: removed in Your
/// computers, a computer listed with a LAN key no computer in the book holds (and named otherwise)
/// is simply removed: the shack is not sent to look in the other list.
#[tokio::test(flavor = "multi_thread")]
async fn a_listing_whose_lan_key_the_book_does_not_hold_is_simply_removed() {
    use crate::remote_service::lan::tests::{LanStore, Scratch};
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let other: [u8; 32] = [7; 32];
    let (_, station) = station_key::Signer::generate().unwrap();
    let store = LanStore::holding(
        &station,
        "60000000-0000-4000-8000-0000000000aa",
        &[(&other, "Den PC")],
    );
    let scratch = Scratch::new();
    let service = relaunched(&cloud, &vault, &store, &scratch, fixture_now);
    eventually(&service, "the book read", |s| {
        s.lan.as_ref().is_some_and(|lan| lan.devices.len() == 1)
    })
    .await;
    // Its proof names a LAN key; none it names is in the book.
    let _ = listed(
        &service,
        &cloud,
        vec![computer_row(&token("browser"), "operate")],
    )
    .await;
    let removed = service
        .action(Action::Device {
            device_id: COMPUTER_ID.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await;
    assert_eq!(removed.err(), None);
    assert_eq!(in_book(&service.status().unwrap()).len(), 1);
}

/// ★ A GUEST'S LISTING THAT NAMES A COMPUTER'S LAN KEY IS NOT TAKEN FOR IT: a guest signs in
/// naming the LAN key of a computer paired at the shack. Remove of the guest's listing leaves that
/// computer in the book, and the shack is told to check the other list.
#[tokio::test(flavor = "multi_thread")]
async fn a_guests_listing_naming_a_paired_computers_key_does_not_remove_it() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(
        &cloud.origin,
        vec![Invite {
            mailbox: mailbox("K9.Guest@example.test"),
            role: Role::Operate,
        }],
    );
    let mut row = computer_row(&token("guest"), "operate");
    row["name"] = "GUEST-PC".into();
    let (service, _store, _scratch, lan_id) = on_both_roads(&cloud, &vault, &[], vec![row]).await;
    assert_eq!(pinned(&service), vec![COMPUTER_ID.to_string()], "premise");
    assert!(
        ties_kept(&vault).ties.iter().any(|t| t.mailbox.is_some()),
        "premise: its claim kept"
    );
    let removed = service
        .action(Action::Device {
            device_id: COMPUTER_ID.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await;
    assert_eq!(in_book(&service.status().unwrap()), vec![lan_id], "taken");
    assert_eq!(removed.err(), Some("removeElsewhere"));
}

/// ★ AN INVITATION TAKEN BACK TAKES THAT GUEST'S COMPUTERS OFF THE LAN AT ONCE, the service
/// unreachable: the one its link names as theirs, and a guest's computer kept with no link to say
/// whose. CONTROL: the owner's computer and one paired by code stay.
#[tokio::test(flavor = "multi_thread")]
async fn uninviting_a_guest_takes_their_computers_off_the_lan_at_once() {
    let cloud = fake_cloud().await;
    let guest = mailbox("K9.Guest@example.test");
    let vault = owned_vault(
        &cloud.origin,
        vec![Invite {
            mailbox: guest.clone(),
            role: Role::Operate,
        }],
    );
    let (theirs, unknown, by_code) = ([4u8; 32], [5u8; 32], [6u8; 32]);
    let (service, store, _scratch, owners) = on_both_roads(
        &cloud,
        &vault,
        &[
            (&theirs, "Guest PC"),
            (&unknown, "Old guest PC"),
            (&by_code, "Den PC"),
        ],
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    let mut cache = vault.device_cache().unwrap().unwrap();
    for pin in [theirs, unknown] {
        cache
            .keep(CachedDevice {
                pin: tempo_client::hex(&pin),
                role: Role::Operate,
                guest: true,
                issued_at: fixture_now() - 100,
                confirmed_at: 0,
            })
            .unwrap();
    }
    vault.save_account_entry("account-devices", &cache).unwrap();
    keep_ties(
        &service,
        &vault,
        vec![Tie {
            lan: short(&tempo_client::hex(&theirs)),
            key: "0123456789abcdef".into(),
            mailbox: Some(mailbox_tag(&guest)),
            enrolled: true,
            vouched: true,
        }],
    );
    cloud.refusing.store(true, Ordering::SeqCst);
    service
        .action(Action::Uninvite {
            email: "K9.Guest@example.test".into(),
        })
        .await
        .unwrap();
    let status = service.status().unwrap();
    let book = in_book(&status);
    assert_eq!(book, vec![owners, lan::device_id(&by_code)], "{book:?}");
    let kept = store.devices.lock().unwrap().clone().unwrap();
    assert!(!kept.contains(&tempo_client::hex(&theirs)), "still kept");
    // The shack is told of the one taken with no link to say it was theirs.
    assert_eq!(
        status.account.also_uninvited,
        vec!["Old guest PC".to_string()]
    );
}

/// ★ A REMOVAL THE LAN'S STORE DID NOT KEEP IS STILL A REMOVAL AFTER A RESTART: with the LAN's
/// store locked, Remove in Paired computers takes the computer out in memory and shows the store's
/// failure; the store still lists it. Launched again with the store unlocked, the computer is taken
/// out of the book at once, and the store no longer lists it.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_the_lan_store_did_not_keep_holds_after_a_restart() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let (service, store, scratch, lan_id) = on_both_roads(
        &cloud,
        &vault,
        &[],
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    store.locked.store(true, Ordering::SeqCst);
    let removed = service
        .action(Action::LanRevoke { device_id: lan_id })
        .await;
    assert_eq!(removed.err(), Some("credentialStoreUnavailable"), "premise");
    let lan_pin = sha256(&key("computerLan"));
    let kept = || store.devices.lock().unwrap().clone().unwrap_or_default();
    assert!(
        kept().contains(&lan_pin),
        "premise: the store still lists it"
    );
    drop(service);
    store.locked.store(false, Ordering::SeqCst);
    let service = relaunched(&cloud, &vault, &store, &scratch, fixture_now);
    eventually(&service, "the book read, and it taken out", |s| {
        s.lan
            .as_ref()
            .is_some_and(|lan| lan.key.is_some() && lan.devices.is_empty())
    })
    .await;
    assert!(!kept().contains(&lan_pin), "still in the store");
}

/// ★ APPROVED AGAIN AT THE SHACK, A REMOVED COMPUTER STAYS: removed, it asks again and the
/// operator approves it at the shack. Its key is the operator's again: the next list leaves it
/// pinned, and keeps no removal for it.
#[tokio::test(flavor = "multi_thread")]
async fn a_removed_computer_approved_again_at_the_shack_stays() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let row = computer_row(&token("computer"), "operate");
    let (service, _store, _scratch, _) =
        on_both_roads(&cloud, &vault, &[], vec![row.clone()]).await;
    service
        .action(Action::Device {
            device_id: COMPUTER_ID.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    let mut asking = row;
    asking["approved"] = 0.into();
    asking["addedBy"] = "shack".into();
    asking["proof"] = serde_json::Value::Null;
    *cloud.devices.lock().unwrap() = json!({ "devices": [asking] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    service
        .action(Action::Device {
            device_id: COMPUTER_ID.into(),
            approve: true,
            key: Some(sha256(&key("road"))),
            kind: Some(Kind::Computer),
        })
        .await
        .unwrap();
    assert_eq!(pinned(&service), vec![COMPUTER_ID.to_string()], "premise");
    let status = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(
        pinned(&service),
        vec![COMPUTER_ID.to_string()],
        "removed again"
    );
    assert!(status.account.removals.is_empty());
    assert_eq!(listed_ids(&cloud), vec![COMPUTER_ID.to_string()]);
}

/// ★ AN INVITATION TAKEN BACK GOES ONLY BY WHAT A COMPUTER PROVED: a guest signs in naming the LAN
/// key of the owner's computer paired by code (a claim the service does not check). Taking that
/// guest's invitation back leaves the computer in the book, with no removal kept for it, and tells
/// the shack of no other computer.
#[tokio::test(flavor = "multi_thread")]
async fn uninviting_a_guest_leaves_a_computer_their_listing_only_claims() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(
        &cloud.origin,
        vec![Invite {
            mailbox: mailbox("K9.Guest@example.test"),
            role: Role::Operate,
        }],
    );
    let mut row = computer_row(&token("guest"), "operate");
    row["name"] = "GUEST-PC".into();
    let (service, _store, _scratch, lan_id) = on_both_roads(&cloud, &vault, &[], vec![row]).await;
    let lan = short(&sha256(&key("computerLan")));
    assert!(
        ties_kept(&vault)
            .ties
            .iter()
            .any(|t| t.lan == lan && t.mailbox.is_some() && !t.enrolled),
        "premise: its claim kept"
    );
    cloud.refusing.store(true, Ordering::SeqCst);
    service
        .action(Action::Uninvite {
            email: "K9.Guest@example.test".into(),
        })
        .await
        .unwrap();
    let status = service.status().unwrap();
    assert_eq!(in_book(&status), vec![lan_id]);
    assert!(
        !ties_kept(&vault).removed.iter().any(|r| r.key == lan),
        "kept as removed"
    );
    assert!(status.account.also_uninvited.is_empty());
}

/// ★ NO GUEST'S CLAIM KEEPS ANOTHER GUEST'S COMPUTER ON THE LAN: a guest's computer kept with no
/// link to say whose, and a second guest's listing claiming its LAN key. Taking the first guest's
/// invitation back still takes it off the LAN, and the shack is told of it by name.
#[tokio::test(flavor = "multi_thread")]
async fn a_second_guests_claim_keeps_no_computer_on_the_lan_past_an_uninvite() {
    let cloud = fake_cloud().await;
    let (first, second) = (
        mailbox("K9.Guest@example.test"),
        mailbox("N0.Other@example.test"),
    );
    let vault = owned_vault(
        &cloud.origin,
        vec![
            Invite {
                mailbox: first.clone(),
                role: Role::Operate,
            },
            Invite {
                mailbox: second.clone(),
                role: Role::Operate,
            },
        ],
    );
    let theirs = [5u8; 32];
    let (service, _store, _scratch, owners) = on_both_roads(
        &cloud,
        &vault,
        &[(&theirs, "Old guest PC")],
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    let mut cache = vault.device_cache().unwrap().unwrap();
    cache
        .keep(CachedDevice {
            pin: tempo_client::hex(&theirs),
            role: Role::Operate,
            guest: true,
            issued_at: fixture_now() - 100,
            confirmed_at: 0,
        })
        .unwrap();
    vault.save_account_entry("account-devices", &cache).unwrap();
    keep_ties(
        &service,
        &vault,
        vec![Tie {
            lan: short(&tempo_client::hex(&theirs)),
            key: "0123456789abcdef".into(),
            mailbox: Some(mailbox_tag(&second)),
            enrolled: false,
            vouched: true,
        }],
    );
    cloud.refusing.store(true, Ordering::SeqCst);
    service
        .action(Action::Uninvite {
            email: "K9.Guest@example.test".into(),
        })
        .await
        .unwrap();
    let status = service.status().unwrap();
    assert_eq!(in_book(&status), vec![owners]);
    assert_eq!(
        status.account.also_uninvited,
        vec!["Old guest PC".to_string()]
    );
}

/// ★ A REMOVAL PUSHED OUT OF WHAT IS KEPT STILL REFUSES AN OLD PROOF ON THE INTERNET ROAD: the
/// computer removed in Paired computers, then as many other keys as are kept, so neither of its
/// removals fits. Its listing comes back with the proof it had: granted nothing, by the floor its
/// removals left, and shown with why.
#[tokio::test(flavor = "multi_thread")]
async fn a_removal_pushed_out_of_what_is_kept_still_refuses_an_old_proof_on_the_internet_road() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let row = computer_row(&token("computer"), "operate");
    let (service, _store, _scratch, lan_id) =
        on_both_roads(&cloud, &vault, &[], vec![row.clone()]).await;
    service
        .action(Action::LanRevoke { device_id: lan_id })
        .await
        .unwrap();
    let others: Vec<String> = (100..112u8).map(|i| tempo_client::hex(&[i; 32])).collect();
    let enrolment = service.control.lock().unwrap().enrolment.clone();
    enrolment.remove(&others).unwrap();
    let kept = ties_kept(&vault);
    let road = short(&sha256(&key("road")));
    assert!(
        kept.floor > 0 && !kept.removed.iter().any(|r| r.key == road),
        "premise: its removals pushed out"
    );
    let mut again = row;
    again["generation"] = 4.into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [again] }).to_string();
    let status = service.action(Action::Refresh {}).await.unwrap();
    assert!(
        pinned(&service).is_empty(),
        "granted again on its old proof"
    );
    assert!(on_tunnel(&service).is_empty());
    assert_eq!(
        status.account.unconfirmed.get(COMPUTER_ID).copied(),
        Some("tokenRemoved")
    );
}

/// ★ WHILE THE REMOVALS DO NOT READ, NO PROOF IS GRANTED: the station cannot tell whether a
/// listing's keys were removed at the shack, so it grants nothing on a proof and shows why.
/// CONTROL: once they read, the same listing is granted.
#[tokio::test(flavor = "multi_thread")]
async fn while_the_removals_do_not_read_no_proof_is_granted() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    vault.refusing.lock().unwrap().insert("account-ties");
    let (service, _store, _scratch, _) = on_both_roads(
        &cloud,
        &vault,
        &[],
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    let status = service.status().unwrap();
    assert!(pinned(&service).is_empty(), "granted, the removals unread");
    assert_eq!(
        status.account.unconfirmed.get(COMPUTER_ID).copied(),
        Some("credentialStoreUnavailable")
    );
    vault.refusing.lock().unwrap().clear();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(pinned(&service), vec![COMPUTER_ID.to_string()]);
}

/// ★ A RECORD OF REMOVALS THAT DOES NOT READ AT ALL IS CLEARED BY RESET NETWORK IDENTITY: while it
/// does not read, the LAN admits nobody, a computer paired by code included, and nothing writes
/// over it. Reset network identity writes it afresh with its floor now, so every proof signed
/// before is refused, and the LAN admits again: a computer paired by code after it. (A missing one reads as none:
/// `the_links_and_removals_kept_fit_the_smallest_credential_blob` and every test here start so.)
#[tokio::test(flavor = "multi_thread")]
async fn reset_network_identity_clears_a_record_of_removals_that_does_not_read() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    vault
        .values
        .lock()
        .unwrap()
        .insert("account-ties".into(), "{not a record".into());
    let by_code = [6u8; 32];
    let (service, _store, _scratch, _) =
        on_both_roads(&cloud, &vault, &[(&by_code, "Den PC")], Vec::new()).await;
    let enrolment = service.control.lock().unwrap().enrolment.clone();
    assert!(!enrolment.admits(&by_code), "premise: admitted, unread");
    service.action(Action::LanReset {}).await.unwrap();
    assert_eq!(ties_kept(&vault).floor, fixture_now());
    // The reset took the one paired out; one paired by code after it is admitted.
    assert!(enrolment.admits(&[7; 32]), "the LAN still admits nobody");
}

/// ★ A BROWSER KEEPS NO LINK AND NO REMOVAL FOR THIS NETWORK: it has no road here, so its sign-in
/// takes no place a computer's link needs, and Remove keeps nothing of it for the LAN (its pin, its
/// listing and the removals waiting for the service end it). CONTROL: the computer listed beside
/// it keeps its link.
#[tokio::test(flavor = "multi_thread")]
async fn a_browser_keeps_no_link_and_no_removal_for_this_network() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let (service, _store, _scratch, _) = on_both_roads(
        &cloud,
        &vault,
        &[],
        vec![
            browser_row(&token("browser")),
            computer_row(&token("computer"), "operate"),
        ],
    )
    .await;
    let keys: Vec<String> = ties_kept(&vault).ties.into_iter().map(|t| t.key).collect();
    assert_eq!(keys, vec![short(&sha256(&key("road")))]);
    assert!(pinned(&service).contains(&BROWSER.to_string()), "premise");
    service
        .action(Action::Device {
            device_id: BROWSER.into(),
            approve: false,
            key: None,
            kind: None,
        })
        .await
        .unwrap();
    assert!(!pinned(&service).contains(&BROWSER.to_string()));
    assert!(ties_kept(&vault).removed.is_empty());
}

/// ★ A LINK THE STORE WILL NOT KEEP GRANTS NOTHING: the removals read, and the store then refusing
/// what links a computer's two roads, a computer's listing is granted nothing (a Remove would end
/// only one road) and shown with why. CONTROL: once the store keeps it, the same listing is granted,
/// with its link.
#[tokio::test(flavor = "multi_thread")]
async fn a_link_the_store_will_not_keep_grants_nothing() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let (service, _store, _scratch, _) = on_both_roads(&cloud, &vault, &[], Vec::new()).await;
    let enrolment = service.control.lock().unwrap().enrolment.clone();
    assert!(enrolment.kept().is_some(), "premise: the removals read");
    vault.refusing.lock().unwrap().insert("account-ties");
    *cloud.devices.lock().unwrap() =
        json!({ "devices": [computer_row(&token("computer"), "operate")] }).to_string();
    let status = service.action(Action::Refresh {}).await.unwrap();
    assert!(pinned(&service).is_empty(), "granted with no link kept");
    assert_eq!(
        status.account.unconfirmed.get(COMPUTER_ID).copied(),
        Some("credentialStoreUnavailable")
    );
    vault.refusing.lock().unwrap().clear();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(pinned(&service), vec![COMPUTER_ID.to_string()]);
    let road = short(&sha256(&key("road")));
    assert!(ties_kept(&vault).ties.iter().any(|t| t.key == road));
}

/// ★ WHILE THE REMOVALS DO NOT READ, NO BROWSER OR PHONE IS GRANTED ON ITS PROOF: a listing that
/// names no LAN key keeps no link, so nothing but the unread removals stands in its way, and its
/// proof is not even checked. CONTROL: once they read, the browser is granted, and the phone's
/// proof is checked (the fixture signed none for a phone, so it is refused for that).
#[tokio::test(flavor = "multi_thread")]
async fn while_the_removals_do_not_read_no_browser_or_phone_is_granted() {
    const PHONE: &str = "10000000-0000-4000-8000-0000000000e1";
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    vault.refusing.lock().unwrap().insert("account-ties");
    let mut phone = computer_row(&token("browserNativeAud"), "operate");
    phone["id"] = PHONE.into();
    phone["publicKey"] = key("browser").into();
    phone["kind"] = "phone".into();
    phone["platform"] = "android".into();
    phone["lanKey"] = serde_json::Value::Null;
    let (service, _store, _scratch, _) = on_both_roads(
        &cloud,
        &vault,
        &[],
        vec![browser_row(&token("browser")), phone],
    )
    .await;
    let status = service.status().unwrap();
    assert!(pinned(&service).is_empty(), "granted, the removals unread");
    for id in [BROWSER, PHONE] {
        assert_eq!(
            status.account.unconfirmed.get(id).copied(),
            Some("credentialStoreUnavailable"),
            "{id}"
        );
    }
    vault.refusing.lock().unwrap().clear();
    let status = service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(pinned(&service), vec![BROWSER.to_string()]);
    assert_eq!(
        status.account.unconfirmed.get(PHONE).copied(),
        Some("tokenNonce")
    );
}

/// The fixture's guest, and a second person, both invited to operate.
fn two_guests_vault(origin: &str) -> MemoryVault {
    owned_vault(
        origin,
        ["K9.Guest@example.test", "N0.Other@example.test"]
            .into_iter()
            .map(|address| Invite {
                mailbox: mailbox(address),
                role: Role::Operate,
            })
            .collect(),
    )
}
/// Keeps `device` as the LAN's entry for its pin, and the copy in memory follows.
fn keep_cached(service: &Service, vault: &MemoryVault, device: CachedDevice) {
    let mut cache = vault
        .device_cache()
        .unwrap()
        .unwrap_or_else(|| tempo_client::account::DeviceCache::new(STATION));
    cache.keep(device).unwrap();
    vault.save_account_entry("account-devices", &cache).unwrap();
    service.control.lock().unwrap().enrolment.reread();
}
/// The LAN's entry for the fixture's computer.
fn computer_cached(vault: &MemoryVault) -> Option<CachedDevice> {
    let pin = sha256(&key("computerLan"));
    vault
        .device_cache()
        .unwrap()
        .and_then(|cache| cache.devices.into_iter().find(|d| d.pin == pin))
}

/// ★ A GUEST'S LISTING NEVER MAKES THE OWNER'S ENROLLED COMPUTER A GUEST'S: the owner's computer
/// enrolled on its own sign-in, and a guest's listing claiming its LAN key. Its entry for the LAN
/// stays the owner's, and taking that guest's invitation back leaves it in the book.
#[tokio::test(flavor = "multi_thread")]
async fn a_guests_listing_never_makes_the_owners_enrolled_computer_a_guests() {
    let cloud = fake_cloud().await;
    let vault = two_guests_vault(&cloud.origin);
    let (service, _store, _scratch, lan_id) = on_both_roads(&cloud, &vault, &[], Vec::new()).await;
    let owners = CachedDevice {
        pin: sha256(&key("computerLan")),
        role: Role::Operate,
        guest: false,
        issued_at: fixture_now() - 500,
        confirmed_at: 0,
    };
    keep_cached(&service, &vault, owners.clone());
    keep_ties(
        &service,
        &vault,
        vec![Tie {
            lan: short(&sha256(&key("computerLan"))),
            key: short(&sha256(&key("road"))),
            mailbox: None,
            enrolled: true,
            vouched: false,
        }],
    );
    let mut row = computer_row(&token("guest"), "operate");
    row["name"] = "GUEST-PC".into();
    *cloud.devices.lock().unwrap() = json!({ "devices": [row] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(pinned(&service), vec![COMPUTER_ID.to_string()], "premise");
    let cached = computer_cached(&vault).unwrap();
    assert_eq!(
        (cached.guest, cached.issued_at),
        (owners.guest, owners.issued_at),
        "made the guest's"
    );
    cloud.refusing.store(true, Ordering::SeqCst);
    service
        .action(Action::Uninvite {
            email: "K9.Guest@example.test".into(),
        })
        .await
        .unwrap();
    let status = service.status().unwrap();
    assert_eq!(in_book(&status), vec![lan_id]);
    assert!(status.account.also_uninvited.is_empty());
}

/// ★ A GUEST'S LISTING NEVER EXTENDS ANOTHER GUEST'S COMPUTER: a second guest's computer enrolled
/// on their own sign-in, and the fixture's guest's listing, signed later, claiming its LAN key. Its
/// entry for the LAN keeps the proof its own guest signed, so it is held to that proof's age.
#[tokio::test(flavor = "multi_thread")]
async fn a_guests_listing_never_extends_another_guests_computer() {
    let cloud = fake_cloud().await;
    let vault = two_guests_vault(&cloud.origin);
    let (service, _store, _scratch, _) = on_both_roads(&cloud, &vault, &[], Vec::new()).await;
    let theirs = CachedDevice {
        pin: sha256(&key("computerLan")),
        role: Role::Operate,
        guest: true,
        issued_at: fixture_now() - 5_000,
        confirmed_at: 0,
    };
    keep_cached(&service, &vault, theirs.clone());
    keep_ties(
        &service,
        &vault,
        vec![Tie {
            lan: short(&sha256(&key("computerLan"))),
            key: "0123456789abcdef".into(),
            mailbox: Some(mailbox_tag(&mailbox("N0.Other@example.test"))),
            enrolled: true,
            vouched: false,
        }],
    );
    *cloud.devices.lock().unwrap() =
        json!({ "devices": [computer_row(&token("guest"), "operate")] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(pinned(&service), vec![COMPUTER_ID.to_string()], "premise");
    let cached = computer_cached(&vault).unwrap();
    assert_eq!(
        (cached.guest, cached.issued_at),
        (theirs.guest, theirs.issued_at),
        "extended"
    );
}

/// ★ A GUEST'S LISTING KEEPS THEIR OWN ENROLLED COMPUTER'S ENTRY UP TO DATE: the computer that
/// guest enrolled here, and their listing naming it with a newer proof. CONTROL for the two above.
#[tokio::test(flavor = "multi_thread")]
async fn a_guests_listing_refreshes_their_own_enrolled_computer() {
    let cloud = fake_cloud().await;
    let vault = two_guests_vault(&cloud.origin);
    let (service, _store, _scratch, _) = on_both_roads(&cloud, &vault, &[], Vec::new()).await;
    keep_cached(
        &service,
        &vault,
        CachedDevice {
            pin: sha256(&key("computerLan")),
            role: Role::Operate,
            guest: true,
            issued_at: fixture_now() - 5_000,
            confirmed_at: 0,
        },
    );
    keep_ties(
        &service,
        &vault,
        vec![Tie {
            lan: short(&sha256(&key("computerLan"))),
            key: short(&sha256(&key("road"))),
            mailbox: Some(mailbox_tag(&mailbox("K9.Guest@example.test"))),
            enrolled: true,
            vouched: false,
        }],
    );
    *cloud.devices.lock().unwrap() =
        json!({ "devices": [computer_row(&token("guest"), "operate")] }).to_string();
    service.action(Action::Refresh {}).await.unwrap();
    let cached = computer_cached(&vault).unwrap();
    assert!(cached.guest);
    assert_eq!(cached.issued_at, fixture_now() - 100, "not refreshed");
}

/// The station's clock for [`reset_network_identity_brings_a_floor_set_ahead_back_to_now`]: its own,
/// so no other test's clock moves with it.
static RESET_CLOCK: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1_800_000_000);
fn reset_clock() -> u64 {
    RESET_CLOCK.load(Ordering::SeqCst)
}

/// ★ RESET NETWORK IDENTITY BRINGS A FLOOR SET AHEAD BACK TO NOW: the floor, and a removal, kept
/// while the station's clock ran years ahead, refuse every sign-in. Reset network identity sets the
/// floor to its own now and drops the removal kept in the future, so a proof signed after it is
/// granted again.
#[tokio::test(flavor = "multi_thread")]
async fn reset_network_identity_brings_a_floor_set_ahead_back_to_now() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let ahead = fixture_now() + 5 * 365 * 86_400;
    vault
        .save_account_entry(
            "account-ties",
            &Ties {
                removed: vec![crate::remote_service::vault::Removed {
                    key: "0123456789abcdef".into(),
                    at: ahead,
                }],
                floor: ahead,
                ..Ties::default()
            },
        )
        .unwrap();
    let row = computer_row(&token("computer"), "operate");
    let (service, _store, _scratch, _) =
        on_both_roads_at(&cloud, &vault, &[], vec![row], reset_clock).await;
    let status = service.status().unwrap();
    assert_eq!(
        status.account.unconfirmed.get(COMPUTER_ID).copied(),
        Some("tokenRemoved"),
        "premise"
    );
    RESET_CLOCK.store(fixture_now() - 1_000, Ordering::SeqCst);
    // Its listing's other road is not known to the station: the shack is told to check that list.
    let _ = service.action(Action::LanReset {}).await;
    RESET_CLOCK.store(fixture_now(), Ordering::SeqCst);
    let kept = ties_kept(&vault);
    assert_eq!(kept.floor, fixture_now() - 1_000);
    assert!(kept.removed.iter().all(|r| r.at < ahead), "kept ahead");
    service.action(Action::Refresh {}).await.unwrap();
    assert_eq!(pinned(&service), vec![COMPUTER_ID.to_string()]);
}

/// ★ A RECORD OF REMOVALS THAT DOES NOT READ AT ALL IS SHOWN WITH ITS WAY OUT: the shack says so,
/// so the operator finds Reset network identity; once reset, it says nothing.
#[tokio::test(flavor = "multi_thread")]
async fn a_record_of_removals_that_does_not_read_is_shown() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    vault
        .values
        .lock()
        .unwrap()
        .insert("account-ties".into(), "{not a record".into());
    let (service, _store, _scratch, _) = on_both_roads(&cloud, &vault, &[], Vec::new()).await;
    assert!(service.status().unwrap().account.removals_unreadable);
    let shown = serde_json::to_value(service.status().unwrap()).unwrap();
    assert_eq!(shown["account"]["removalsUnreadable"], true);
    service.action(Action::LanReset {}).await.unwrap();
    assert!(!service.status().unwrap().account.removals_unreadable);
}

// ---- The shack's one Devices list (display only) ------------------------------------------------

/// A row of the shack's Devices list: its listed ids, its LAN ids, the guest's address, not known whose.
type Shown = (Vec<String>, Vec<String>, Option<String>, bool);
/// The rows the shack's Devices list is given.
fn device_rows(status: &Status) -> Vec<Shown> {
    status
        .device_rows
        .iter()
        .map(|r| (r.listed.clone(), r.lan.clone(), r.guest.clone(), r.unknown))
        .collect()
}

/// ★ ONE ROW PER COMPUTER: a computer on the LAN and, by its own sign-in, on the internet is one row
/// with both its ids, exactly what a Remove on either takes; the owner's. CONTROL: another computer
/// paired by code is its own row, and the shack sees the rows under `deviceRows`.
#[tokio::test(flavor = "multi_thread")]
async fn the_devices_list_shows_a_computer_on_both_roads_once() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let other: [u8; 32] = [7; 32];
    let (service, _store, _scratch, lan_id) = on_both_roads(
        &cloud,
        &vault,
        &[(&other, "Den PC")],
        vec![computer_row(&token("computer"), "operate")],
    )
    .await;
    let status = service.status().unwrap();
    assert_eq!(
        device_rows(&status),
        vec![
            (vec![COMPUTER_ID.to_string()], vec![lan_id], None, false),
            (Vec::new(), vec![lan::device_id(&other)], None, false),
        ]
    );
    let shown = serde_json::to_value(&status).unwrap();
    assert_eq!(shown["deviceRows"][0]["listed"][0], COMPUTER_ID);
    assert_eq!(shown["deviceRows"][0]["guest"], serde_json::Value::Null);
    assert!(shown.get("whose").is_none(), "never sent");
}

/// ★ A GUEST'S DEVICE IS SHOWN UNDER THE GUEST, after a restart too, when its proof is no longer
/// waiting for anything: whose it is comes from its proof as checked here, never the service's say.
/// CONTROL: the owner's own computer is the owner's.
#[tokio::test(flavor = "multi_thread")]
async fn a_guests_device_is_shown_under_the_guest_after_a_restart_too() {
    let guest = mailbox("K9.Guest@example.test");
    for (proof, whose) in [("guest", Some(guest.clone())), ("computer", None)] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(
            &cloud.origin,
            vec![Invite {
                mailbox: guest.clone(),
                role: Role::Operate,
            }],
        );
        let rows = vec![computer_row(&token(proof), "operate")];
        let first = launch_granting_on_sign_in(&cloud, &vault);
        let status = listed(&first, &cloud, rows.clone()).await;
        assert_eq!(
            status.pinned_devices,
            vec![COMPUTER_ID.to_string()],
            "{proof}: premise"
        );
        assert_eq!(
            device_rows(&status),
            vec![(
                vec![COMPUTER_ID.to_string()],
                Vec::new(),
                whose.clone(),
                false
            )],
            "{proof}"
        );
        drop(first);
        let again = restarted(&cloud, &vault).await;
        let status = listed(&again, &cloud, rows).await;
        assert_eq!(
            device_rows(&status),
            vec![(vec![COMPUTER_ID.to_string()], Vec::new(), whose, false)],
            "{proof}: after a restart"
        );
    }
}

/// ★ A GUEST'S DEVICE IS NO LONGER SHOWN UNDER THEM ONCE THEIR INVITATION IS TAKEN BACK, though the
/// service still lists it: not known whose, never the owner's, and with no restart. CONTROL: before
/// the Uninvite it is theirs.
#[tokio::test(flavor = "multi_thread")]
async fn a_guests_device_leaves_the_guest_when_their_invitation_is_taken_back() {
    let guest = mailbox("K9.Guest@example.test");
    let cloud = fake_cloud().await;
    let vault = owned_vault(
        &cloud.origin,
        vec![Invite {
            mailbox: guest.clone(),
            role: Role::Operate,
        }],
    );
    let rows = vec![computer_row(&token("guest"), "operate")];
    let service = launch_granting_on_sign_in(&cloud, &vault);
    let status = listed(&service, &cloud, rows.clone()).await;
    assert_eq!(
        device_rows(&status),
        vec![(
            vec![COMPUTER_ID.to_string()],
            Vec::new(),
            Some(guest),
            false
        )],
        "premise: theirs"
    );
    service
        .action(Action::Uninvite {
            email: "K9.Guest@example.test".into(),
        })
        .await
        .unwrap();
    let status = listed(&service, &cloud, rows).await;
    assert_eq!(
        device_rows(&status),
        vec![(vec![COMPUTER_ID.to_string()], Vec::new(), None, true)]
    );
}

/// ★ A DEVICE ADDED BY A SIGN-IN WHOSE PROOF NO LONGER HOLDS HERE IS SHOWN AS NOT KNOWN, never as the
/// owner's: the issuer's keys gone, nothing can be checked. CONTROL: a device added at the shack is
/// the owner's whatever is kept.
#[tokio::test(flavor = "multi_thread")]
async fn a_sign_in_device_whose_proof_cannot_be_checked_is_not_known() {
    let cloud = fake_cloud().await;
    let vault = owned_vault(&cloud.origin, Vec::new());
    let first = launch_granting_on_sign_in(&cloud, &vault);
    let mut shack = browser_row(&token("browser"));
    shack["id"] = "10000000-0000-4000-8000-0000000000ee".into();
    shack["addedBy"] = "shack".into();
    shack["proof"] = serde_json::Value::Null;
    let rows = vec![computer_row(&token("computer"), "operate"), shack];
    let _ = listed(&first, &cloud, rows.clone()).await;
    drop(first);
    vault.values.lock().unwrap().remove("account-keys");
    let again = restarted(&cloud, &vault).await;
    let status = listed(&again, &cloud, rows).await;
    assert_eq!(
        device_rows(&status),
        vec![
            (vec![COMPUTER_ID.to_string()], Vec::new(), None, true),
            (
                vec!["10000000-0000-4000-8000-0000000000ee".to_string()],
                Vec::new(),
                None,
                false
            ),
        ]
    );
}

/// ★ THE TIME LEFT OF A HOLD IS SHOWN ONLY WHERE THE REMOTE PC IS SHOWN IT: a held sign-in device
/// whose owner the service confirms it notified shows its window; one with no confirmed notice
/// shows no time, so a missing email never looks like a coming unlock.
#[tokio::test(flavor = "multi_thread")]
async fn the_devices_list_shows_a_holds_time_only_with_a_confirmed_notice() {
    for notified in [1, 0] {
        let cloud = fake_cloud().await;
        let vault = owned_vault(&cloud.origin, Vec::new());
        let service = launch_granting_on_sign_in(&cloud, &vault);
        let status = listed(&service, &cloud, vec![notified_row(notified)]).await;
        assert_eq!(
            status.held_devices,
            vec![BROWSER.to_string()],
            "{notified}: premise"
        );
        let left = status.device_rows[0].hold_left;
        match notified {
            1 => assert!(
                left.is_some_and(|s| s > 0 && s <= account::TX_HOLD.as_secs()),
                "{left:?}"
            ),
            _ => assert_eq!(left, None),
        }
    }
}
