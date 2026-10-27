//! A computer enrolling on this network by its own sign-in, with no code and no press: its proof is
//! checked at the shack from what the station keeps (internet down), for the key its TLS session
//! proved, and only then is it paired.
//!
//! The proofs are tempo-client's account fixture, minted once for public keys whose private halves
//! were never kept. So the admission itself is driven directly ([`lan_enrol::admit`], what the session
//! runs), and over TLS a fresh key shows that a valid proof for another key is refused.
//!
//! A computer paired again with a code at the shack is held to no proof's age from then on: there a
//! fresh key stands for an enrolled computer, kept in the device cache as an enrolment keeps it.
//!
//! The device cache's writers one at a time: each is caught inside the store where it would race
//! (held in a read or a save until the test lets it go), with another writer run beside it, so an
//! overlap is made on purpose rather than waited for.
use super::super::enrol as lan_enrol;
use super::*;
use crate::remote_service::tests::MemoryVault;
use crate::remote_service::vault::{
    AccountKeys, AccountVault, DevicesHeld, Notified, People, Revokes, Tie, Ties, Uninvites,
};
use crate::remote_service::{account, Controller};
use std::sync::atomic::AtomicU64;
use std::sync::mpsc;
use tempo_client::account::{
    mailbox, CachedDevice, DeviceCache, Invite, KeySet, Owner, Role, Tenant, Vouched,
    CACHED_DEVICES, PROOF_MAX_AGE_S,
};
use tempo_client::id_token::LEEWAY_S;

const CASES: &str =
    include_str!("../../../../../crates/tempo-client/tests/fixtures/account-proof-cases.json");
/// The service's id for the station, which its owner was proven under.
const SERVICE_STATION: &str = "20000000-0000-4000-8000-000000000001";

fn cases() -> Value {
    serde_json::from_str(CASES).unwrap()
}
fn fixture_now() -> u64 {
    cases()["now"].as_u64().unwrap()
}
fn issued_at() -> u64 {
    cases()["issuedAt"].as_u64().unwrap()
}
fn key(name: &str) -> String {
    cases()["keys"][name].as_str().unwrap().to_string()
}
fn token(name: &str) -> String {
    cases()["tokens"][name].as_str().unwrap().to_string()
}
fn spki(name: &str) -> Vec<u8> {
    tempo_stream::protocol::hex_bytes(&key(name)).unwrap()
}

/// What a station bound to the fixture's owner keeps, with `invites`.
fn ready(vault: &MemoryVault, invites: Vec<Invite>) -> lan_enrol::Ready {
    ready_on(Arc::new(vault.clone()), invites)
}
/// The same, its device cache kept in `vault`.
fn ready_on(vault: Arc<dyn AccountVault>, invites: Vec<Invite>) -> lan_enrol::Ready {
    lan_enrol::Ready {
        tenant: Box::leak(Box::new(Tenant {
            origin: "http://127.0.0.1:9",
            issuer: "https://issuer.test.invalid/",
            native_client_id: "test-native-client",
            spa_client_id: "test-spa-client",
        })),
        owner: Owner {
            station_id: SERVICE_STATION.into(),
            issuer: "https://issuer.test.invalid/".into(),
            subject: cases()["owner"].as_str().unwrap().into(),
            proof_kid: "test-p".into(),
            proof_issued_at: issued_at(),
        },
        keys: KeySet::fetched(&cases()["jwks"], fixture_now()).unwrap(),
        invites,
        vault,
        now_s: fixture_now,
    }
}
fn armed(vault: &MemoryVault, invites: Vec<Invite>) -> Arc<lan_enrol::Enrolment> {
    let enrolment = Arc::new(lan_enrol::Enrolment::default());
    enrolment.arm(Some(ready(vault, invites)));
    enrolment
}
/// A station with its LAN key and nobody paired: its book.
fn empty_book() -> Arc<Book> {
    book_on(&LanStore::holding(&fixture_key(), STATION, &[]))
}
/// The fixture's computer asking, its TLS session having proved `tls_key`.
fn computer(proof: &str, tls_key: &str) -> lan_enrol::Asked {
    lan_enrol::Asked::new("DESKTOP-7Q", "computer", &key("road"), proof, spki(tls_key)).unwrap()
}

/// ★ The owner's computer, its proof for exactly the key it proved: kept for the LAN (by that key's
/// pin, a proof's signing time, not yet heard from the service), then paired. The same proof
/// with another key proved: nothing.
#[test]
fn the_owners_computer_enrols_by_the_key_its_session_proved() {
    let vault = MemoryVault::default();
    let enrolment = armed(&vault, Vec::new());
    let book = empty_book();
    let pin = tls::pin(&spki("computerLan"));

    let other = computer(&token("computer"), "stationLan");
    assert_eq!(
        lan_enrol::admit(&book, &enrolment, STATION, &other, fixture_now()),
        Err("tokenNonce")
    );
    assert_eq!(book.ids(), Vec::<String>::new());
    assert_eq!(vault.device_cache().unwrap(), None);

    // Only a computer or a phone enrols on the LAN.
    assert!(lan_enrol::Asked::new(
        "Browser",
        "browser",
        &key("browser"),
        &token("browser"),
        spki("browser")
    )
    .is_none());
    let asked = computer(&token("computer"), "computerLan");
    let device = lan_enrol::admit(&book, &enrolment, STATION, &asked, fixture_now()).unwrap();
    assert_eq!(device, book::device_id(&pin));
    assert_eq!(book.paired(&pin), Some(device));
    let cache = vault.device_cache().unwrap().unwrap();
    assert_eq!(cache.station_id, SERVICE_STATION);
    assert_eq!(
        cache.devices,
        vec![CachedDevice {
            pin: tempo_client::hex(&pin),
            role: Role::Operate,
            guest: false,
            issued_at: issued_at(),
            confirmed_at: 0,
        }]
    );
}

/// The offline age, one rule from the proof's signing (operator rulings): taken up to the last
/// second of thirty days, refused from then on.
#[test]
fn a_proof_enrols_for_thirty_days_from_its_signing() {
    for (age, enrols) in [(PROOF_MAX_AGE_S - 1, true), (PROOF_MAX_AGE_S, false)] {
        let vault = MemoryVault::default();
        let book = empty_book();
        let asked = computer(&token("computer"), "computerLan");
        let result = lan_enrol::admit(
            &book,
            &armed(&vault, Vec::new()),
            STATION,
            &asked,
            issued_at() + age,
        );
        assert_eq!(result.is_ok(), enrols, "{age}: {result:?}");
        if !enrols {
            assert_eq!(result, Err("tokenIssuedAt"));
            assert_eq!(book.ids(), Vec::<String>::new());
        }
    }
}

/// Someone invited to operate enrols as a guest; invited only to listen, or not invited, they do
/// not; nor does anyone while the station has nothing to check against.
#[test]
fn only_an_operating_invitation_or_the_owner_enrols() {
    let invited = |role| {
        vec![Invite {
            mailbox: mailbox("K9.Guest@example.test"),
            role,
        }]
    };
    let vault = MemoryVault::default();
    let book = empty_book();
    let guest = computer(&token("guest"), "computerLan");
    lan_enrol::admit(
        &book,
        &armed(&vault, invited(Role::Operate)),
        STATION,
        &guest,
        fixture_now(),
    )
    .unwrap();
    let kept = vault.device_cache().unwrap().unwrap().devices;
    assert_eq!((kept[0].role, kept[0].guest), (Role::Operate, true));

    let unarmed = Arc::new(lan_enrol::Enrolment::default());
    for (enrolment, proof, why) in [
        (
            armed(&vault, invited(Role::Listen)),
            "guest",
            "listenNative",
        ),
        (armed(&vault, Vec::new()), "stranger", "proofNotYours"),
        (unarmed, "computer", "enrolClosed"),
    ] {
        let vault = MemoryVault::default();
        let book = empty_book();
        let asked = computer(&token(proof), "computerLan");
        assert_eq!(
            lan_enrol::admit(&book, &enrolment, STATION, &asked, fixture_now()),
            Err(why),
            "{proof}"
        );
        assert_eq!(book.ids(), Vec::<String>::new(), "{proof}");
        assert_eq!(vault.device_cache().unwrap(), None, "{proof}");
    }
}

/// The station's listener as `listener` starts it, taking unknown keys while `enrolment` is open
/// (or its pairing window is).
fn enrolling(s: &mut Shack, enrolment: Arc<lan_enrol::Enrolment>) {
    let desk = s.shared.desk.clone().unwrap();
    let identity = tls::Identity::new(&s.station_key, STATION.into()).unwrap();
    let (paired, pairing) = desk.book.verifier();
    let unknown = enrolment.or_pairing(pairing);
    s.shared.tls = tls::server(&identity, paired, unknown).unwrap();
    s.shared.desk =
        Some(super::pairing::Desk::new(desk.book.clone(), &identity, enrolment).unwrap());
}
fn enrol_message(proof: &str) -> Value {
    json!({"type":"enrol","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2,
        "name":"DESKTOP-7Q","kind":"computer","publicKey":key("road"),"proof":proof})
}

/// ★ Over the LAN: with enrolment open, an unknown key gets as far as an enrol-only connection and
/// no further. A valid proof made for another key is refused by name (the session checks the key
/// TLS proved, not one a message names), counts against the address, and pairs nothing; a hello
/// from that key is not welcomed. With enrolment closed (and no window), TLS refuses the key.
#[tokio::test(flavor = "multi_thread")]
async fn over_the_lan_an_unknown_key_reaches_only_the_enrolment() {
    let (_tx, stop) = watch::channel(false);
    let vault = MemoryVault::default();
    let mut s = shack(home());
    let stranger = Computer::new();
    let (task, refused) = dial(&s, &stranger.key, "192.168.1.31:50000", stop.clone()).await;
    assert!(refused.is_err(), "closed: refused in the handshake");
    task.await.unwrap();

    enrolling(&mut s, armed(&vault, Vec::new()));
    let (task, socket) = dial(&s, &stranger.key, "192.168.1.31:50000", stop.clone()).await;
    let mut socket = socket.unwrap();
    let answer = hello(&mut socket, VERSIONS).await;
    assert!(answer.get("closed").is_some(), "no welcome: {answer}");
    task.await.unwrap();

    for _ in 0..FAILURES_BEFORE_IGNORED {
        let (task, socket) = dial(&s, &stranger.key, "192.168.1.32:50000", stop.clone()).await;
        let mut socket = socket.unwrap();
        send(&mut socket, enrol_message(&token("computer"))).await;
        assert_eq!(
            next(&mut socket).await,
            json!({"type":"refused","reason":"tokenNonce"})
        );
        task.await.unwrap();
    }
    assert_eq!(
        s.shared.desk.as_ref().unwrap().book.ids(),
        vec![s.device.clone()]
    );
    assert_eq!(vault.device_cache().unwrap(), None);
    assert_eq!(
        s.shared
            .gate
            .admit("192.168.1.32".parse().unwrap(), Instant::now())
            .err(),
        Some(super::gate::Refused::Ignored),
        "each refused proof counted"
    );
}

/// The pairing window still works beside it, and an enrolment while the station has nothing to
/// check against is turned away by name, not counted.
#[tokio::test(flavor = "multi_thread")]
async fn pairing_stands_beside_enrolment() {
    let (_tx, stop) = watch::channel(false);
    let mut s = shack(home());
    enrolling(&mut s, Arc::new(lan_enrol::Enrolment::default()));
    let desk = s.shared.desk.clone().unwrap();
    desk.book.open(Instant::now(), 0).unwrap();
    let stranger = Computer::new();
    for _ in 0..FAILURES_BEFORE_IGNORED {
        let (task, socket) = dial(&s, &stranger.key, "192.168.1.33:50000", stop.clone()).await;
        let mut socket = socket.unwrap();
        send(&mut socket, enrol_message(&token("computer"))).await;
        assert_eq!(
            next(&mut socket).await,
            json!({"type":"refused","reason":"enrolClosed"})
        );
        task.await.unwrap();
    }
    assert!(s
        .shared
        .gate
        .admit("192.168.1.33".parse().unwrap(), Instant::now())
        .is_ok());
    // And the window's own ceremony starts as it did: the station's proof comes first.
    let (_task, socket) = dial(&s, &stranger.key, "192.168.1.34:50000", stop.clone()).await;
    let mut socket = socket.unwrap();
    send(
        &mut socket,
        json!({"type":"pair","protocol":VERSIONS.0,"stream":VERSIONS.1,"operation":VERSIONS.2,
            "name":"Laptop","nonce":"00".repeat(32)}),
    )
    .await;
    assert_eq!(next(&mut socket).await["type"], "pairProof");
}

/// ★ The computer's own enrolment (`crate::lan_client::account::enrol`) against the station's real
/// channel: its message is the one the station reads (parsed whole and exactly), and the station's
/// refusal of a proof made for another key comes back named as the window says it
/// (`proofRefused`), with nothing paired. CONTROL: with nothing to check a sign-in against, the
/// station refuses the unknown key in TLS (`stationNotSignedIn`); and a computer pinning another
/// station's key goes no further than TLS (`keyChanged`).
#[tokio::test(flavor = "multi_thread")]
async fn the_computers_enrolment_is_the_one_the_station_reads() {
    use crate::lan_client::account::enrol as computer_enrols;
    let (_tx, stop) = watch::channel(false);
    let vault = MemoryVault::default();
    let mut s = shack(home());
    let stranger = Computer::new();
    let asked = |at, station: String| {
        let pkcs8 = stranger.key.clone();
        async move {
            computer_enrols(
                at,
                &pkcs8,
                &station,
                "Den PC",
                &key("road"),
                &token("computer"),
            )
            .await
        }
    };
    let closed = super::computer::behind_a_port(&s, stop.clone()).await;
    assert_eq!(
        asked(closed, s.public_key.clone()).await,
        Err("stationNotSignedIn")
    );
    enrolling(&mut s, armed(&vault, Vec::new()));
    let open = super::computer::behind_a_port(&s, stop.clone()).await;
    assert_eq!(asked(open, s.public_key.clone()).await, Err("proofRefused"));
    assert_eq!(
        s.shared.desk.as_ref().unwrap().book.ids(),
        vec![s.device.clone()],
        "nothing paired"
    );
    assert_eq!(vault.device_cache().unwrap(), None);
    assert_eq!(
        asked(open, super::computer::another_key()).await,
        Err("keyChanged")
    );
}

// ---- A proof's age on the LAN --------------------------------------------------------------------

/// A device kept for the LAN by `pin`, its proof signed at `issued_at`.
fn kept(pin: &[u8; 32], guest: bool, issued_at: u64) -> CachedDevice {
    CachedDevice {
        pin: tempo_client::hex(pin),
        role: Role::Operate,
        guest,
        issued_at,
        confirmed_at: 0,
    }
}
/// A store holding the LAN's device cache with `devices`.
fn caching(devices: Vec<CachedDevice>) -> MemoryVault {
    let vault = MemoryVault::default();
    let mut cache = DeviceCache::new(SERVICE_STATION);
    for device in devices {
        cache.keep(device).unwrap();
    }
    vault
        .save_device_cache(&DevicesHeld::take(&vault), &cache)
        .unwrap();
    vault
}

/// ★ A computer enrolled on its own sign-in is admitted for thirty days from its proof's signing, the
/// owner's and a guest's alike, and not a second longer; a computer paired by code is not held to
/// it, and the book's own refusal stands. Until the cache has been read nobody is admitted, as the
/// book admits nobody until it has read its own.
#[test]
fn an_enrolled_computer_is_admitted_for_thirty_days_from_its_proof() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let signed = 1_000_000;
    let (owners, guests, by_code) = ([1; 32], [2; 32], [3; 32]);
    let vault = caching(vec![
        kept(&owners, false, signed),
        kept(&guests, true, signed),
    ]);
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(Arc::new(vault), clock));
    let paired = enrolment.bounded(Arc::new(|pin: &[u8; 32]| Some(book::device_id(pin))));
    let admitted = |pin: &[u8; 32]| paired(pin) == Some(book::device_id(pin));
    CLOCK.store(signed, Ordering::SeqCst);
    assert!(!admitted(&owners) && !admitted(&by_code), "not read yet");

    enrolment.load();
    for at in [signed, signed + PROOF_MAX_AGE_S - 1] {
        CLOCK.store(at, Ordering::SeqCst);
        assert!(
            admitted(&owners) && admitted(&guests) && admitted(&by_code),
            "{at}"
        );
        assert_eq!(enrolment.expired(), Vec::<[u8; 32]>::new());
    }
    CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
    assert!(!admitted(&owners), "the owner's, thirty days on");
    assert!(!admitted(&guests), "a guest's, thirty days on");
    assert!(admitted(&by_code), "paired by code: the book alone decides");
    assert_eq!(enrolment.expired(), vec![owners, guests]);
    // Signed in the future (past the clock's leeway): not yet.
    CLOCK.store(signed - 61, Ordering::SeqCst);
    assert!(!admitted(&owners));

    CLOCK.store(signed, Ordering::SeqCst);
    let refused = enrolment.bounded(Arc::new(|_: &[u8; 32]| None));
    assert_eq!(refused(&owners), None, "never more than the book admits");
    // With no account at all (a test's LAN), nothing is enrolled and nothing held to an age.
    assert!(lan_enrol::Enrolment::default().admits(&owners));
}

/// Enrolling again once the old proof has aged out replaces it: admitted from the new proof's
/// signing, at once, with no re-read of the store.
#[test]
fn enrolling_again_admits_from_the_new_proof() {
    let pin = tls::pin(&spki("computerLan"));
    let vault = caching(vec![kept(&pin, false, issued_at() - PROOF_MAX_AGE_S)]);
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        fixture_now,
    ));
    enrolment.arm(Some(ready(&vault, Vec::new())));
    enrolment.load();
    assert!(!enrolment.admits(&pin), "premise: aged out");
    let book = empty_book();
    let asked = computer(&token("computer"), "computerLan");
    lan_enrol::admit(&book, &enrolment, STATION, &asked, fixture_now()).unwrap();
    assert!(enrolment.admits(&pin));
    assert_eq!(
        vault.device_cache().unwrap().unwrap().devices,
        vec![kept(&pin, false, issued_at())]
    );
}

/// The LAN on the station `s` serves, its book `book` and its enrolment `enrolment`.
fn lan_enrolled(
    s: &Shack,
    scratch: &Scratch,
    network: Network,
    book: Arc<Book>,
    enrolment: Arc<lan_enrol::Enrolment>,
) -> Lan {
    Lan::start(
        scratch.path(),
        Deps {
            authority: s.shared.authority.clone(),
            engine: s.shared.engine.clone(),
            feeds: s.shared.feeds.clone(),
            book,
            enrolment,
            resolve: Arc::new(move |_| only(Ok(network))),
            advertise: advertise(),
            firewall: firewall_says(),
            dispatcher: s.shared.dispatcher.clone(),
            commands: s.shared.commands.clone(),
            station_key: s.shared.station_key.clone(),
        },
    )
}

/// ★ At launch, before anything listens, an enrolled computer past its proof's age leaves the book
/// and the LAN's device cache (it signs in again to come back). One within its age stays, and so
/// does a computer paired by code.
#[tokio::test]
async fn at_launch_a_computer_past_its_proofs_age_leaves_the_lan() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let signed = 1_000_000;
    let (aged, fresh, by_code) = ([1; 32], [2; 32], [3; 32]);
    let vault = caching(vec![
        kept(&aged, false, signed),
        kept(&fresh, true, signed + 1),
    ]);
    CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
    let s = shack(home());
    let store = LanStore::holding(
        &s.station_key,
        STATION,
        &[(&aged, "Aged"), (&fresh, "Fresh"), (&by_code, "Den PC")],
    );
    let scratch = Scratch::new();
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        clock,
    ));
    let lan = lan_enrolled(&s, &scratch, home(), book_on(&store), enrolment);
    let names = |status: &LanStatus| {
        let mut names: Vec<String> = status.devices.iter().map(|d| d.name.clone()).collect();
        names.sort();
        names
    };
    let status = eventually(&lan, "the aged one stayed", |st| names(st).len() == 2).await;
    assert_eq!(names(&status), vec!["Den PC", "Fresh"]);
    assert_eq!(
        vault.device_cache().unwrap().unwrap().devices,
        vec![kept(&fresh, true, signed + 1)]
    );
}

/// ★ Over the LAN's own listener: an enrolled computer is welcomed within its proof's age; once that
/// passes, its session ends at its next tick and its next connection is refused. CONTROL: a computer
/// paired by code on the same listener is welcomed after.
#[tokio::test(flavor = "multi_thread")]
async fn the_lans_listener_holds_an_enrolled_computer_to_its_proofs_age() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return;
    };
    let port = std::net::TcpListener::bind((address, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let network = Network::new(address, 32).unwrap();
    let signed = 1_000_000;
    let (station_key, enrolled, by_code) = (fixture_key(), Computer::new(), Computer::new());
    let store = LanStore::holding(
        &station_key,
        STATION,
        &[(&enrolled.pin, "Enrolled"), (&by_code.pin, "Den PC")],
    );
    let s = shack_with(network, station_key, enrolled, store);
    let vault = caching(vec![kept(&s.computer.pin, false, signed)]);
    CLOCK.store(signed + PROOF_MAX_AGE_S - 1, Ordering::SeqCst);
    let scratch = Scratch::new();
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(Arc::new(vault), clock));
    let lan = lan_enrolled(&s, &scratch, network, book_on(&s.store), enrolment);
    lan.turn_on(None, Some(port)).unwrap();
    let at = SocketAddr::new(address.into(), port);
    eventually(&lan, "never listened", |st| {
        st.listening.as_deref() == Some(at.to_string().as_str())
    })
    .await;
    let dial_at = |key: &str| {
        let (key, station) = (key.to_string(), s.public_key.clone());
        async move {
            let stream = tokio::net::TcpStream::connect(at).await.unwrap();
            open(stream, &key, &station).await
        }
    };
    let mut socket = dial_at(&s.computer.key).await.unwrap();
    assert_eq!(hello(&mut socket, VERSIONS).await["type"], "welcome");

    CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
    let ended = loop {
        let message = next(&mut socket).await;
        assert!(
            message["silent"].is_null(),
            "the session outlived the proof's age"
        );
        if message["closed"].is_string() {
            break message;
        }
    };
    assert!(ended["closed"].is_string());
    let control = hello(&mut dial_at(&by_code.key).await.unwrap(), VERSIONS).await;
    assert_eq!(control["type"], "welcome", "{control}");
    let again = match dial_at(&s.computer.key).await {
        Ok(mut socket) => hello(&mut socket, VERSIONS).await,
        Err(refused) => json!({ "refused": refused }),
    };
    assert_ne!(again["type"], "welcome", "{again}");
}

// ---- Paired again with a code at the shack -------------------------------------------------------

/// ★ A computer enrolled on its own sign-in and then paired again with a code at the shack, within
/// its proof's thirty days, is held to that age no longer: on its proof's thirtieth day the
/// listener still welcomes it, a list (`Lan::expire`) leaves it in the book, and the station
/// started again that day reads it as admitted, as every computer paired by code is, until it is
/// removed at the shack. CONTROL: a computer enrolled at the same time and never paired by code
/// leaves that day, as before. Skipped, saying so, on a box with no private address of its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_computer_paired_again_with_a_code_outlasts_its_proofs_age() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return;
    };
    let port = std::net::TcpListener::bind((address, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let network = Network::new(address, 32).unwrap();
    let signed = 1_000_000;
    let (station_key, again, other) = (fixture_key(), Computer::new(), Computer::new());
    let store = LanStore::holding(
        &station_key,
        STATION,
        &[(&again.pin, "DESKTOP-7Q"), (&other.pin, "Other PC")],
    );
    let s = shack_with(network, station_key, again.clone(), store);
    let vault = caching(vec![
        kept(&again.pin, false, signed),
        kept(&other.pin, false, signed),
    ]);
    CLOCK.store(signed + 86_400, Ordering::SeqCst);
    let scratch = Scratch::new();
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        clock,
    ));
    let lan = lan_enrolled(&s, &scratch, network, book_on(&s.store), enrolment.clone());
    lan.turn_on(None, Some(port)).unwrap();
    let at = SocketAddr::new(address.into(), port);
    eventually(&lan, "never listened", |st| {
        st.listening.as_deref() == Some(at.to_string().as_str())
    })
    .await;

    // Paired again with the code the shack shows, by the key it enrolled with.
    lan.pair().unwrap();
    let shown = lan.status().pairing.unwrap().code;
    let code: [u8; 8] = tempo_stream::protocol::hex_bytes(&shown)
        .unwrap()
        .try_into()
        .unwrap();
    let stream = tokio::net::TcpStream::connect(at).await.unwrap();
    let mut pairing = super::ceremony::open_pairing(stream, &again.key)
        .await
        .unwrap();
    assert_eq!(
        super::ceremony::prove_code(&mut pairing, &again, &code, "DESKTOP-7Q").await,
        json!({"type":"paired","deviceId":again.device,"stationId":STATION})
    );

    // Its proof's thirtieth day, and the station's list.
    CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
    lan.expire();
    let names: Vec<String> = lan.status().devices.into_iter().map(|d| d.name).collect();
    assert_eq!(
        names,
        vec!["DESKTOP-7Q"],
        "after a list on its proof's thirtieth day"
    );
    let dial_at = |key: &str| {
        let (key, station, lan) = (key.to_string(), s.public_key.clone(), &lan);
        async move {
            // Nothing else open from here first: a refusal is then the handshake's, not the gate's.
            let deadline = Instant::now() + Duration::from_secs(5);
            while lan.open_from(address.into()) > 0 {
                assert!(
                    Instant::now() < deadline,
                    "a connection from here stayed open"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let stream = tokio::net::TcpStream::connect(at).await.unwrap();
            match open(stream, &key, &station).await {
                Ok(mut socket) => hello(&mut socket, VERSIONS).await,
                Err(refused) => json!({ "refused": refused }),
            }
        }
    };
    let welcome = dial_at(&again.key).await;
    assert_eq!(
        welcome["type"], "welcome",
        "paired again by code: {welcome}"
    );
    assert_eq!(welcome["deviceId"], again.device);
    let control = dial_at(&other.key).await;
    assert_ne!(
        control["type"], "welcome",
        "never paired by code: {control}"
    );
    assert_eq!(vault.device_cache().unwrap().unwrap().devices, Vec::new());
    // The station started again that day reads the same from the store.
    let restarted = lan_enrol::Enrolment::kept_in(Arc::new(vault.clone()), clock);
    restarted.load();
    assert!(restarted.admits(&again.pin), "at launch");
    assert_eq!(restarted.expired(), Vec::<[u8; 32]>::new());

    // Removed at the shack: out at once.
    lan.revoke(&again.device).unwrap();
    let removed = dial_at(&again.key).await;
    assert_ne!(
        removed["type"], "welcome",
        "removed at the shack: {removed}"
    );
}

/// The station's account entries in `inner`, its device cache refused as a credential store that
/// will not answer refuses it: the read while `reads` is set, the write while `saves` is; and what
/// a Remove leaves behind alone, read, while `ties_reads` is.
#[derive(Clone, Default)]
struct Refusing {
    inner: MemoryVault,
    reads: Arc<AtomicBool>,
    saves: Arc<AtomicBool>,
    ties_reads: Arc<AtomicBool>,
}
impl AccountVault for Refusing {
    fn owner(&self) -> Result<Option<Owner>, &'static str> {
        self.inner.owner()
    }
    fn save_owner(&self, owner: &Owner) -> Result<(), &'static str> {
        self.inner.save_owner(owner)
    }
    fn account_keys(&self) -> Result<Option<AccountKeys>, &'static str> {
        self.inner.account_keys()
    }
    fn save_account_keys(&self, keys: &AccountKeys) -> Result<(), &'static str> {
        self.inner.save_account_keys(keys)
    }
    fn device_cache(&self) -> Result<Option<DeviceCache>, &'static str> {
        if self.reads.load(Ordering::SeqCst) {
            return Err("credentialStoreUnavailable");
        }
        self.inner.device_cache()
    }
    fn save_device_cache(
        &self,
        held: &DevicesHeld<'_>,
        cache: &DeviceCache,
    ) -> Result<(), &'static str> {
        if self.saves.load(Ordering::SeqCst) {
            return Err("credentialStoreUnavailable");
        }
        self.inner.save_device_cache(held, cache)
    }
    fn devices_lock(&self) -> &Mutex<()> {
        self.inner.devices_lock()
    }
    fn people(&self) -> Result<Option<People>, &'static str> {
        self.inner.people()
    }
    fn save_people(&self, people: &People) -> Result<(), &'static str> {
        self.inner.save_people(people)
    }
    fn revokes(&self) -> Result<Option<Revokes>, &'static str> {
        self.inner.revokes()
    }
    fn save_revokes(&self, revokes: &Revokes) -> Result<(), &'static str> {
        self.inner.save_revokes(revokes)
    }
    fn uninvites(&self) -> Result<Option<Uninvites>, &'static str> {
        self.inner.uninvites()
    }
    fn save_uninvites(&self, uninvites: &Uninvites) -> Result<(), &'static str> {
        self.inner.save_uninvites(uninvites)
    }
    fn notified(&self) -> Result<Option<Notified>, &'static str> {
        self.inner.notified()
    }
    fn save_notified(&self, notified: &Notified) -> Result<(), &'static str> {
        self.inner.save_notified(notified)
    }
    fn ties(&self) -> Result<Option<Ties>, &'static str> {
        if self.reads.load(Ordering::SeqCst) || self.ties_reads.load(Ordering::SeqCst) {
            return Err("credentialStoreUnavailable");
        }
        self.inner.ties()
    }
    fn save_ties(&self, held: &DevicesHeld<'_>, ties: &Ties) -> Result<(), &'static str> {
        if self.saves.load(Ordering::SeqCst) {
            return Err("credentialStoreUnavailable");
        }
        self.inner.save_ties(held, ties)
    }
}

/// `computer` paired again at the shack `s`, a window opened for it with the code then shown: the
/// shack's last word.
async fn paired_again(
    s: &Shack,
    computer: &Computer,
    peer: &str,
    stop: watch::Receiver<bool>,
) -> Value {
    let book = s.shared.desk.as_ref().unwrap().book.clone();
    book.open(Instant::now(), 0).unwrap();
    let code = super::ceremony::shown_code(&book);
    let (_task, pairing) = super::ceremony::dial_pairing(s, &computer.key, peer, stop).await;
    super::ceremony::prove_code(&mut pairing.unwrap(), computer, &code, "Shack laptop").await
}

/// ★ Fails closed: a code pairing whose computer the LAN's device cache will not let go of, because
/// the store will not keep the change or will not be read, is refused as `unavailable`, and the
/// computer stays held to its proof's age, out on its thirtieth day as before. So does one whose
/// pairing the book will not keep, with the cache untouched: it changes only once the book keeps the
/// pairing. CONTROL: once the stores answer, the same computer is paired and held to no age.
#[tokio::test(flavor = "multi_thread")]
async fn a_code_pairing_the_cache_will_not_take_leaves_the_computer_held() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let signed = 1_000_000;
    let (_tx, stop) = watch::channel(false);
    let mut s = shack(home());
    let computer = s.computer.clone();
    let vault = Refusing::default();
    let mut cache = DeviceCache::new(SERVICE_STATION);
    cache.keep(kept(&computer.pin, false, signed)).unwrap();
    vault
        .inner
        .save_device_cache(&DevicesHeld::take(&vault.inner), &cache)
        .unwrap();
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        clock,
    ));
    CLOCK.store(signed + 86_400, Ordering::SeqCst);
    enrolment.load();
    enrolling(&mut s, enrolment.clone());
    let held = |when: &str| {
        CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
        assert!(
            !enrolment.admits(&computer.pin),
            "{when}: admitted on day 30"
        );
        assert_eq!(enrolment.expired(), vec![computer.pin], "{when}");
        CLOCK.store(signed + 86_400, Ordering::SeqCst);
    };
    let refused = json!({"type":"refused","reason":"unavailable"});

    vault.saves.store(true, Ordering::SeqCst);
    let answer = paired_again(&s, &computer, "192.168.1.40:50000", stop.clone()).await;
    assert_eq!(answer, refused, "the cache would not keep the change");
    vault.saves.store(false, Ordering::SeqCst);
    held("the cache would not keep the change");

    vault.reads.store(true, Ordering::SeqCst);
    let answer = paired_again(&s, &computer, "192.168.1.41:50000", stop.clone()).await;
    assert_eq!(answer, refused, "the cache would not be read");
    vault.reads.store(false, Ordering::SeqCst);
    held("the cache would not be read");

    s.store.locked.store(true, Ordering::SeqCst);
    let answer = paired_again(&s, &computer, "192.168.1.42:50000", stop.clone()).await;
    assert_eq!(answer, refused, "the book would not keep the pairing");
    s.store.locked.store(false, Ordering::SeqCst);
    held("the book would not keep the pairing");
    assert_eq!(
        vault.device_cache().unwrap().unwrap().devices,
        vec![kept(&computer.pin, false, signed)],
        "the cache untouched"
    );

    let answer = paired_again(&s, &computer, "192.168.1.43:50000", stop).await;
    assert_eq!(
        answer,
        json!({"type":"paired","deviceId":computer.device,"stationId":STATION})
    );
    CLOCK.store(signed + PROOF_MAX_AGE_S, Ordering::SeqCst);
    assert!(enrolment.admits(&computer.pin), "paired by code: no age");
    assert_eq!(enrolment.expired(), Vec::<[u8; 32]>::new());
}

// ---- The device cache's writers, one at a time ---------------------------------------------------

/// How long a writer is given beside one held inside its save. With nothing between the two it
/// finishes at once, and its change is then lost to the held one's save; with the cache's lock it
/// waits for the held one, which is let go after this.
const BESIDE_FOR: Duration = Duration::from_millis(500);

/// A held store call's two ends: the one told that a writer is in it, the one that lets it go.
type Ends = (mpsc::Sender<()>, mpsc::Receiver<()>);

/// One store call held once it is asked for: told when a writer is inside it, then let go.
#[derive(Clone, Default)]
struct Hold(Arc<Mutex<Option<Ends>>>);
impl Hold {
    /// Holds the next call: the first end hears a writer held in it, the second lets it go.
    fn next(&self) -> (mpsc::Receiver<()>, mpsc::Sender<()>) {
        let ((entered, held), (go, waits)) = (mpsc::channel(), mpsc::channel());
        *self.0.lock().unwrap() = Some((entered, waits));
        (held, go)
    }
    /// Holds no call: one asked for and not reached is called off.
    fn clear(&self) {
        self.0.lock().unwrap().take();
    }
    /// In a store call: waits here while it is held, never more than ten seconds.
    fn here(&self) {
        let hold = self.0.lock().unwrap().take();
        if let Some((entered, waits)) = hold {
            let _ = entered.send(());
            let _ = waits.recv_timeout(Duration::from_secs(10));
        }
    }
}

/// The station's account entries in `inner`, a writer of its device cache held when asked: inside
/// its read (`reads`: the cache read, nothing changed yet), inside its save (`saves`: read and
/// changed, not yet written), or asking for the cache's lock again after its read (`relocks`: it let
/// the lock go between its read and its save).
#[derive(Clone, Default)]
struct Holding {
    inner: MemoryVault,
    reads: Hold,
    saves: Hold,
    relocks: Hold,
}
thread_local! {
    /// Whether this thread has read the device cache.
    static READ_THE_CACHE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
impl AccountVault for Holding {
    fn owner(&self) -> Result<Option<Owner>, &'static str> {
        self.inner.owner()
    }
    fn save_owner(&self, owner: &Owner) -> Result<(), &'static str> {
        self.inner.save_owner(owner)
    }
    fn account_keys(&self) -> Result<Option<AccountKeys>, &'static str> {
        self.inner.account_keys()
    }
    fn save_account_keys(&self, keys: &AccountKeys) -> Result<(), &'static str> {
        self.inner.save_account_keys(keys)
    }
    fn device_cache(&self) -> Result<Option<DeviceCache>, &'static str> {
        READ_THE_CACHE.with(|read| read.set(true));
        let read = self.inner.device_cache();
        self.reads.here();
        read
    }
    fn save_device_cache(
        &self,
        held: &DevicesHeld<'_>,
        cache: &DeviceCache,
    ) -> Result<(), &'static str> {
        self.saves.here();
        self.inner.save_device_cache(held, cache)
    }
    fn devices_lock(&self) -> &Mutex<()> {
        if READ_THE_CACHE.with(std::cell::Cell::get) {
            self.relocks.here();
        }
        self.inner.devices_lock()
    }
    fn people(&self) -> Result<Option<People>, &'static str> {
        self.inner.people()
    }
    fn save_people(&self, people: &People) -> Result<(), &'static str> {
        self.inner.save_people(people)
    }
    fn revokes(&self) -> Result<Option<Revokes>, &'static str> {
        self.inner.revokes()
    }
    fn save_revokes(&self, revokes: &Revokes) -> Result<(), &'static str> {
        self.inner.save_revokes(revokes)
    }
    fn uninvites(&self) -> Result<Option<Uninvites>, &'static str> {
        self.inner.uninvites()
    }
    fn save_uninvites(&self, uninvites: &Uninvites) -> Result<(), &'static str> {
        self.inner.save_uninvites(uninvites)
    }
    fn notified(&self) -> Result<Option<Notified>, &'static str> {
        self.inner.notified()
    }
    fn save_notified(&self, notified: &Notified) -> Result<(), &'static str> {
        self.inner.save_notified(notified)
    }
    fn ties(&self) -> Result<Option<Ties>, &'static str> {
        self.inner.ties()
    }
    fn save_ties(&self, held: &DevicesHeld<'_>, ties: &Ties) -> Result<(), &'static str> {
        self.inner.save_ties(held, ties)
    }
}

/// The LAN's store `inner`, the book's next write of who is paired held when asked: the book's own
/// lock taken, nothing written yet.
#[derive(Clone, Default)]
struct HoldingLan {
    inner: LanStore,
    saves: Hold,
}
impl LanVault for HoldingLan {
    fn lan_key(&self) -> Result<Option<LanKey>, &'static str> {
        self.inner.lan_key()
    }
    fn save_lan_key(&self, key: &LanKey) -> Result<(), &'static str> {
        self.inner.save_lan_key(key)
    }
    fn lan_devices(&self) -> Result<Option<LanDevices>, &'static str> {
        self.inner.lan_devices()
    }
    fn save_lan_devices(&self, devices: &LanDevices) -> Result<(), &'static str> {
        self.saves.here();
        self.inner.save_lan_devices(devices)
    }
    fn remove_lan_devices(&self) -> Result<(), &'static str> {
        self.saves.here();
        self.inner.remove_lan_devices()
    }
}

/// A store holding the LAN's device cache with `devices`, its writers held when asked.
fn holding(devices: Vec<CachedDevice>) -> Holding {
    Holding {
        inner: caching(devices),
        ..Holding::default()
    }
}
/// The enrolment of the station bound to the fixture's owner, its device cache kept in `vault` and
/// its proofs' ages read by `clock`: armed, and the cache read, as at launch.
fn enrolment_in(vault: &Holding, clock: fn() -> u64) -> Arc<lan_enrol::Enrolment> {
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        clock,
    ));
    enrolment.arm(Some(ready_on(Arc::new(vault.clone()), Vec::new())));
    enrolment.load();
    enrolment
}
/// The station's book kept in `store`, with these computers paired, read.
fn book_in(store: &HoldingLan) -> Arc<Book> {
    let book = Arc::new(Book::new(Arc::new(store.clone())));
    book.load();
    book
}
/// The pins the device cache in `vault` holds, in its order.
fn cached_pins(vault: &Holding) -> Vec<String> {
    let cache = vault.device_cache().unwrap().unwrap();
    cache.devices.into_iter().map(|device| device.pin).collect()
}
/// A LAN key the service can list a computer by (lowercase hex) and the pin it gives.
fn lan_key(byte: u8) -> (String, [u8; 32]) {
    let spki = [byte; 91];
    (tempo_client::hex(&spki), tls::pin(&spki))
}
/// A row of the service's list: a computer approved, named by its LAN key.
fn approved(lan: &str) -> Value {
    json!({"kind": "computer", "approved": 1, "publicKey": key("road"), "lanKey": lan})
}
/// The fixture's computer enrols in `book` on its own sign-in, its proof fresh.
fn enrols(book: &Book, enrolment: &lan_enrol::Enrolment) {
    let asked = computer(&token("computer"), "computerLan");
    lan_enrol::admit(book, enrolment, STATION, &asked, fixture_now()).unwrap();
}
/// Whom the LAN admits holding the key with `pin`: the book's answer held to its proof's age, as TLS
/// and every session tick ask.
fn on_the_lan(
    book: &Arc<Book>,
    enrolment: &Arc<lan_enrol::Enrolment>,
    pin: &[u8; 32],
) -> Option<String> {
    let (paired, _) = book.verifier();
    enrolment.bounded(paired)(pin)
}

/// `writer` ran to its end within ten seconds: a lock that never came free stops it here.
fn finished(writer: std::thread::JoinHandle<()>, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !writer.is_finished() {
        assert!(Instant::now() < deadline, "{what}: never finished");
        std::thread::sleep(Duration::from_millis(2));
    }
    writer.join().unwrap();
}

/// `first` held by `hold` inside the store call it guards, and `second` run beside it until it is
/// done or has had [`BESIDE_FOR`]; then `first` let go. Both ran to their end.
fn beside(
    hold: &Hold,
    first: impl FnOnce() + Send + 'static,
    second: impl FnOnce() + Send + 'static,
) {
    let (held, go) = hold.next();
    let first = std::thread::spawn(first);
    held.recv_timeout(Duration::from_secs(10))
        .expect("the first writer was never held");
    let second = std::thread::spawn(second);
    let since = Instant::now();
    while !second.is_finished() && since.elapsed() < BESIDE_FOR {
        std::thread::sleep(Duration::from_millis(2));
    }
    let _ = go.send(());
    finished(first, "the held writer");
    finished(second, "the writer beside it");
}

/// ★ An enrolment during a list. The list's sweep has read the LAN's device cache and is about to
/// save what it made of it when a computer enrols: the enrolment waits for that save and only then
/// adds its entry, so the computer it puts in the book is held to its proof's age, refused on the
/// thirtieth day after the list's own re-read and after a restart. With each writer reading and
/// saving on its own, the sweep's save (made from the cache as it was before the enrolment) dropped
/// the new entry while the computer stayed in the book: admitted with no age at all, and out of
/// reach of a removal on the web, which reaches the LAN only through the cache.
#[test]
fn an_enrolment_during_a_list_is_held_to_its_proofs_age() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    CLOCK.store(fixture_now(), Ordering::SeqCst);
    let pin = tls::pin(&spki("computerLan"));
    // Enrolled before, and on the list.
    let (other_key, other) = lan_key(7);
    let vault = holding(vec![kept(&other, false, issued_at())]);
    let enrolment = enrolment_in(&vault, clock);
    let book = empty_book();

    let (listed, list) = (vec![approved(&other_key)], vault.clone());
    let (enrolling, into) = (enrolment.clone(), book.clone());
    beside(
        &vault.saves,
        move || {
            account::sweep(&listed, &list, SERVICE_STATION, fixture_now()).unwrap();
        },
        move || enrols(&into, &enrolling),
    );
    assert_eq!(book.paired(&pin), Some(book::device_id(&pin)), "enrolled");

    // The list then reads the cache again (`Lan::expire`); and the proof's thirtieth day comes.
    enrolment.reread();
    CLOCK.store(issued_at() + PROOF_MAX_AGE_S, Ordering::SeqCst);
    assert_eq!(
        on_the_lan(&book, &enrolment, &pin),
        None,
        "on the LAN past its proof's age, the cache holding {:?}",
        cached_pins(&vault)
    );
    assert_eq!(
        cached_pins(&vault),
        vec![tempo_client::hex(&other), tempo_client::hex(&pin)]
    );
    assert_eq!(enrolment.expired(), vec![other, pin], "the next list's");
    let restarted = lan_enrol::Enrolment::kept_in(Arc::new(vault.clone()), clock);
    restarted.load();
    assert!(
        !restarted.admits(&pin),
        "started again on its thirtieth day"
    );
}

/// What a writer of the device cache is given: the store, and the enrolment kept in it.
type Writer = Box<dyn FnOnce(&Holding, &lan_enrol::Enrolment) + Send>;

/// ★ Every writer of the LAN's device cache waits for an enrolment held between its read and its
/// save, and makes its change on what that one saved: a list's sweep, a list keeping a device it
/// vouched for, an expiry's removal, a code pairing's. After each pair the cache holds both changes.
/// With each writer reading and saving on its own, the held save (made from the cache as it was
/// before) put back what the other took out, or dropped what it added.
#[test]
fn every_writer_of_the_device_cache_waits_for_one_held_between_its_read_and_its_save() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    CLOCK.store(fixture_now(), Ordering::SeqCst);
    let enrolled = tempo_client::hex(&tls::pin(&spki("computerLan")));
    // Q past its proof's age; R and U within it, U off the list; S vouched for by the list.
    let ((q_key, q), (r_key, r), (_, u), (_, s)) = (lan_key(1), lan_key(2), lan_key(3), lan_key(4));
    let [q_hex, r_hex, u_hex, s_hex] = [q, r, u, s].map(|pin| tempo_client::hex(&pin));
    let list = vec![
        approved(&q_key),
        approved(&r_key),
        approved(&key("computerLan")),
    ];
    let vouched = Vouched {
        subject: "the owner".into(),
        role: Role::Operate,
        guest: false,
        issued_at: issued_at(),
        mfa: false,
        mailbox: None,
    };
    let writers: Vec<(&str, Writer, Vec<&String>)> = vec![
        (
            "a list's sweep",
            Box::new(move |vault: &Holding, _: &lan_enrol::Enrolment| {
                account::sweep(&list, vault, SERVICE_STATION, fixture_now()).unwrap();
            }),
            vec![&q_hex, &r_hex, &enrolled],
        ),
        (
            "a list keeping a device it vouched for",
            Box::new(move |vault: &Holding, _: &lan_enrol::Enrolment| {
                let pin = tempo_client::hex(&s);
                Controller::keep_for_lan(
                    vault,
                    SERVICE_STATION,
                    &vouched,
                    "",
                    pin,
                    false,
                    fixture_now(),
                )
                .unwrap();
            }),
            vec![&q_hex, &r_hex, &u_hex, &enrolled, &s_hex],
        ),
        (
            "an expiry",
            Box::new(move |_: &Holding, enrolment: &lan_enrol::Enrolment| {
                enrolment.forget(&[q]);
            }),
            vec![&r_hex, &u_hex, &enrolled],
        ),
        (
            "a code pairing",
            Box::new(move |_: &Holding, enrolment: &lan_enrol::Enrolment| {
                enrolment.paired_by_code(&r).unwrap();
            }),
            vec![&q_hex, &u_hex, &enrolled],
        ),
    ];
    let (mut cached, mut wanted) = (Vec::new(), Vec::new());
    for (what, writer, after) in writers {
        let vault = holding(vec![
            kept(&q, false, issued_at() - PROOF_MAX_AGE_S),
            kept(&r, false, issued_at()),
            kept(&u, false, issued_at()),
        ]);
        let enrolment = enrolment_in(&vault, clock);
        let book = empty_book();
        let (enrolling, into) = (enrolment.clone(), book.clone());
        let (other, beside_it) = (vault.clone(), enrolment.clone());
        beside(
            &vault.saves,
            move || enrols(&into, &enrolling),
            move || writer(&other, &beside_it),
        );
        cached.push((what, cached_pins(&vault)));
        wanted.push((what, after.into_iter().cloned().collect::<Vec<String>>()));
    }
    assert_eq!(cached, wanted, "each beside an enrolment");
}

/// ★ A list never lands between an enrolment's entry in the device cache and its pairing in the
/// book. The enrolment held in the book's write (its entry kept, the computer not yet paired), a
/// list that does not name the computer (it enrolled after the list was asked for) waits for it,
/// then takes it out of the cache and the book alike, as it takes out every computer it does not
/// name. Before, the list took the entry out while the book did not hold the computer yet, so
/// taking it out of the book did nothing, and the enrolment then paired it: in the book with no
/// entry, admitted past its proof's age.
#[test]
fn a_list_never_lands_between_an_enrolments_cache_entry_and_its_pairing() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    CLOCK.store(fixture_now(), Ordering::SeqCst);
    let pin = tls::pin(&spki("computerLan"));
    let (other_key, other) = lan_key(7);
    let vault = holding(vec![kept(&other, false, issued_at())]);
    let enrolment = enrolment_in(&vault, clock);
    let store = HoldingLan {
        inner: LanStore::holding(&fixture_key(), STATION, &[]),
        ..HoldingLan::default()
    };
    let book = book_in(&store);

    let (enrolling, into) = (enrolment.clone(), book.clone());
    let (listed, list, from) = (vec![approved(&other_key)], vault.clone(), book.clone());
    beside(
        &store.saves,
        move || enrols(&into, &enrolling),
        move || {
            // The list as the controller hears it: the sweep, then each computer it hands back
            // taken out of the LAN (`Lan::revoke`: out of the book, then the store told).
            let gone = account::sweep(&listed, &list, SERVICE_STATION, fixture_now()).unwrap();
            for pin in gone
                .iter()
                .filter_map(|pin| super::super::pairing::hex32(pin))
            {
                if from.forget(&book::device_id(&pin)) {
                    from.keep().unwrap();
                }
            }
        },
    );
    enrolment.reread();
    CLOCK.store(issued_at() + PROOF_MAX_AGE_S, Ordering::SeqCst);
    assert_eq!(
        (book.paired(&pin), cached_pins(&vault)),
        (None, vec![tempo_client::hex(&other)]),
        "a computer the list does not name: out of the cache and the book alike"
    );
    assert_eq!(on_the_lan(&book, &enrolment, &pin), None);
}

/// ★ An expiry never takes out a computer enrolled again meanwhile, and the cache's lock and the
/// book's never wait on each other in a ring. The list's expiry has found the computer past its
/// proof's age and taken it out of the book; while it writes the book (holding the book's lock) the
/// computer signs in again and enrols, its fresh entry kept, and waits for the book. Once the book
/// is written the expiry comes to the cache, finds there an entry within its age, and leaves it: the
/// computer, paired again, is held to its new proof. Before, the expiry took the fresh entry out,
/// leaving it in the book with none, admitted past its new proof's age. The order that keeps the
/// two locks from a ring: an enrolment holds the cache's lock while it takes the book's, and the
/// expiry lets the book's go before it takes the cache's. Both finished.
#[test]
fn an_expiry_never_takes_out_a_computer_enrolled_again_meanwhile() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    CLOCK.store(fixture_now(), Ordering::SeqCst);
    let pin = tls::pin(&spki("computerLan"));
    let vault = holding(vec![kept(&pin, false, issued_at() - PROOF_MAX_AGE_S)]);
    let enrolment = enrolment_in(&vault, clock);
    let store = HoldingLan {
        inner: LanStore::holding(&fixture_key(), STATION, &[(&pin, "DESKTOP-7Q")]),
        ..HoldingLan::default()
    };
    let book = book_in(&store);
    assert_eq!(
        enrolment.expired(),
        vec![pin],
        "premise: past its proof's age"
    );

    let (held, go) = store.saves.next();
    let (expiring, from) = (enrolment.clone(), book.clone());
    let expiry = std::thread::spawn(move || {
        super::super::expire(&from, &Authority::default(), &expiring);
    });
    held.recv_timeout(Duration::from_secs(10))
        .expect("the expiry never wrote the book");
    let (enrolling, into) = (enrolment.clone(), book.clone());
    let again = std::thread::spawn(move || enrols(&into, &enrolling));
    // Its fresh entry kept, the enrolment is at the book's lock, which the expiry holds.
    let fresh = vec![kept(&pin, false, issued_at())];
    let deadline = Instant::now() + Duration::from_secs(10);
    while vault.device_cache().unwrap().unwrap().devices != fresh {
        assert!(
            Instant::now() < deadline,
            "the enrolment never kept its entry"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    let _ = go.send(());
    finished(expiry, "the expiry");
    finished(again, "the enrolment");

    assert_eq!(
        book.paired(&pin),
        Some(book::device_id(&pin)),
        "paired again"
    );
    CLOCK.store(issued_at() + PROOF_MAX_AGE_S, Ordering::SeqCst);
    assert_eq!(
        on_the_lan(&book, &enrolment, &pin),
        None,
        "on the LAN past its new proof's age, the cache holding {:?}",
        cached_pins(&vault)
    );
    assert_eq!(enrolment.expired(), vec![pin]);
}

/// ★ The cache's copy in memory, which every admission reads, never goes back to an older read. A
/// re-read of the cache (each list's, and the launch's) held once it has read, an enrolment beside
/// it: the enrolment waits, and the copy ends holding its entry. Before, the re-read put back what
/// it read before the enrolment's save after the enrolment had set the copy: the computer in the
/// book and no age held to it until the cache was read again, which with the internet down (no
/// list) is the next launch.
#[test]
fn the_caches_copy_in_memory_never_goes_back_to_an_older_read() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    CLOCK.store(fixture_now(), Ordering::SeqCst);
    let pin = tls::pin(&spki("computerLan"));
    let vault = holding(Vec::new());
    let enrolment = enrolment_in(&vault, clock);
    let book = empty_book();

    let (rereading, enrolling, into) = (enrolment.clone(), enrolment.clone(), book.clone());
    beside(
        &vault.reads,
        move || rereading.reread(),
        move || enrols(&into, &enrolling),
    );
    assert_eq!(book.paired(&pin), Some(book::device_id(&pin)), "enrolled");
    CLOCK.store(issued_at() + PROOF_MAX_AGE_S, Ordering::SeqCst);
    assert_eq!(
        on_the_lan(&book, &enrolment, &pin),
        None,
        "on the LAN past its proof's age, the cache holding {:?}",
        cached_pins(&vault)
    );
}

/// ★ Each writer holds the cache from its read to its save: it takes the lock once, before it reads.
/// Each writer runs alone; one that asked for the lock again after reading (having let it go between
/// its read and its save) is held there, and a list keeping a device runs then. No writer is held,
/// and the cache holds both changes. One that let the lock go would have saved over the list's.
#[test]
fn each_writer_holds_the_cache_from_its_read_to_its_save() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    CLOCK.store(fixture_now(), Ordering::SeqCst);
    let enrolled = tempo_client::hex(&tls::pin(&spki("computerLan")));
    let ((q_key, q), (r_key, r), (_, u), (_, s), (_, t)) =
        (lan_key(1), lan_key(2), lan_key(3), lan_key(4), lan_key(5));
    let [q_hex, r_hex, u_hex, s_hex, t_hex] = [q, r, u, s, t].map(|pin| tempo_client::hex(&pin));
    let list = vec![
        approved(&q_key),
        approved(&r_key),
        approved(&key("computerLan")),
    ];
    // A list keeping a device it vouched for, by its pin.
    let keeping = |pin: [u8; 32]| -> Writer {
        Box::new(move |vault: &Holding, _: &lan_enrol::Enrolment| {
            let vouched = Vouched {
                subject: "the owner".into(),
                role: Role::Operate,
                guest: false,
                issued_at: issued_at(),
                mfa: false,
                mailbox: None,
            };
            let pin = tempo_client::hex(&pin);
            Controller::keep_for_lan(
                vault,
                SERVICE_STATION,
                &vouched,
                "",
                pin,
                false,
                fixture_now(),
            )
            .unwrap();
        })
    };
    let writers: Vec<(&str, Writer, Vec<&String>)> = vec![
        (
            "an enrolment",
            Box::new(|_: &Holding, enrolment: &lan_enrol::Enrolment| {
                enrols(&empty_book(), enrolment);
            }),
            vec![&q_hex, &r_hex, &u_hex, &enrolled, &s_hex],
        ),
        (
            "a list's sweep",
            Box::new(move |vault: &Holding, _: &lan_enrol::Enrolment| {
                account::sweep(&list, vault, SERVICE_STATION, fixture_now()).unwrap();
            }),
            vec![&q_hex, &r_hex, &s_hex],
        ),
        (
            "a list keeping a device it vouched for",
            keeping(t),
            vec![&q_hex, &r_hex, &u_hex, &t_hex, &s_hex],
        ),
        (
            "an expiry",
            Box::new(move |_: &Holding, enrolment: &lan_enrol::Enrolment| {
                enrolment.forget(&[q]);
            }),
            vec![&r_hex, &u_hex, &s_hex],
        ),
        (
            "a code pairing",
            Box::new(move |_: &Holding, enrolment: &lan_enrol::Enrolment| {
                enrolment.paired_by_code(&r).unwrap();
            }),
            vec![&q_hex, &u_hex, &s_hex],
        ),
    ];
    let (mut cached, mut wanted) = (Vec::new(), Vec::new());
    for (what, writer, after) in writers {
        let vault = holding(vec![
            kept(&q, false, issued_at() - PROOF_MAX_AGE_S),
            kept(&r, false, issued_at()),
            kept(&u, false, issued_at()),
        ]);
        let enrolment = enrolment_in(&vault, clock);
        let (held, go) = vault.relocks.next();
        let (on, beside_it) = (vault.clone(), enrolment.clone());
        let first = std::thread::spawn(move || writer(&on, &beside_it));
        let deadline = Instant::now() + Duration::from_secs(10);
        while !first.is_finished() && held.try_recv().is_err() {
            assert!(Instant::now() < deadline, "{what}: neither done nor held");
            std::thread::sleep(Duration::from_millis(2));
        }
        // Only the first writer is looked at here.
        vault.relocks.clear();
        let (on, beside_it) = (vault.clone(), enrolment.clone());
        let list = std::thread::spawn(move || keeping(s)(&on, &beside_it));
        finished(list, "a list keeping a device");
        let _ = go.send(());
        finished(first, what);
        cached.push((what, cached_pins(&vault)));
        wanted.push((what, after.into_iter().cloned().collect::<Vec<String>>()));
    }
    assert_eq!(cached, wanted, "each, then a list keeping a device");
}

// ---- What a Remove at the shack leaves behind --------------------------------------------------

/// SHA-256 of the fixture's key `name`, lowercase hex: a LAN key's pin, an internet key's
/// fingerprint.
fn hashed(name: &str) -> String {
    tempo_client::hex(ring::digest::digest(&ring::digest::SHA256, &spki(name)).as_ref())
}

/// ★ AN OLD PROOF ENROLS NO COMPUTER REMOVED AT THE SHACK: the fixture's computer enrolled, then
/// removed at the shack (its LAN key kept as removed), enrols again with the proof it had: refused,
/// and not back in the book. So too with its internet key removed instead. A proof signed after
/// the removal enrols it, and its LAN key is no longer removed.
#[test]
fn an_old_proof_enrols_no_computer_removed_at_the_shack() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let pin = tls::pin(&spki("computerLan"));
    for removed in [hashed("computerLan"), hashed("road")] {
        CLOCK.store(fixture_now(), Ordering::SeqCst);
        let vault = MemoryVault::default();
        let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
            Arc::new(vault.clone()),
            clock,
        ));
        enrolment.arm(Some(ready(&vault, Vec::new())));
        enrolment.load();
        let book = empty_book();
        enrols(&book, &enrolment);
        assert!(book.forget(&book::device_id(&pin)), "premise");
        enrolment.remove(std::slice::from_ref(&removed)).unwrap();
        let asked = computer(&token("computer"), "computerLan");
        assert_eq!(
            lan_enrol::admit(&book, &enrolment, STATION, &asked, fixture_now()),
            Err("tokenRemoved"),
            "{removed}"
        );
        assert_eq!(book.paired(&pin), None, "{removed}");

        // Removed before this proof was signed, past the clocks' leeway: it is a sign-in since.
        let vault = MemoryVault::default();
        let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
            Arc::new(vault.clone()),
            clock,
        ));
        enrolment.arm(Some(ready(&vault, Vec::new())));
        enrolment.load();
        CLOCK.store(issued_at() - LEEWAY_S - 1, Ordering::SeqCst);
        enrolment.remove(std::slice::from_ref(&removed)).unwrap();
        CLOCK.store(fixture_now(), Ordering::SeqCst);
        lan_enrol::admit(&book, &enrolment, STATION, &asked, fixture_now()).unwrap();
        assert_eq!(book.paired(&pin), Some(book::device_id(&pin)), "{removed}");
        assert!(!enrolment.is_removed(&pin), "{removed}");
    }
}

/// ★ A COMPUTER WHOSE REMOVAL THE BOOK'S STORE DID NOT KEEP IS ADMITTED NOWHERE: its key kept as
/// removed, a computer the book still holds is refused at TLS and at every session tick, in this
/// run and after a restart; a pairing by code at the shack takes the removal out. CONTROL: another
/// computer stays admitted throughout.
#[test]
fn a_removed_key_the_book_still_holds_is_admitted_nowhere() {
    let (gone, stays) = ([1; 32], [2; 32]);
    let vault = caching(Vec::new());
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        fixture_now,
    ));
    enrolment.load();
    let paired = enrolment.bounded(Arc::new(|pin: &[u8; 32]| Some(book::device_id(pin))));
    assert!(
        paired(&gone).is_some() && paired(&stays).is_some(),
        "premise"
    );
    enrolment.remove(&[tempo_client::hex(&gone)]).unwrap();
    assert_eq!(paired(&gone), None);
    assert!(paired(&stays).is_some());
    let restarted = lan_enrol::Enrolment::kept_in(Arc::new(vault.clone()), fixture_now);
    restarted.load();
    assert!(!restarted.admits(&gone), "after a restart");
    assert!(restarted.admits(&stays));
    enrolment.paired_by_code(&gone).unwrap();
    assert!(paired(&gone).is_some(), "paired again by code at the shack");
}

/// ★ A GUEST'S LINK HOLDS ONLY ON BOTH ROADS, AS THE SAME GUEST: the guest's listing alone (a claim
/// on a LAN key) links nothing; once its computer proves that key here as the same guest, it does.
/// Another person's claim on the key changes nothing. Enrolled by the owner, the link is the
/// owner's, and no guest's claim replaces it.
#[test]
fn a_guests_link_holds_only_when_both_roads_are_theirs() {
    let guest = mailbox("K9.Guest@example.test");
    let vault = MemoryVault::default();
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        fixture_now,
    ));
    enrolment.arm(Some(ready(
        &vault,
        vec![Invite {
            mailbox: guest.clone(),
            role: Role::Operate,
        }],
    )));
    enrolment.load();
    let (lan, road) = (hashed("computerLan"), hashed("road"));
    let tie = || {
        enrolment.reread();
        enrolment
            .ties()
            .into_iter()
            .find(|t| t.lan == lan_enrol::short(&lan))
            .unwrap()
    };
    lan_enrol::tie_vouched(&vault, &lan, &road, Some(&guest), fixture_now()).unwrap();
    assert!(!lan_enrol::linked(&tie()), "a guest's claim alone");

    let book = empty_book();
    let asked = computer(&token("guest"), "computerLan");
    lan_enrol::admit(&book, &enrolment, STATION, &asked, fixture_now()).unwrap();
    assert!(lan_enrol::linked(&tie()), "both roads, the same guest");
    lan_enrol::tie_vouched(
        &vault,
        &lan,
        &road,
        Some("other@example.test"),
        fixture_now(),
    )
    .unwrap();
    assert_eq!(tie().mailbox, Some(lan_enrol::mailbox_tag(&guest)));
    assert!(lan_enrol::linked(&tie()));

    enrols(&book, &enrolment);
    assert_eq!(tie().mailbox, None, "the owner's now");
    assert!(lan_enrol::linked(&tie()));
    lan_enrol::tie_vouched(&vault, &lan, &road, Some(&guest), fixture_now()).unwrap();
    assert_eq!(
        tie().mailbox,
        None,
        "a guest's claim replaced the owner's link"
    );
}

/// What a Remove leaves behind fits the smallest credential blob (Windows, 2560 bytes of UTF-16),
/// full: as many links and removals as are kept, at their longest; and no more are kept.
#[test]
fn the_links_and_removals_kept_fit_the_smallest_credential_blob() {
    let vault = MemoryVault::default();
    let enrolment = lan_enrol::Enrolment::kept_in(Arc::new(vault.clone()), fixture_now);
    enrolment.load();
    let guest = mailbox("K9.Guest@example.test");
    for i in 0..20u8 {
        let lan = tempo_client::hex(&[i; 32]);
        lan_enrol::tie_vouched(&vault, &lan, &hashed("road"), Some(&guest), fixture_now()).unwrap();
    }
    let keys: Vec<String> = (100..130u8).map(|i| tempo_client::hex(&[i; 32])).collect();
    enrolment.remove(&keys).unwrap();
    let mut kept = vault.ties().unwrap().unwrap();
    assert_eq!(
        (kept.ties.len(), kept.removed.len()),
        (tempo_client::account::CACHED_DEVICES, 12)
    );
    assert_ne!(
        kept.floor, 0,
        "premise: the removals pushed out left a floor"
    );
    // The worst case: every link a guest's, proved on both roads, and every time as long as unix
    // seconds get (ten digits, to the year 2286).
    for tie in &mut kept.ties {
        tie.enrolled = true;
        tie.vouched = true;
    }
    for removed in &mut kept.removed {
        removed.at = 9_999_999_999;
    }
    kept.floor = 9_999_999_999;
    let chars = serde_json::to_string(&kept).unwrap().chars().count();
    assert!(chars <= 1280, "{chars} chars, over 2560 bytes of UTF-16");
}

/// ★ A GUEST'S COMPUTER IS THEIRS BY WHAT IT PROVED HERE ALONE: an invitation taken back names a
/// computer that enrolled on the LAN as that guest, even before its listing was checked; never one
/// the owner's link holds, however it was checked, nor one only that guest's listing claims; and
/// a guest's computer kept with no proved link, whoever claims it, as one of unknown owner.
#[test]
fn a_guests_computers_are_those_they_proved_here() {
    let guest = mailbox("K9.Guest@example.test");
    let tag = lan_enrol::mailbox_tag(&guest);
    let pin = |i: u8| tempo_client::hex(&[i; 32]);
    let tie = |i: u8, mailbox: Option<String>, enrolled: bool, vouched: bool| Tie {
        lan: lan_enrol::short(&pin(i)),
        key: lan_enrol::short(&tempo_client::hex(&[i + 50; 32])),
        mailbox,
        enrolled,
        vouched,
    };
    let vault = caching(vec![
        kept(&[1; 32], true, issued_at()),
        kept(&[2; 32], true, issued_at()),
        kept(&[5; 32], true, issued_at()),
    ]);
    vault
        .save_ties(
            &DevicesHeld::take(&vault),
            &Ties {
                ties: vec![
                    // Enrolled as the guest, its listing not checked yet.
                    tie(1, Some(tag.clone()), true, false),
                    // Only the guest's listing claims it.
                    tie(2, Some(tag.clone()), false, true),
                    // The owner's, checked on the internet road alone.
                    tie(3, None, false, true),
                    // The owner's, enrolled.
                    tie(4, None, true, false),
                ],
                ..Ties::default()
            },
        )
        .unwrap();
    let enrolment = lan_enrol::Enrolment::kept_in(Arc::new(vault), fixture_now);
    enrolment.load();
    let (theirs, unknown) = enrolment.guests(&guest);
    assert_eq!(theirs, vec![lan_enrol::short(&pin(1))]);
    assert_eq!(
        unknown,
        vec![lan_enrol::short(&pin(2)), lan_enrol::short(&pin(5))]
    );
}

/// ★ A REMOVAL HOLDS THROUGH THE CLOCKS' LEEWAY: the station's clock and the issuer's may differ by
/// up to a minute, so a proof signed up to a minute after a removal (by the station's clock) is
/// refused: a station clock running slow refuses more, never less. One signed later enrols.
#[test]
fn a_removal_holds_through_the_clocks_leeway() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let pin = tls::pin(&spki("computerLan"));
    for (removed_at, refused) in [
        (issued_at() - 30, true),
        (issued_at() - LEEWAY_S, true),
        (issued_at() - LEEWAY_S - 1, false),
    ] {
        let vault = MemoryVault::default();
        let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
            Arc::new(vault.clone()),
            clock,
        ));
        enrolment.arm(Some(ready(&vault, Vec::new())));
        CLOCK.store(removed_at, Ordering::SeqCst);
        enrolment.load();
        enrolment.remove(&[hashed("computerLan")]).unwrap();
        CLOCK.store(fixture_now(), Ordering::SeqCst);
        let book = empty_book();
        let asked = computer(&token("computer"), "computerLan");
        let admitted = lan_enrol::admit(&book, &enrolment, STATION, &asked, fixture_now());
        match refused {
            true => assert_eq!(admitted, Err("tokenRemoved"), "removed at {removed_at}"),
            false => assert!(admitted.is_ok(), "removed at {removed_at}: {admitted:?}"),
        }
        assert_eq!(
            book.paired(&pin).is_none(),
            refused,
            "removed at {removed_at}"
        );
    }
}

/// ★ A REMOVAL PUSHED OUT OF WHAT IS KEPT STILL REFUSES AN OLD PROOF: the fixture's computer removed
/// at the shack, then as many other keys as are kept (one Reset network identity of a few linked
/// computers does it), so its own removal no longer fits. Its old proof is still refused, by the
/// floor its removal left. CONTROL: removed before that proof was signed, it enrols; and the floor
/// goes once no proof signed by it can still be in age.
#[test]
fn a_removal_pushed_out_of_what_is_kept_still_refuses_an_old_proof() {
    static CLOCK: AtomicU64 = AtomicU64::new(0);
    fn clock() -> u64 {
        CLOCK.load(Ordering::SeqCst)
    }
    let pin = tls::pin(&spki("computerLan"));
    let others: Vec<String> = (100..112u8).map(|i| tempo_client::hex(&[i; 32])).collect();
    for (removed_at, refused) in [(fixture_now(), true), (issued_at() - LEEWAY_S - 1, false)] {
        let vault = MemoryVault::default();
        let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
            Arc::new(vault.clone()),
            clock,
        ));
        enrolment.arm(Some(ready(&vault, Vec::new())));
        CLOCK.store(removed_at, Ordering::SeqCst);
        enrolment.load();
        enrolment.remove(&[hashed("computerLan")]).unwrap();
        enrolment.remove(&others).unwrap();
        let kept = vault.ties().unwrap().unwrap();
        let own = lan_enrol::short(&hashed("computerLan"));
        assert!(
            !kept.removed.iter().any(|r| r.key == own),
            "premise: its removal pushed out"
        );
        assert_eq!(kept.floor, removed_at);

        CLOCK.store(fixture_now(), Ordering::SeqCst);
        let book = empty_book();
        let asked = computer(&token("computer"), "computerLan");
        let admitted = lan_enrol::admit(&book, &enrolment, STATION, &asked, fixture_now());
        match refused {
            true => assert_eq!(admitted, Err("tokenRemoved")),
            false => assert!(admitted.is_ok(), "{admitted:?}"),
        }
        assert_eq!(
            book.paired(&pin).is_none(),
            refused,
            "removed at {removed_at}"
        );

        CLOCK.store(removed_at + PROOF_MAX_AGE_S + LEEWAY_S, Ordering::SeqCst);
        enrolment.remove(&[tempo_client::hex(&[99; 32])]).unwrap();
        assert_eq!(
            vault.ties().unwrap().unwrap().floor,
            0,
            "past every proof's age"
        );
    }
}

/// ★ NO CLAIM PUSHES OUT A LINK A COMPUTER PROVED HERE: as many links as are kept, each proved at
/// enrolment, then a guest's listings claiming other LAN keys, more than are kept (each sign-in of
/// a guest can make one): every proved link stays, and no claim is kept in place of one.
#[test]
fn no_claim_pushes_out_a_link_a_computer_proved_here() {
    let vault = MemoryVault::default();
    let guest = mailbox("K9.Guest@example.test");
    let proved: Vec<Tie> = (0..CACHED_DEVICES as u8)
        .map(|i| Tie {
            lan: lan_enrol::short(&tempo_client::hex(&[i; 32])),
            key: lan_enrol::short(&tempo_client::hex(&[i + 50; 32])),
            mailbox: (i % 2 == 0).then(|| lan_enrol::mailbox_tag(&guest)),
            enrolled: true,
            vouched: i % 3 == 0,
        })
        .collect();
    vault
        .save_ties(
            &DevicesHeld::take(&vault),
            &Ties {
                ties: proved.clone(),
                ..Ties::default()
            },
        )
        .unwrap();
    for i in 100..120u8 {
        let lan = tempo_client::hex(&[i; 32]);
        lan_enrol::tie_vouched(&vault, &lan, &hashed("road"), Some(&guest), fixture_now()).unwrap();
    }
    assert_eq!(vault.ties().unwrap().unwrap().ties, proved);
}

/// ★ A FIRST READ THAT DID NOT COMPLETE IS READ AGAIN: at launch the signed-in computers read and
/// what a Remove leaves behind did not, so nobody is admitted, a computer paired by code included.
/// The next read asked (each new connection asks one first) reads it, and the computer is admitted.
#[test]
fn a_first_read_that_did_not_complete_is_read_again() {
    let by_code = [3; 32];
    let vault = Refusing {
        inner: caching(Vec::new()),
        ..Refusing::default()
    };
    vault.ties_reads.store(true, Ordering::SeqCst);
    let enrolment = lan_enrol::Enrolment::kept_in(Arc::new(vault.clone()), fixture_now);
    enrolment.load();
    assert!(!enrolment.admits(&by_code), "premise: not read");
    vault.ties_reads.store(false, Ordering::SeqCst);
    enrolment.load();
    assert!(enrolment.admits(&by_code));
}

/// ★ A CONNECTION READS AGAIN AT MOST ONCE A WHILE: what a Remove leaves behind did not read, so
/// each connection asks a read first; a flood of them reads the store once per interval, never once
/// each, and the next after it reads it again.
#[test]
fn a_connection_reads_again_at_most_once_a_while() {
    let by_code = [3; 32];
    let vault = Refusing {
        inner: caching(Vec::new()),
        ..Refusing::default()
    };
    vault.ties_reads.store(true, Ordering::SeqCst);
    let enrolment = lan_enrol::Enrolment::kept_in(Arc::new(vault.clone()), fixture_now);
    let start = std::time::Instant::now();
    enrolment.retry(start);
    assert!(!enrolment.admits(&by_code), "premise: not read");
    vault.ties_reads.store(false, Ordering::SeqCst);
    enrolment.retry(start + Duration::from_secs(1));
    assert!(!enrolment.admits(&by_code), "read again at once");
    enrolment.retry(start + lan_enrol::RETRY_EVERY);
    assert!(enrolment.admits(&by_code));
}

/// ★ OVER THE LAN'S OWN LISTENER, A CONNECTION READS AGAIN WHAT DID NOT READ: the listener started
/// with what a Remove leaves behind unread, so it admits nobody; once the store answers, the next
/// connection of a computer paired by code reads it first and is welcomed, with nothing else
/// reading it meanwhile. Skipped, saying so, on a box with no private address of its own.
#[tokio::test(flavor = "multi_thread")]
async fn a_connection_reads_again_what_did_not_read() {
    let Some(address) = own_private_address() else {
        eprintln!("skipped: this box has no private IPv4 address of its own to listen on");
        return;
    };
    let port = std::net::TcpListener::bind((address, 0))
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let network = Network::new(address, 32).unwrap();
    let (station_key, by_code) = (fixture_key(), Computer::new());
    let store = LanStore::holding(&station_key, STATION, &[(&by_code.pin, "Den PC")]);
    let s = shack_with(network, station_key, by_code, store);
    let vault = Refusing {
        inner: caching(Vec::new()),
        ..Refusing::default()
    };
    vault.ties_reads.store(true, Ordering::SeqCst);
    let scratch = Scratch::new();
    let enrolment = Arc::new(lan_enrol::Enrolment::kept_in(
        Arc::new(vault.clone()),
        fixture_now,
    ));
    let lan = lan_enrolled(&s, &scratch, network, book_on(&s.store), enrolment.clone());
    lan.turn_on(None, Some(port)).unwrap();
    let at = SocketAddr::new(address.into(), port);
    eventually(&lan, "never listened", |st| {
        st.listening.as_deref() == Some(at.to_string().as_str())
    })
    .await;
    assert!(!enrolment.is_read(), "premise: not read");
    vault.ties_reads.store(false, Ordering::SeqCst);
    let stream = tokio::net::TcpStream::connect(at).await.unwrap();
    let welcome = match open(stream, &s.computer.key, &s.public_key).await {
        Ok(mut socket) => hello(&mut socket, VERSIONS).await,
        Err(refused) => json!({ "refused": refused }),
    };
    assert_eq!(welcome["type"], "welcome", "{welcome}");
}
